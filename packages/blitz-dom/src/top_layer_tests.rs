use crate::{
    Attribute, BaseDocument, DocumentConfig, NodeId, TopLayerConfig, TopLayerError,
    UserAgentStylesheet, qual_name,
};
use blitz_traits::shell::Viewport;

fn class(value: &str) -> Vec<Attribute> {
    vec![Attribute {
        name: qual_name!("class"),
        value: value.into(),
    }]
}

fn fixture() -> (BaseDocument, [NodeId; 5]) {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0;padding:0}.clip{position:relative;transform:translate(100px,80px);overflow:hidden;width:20px;height:20px;color:rgb(12,34,56);font:19px/24px monospace}.layer{position:fixed;left:40px;top:30px;width:120px;height:80px}.clip>.layer{background:rgb(255,0,0)}.layer:not(:popover-open){display:none}.second{left:70px;top:50px;background:blue!important}.peer{width:5px;height:40px}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let clip = m.create_element(qual_name!("div", html), class("clip"));
    let layer = m.create_element(qual_name!("div", html), class("layer"));
    let second = m.create_element(qual_name!("div", html), class("layer second"));
    let peer = m.create_element(qual_name!("div", html), class("peer"));
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[clip, peer]);
    m.append_children(clip, &[layer]);
    m.append_children(layer, &[second]);
    drop(m);
    doc.resolve(0.);
    (doc, [body, clip, layer, second, peer])
}

#[test]
fn top_layer_escapes_layout_clip_and_transform_but_keeps_dom_and_stylo_ancestry() {
    let (mut doc, [body, clip, layer, _, peer]) = fixture();
    let peer_before = doc.get_client_bounding_rect(peer).unwrap();
    assert!(
        doc.set_top_layer(layer, Some(TopLayerConfig::default()))
            .unwrap()
    );
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(layer).unwrap();
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (40., 30., 120., 80.)
    );
    assert_eq!(doc.get_node(layer).unwrap().parent, Some(clip));
    assert_eq!(doc.get_node(clip).unwrap().parent, Some(body));
    assert_eq!(doc.get_client_bounding_rect(peer).unwrap().y, peer_before.y);
    assert_eq!(doc.resolved_style_value(layer, "color"), "rgb(12, 34, 56)");
    assert_eq!(doc.resolved_style_value(layer, "font-size"), "19px");
    assert_eq!(
        doc.resolved_style_value(layer, "background-color"),
        "rgb(255, 0, 0)"
    );
    assert_eq!(doc.hit(60., 45.).unwrap().node_id, layer);
    assert!(
        !doc.get_node(clip)
            .unwrap()
            .layout_children
            .borrow()
            .as_ref()
            .unwrap()
            .contains(&layer)
    );
    doc.set_viewport_scroll(crate::Point { x: 17., y: 90. });
    assert_eq!(doc.get_client_bounding_rect(layer).unwrap().y, 30.);
    assert_eq!(doc.element_from_point(60., 45.), Some(layer));
    assert_eq!(doc.hit(77., 135.).unwrap().node_id, layer);
    assert_eq!(
        doc.get_node(layer).unwrap().absolute_position(0., 0.).y,
        120.
    );
    doc.set_top_layer(layer, None).unwrap();
    doc.resolve(0.);
    assert!(doc.top_layers().is_empty());
    assert_eq!(doc.get_client_bounding_rect(layer).unwrap().height, 0.);
}

#[test]
fn top_layer_order_update_reopen_modal_backdrop_and_nested_roots() {
    let (mut doc, [_, _, a, b, _]) = fixture();
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.set_top_layer(b, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    assert_eq!(doc.hit(80., 60.).unwrap().node_id, b);
    assert_eq!(doc.get_client_bounding_rect(b).unwrap().x, 70.);
    assert_eq!(doc.get_node(b).unwrap().parent, Some(a));
    doc.set_top_layer(
        a,
        Some(TopLayerConfig {
            modal: true,
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(doc.top_layers()[1].node_id, b, "update must not reorder");
    doc.resolve(0.);
    assert_eq!(doc.hit(80., 60.).unwrap().node_id, b);
    assert_eq!(
        doc.hit(350., 250.).unwrap().node_id,
        a,
        "modal backdrop blocks background"
    );
    doc.set_top_layer(a, None).unwrap();
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    assert_eq!(doc.hit(80., 60.).unwrap().node_id, a);
}

#[test]
fn top_layer_detach_and_drop_retire_entries_and_reused_slots_never_reopen() {
    let (mut doc, [body, clip, a, b, _]) = fixture();
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.set_top_layer(b, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    doc.mutate().remove_node(clip);
    assert!(doc.top_layers().is_empty());
    assert!(!doc.get_node(a).unwrap().flags.is_top_layer());
    assert_eq!(
        doc.set_top_layer(a, Some(TopLayerConfig::default())),
        Err(TopLayerError::InvalidTarget)
    );
    doc.mutate().append_children(body, &[clip]);
    doc.resolve(0.);
    assert!(
        doc.top_layers().is_empty(),
        "reattachment does not resurrect a lease"
    );
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.mutate().remove_and_drop_node(clip);
    assert!(doc.top_layers().is_empty());
    let replacement = doc.mutate().create_element(qual_name!("div", html), vec![]);
    assert_ne!(replacement, a);
    assert_eq!(doc.set_top_layer(a, None), Err(TopLayerError::MissingNode));
}

#[test]
fn top_layer_admission_is_bounded_and_rejection_does_not_change_live_entries() {
    let (mut doc, [body, _, _, _, _]) = fixture();
    assert_eq!(
        doc.set_top_layer(doc.root_element().id, Some(TopLayerConfig::default())),
        Err(TopLayerError::InvalidTarget)
    );
    let mut nodes = Vec::new();
    {
        let mut m = doc.mutate();
        for _ in 0..=crate::MAX_TOP_LAYERS {
            let node = m.create_element(qual_name!("div", html), vec![]);
            m.append_children(body, &[node]);
            nodes.push(node);
        }
    }
    for &id in &nodes[..crate::MAX_TOP_LAYERS] {
        doc.set_top_layer(id, Some(TopLayerConfig::default()))
            .unwrap();
    }
    let last = nodes[crate::MAX_TOP_LAYERS];
    assert_eq!(
        doc.set_top_layer(last, Some(TopLayerConfig::default())),
        Err(TopLayerError::Capacity)
    );
    assert!(!doc.get_node(last).unwrap().flags.is_top_layer());
    assert_eq!(doc.top_layers().len(), crate::MAX_TOP_LAYERS);
    doc.set_top_layer(nodes[0], None).unwrap();
    doc.set_top_layer(last, Some(TopLayerConfig::default()))
        .unwrap();
    assert_eq!(doc.top_layers().last().unwrap().node_id, last);
}

#[test]
fn top_layer_hidden_ancestors_suppress_box_and_backdrop_until_style_recovers() {
    let (mut doc, [_, clip, a, _, _]) = fixture();
    doc.set_top_layer(
        a,
        Some(TopLayerConfig {
            modal: true,
            ..Default::default()
        }),
    )
    .unwrap();
    doc.resolve(0.);
    assert!(doc.top_layer_is_rendered(a));
    doc.mutate()
        .set_attribute(clip, qual_name!("style"), "display:none");
    doc.resolve(0.);
    assert!(!doc.top_layer_is_rendered(a));
    let hidden = doc.get_client_bounding_rect(a).unwrap();
    assert_eq!(
        (hidden.x, hidden.y, hidden.width, hidden.height),
        (0., 0., 0., 0.)
    );
    assert_ne!(doc.hit(350., 250.).map(|hit| hit.node_id), Some(a));
    assert_eq!(
        doc.top_layers().len(),
        1,
        "only the current lease survives hiding"
    );
    doc.mutate().clear_attribute(clip, qual_name!("style"));
    doc.resolve(0.);
    assert!(doc.top_layer_is_rendered(a));
    assert_eq!(doc.hit(350., 250.).unwrap().node_id, a);
    let before = doc.top_layers().to_vec();
    let mut color = crate::util::Color::from_rgb8(0, 0, 0);
    color.components[3] = f32::NAN;
    assert_eq!(
        doc.set_top_layer(
            a,
            Some(TopLayerConfig {
                modal: true,
                backdrop: Some(color)
            })
        ),
        Err(TopLayerError::InvalidBackdrop)
    );
    assert_eq!(doc.top_layers(), before);
}

#[test]
fn top_layer_auto_insets_follow_viewport_resize_and_retirement_reclaims_layout_roots() {
    let (mut doc, [body, _, a, _, peer]) = fixture();
    doc.mutate().set_attribute(
        a,
        qual_name!("style"),
        "left:auto;right:7px;top:auto;bottom:9px",
    );
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    let first = doc.get_client_bounding_rect(a).unwrap();
    assert_eq!((first.x, first.y), (273., 211.));
    doc.viewport_mut().window_size = (500, 400);
    doc.resolve(0.);
    let resized = doc.get_client_bounding_rect(a).unwrap();
    assert_eq!((resized.x, resized.y), (373., 311.));
    doc.mutate().append_children(body, &[a]);
    // A connected move retains the current lease and its viewport geometry,
    // while selectors and inheritance follow the new logical parent.
    assert_eq!(doc.top_layers()[0].node_id, a);
    doc.resolve(0.);
    assert_eq!(doc.get_node(a).unwrap().parent, Some(body));
    assert_eq!(doc.get_client_bounding_rect(a).unwrap().x, 373.);
    assert_eq!(doc.get_client_bounding_rect(peer).unwrap().y, 20.);
}

#[cfg(feature = "writing-mode")]
#[test]
fn vertical_top_layer_uses_its_own_axes_for_layout_cssom_and_viewport_scroll() {
    let (mut doc, [_, _, a, _, _]) = fixture();
    doc.mutate()
        .set_attribute(a, qual_name!("style"), "writing-mode:vertical-rl");
    doc.set_top_layer(a, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(a).unwrap();
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (40., 30., 120., 80.)
    );
    doc.set_viewport_scroll(crate::Point { x: 13., y: 90. });
    assert_eq!(doc.get_client_bounding_rect(a).unwrap(), rect);
    assert_eq!(doc.element_from_point(50., 40.), Some(a));
}

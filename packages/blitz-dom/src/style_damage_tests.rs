use crate::profile::{Phase, Profile};
use crate::{Attribute, BaseDocument, DocumentConfig, UserAgentStylesheet, qual_name};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::Viewport;

fn fixture(incremental: bool) -> (BaseDocument, [NodeId; 5], Profile) {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
        incremental: Some(incremental),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0;padding:0}.root{position:relative}.row{position:absolute;top:0;left:0;width:160px;height:32px;font:16px/20px monospace}.peer{position:absolute;left:200px;top:0;font:16px/20px monospace}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(
        qual_name!("body", html),
        vec![Attribute {
            name: qual_name!("class"),
            value: "root".into(),
        }],
    );
    let row = m.create_element(
        qual_name!("div", html),
        vec![Attribute {
            name: qual_name!("class"),
            value: "row".into(),
        }],
    );
    let span = m.create_element(qual_name!("span", html), vec![]);
    let peer = m.create_element(
        qual_name!("div", html),
        vec![Attribute {
            name: qual_name!("class"),
            value: "peer".into(),
        }],
    );
    let text = m.create_text_node("alpha beta gamma delta");
    let other = m.create_text_node("peer text");
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[row, peer]);
    m.append_children(row, &[span]);
    m.append_children(span, &[text]);
    m.append_children(peer, &[other]);
    drop(m);
    let profile = Profile::default();
    doc.set_profile(Some(profile.clone()));
    doc.resolve(0.);
    let text_box = doc.get_client_bounding_rect(span).unwrap();
    assert!(
        text_box.width > 0. && text_box.height > 0.,
        "real font required"
    );
    assert!(profile.snapshot().sample(Phase::TextShape).calls > 0);
    assert!(profile.reset());
    (doc, [html, body, row, span, peer], profile)
}

#[test]
fn inline_offset_and_height_changes_reuse_text_then_clear_to_stylesheet() {
    let (mut doc, nodes, profile) = fixture(true);
    let row = nodes[2];
    doc.mutate()
        .set_attribute(row, qual_name!("style"), "top:90px;height:48px");
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(row).unwrap();
    assert_eq!((rect.y, rect.height), (90., 48.));
    assert_eq!(profile.snapshot().sample(Phase::TextShape).calls, 0);
    assert!(profile.reset());
    doc.mutate().clear_attribute(row, qual_name!("style"));
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(row).unwrap();
    assert_eq!((rect.y, rect.height), (0., 32.));
    assert_eq!(profile.snapshot().sample(Phase::TextShape).calls, 0);
}

#[test]
fn equivalent_inline_declarations_do_not_shape_or_break_cached_text() {
    let (mut doc, nodes, profile) = fixture(true);
    doc.mutate()
        .set_attribute(nodes[2], qual_name!("style"), "top:0px;height:32px");
    doc.resolve(0.);
    assert_eq!(profile.snapshot().sample(Phase::TextShape).calls, 0);
    assert_eq!(profile.snapshot().sample(Phase::TextBreak).calls, 0);
}

#[test]
fn inline_style_differences_match_full_reconstruction_across_layout_and_text_changes() {
    let (mut incremental, ids, _) = fixture(true);
    let (mut complete, full_ids, _) = fixture(false);
    for value in [
        "top:91px;left:7px;height:48px",
        "width:72px;padding:3px;border:2px solid red",
        "font:16px/20px monospace;width:72px",
        "font:16px/30px monospace;width:72px",
        "font:16px/30px monospace;width:72px;text-align:right",
        "font:16px/30px monospace;width:72px;text-align:center",
        "font-size:24px;line-height:30px",
        "font-weight:700;font-family:serif",
        "white-space:nowrap;letter-spacing:2px",
        "direction:rtl;text-transform:uppercase",
        "color:blue;opacity:0.6;transform:translateY(8px)",
        "display:none",
        "display:flex;align-items:center;justify-content:end",
        "position:relative;top:4px;width:80px;height:auto",
        "display:contents",
        "",
    ] {
        incremental
            .mutate()
            .set_attribute(ids[2], qual_name!("style"), value);
        complete
            .mutate()
            .set_attribute(full_ids[2], qual_name!("style"), value);
        incremental.resolve(0.);
        complete.resolve(0.);
        for (&id, &full) in ids.iter().zip(&full_ids) {
            let a = incremental.get_client_bounding_rect(id);
            let b = complete.get_client_bounding_rect(full);
            assert_eq!(a.is_some(), b.is_some(), "{value}: {id:?}");
            if let (Some(a), Some(b)) = (a, b) {
                for (actual, expected) in [
                    (a.x, b.x),
                    (a.y, b.y),
                    (a.width, b.width),
                    (a.height, b.height),
                ] {
                    assert!(
                        (actual - expected).abs() < 0.01,
                        "{value}: {id:?}: {a:?} != {b:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn style_attribute_selectors_still_restyle_related_text() {
    let (mut doc, nodes, profile) = fixture(true);
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        ".row[style*=\"99\"] + .peer{font-size:26px}",
    ));
    doc.resolve(0.);
    let before = doc.get_client_bounding_rect(nodes[4]).unwrap().width;
    assert!(profile.reset());
    doc.mutate()
        .set_attribute(nodes[2], qual_name!("style"), "top:99px");
    doc.resolve(0.);
    let after = doc.get_client_bounding_rect(nodes[4]).unwrap().width;
    assert!(
        after > before * 1.2,
        "selector restyle lost: {before} -> {after}"
    );
    assert!(profile.snapshot().sample(Phase::TextShape).calls > 0);
}

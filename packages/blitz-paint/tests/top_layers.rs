use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{
    Attribute, BaseDocument, DocumentConfig, TopLayerConfig, UserAgentStylesheet, qual_name,
};
use blitz_traits::shell::Viewport;

fn class(value: &str) -> Vec<Attribute> {
    vec![Attribute {
        name: qual_name!("class"),
        value: value.into(),
    }]
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4]
        .try_into()
        .unwrap()
}

#[test]
fn native_top_layers_paint_once_after_document_and_escape_ancestor_clip_opacity_transform() {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(200, 160, 1., Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0;padding:0;background:white}.clip{position:relative;transform:translate(100px,90px);opacity:0.1;overflow:hidden;width:10px;height:10px}.layer{position:fixed;left:20px;top:20px;width:80px;height:70px;background:rgba(255,0,0,0.5)}.second{left:50px;top:40px;width:60px;height:50px;background:blue}.layer:not(:popover-open){display:none}.peer{position:fixed;left:0;top:0;width:200px;height:160px;background:lime;z-index:999999}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let clip = m.create_element(qual_name!("div", html), class("clip"));
    let a = m.create_element(qual_name!("div", html), class("layer"));
    let b = m.create_element(qual_name!("div", html), class("layer second"));
    let peer = m.create_element(qual_name!("div", html), class("peer"));
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[clip, peer]);
    m.append_children(clip, &[a]);
    m.append_children(a, &[b]);
    drop(m);
    doc.set_top_layer(
        a,
        Some(TopLayerConfig {
            modal: true,
            contain_scroll: true,
            backdrop: Some(blitz_dom::util::Color::from_rgba8(0, 0, 0, 128)),
        }),
    )
    .unwrap();
    doc.set_top_layer(b, Some(TopLayerConfig::default()))
        .unwrap();
    doc.resolve(0.);
    let mut raster = VelloCpuImageRenderer::new(200, 160);
    let mut pixels = vec![0; 200 * 160 * 4];
    raster.render(
        |scene| blitz_paint::paint_scene(scene, &mut doc, 1., 200, 160, 0, 0),
        &mut pixels,
    );
    assert_eq!(
        pixel(&pixels, 60, 50),
        [0, 0, 255, 255],
        "nested layer is above its logical parent and huge document z-index"
    );
    let once = pixel(&pixels, 30, 30);
    assert!(
        (125..=130).contains(&once[0]) && (61..=65).contains(&once[1]) && once[2] == 0,
        "one half-alpha red pass over one black backdrop: {once:?}"
    );
    assert_eq!(
        pixel(&pixels, 150, 120),
        [0, 127, 0, 255],
        "modal backdrop escapes ancestor clip and opacity"
    );
    doc.set_top_layer(b, None).unwrap();
    doc.set_top_layer(a, None).unwrap();
    doc.resolve(0.);
    raster.render(
        |scene| blitz_paint::paint_scene(scene, &mut doc, 1., 200, 160, 0, 0),
        &mut pixels,
    );
    assert_eq!(
        pixel(&pixels, 60, 50),
        [0, 255, 0, 255],
        "closing removes both top paint and backdrop"
    );
}

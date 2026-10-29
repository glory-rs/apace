use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use apace_dom::{Attribute, BaseDocument, DocumentConfig, NodeId, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

fn fixture(scale: f32, multiline: bool, password: bool) -> (BaseDocument, NodeId, u32) {
    let width = (240. * scale) as u32;
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(width, width, scale, Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0;padding:0;background:white}input,textarea{position:absolute;left:20px;top:20px;width:90px;height:60px;padding:0;border:0;margin:0;outline:none;background:white;color:blue;font:20px/24px monospace;text-decoration:underline}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let id = m.create_element(
        if multiline {
            qual_name!("textarea", html)
        } else {
            qual_name!("input", html)
        },
        if password {
            vec![Attribute {
                name: qual_name!("type"),
                value: "password".into(),
            }]
        } else {
            vec![]
        },
    );
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[id]);
    drop(m);
    doc.resolve(0.);
    (doc, id, width)
}
fn render(doc: &mut BaseDocument, width: u32, scale: f32) -> Vec<u8> {
    doc.resolve(0.);
    let mut pixels = vec![0; (width * width * 4) as usize];
    VelloCpuImageRenderer::new(width, width).render(
        |scene| apace_paint::paint_scene(scene, doc, f64::from(scale), width, width, 0, 0),
        &mut pixels,
    );
    pixels
}
fn raw(doc: &BaseDocument, id: NodeId) -> &apace_dom::node::TextInputData {
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .text_input_data()
        .unwrap()
}
#[test]
fn hints_paint_clip_and_restore_without_becoming_values_passwords_or_scroll_content() {
    for scale in [1., 2.] {
        for multiline in [false, true] {
            for password in [false, true] {
                if multiline && password {
                    continue;
                }
                let (mut doc, id, width) = fixture(scale, multiline, password);
                let blank = render(&mut doc, width, scale);
                let hint = if multiline {
                    "First\nsecond long long long\nthird\nfourth"
                } else {
                    "Long hint across the whole page"
                };
                doc.mutate()
                    .set_attribute(id, qual_name!("placeholder"), hint);
                let hinted = render(&mut doc, width, scale);
                assert_ne!(hinted, blank, "empty hint has actual CPU glyphs");
                assert!(raw(&doc, id).editor.raw_text().is_empty());
                assert_eq!(raw(&doc, id).scroll_offset, 0.);
                let mut changed = 0;
                for (i, (a, b)) in hinted
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(blank.as_chunks::<4>().0.iter())
                    .enumerate()
                {
                    if a != b {
                        changed += 1;
                        let x = (i as u32 % width) as f32;
                        let y = (i as u32 / width) as f32;
                        assert!(
                            x >= 20. * scale
                                && x < 110. * scale
                                && y >= 20. * scale
                                && y < 80. * scale,
                            "hint and underline must stay within content box at {x},{y}"
                        );
                        assert_eq!(a[0], a[1]);
                        assert_eq!(
                            a[1], a[2],
                            "hint is neutral gray, not the real value's blue"
                        );
                    }
                }
                assert!(changed > 20);
                // A value hides the hint, and removing hidden metadata cannot change any pixels.
                doc.with_text_input(id, |mut d| {
                    d.select_all();
                    d.insert_or_replace_selection("a very long real value which scrolls");
                });
                doc.set_focus_to(id);
                doc.with_text_input(id, |mut d| d.move_to_text_end());
                let filled = render(&mut doc, width, scale);
                if !multiline {
                    assert!(raw(&doc, id).scroll_offset > 0.);
                }
                doc.mutate().clear_attribute(id, qual_name!("placeholder"));
                assert_eq!(render(&mut doc, width, scale), filled);
                doc.mutate()
                    .set_attribute(id, qual_name!("placeholder"), hint);
                doc.clear_focus();
                doc.with_text_input(id, |mut d| {
                    d.select_all();
                    d.insert_or_replace_selection("");
                });
                assert_eq!(
                    render(&mut doc, width, scale),
                    hinted,
                    "clearing restores exact hint with no stale scroll offset"
                );
                doc.mutate().clear_attribute(id, qual_name!("placeholder"));
                assert_eq!(render(&mut doc, width, scale), blank);
            }
        }
    }
}

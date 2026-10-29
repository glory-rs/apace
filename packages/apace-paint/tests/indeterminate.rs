use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use apace_dom::{Attribute, BaseDocument, DocumentConfig, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

#[test]
fn mixed_checkbox_paints_a_dash_independently_of_checked_and_clears_back_to_both_boolean_states() {
    for scale in [1., 2.] {
        let width = (80. * scale) as u32;
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(width, width, scale, Default::default())),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
            "html,body{margin:0;background:white}input{position:absolute;left:12px;top:12px;width:36px;height:36px;color:rgb(0,0,255)}",
        ));
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html", html), vec![]);
        let body = m.create_element(qual_name!("body", html), vec![]);
        let input = m.create_element(
            qual_name!("input", html),
            vec![Attribute {
                name: qual_name!("type"),
                value: "checkbox".into(),
            }],
        );
        m.append_children(root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[input]);
        drop(m);
        let mut raster = VelloCpuImageRenderer::new(width, width);
        let mut frames = Vec::new();
        for (checked, mixed) in [
            (false, false),
            (true, false),
            (true, true),
            (false, true),
            (false, false),
            (true, false),
        ] {
            let mut m = doc.mutate();
            if checked {
                m.set_attribute(input, qual_name!("checked"), "true");
            } else {
                m.clear_attribute(input, qual_name!("checked"));
            }
            assert!(m.set_input_indeterminate(input, mixed));
            drop(m);
            doc.resolve(0.);
            let mut pixels = vec![0; (width * width * 4) as usize];
            raster.render(
                |scene| {
                    apace_paint::paint_scene(scene, &mut doc, f64::from(scale), width, width, 0, 0)
                },
                &mut pixels,
            );
            frames.push(pixels);
        }
        assert!(frames[0] != frames[1], "boolean rendering must differ");
        assert!(frames[1] != frames[2], "dash differs from checkmark");
        assert!(frames[0] != frames[2], "mixed differs from empty");
        assert!(
            frames[2] == frames[3],
            "underlying boolean cannot change mixed painting"
        );
        assert!(frames[0] == frames[4], "clearing mixed restores empty");
        assert!(frames[1] == frames[5], "clearing mixed restores checked");
    }
}

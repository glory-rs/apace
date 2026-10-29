use crate::{Attribute, BaseDocument, DocumentConfig, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

#[test]
fn boxed_contents_cycles_retire_old_rosters_and_anonymous_boxes_without_replacing_editor() {
    for incremental in [false, true] {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1., Default::default())),
            incremental: Some(incremental),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
            "html,body{margin:0;padding:0}div{display:block}input{width:80px;height:24px}",
        ));
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html", html), vec![]);
        let body = m.create_element(qual_name!("body", html), vec![]);
        let owner = m.create_element(qual_name!("div", html), vec![]);
        let prefix = m.create_text_node("prefix ");
        let editor = m.create_element(
            qual_name!("input", html),
            vec![Attribute {
                name: qual_name!("value"),
                value: "kept".into(),
            }],
        );
        let block = m.create_element(qual_name!("div", html), vec![]);
        let text = m.create_text_node("suffix");
        m.append_children(root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[owner]);
        m.append_children(owner, &[prefix, editor, block]);
        m.append_children(block, &[text]);
        drop(m);
        doc.resolve(0.);
        let baseline = doc.nodes.len();
        for round in 0..32 {
            for display in ["contents", "block"] {
                doc.mutate().set_attribute(
                    owner,
                    qual_name!("style"),
                    &format!("display:{display}"),
                );
                doc.resolve(f64::from(round));
                assert!(doc.get_node(editor).unwrap().has_boxes());
                assert_eq!(
                    doc.get_node(editor)
                        .unwrap()
                        .element_data()
                        .unwrap()
                        .text_input_data()
                        .unwrap()
                        .editor
                        .raw_text(),
                    "kept"
                );
                if display == "contents" {
                    assert!(
                        doc.get_node(owner)
                            .unwrap()
                            .layout_children
                            .borrow()
                            .is_none()
                    );
                }
                assert!(
                    doc.nodes.len() <= baseline + 8,
                    "anonymous boxes grew across current layouts"
                );
                for (_, node) in doc.nodes.iter() {
                    for child in node.layout_children.borrow().iter().flatten() {
                        assert!(doc.get_node(*child).is_some());
                    }
                    for child in node.paint_children.borrow().iter().flatten() {
                        assert!(doc.get_node(*child).is_some());
                    }
                }
            }
        }
    }
}

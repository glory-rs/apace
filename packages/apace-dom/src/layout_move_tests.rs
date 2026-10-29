use crate::{Attribute, BaseDocument, DocumentConfig, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

fn assert_consistent(doc: &BaseDocument) {
    for (id, node) in doc.nodes.iter() {
        for child in node.layout_children.borrow().iter().flatten() {
            assert_eq!(doc.nodes[*child].layout_parent.get(), Some(id));
        }
        for child in node.hoisted_children.borrow().iter() {
            assert!(doc.nodes.contains_key(*child));
            assert_eq!(doc.nodes[*child].oof_containing_block.get(), Some(id));
        }
    }
}

fn class(value: &str) -> Vec<Attribute> {
    vec![Attribute {
        name: qual_name!("class"),
        value: value.into(),
    }]
}

#[test]
fn repeated_hidden_owner_moves_preserve_identity_editor_and_bounded_boxes() {
    for incremental in [true, false] {
        for display in ["flex", "grid", "block"] {
            for contents in [false, true] {
                for positioned in [false, true] {
                    let mut doc = BaseDocument::new(DocumentConfig {
                        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
                        incremental: Some(incremental),
                        ..Default::default()
                    });
                    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(&format!(
                        "html,body{{margin:0;padding:0}}[hidden]{{display:none!important}}\
                         .old{{display:{display};width:200px;position:relative}}\
                         .new{{display:flex;width:200px;position:absolute;left:0;top:0;opacity:0}}\
                         .item{{display:{};width:140px}}\
                         .atomic{{display:inline-flex;width:140px}}\
                         input{{width:140px;height:24px;padding:0;border:0;box-sizing:border-box;{}}}",
                        if contents { "contents" } else { "inline-flex" },
                        if positioned { "position:absolute;left:5px;top:8px" } else { "" },
                    )));
                    let root = doc.root_node().id;
                    let mut m = doc.mutate();
                    let html = m.create_element(qual_name!("html", html), vec![]);
                    let body = m.create_element(qual_name!("body", html), vec![]);
                    let hidden = m.create_element(qual_name!("div", html), vec![]);
                    let old = m.create_element(qual_name!("div", html), class("old"));
                    let new = m.create_element(qual_name!("div", html), class("new"));
                    let item = m.create_element(qual_name!("span", html), class("item"));
                    let atomic = m.create_element(qual_name!("span", html), class("atomic"));
                    let input = m.create_element(qual_name!("input", html), vec![]);
                    let start = m.create_comment_node("group start");
                    let end = m.create_comment_node("group end");
                    let block = m.create_element(qual_name!("div", html), vec![]);
                    let text = m.create_text_node("block sibling");
                    m.append_children(root, &[html]);
                    m.append_children(html, &[body]);
                    m.append_children(body, &[hidden, new]);
                    m.append_children(hidden, &[old]);
                    m.append_children(old, &[start, item, end, block]);
                    m.append_children(block, &[text]);
                    m.append_children(item, &[atomic]);
                    m.append_children(atomic, &[input]);
                    drop(m);
                    doc.resolve(0.);
                    doc.set_focus_to(input);
                    doc.with_text_input(input, |mut driver| {
                        driver.editor.set_text("retained draft");
                        driver.move_to_byte(2);
                        driver.extend_selection_to_byte(8);
                        driver.set_compose_byte_range(2, 8);
                    });
                    let initial = doc.nodes.len();
                    for _ in 0..16 {
                        let mut m = doc.mutate();
                        m.set_attribute(hidden, qual_name!("hidden"), "");
                        m.append_children(new, &[start, item, end]);
                        drop(m);
                        doc.resolve(0.);
                        assert_consistent(&doc);
                        assert_eq!(doc.nodes[item].parent, Some(new));
                        assert_eq!(doc.get_focussed_node_id(), Some(input));
                        let input_data = doc.nodes[input]
                            .element_data()
                            .unwrap()
                            .text_input_data()
                            .unwrap();
                        assert_eq!(input_data.editor.raw_text(), "retained draft");
                        assert_eq!(input_data.editor.raw_selection().text_range(), 2..8);
                        assert_eq!(input_data.editor.raw_compose(), &Some(2..8));
                        assert!(
                            (doc.get_client_bounding_rect(input).unwrap().width - 140.).abs() < 0.1
                        );
                        let mut m = doc.mutate();
                        m.clear_attribute(hidden, qual_name!("hidden"));
                        m.insert_nodes_before(block, &[start, item, end]);
                        drop(m);
                        doc.resolve(0.);
                        assert_consistent(&doc);
                        assert_eq!(doc.nodes[item].parent, Some(old));
                        // Same-parent reorder must retire and reconstruct the
                        // same edges without duplicates or a new editor.
                        doc.mutate().append_children(old, &[start, item, end]);
                        doc.resolve(0.);
                        assert_consistent(&doc);
                        assert_eq!(doc.get_focussed_node_id(), Some(input));
                        assert!(doc.nodes.len() <= initial + 4, "anonymous boxes leaked");
                    }
                }
            }
        }
    }
}

#[test]
fn connected_move_out_of_hidden_flex_owner_retires_its_old_layout_reference() {
    for incremental in [true, false] {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1., Default::default())),
            incremental: Some(incremental),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
            "html,body{margin:0;padding:0}[hidden]{display:none!important}\
             .old,.new{display:flex;flex-direction:column;width:200px}\
             .new{position:absolute;top:0;left:0;opacity:0}\
             .item{display:inline-flex;width:140px;flex-shrink:0}\
             input{width:100%;height:24px;padding:0;border:0;box-sizing:border-box}",
        ));
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html", html), vec![]);
        let body = m.create_element(qual_name!("body", html), vec![]);
        let hidden = m.create_element(qual_name!("div", html), vec![]);
        let old = m.create_element(qual_name!("div", html), class("old"));
        let new = m.create_element(qual_name!("div", html), class("new"));
        let item = m.create_element(qual_name!("div", html), class("item"));
        let input = m.create_element(qual_name!("input", html), vec![]);
        let start = m.create_comment_node("group start");
        let end = m.create_comment_node("group end");
        m.append_children(root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[hidden, new]);
        m.append_children(hidden, &[old]);
        m.append_children(old, &[start, item, end]);
        m.append_children(item, &[input]);
        drop(m);
        doc.resolve(0.);
        assert_eq!(doc.nodes[item].layout_parent.get(), Some(old));
        let mut m = doc.mutate();
        m.set_attribute(hidden, qual_name!("hidden"), "");
        m.append_children(new, &[start, item, end]);
        drop(m);
        doc.resolve(0.);
        assert_eq!(doc.nodes[item].layout_parent.get(), Some(new));
        assert!(
            !doc.nodes[old]
                .layout_children
                .borrow()
                .as_ref()
                .is_some_and(|children| children.contains(&item))
        );
        assert!((doc.get_client_bounding_rect(input).unwrap().width - 140.).abs() < 0.1);
    }
}

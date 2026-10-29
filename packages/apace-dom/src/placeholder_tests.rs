use crate::{Attribute, BaseDocument, DocumentConfig, NodeId, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

fn fixture(multiline: bool, value: &str) -> (BaseDocument, NodeId, NodeId) {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0}input,textarea{width:100px;height:60px;font:16px/20px monospace}label{display:block;width:20px;height:20px}:placeholder-shown+label{width:80px}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let input = m.create_element(
        if multiline {
            qual_name!("textarea", html)
        } else {
            qual_name!("input", html)
        },
        vec![Attribute {
            name: qual_name!("placeholder"),
            value: "Hint".into(),
        }],
    );
    let label = m.create_element(qual_name!("label", html), vec![]);
    if multiline {
        let text = m.create_text_node(value);
        m.append_children(input, &[text]);
    } else {
        m.set_attribute(input, qual_name!("value"), value);
    }
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[input, label]);
    drop(m);
    doc.resolve(0.);
    (doc, input, label)
}
fn input(doc: &BaseDocument, id: NodeId) -> &crate::node::TextInputData {
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .text_input_data()
        .unwrap()
}
fn marker(doc: &BaseDocument, id: NodeId) -> f64 {
    doc.get_client_bounding_rect(id).unwrap().width
}

#[test]
fn placeholder_state_tracks_live_value_and_initial_textarea_children_in_sibling_selectors() {
    for multiline in [false, true] {
        for initial in ["", "value"] {
            let (mut doc, id, label) = fixture(multiline, initial);
            assert_eq!(
                marker(&doc, label),
                if initial.is_empty() { 80. } else { 20. }
            );
            assert_eq!(
                input(&doc, id).placeholder_layout().is_some(),
                initial.is_empty()
            );
            doc.with_text_input(id, |mut d| {
                d.select_all();
                d.insert_or_replace_selection("filled");
            });
            doc.resolve(0.);
            assert_eq!(marker(&doc, label), 20.);
            assert!(input(&doc, id).placeholder_layout().is_none());
            doc.with_text_input(id, |mut d| {
                d.select_all();
                d.insert_or_replace_selection("");
            });
            doc.resolve(0.);
            assert_eq!(marker(&doc, label), 80.);
            assert!(input(&doc, id).placeholder_layout().is_some());
            doc.mutate().clear_attribute(id, qual_name!("placeholder"));
            doc.resolve(0.);
            assert_eq!(marker(&doc, label), 20.);
            assert!(input(&doc, id).placeholder_layout().is_none());
        }
    }
}

#[test]
fn hint_changes_preserve_real_editor_generation_selection_and_marked_text() {
    for multiline in [false, true] {
        let (mut doc, id, _) = fixture(multiline, "A中Z");
        doc.with_text_input(id, |mut d| {
            d.select_byte_range(1, 4);
            d.set_compose("拼", Some((3, 3)));
        });
        let raw = input(&doc, id).editor.raw_text().to_owned();
        let selection = *input(&doc, id).editor.raw_selection();
        let generation = input(&doc, id).editor.generation();
        let address = std::ptr::from_ref(input(&doc, id).editor.as_ref());
        for text in [Some("new hint"), Some(""), None, Some("restored")] {
            if let Some(text) = text {
                doc.mutate()
                    .set_attribute(id, qual_name!("placeholder"), text);
            } else {
                doc.mutate().clear_attribute(id, qual_name!("placeholder"));
            }
            doc.resolve(0.);
            let data = input(&doc, id);
            assert_eq!(std::ptr::from_ref(data.editor.as_ref()), address);
            assert_eq!(data.editor.generation(), generation);
            assert_eq!(data.editor.raw_text(), raw);
            assert_eq!(*data.editor.raw_selection(), selection);
            assert!(data.editor.is_composing());
            assert!(data.placeholder_layout().is_none());
        }
        doc.with_text_input(id, |mut d| {
            d.clear_compose();
            d.select_all();
            d.delete_selection();
        });
        doc.resolve(0.);
        assert!(input(&doc, id).placeholder_layout().is_some());
        assert!(input(&doc, id).editor.raw_text().is_empty());
    }
}

#[test]
fn hints_have_bounded_independent_wrapping_scale_and_password_geometry() {
    for multiline in [false, true] {
        let (mut doc, id, _) = fixture(multiline, "");
        let long = if multiline {
            "first\nsecond long long long"
        } else {
            "long long long long long"
        };
        doc.mutate()
            .set_attribute(id, qual_name!("placeholder"), long);
        doc.resolve(0.);
        let data = input(&doc, id);
        assert!(data.editor.raw_text().is_empty());
        assert!(data.display_editor().raw_text().is_empty());
        let hint = data.placeholder_layout().unwrap();
        assert_eq!(hint.lines().count() > 1, multiline);
        let height = hint.height();
        doc.set_viewport(Viewport::new(800, 600, 2., Default::default()));
        doc.resolve(0.);
        assert!(input(&doc, id).placeholder_layout().unwrap().height() >= height * 1.9);
        doc.mutate()
            .set_attribute(id, qual_name!("placeholder"), &"x".repeat(4097));
        doc.resolve(0.);
        assert!(input(&doc, id).placeholder_layout().is_none());
        doc.mutate()
            .set_attribute(id, qual_name!("placeholder"), "password hint");
        if !multiline {
            doc.mutate()
                .set_attribute(id, qual_name!("type"), "password");
        }
        doc.resolve(0.);
        assert!(input(&doc, id).placeholder_layout().is_some());
        assert!(input(&doc, id).display_editor().raw_text().is_empty());
        doc.with_text_input(id, |mut d| d.move_to_point(80., 10.));
        assert_eq!(input(&doc, id).editor.raw_selection().focus().index(), 0);
    }
}

#[test]
fn empty_hint_keeps_pseudo_state_but_no_layout_and_attribute_rules_still_change_fonts() {
    let (mut doc, id, label) = fixture(false, "");
    doc.mutate()
        .set_attribute(id, qual_name!("placeholder"), "");
    doc.resolve(0.);
    assert_eq!(marker(&doc, label), 80.);
    assert!(input(&doc, id).placeholder_shown());
    assert!(input(&doc, id).placeholder_layout().is_none());
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "input[placeholder=large]{font-size:32px;line-height:40px;text-align:right}",
    ));
    doc.mutate()
        .set_attribute(id, qual_name!("placeholder"), "small");
    doc.resolve(0.);
    let old_height = input(&doc, id).placeholder_layout().unwrap().height();
    doc.mutate()
        .set_attribute(id, qual_name!("placeholder"), "large");
    doc.resolve(0.);
    assert!(input(&doc, id).placeholder_layout().unwrap().height() > old_height * 1.5);
    doc.mutate().clear_attribute(id, qual_name!("placeholder"));
    doc.resolve(0.);
    assert_eq!(marker(&doc, label), 20.);
    doc.mutate()
        .set_attribute(id, qual_name!("placeholder"), "hint");
    doc.mutate()
        .set_attribute(id, qual_name!("type"), "checkbox");
    doc.resolve(0.);
    assert_eq!(marker(&doc, label), 20.);
    doc.mutate().set_attribute(id, qual_name!("type"), "text");
    doc.resolve(0.);
    assert_eq!(marker(&doc, label), 80.);
}

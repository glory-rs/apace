use crate::{Attribute, BaseDocument, DocumentConfig, UserAgentStylesheet, qual_name};
use blitz_traits::shell::Viewport;
use style_dom::ElementState;

#[test]
fn mixed_property_is_independent_preserved_on_attach_and_invalidates_related_selectors() {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(200, 120, 1., Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "html,body{margin:0}label{display:block;width:20px;height:20px}input:indeterminate+label{width:80px}input:checked+label{height:40px}",
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
    let radio = m.create_element(
        qual_name!("input", html),
        vec![Attribute {
            name: qual_name!("type"),
            value: "radio".into(),
        }],
    );
    let label = m.create_element(qual_name!("label", html), vec![]);
    assert!(m.set_input_indeterminate(input, true));
    assert!(!m.set_input_indeterminate(radio, true));
    assert!(!m.set_input_indeterminate(body, true));
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[input, label, radio]);
    drop(m);
    doc.resolve(0.);
    let element = doc.get_node(input).unwrap().element_data().unwrap();
    assert_eq!(element.checkbox_input_checked(), Some(false));
    assert_eq!(element.checkbox_input_indeterminate(), Some(true));
    let rect = doc.get_client_bounding_rect(label).unwrap();
    assert_eq!((rect.width, rect.height), (80., 20.));
    doc.mutate()
        .set_attribute(input, qual_name!("checked"), "true");
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(label).unwrap();
    assert_eq!(
        (rect.width, rect.height),
        (80., 40.),
        "program checkedness preserves mixed"
    );
    doc.snapshot_node_and(
        input,
        ElementState::CHECKED | ElementState::INDETERMINATE,
        |node| {
            assert!(!BaseDocument::toggle_checkbox(
                node.element_data_mut().unwrap()
            ));
            node.mark_ancestors_dirty();
        },
    );
    doc.resolve(0.);
    let element = doc.get_node(input).unwrap().element_data().unwrap();
    assert_eq!(element.checkbox_input_indeterminate(), Some(false));
    let rect = doc.get_client_bounding_rect(label).unwrap();
    assert_eq!(
        (rect.width, rect.height),
        (20., 20.),
        "activation clears both pseudo classes"
    );
    assert!(doc.mutate().set_input_indeterminate(input, true));
    doc.resolve(0.);
    assert_eq!(doc.get_client_bounding_rect(label).unwrap().width, 80.);
    assert!(doc.mutate().set_input_indeterminate(input, false));
    doc.resolve(0.);
    assert_eq!(doc.get_client_bounding_rect(label).unwrap().width, 20.);
    assert!(doc.mutate().set_input_indeterminate(input, true));
    doc.mutate()
        .set_attribute(input, qual_name!("type"), "radio");
    doc.resolve(0.);
    assert!(
        !doc.get_node(input)
            .unwrap()
            .element_data()
            .unwrap()
            .element_state
            .contains(ElementState::INDETERMINATE)
    );
    assert_eq!(doc.get_client_bounding_rect(label).unwrap().width, 20.);
    doc.mutate()
        .set_attribute(input, qual_name!("type"), "checkbox");
    assert!(doc.mutate().set_input_indeterminate(input, true));
    doc.mutate().clear_attribute(input, qual_name!("type"));
    doc.resolve(0.);
    assert!(
        !doc.get_node(input)
            .unwrap()
            .element_data()
            .unwrap()
            .element_state
            .contains(ElementState::INDETERMINATE)
    );
    assert!(
        !doc.mutate()
            .set_input_indeterminate(blitz_traits::node_id::NodeId::from_u64(u64::MAX), true)
    );
}

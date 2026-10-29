use crate::{Attribute, BaseDocument, DocumentConfig, NodeId, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

fn attribute(name: &str, value: &str) -> Attribute {
    Attribute {
        name: crate::QualName::new(
            None,
            crate::Namespace::from(""),
            crate::LocalName::from(name),
        ),
        value: value.into(),
    }
}
fn fixture() -> (BaseDocument, [NodeId; 8]) {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
        "label{display:block;width:20px;height:20px}input:checked+label{width:80px}",
    ));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let first = m.create_element(qual_name!("form", html), vec![attribute("id", "first")]);
    let second = m.create_element(qual_name!("form", html), vec![attribute("id", "second")]);
    let mut radio = |name: &str| {
        m.create_element(
            qual_name!("input", html),
            vec![attribute("type", "radio"), attribute("name", name)],
        )
    };
    let a = radio("group");
    let b = radio("group");
    let c = radio("group");
    let empty = radio("");
    let label = m.create_element(qual_name!("label", html), vec![]);
    let check = m.create_element(
        qual_name!("input", html),
        vec![attribute("type", "checkbox"), attribute("name", "group")],
    );
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[first, second, empty, check]);
    m.append_children(first, &[a, label, b]);
    m.append_children(second, &[c]);
    assert!(m.set_input_checked(check, true));
    drop(m);
    doc.resolve(0.);
    (doc, [first, second, a, b, c, empty, check, label])
}
fn checked(doc: &BaseDocument, id: NodeId) -> bool {
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .checkbox_input_checked()
        .unwrap()
}

#[test]
fn bulk_insertion_keeps_the_last_initially_checked_radio_and_empty_names_do_not_group() {
    let (mut doc, [first, _, _, _, _, empty, _, _]) = fixture();
    let mut m = doc.mutate();
    let attrs = vec![
        attribute("type", "radio"),
        attribute("name", "bulk"),
        attribute("checked", ""),
    ];
    let a = m.create_element(qual_name!("input", html), attrs.clone());
    let b = m.create_element(qual_name!("input", html), attrs);
    m.append_children(first, &[a, b]);
    let independent = m.create_element(
        qual_name!("input", html),
        vec![attribute("type", "radio"), attribute("name", "")],
    );
    m.append_children(first, &[independent]);
    m.set_input_checked(independent, true);
    m.set_input_checked(empty, true);
    drop(m);
    doc.resolve(0.);
    assert!(!checked(&doc, a));
    assert!(checked(&doc, b));
    assert!(checked(&doc, independent));
    assert!(checked(&doc, empty));
}

#[test]
fn radio_property_and_activation_are_one_way_form_scoped_and_do_not_toggle_checkboxes() {
    let (mut doc, [_, _, a, b, c, empty, check, label]) = fixture();
    assert!(doc.set_input_checked(a, true));
    assert!(doc.set_input_checked(c, true));
    assert_eq!(doc.activate_radio(b), Some(true));
    doc.resolve(0.);
    assert!(!checked(&doc, a));
    assert!(checked(&doc, b));
    assert!(checked(&doc, c));
    assert!(checked(&doc, check));
    assert_eq!(doc.get_client_bounding_rect(label).unwrap().width, 20.);
    assert_eq!(doc.activate_radio(b), Some(false));
    assert_eq!(doc.activate_radio(check), None);
    assert!(doc.set_input_checked(a, true));
    doc.resolve(0.);
    assert!(checked(&doc, a));
    assert!(!checked(&doc, b));
    assert_eq!(doc.get_client_bounding_rect(label).unwrap().width, 80.);
    assert!(doc.set_input_checked(empty, true));
    assert_eq!(doc.radio_group(empty), vec![empty]);
    assert!(!doc.set_input_checked(NodeId::from_u64(u64::MAX), true));
}

#[test]
fn radio_association_follows_reparenting_name_type_and_explicit_form_without_old_group_state() {
    let (mut doc, [first, second, a, b, c, _, check, _]) = fixture();
    doc.set_input_checked(a, true);
    doc.set_input_checked(c, true);
    doc.mutate().append_children(second, &[a]);
    doc.resolve(0.);
    assert!(checked(&doc, a));
    assert!(!checked(&doc, c));
    assert!(checked(&doc, check));
    doc.set_input_checked(b, true);
    assert!(checked(&doc, a));
    doc.mutate().set_attribute(a, qual_name!("form"), "first");
    doc.resolve(0.);
    assert!(!checked(&doc, b));
    doc.set_input_checked(b, true);
    assert!(!checked(&doc, a));
    doc.set_input_checked(a, true);
    doc.mutate().set_attribute(a, qual_name!("name"), "other");
    doc.set_input_checked(b, true);
    assert!(checked(&doc, a));
    doc.mutate().set_attribute(a, qual_name!("name"), "group");
    assert!(!checked(&doc, b));
    doc.mutate()
        .set_attribute(a, qual_name!("type"), "checkbox");
    doc.set_input_checked(b, true);
    assert!(checked(&doc, a));
    doc.mutate().set_attribute(a, qual_name!("type"), "radio");
    assert!(!checked(&doc, b));
    doc.mutate().set_attribute(a, qual_name!("form"), "missing");
    doc.set_input_checked(c, true);
    assert!(checked(&doc, a));
    assert!(checked(&doc, c));
    doc.mutate().clear_attribute(a, qual_name!("form"));
    assert!(!checked(&doc, c));
    doc.mutate().remove_node(a);
    doc.set_input_checked(c, true);
    assert!(checked(&doc, a));
    doc.mutate().append_children(first, &[a]);
    assert!(!checked(&doc, b));
    assert!(checked(&doc, c));
    doc.mutate().remove_and_drop_node(a);
    assert_eq!(doc.activate_radio(a), None);
    assert!(!doc.set_input_checked(a, true));
}

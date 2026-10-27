use crate::{Attribute, BaseDocument, DocumentConfig, NodeId, UserAgentStylesheet, qual_name};
use blitz_traits::shell::Viewport;

fn classes(value: &str) -> Vec<Attribute> {
    vec![Attribute {
        name: qual_name!("class"),
        value: value.into(),
    }]
}
fn has_set(doc: &BaseDocument, id: NodeId) -> bool {
    doc.animations
        .sets
        .read()
        .iter()
        .any(|(key, set)| NodeId::from_u64(key.node.id() as u64) == id && !set.is_empty())
}
#[test]
fn display_none_ancestor_retires_only_its_animations_and_transitions_and_show_restarts_animation() {
    for incremental in [false, true] {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1., Default::default())),
            incremental: Some(incremental),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(
            "html,body{margin:0;padding:0}.hidden{display:none!important}\
             @keyframes slide{from{transform:translateX(0px)}to{transform:translateX(100px)}}\
             .animated{width:20px;height:20px;animation:slide 1000ms linear infinite}\
             .transition{height:0;min-height:80px;max-height:80px;transition:min-height 500ms linear,max-height 500ms linear}\
             .end{min-height:0;max-height:0}"
        ));
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html", html), vec![]);
        let body = m.create_element(qual_name!("body", html), vec![]);
        let owner = m.create_element(qual_name!("div", html), vec![]);
        let nested = m.create_element(qual_name!("div", html), vec![]);
        let animated = m.create_element(qual_name!("div", html), classes("animated"));
        let transition = m.create_element(qual_name!("div", html), classes("transition"));
        let unrelated = m.create_element(qual_name!("div", html), classes("animated"));
        m.append_children(root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[owner, unrelated]);
        m.append_children(owner, &[nested]);
        m.append_children(nested, &[animated, transition]);
        drop(m);
        doc.resolve(0.);
        doc.mutate()
            .set_attribute(transition, qual_name!("class"), "transition end");
        doc.resolve(0.1);
        assert!(has_set(&doc, animated) && has_set(&doc, transition) && has_set(&doc, unrelated));
        doc.mutate()
            .set_attribute(owner, qual_name!("class"), "hidden");
        doc.resolve(0.2);
        assert!(
            !has_set(&doc, animated),
            "hidden descendant retains infinite animation"
        );
        assert!(
            !has_set(&doc, transition),
            "hidden descendant retains transition"
        );
        assert!(has_set(&doc, unrelated) && doc.is_animating());
        doc.mutate()
            .set_attribute(unrelated, qual_name!("class"), "hidden");
        doc.resolve(0.3);
        assert!(!doc.is_animating());
        doc.mutate().set_attribute(owner, qual_name!("class"), "");
        doc.resolve(0.4);
        assert!(
            has_set(&doc, animated) && doc.is_animating(),
            "shown animation must restart"
        );
        assert!(doc.animations.sets.read().iter().any(|(key, set)| {
            NodeId::from_u64(key.node.id() as u64) == animated
                && set
                    .animations
                    .iter()
                    .any(|animation| animation.started_at == 0.4)
        }));
        doc.mutate().set_attribute(
            owner,
            qual_name!("style"),
            "display:contents;visibility:hidden",
        );
        doc.resolve(0.5);
        assert!(
            has_set(&doc, animated) && doc.is_animating(),
            "visibility-hidden and display-contents must not cancel animation"
        );
        doc.mutate().remove_and_drop_node(owner);
        doc.resolve(0.6);
        assert!(!doc.is_animating());
    }
}

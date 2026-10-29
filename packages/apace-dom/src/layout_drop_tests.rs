use crate::{Attribute, BaseDocument, DocumentConfig, NodeId, UserAgentStylesheet, qual_name};
use apace_traits::shell::Viewport;

fn class(value: &str) -> Vec<Attribute> {
    vec![Attribute {
        name: qual_name!("class"),
        value: value.into(),
    }]
}

struct Fixture {
    doc: BaseDocument,
    outer: NodeId,
    owner: NodeId,
    host: NodeId,
    control: NodeId,
}

fn fixture(incremental: bool, display: &str, positioned: bool) -> Fixture {
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(400, 300, 1., Default::default())),
        incremental: Some(incremental),
        ..Default::default()
    });
    doc.add_user_agent_stylesheet(UserAgentStylesheet::parse(&format!(
        "html,body{{margin:0;padding:0}}\
         [hidden]{{display:none!important}}\
         .outer{{display:flex;flex-direction:column;position:relative;width:300px}}\
         .owner{{display:{display};font:16px/20px monospace}}\
         .host{{font:16px/20px monospace}}\
         input{{width:80px;height:24px;padding:0;border:0;box-sizing:border-box}}\
         .positioned{{position:absolute;left:5px;top:8px;z-index:7}}"
    )));
    let root = doc.root_node().id;
    let mut m = doc.mutate();
    let html = m.create_element(qual_name!("html", html), vec![]);
    let body = m.create_element(qual_name!("body", html), vec![]);
    let outer = m.create_element(qual_name!("div", html), class("outer"));
    let owner = m.create_element(qual_name!("div", html), class("owner"));
    let host = m.create_element(qual_name!("span", html), class("host"));
    let control = m.create_element(
        qual_name!("input", html),
        if positioned {
            class("positioned")
        } else {
            vec![]
        },
    );
    let before = m.create_comment_node("group start");
    let after = m.create_comment_node("group end");
    let prefix = m.create_text_node("prefix ");
    let suffix = m.create_element(qual_name!("div", html), vec![]);
    let suffix_text = m.create_text_node("block sibling");
    m.append_children(root, &[html]);
    m.append_children(html, &[body]);
    m.append_children(body, &[outer]);
    m.append_children(outer, &[owner]);
    m.append_children(owner, &[before, prefix, host, suffix, after]);
    m.append_children(host, &[control]);
    m.append_children(suffix, &[suffix_text]);
    drop(m);
    doc.resolve(0.);
    let rect = doc.get_client_bounding_rect(control).unwrap();
    assert!((rect.width - 80.).abs() < 0.01);
    assert!((rect.height - 24.).abs() < 0.01);
    Fixture {
        doc,
        outer,
        owner,
        host,
        control,
    }
}

fn assert_live_structural_caches(doc: &BaseDocument) {
    for (id, node) in doc.nodes.iter() {
        for child in node.layout_children.borrow().iter().flatten() {
            assert!(
                doc.nodes.contains_key(*child),
                "{id:?} keeps dead layout child {child:?}"
            );
        }
        for child in node.hoisted_children.borrow().iter() {
            assert!(
                doc.nodes.contains_key(*child),
                "{id:?} keeps dead out-of-flow child {child:?}"
            );
        }
        for child in &node.anonymous_blocks {
            assert!(
                doc.nodes.contains_key(*child),
                "{id:?} keeps dead anonymous block {child:?}"
            );
        }
    }
}

fn assert_live_paint_and_inline_caches(doc: &BaseDocument) {
    assert_live_structural_caches(doc);
    for (id, node) in doc.nodes.iter() {
        for child in node.paint_children.borrow().iter().flatten() {
            assert!(
                doc.nodes.contains_key(*child),
                "{id:?} keeps dead paint child {child:?}"
            );
        }
        for child in node.sc_contribution_cache.borrow().iter() {
            assert!(
                doc.nodes.contains_key(child.node_id),
                "{id:?} keeps dead stacking contribution {:?}",
                child.node_id
            );
        }
        if let Some(context) = &node.stacking_context {
            for child in &context.children {
                assert!(
                    doc.nodes.contains_key(child.node_id),
                    "{id:?} keeps dead stacking child {:?}",
                    child.node_id
                );
            }
        }
        if let Some(layout) = node
            .element_data()
            .and_then(|data| data.inline_layout_data.as_ref())
        {
            for line in layout.layout.lines() {
                for item in line.items() {
                    if let parley::layout::PositionedLayoutItem::InlineBox(ibox) = item {
                        let child = NodeId::from_u64(ibox.id);
                        assert!(
                            doc.nodes.contains_key(child),
                            "{id:?} keeps dead embedded inline box {child:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn root_first_drop_of_anonymous_box_owner_does_not_walk_removed_dom_ancestors() {
    for incremental in [true, false] {
        for hidden in [false, true] {
            let Fixture {
                mut doc,
                outer,
                owner,
                control,
                ..
            } = fixture(incremental, "block", false);
            assert!(
                !doc.nodes[owner].anonymous_blocks.is_empty(),
                "mixed inline/block fixture must own an anonymous box"
            );
            if hidden {
                doc.mutate().set_attribute(outer, qual_name!("hidden"), "");
                doc.resolve(0.);
            }
            doc.mutate().remove_and_drop_node(owner);
            assert!(doc.get_node(control).is_none());
            doc.resolve(0.);
            assert_live_paint_and_inline_caches(&doc);
            doc.mutate().clear_attribute(outer, qual_name!("hidden"));
            doc.resolve(0.);
            assert_live_paint_and_inline_caches(&doc);
        }
    }
}

#[test]
fn hidden_ancestor_drop_then_show_and_reuse_preserves_live_inline_and_layout_references() {
    for incremental in [true, false] {
        for display in ["block", "flex", "grid", "contents"] {
            for resolve_hidden_first in [false, true] {
                for remove_all in [false, true] {
                    let Fixture {
                        mut doc,
                        outer,
                        host,
                        control,
                        ..
                    } = fixture(incremental, display, false);
                    doc.mutate().set_attribute(outer, qual_name!("hidden"), "");
                    if resolve_hidden_first {
                        doc.resolve(0.);
                    }
                    if remove_all {
                        doc.mutate().remove_and_drop_all_children(host);
                    } else {
                        doc.mutate().remove_and_drop_node(control);
                    }
                    doc.resolve(0.);
                    assert!(doc.get_node(control).is_none());
                    assert_live_structural_caches(&doc);
                    // Showing the old container before inserting a replacement must
                    // not replay a cached Parley inline box or stacking entry.
                    doc.mutate().clear_attribute(outer, qual_name!("hidden"));
                    doc.resolve(0.);
                    assert_live_paint_and_inline_caches(&doc);
                    let mut m = doc.mutate();
                    let replacement = m.create_element(qual_name!("input", html), vec![]);
                    m.append_children(host, &[replacement]);
                    drop(m);
                    assert_ne!(replacement, control);
                    doc.resolve(0.);
                    let rect = doc.get_client_bounding_rect(replacement).unwrap();
                    assert!((rect.width - 80.).abs() < 0.01 && (rect.height - 24.).abs() < 0.01);
                    assert_live_paint_and_inline_caches(&doc);
                }
            }
        }
    }
}

#[test]
fn hidden_hoisted_z_indexed_control_drop_retires_out_of_flow_and_stacking_caches() {
    for incremental in [true, false] {
        let Fixture {
            mut doc,
            outer,
            host,
            control,
            ..
        } = fixture(incremental, "block", true);
        assert_eq!(doc.nodes[control].oof_containing_block.get(), Some(outer));
        assert_ne!(doc.nodes[control].layout_parent.get(), Some(outer));
        doc.mutate().set_attribute(outer, qual_name!("hidden"), "");
        doc.resolve(0.);
        doc.mutate().remove_and_drop_all_children(host);
        doc.resolve(0.);
        assert_live_structural_caches(&doc);
        doc.mutate().clear_attribute(outer, qual_name!("hidden"));
        doc.resolve(0.);
        assert_live_paint_and_inline_caches(&doc);
        let mut m = doc.mutate();
        let replacement = m.create_element(qual_name!("input", html), class("positioned"));
        m.append_children(host, &[replacement]);
        drop(m);
        assert_ne!(replacement, control);
        doc.resolve(0.);
        assert_eq!(
            doc.nodes[replacement].oof_containing_block.get(),
            Some(outer)
        );
        let rect = doc.get_client_bounding_rect(replacement).unwrap();
        assert!((rect.width - 80.).abs() < 0.01 && (rect.height - 24.).abs() < 0.01);
        assert_live_paint_and_inline_caches(&doc);
    }
}

#[test]
fn visible_nested_drop_rebuilds_ancestor_stacking_and_embedded_control_caches() {
    for incremental in [true, false] {
        for positioned in [true, false] {
            for remove_all in [false, true] {
                eprintln!(
                    "incremental={incremental} positioned={positioned} remove_all={remove_all}"
                );
                let Fixture {
                    mut doc,
                    host,
                    control,
                    ..
                } = fixture(incremental, "block", positioned);
                if remove_all {
                    doc.mutate().remove_and_drop_all_children(host);
                } else {
                    doc.mutate().remove_and_drop_node(control);
                }
                doc.resolve(0.);
                assert!(doc.get_node(control).is_none());
                assert_live_paint_and_inline_caches(&doc);
            }
        }
    }
}

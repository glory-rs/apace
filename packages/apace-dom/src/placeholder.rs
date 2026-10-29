//! Local placeholder state; no registry of inputs or copies of editable values.
use crate::{BaseDocument, local_name, node::NodeData, traversal::TreeTraverser};
use apace_traits::node_id::NodeId;
use style_dom::ElementState;

impl BaseDocument {
    pub(crate) fn sync_placeholder_state(&mut self, id: NodeId) {
        let Some(element) = self.get_node(id).and_then(|n| n.element_data()) else {
            return;
        };
        let multiline = element.name.local == local_name!("textarea");
        let eligible = multiline
            || element.name.local == local_name!("input")
                && matches!(
                    element.attr(local_name!("type")),
                    None | Some("text" | "password" | "email" | "tel" | "url" | "search")
                );
        let shown = eligible
            && if let Some(input) = element.text_input_data() {
                input.placeholder_shown()
            } else {
                element
                    .attr(local_name!("placeholder"))
                    .is_some_and(|s| s.len() <= 4096)
                    && if multiline {
                        !TreeTraverser::new_with_root(self, id).any(|child| {
                        matches!(&self.nodes[child].data, NodeData::Text(text) if !text.content.is_empty())
                    })
                    } else {
                        element.attr(local_name!("value")).is_none_or(str::is_empty)
                    }
            };
        if element
            .element_state
            .contains(ElementState::PLACEHOLDER_SHOWN)
            == shown
        {
            return;
        }
        self.snapshot_node_and(id, ElementState::PLACEHOLDER_SHOWN, |node| {
            node.element_data_mut()
                .unwrap()
                .element_state
                .set(ElementState::PLACEHOLDER_SHOWN, shown);
            node.mark_ancestors_dirty();
            node.mark_damaged();
        });
    }
}

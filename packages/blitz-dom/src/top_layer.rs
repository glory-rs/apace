//! Document-owned top layers retain DOM/style ancestry but establish independent
//! viewport layout roots. Hosts own opening, placement, focus and dismissal policy.
use crate::layout::damage::ALL_DAMAGE;
use crate::{BaseDocument, NodeId, node::NodeFlags, util::Color};
use style::invalidation::element::restyle_hints::RestyleHint;
use style_dom::ElementState;

pub const MAX_TOP_LAYERS: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TopLayerConfig {
    /// A modal layer catches hits on its backdrop instead of falling through.
    pub modal: bool,
    /// Stop unconsumed user scroll at this root, including editor/touch/fling
    /// chains, while retaining logical DOM ancestry for style and events.
    pub contain_scroll: bool,
    /// Optional viewport backdrop, painted immediately before this layer.
    pub backdrop: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopLayer {
    pub node_id: NodeId,
    pub config: TopLayerConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopLayerError {
    MissingNode,
    InvalidTarget,
    InvalidBackdrop,
    Capacity,
}

impl BaseDocument {
    pub fn top_layers(&self) -> &[TopLayer] {
        &self.top_layers
    }

    /// A top layer does not escape a display:none ancestor. Its lease remains
    /// current while hidden, but neither its box nor its backdrop participates.
    pub fn top_layer_is_rendered(&self, node_id: NodeId) -> bool {
        let mut cursor = Some(node_id);
        while let Some(id) = cursor {
            let Some(node) = self.nodes.get(id) else {
                return false;
            };
            if !node.flags.is_in_document() {
                return false;
            }
            if node.is_element() && node.primary_styles().is_none() {
                return false;
            }
            if node.display_style().is_some_and(|display| {
                display.inside() == style::values::specified::box_::DisplayInside::None
            }) {
                return false;
            }
            cursor = node.parent;
        }
        true
    }

    /// Opening appends once; updating an open entry preserves its order. Closing
    /// and reopening moves it to the front of the visual stack. Validation does
    /// not mutate the tree, including when admission exceeds the live budget.
    pub fn set_top_layer(
        &mut self,
        node_id: NodeId,
        config: Option<TopLayerConfig>,
    ) -> Result<bool, TopLayerError> {
        let node = self.nodes.get(node_id).ok_or(TopLayerError::MissingNode)?;
        if config
            .and_then(|c| c.backdrop)
            .is_some_and(|color| color.components.iter().any(|v| !v.is_finite()))
        {
            return Err(TopLayerError::InvalidBackdrop);
        }
        let index = self
            .top_layers
            .iter()
            .position(|entry| entry.node_id == node_id);
        if config.is_some()
            && (!node.is_element()
                || !node.flags.is_in_document()
                || self
                    .try_root_element()
                    .is_some_and(|root| root.id == node_id))
        {
            return Err(TopLayerError::InvalidTarget);
        }
        if config.is_some() && index.is_none() && self.top_layers.len() >= MAX_TOP_LAYERS {
            return Err(TopLayerError::Capacity);
        }
        if index.map(|i| self.top_layers[i].config) == config {
            return Ok(false);
        }
        match (index, config) {
            (Some(i), Some(config)) => self.top_layers[i].config = config,
            (Some(i), None) => {
                self.top_layers.remove(i);
            }
            (None, Some(config)) => self.top_layers.push(TopLayer { node_id, config }),
            (None, None) => return Ok(false),
        }
        self.update_top_layer_state(node_id, config);
        self.changed_nodes.insert(node_id);
        self.shell_provider.request_redraw();
        Ok(true)
    }

    pub(crate) fn retire_top_layer(&mut self, node_id: NodeId) {
        if let Some(index) = self
            .top_layers
            .iter()
            .position(|entry| entry.node_id == node_id)
        {
            self.top_layers.remove(index);
            self.update_top_layer_state(node_id, None);
        }
    }

    fn update_top_layer_state(&mut self, node_id: NodeId, config: Option<TopLayerConfig>) {
        let state = ElementState::POPOVER_OPEN | ElementState::MODAL;
        self.snapshot_node_and(node_id, state, |node| {
            node.flags.set(NodeFlags::IS_TOP_LAYER, config.is_some());
            node.flags.set(
                NodeFlags::CONTAIN_TOP_LAYER_SCROLL,
                config.is_some_and(|c| c.contain_scroll),
            );
            let element_state = node.element_state_mut();
            element_state.set(ElementState::POPOVER_OPEN, config.is_some());
            element_state.set(ElementState::MODAL, config.is_some_and(|c| c.modal));
            node.insert_damage(ALL_DAMAGE);
            node.set_restyle_hint(RestyleHint::RESTYLE_SELF | RestyleHint::RESTYLE_DESCENDANTS);
            node.mark_ancestors_dirty();
        });
        // Parent box construction must remove/reinsert the root without ever
        // changing its DOM parent or inherited style.
        if let Some(parent) = self.nodes[node_id].parent {
            self.nodes[parent].insert_damage(ALL_DAMAGE);
        }
        self.nodes[node_id].oof_containing_block.set(None);
        self.nodes[node_id].layout_parent.set(None);
    }
}

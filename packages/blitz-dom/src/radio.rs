//! Live radio association. No registry of retired controls or group history.
use crate::{BaseDocument, local_name, node::SpecialElementData, traversal::TreeTraverser};
use blitz_traits::node_id::NodeId;
use style_dom::ElementState;

impl BaseDocument {
    fn check_kind(&self, id: NodeId) -> Option<&str> {
        let element = self.get_node(id)?.element_data()?;
        if element.name.local != local_name!("input") {
            return None;
        }
        element
            .attr(local_name!("type"))
            .filter(|kind| matches!(*kind, "checkbox" | "radio"))
    }
    fn radio_root(&self, mut id: NodeId) -> Option<NodeId> {
        while let Some(parent) = self.get_node(id)?.parent {
            id = parent;
        }
        Some(id)
    }
    fn radio_form(&self, id: NodeId, root: NodeId) -> Option<NodeId> {
        let element = self.get_node(id)?.element_data()?;
        if let Some(name) = element.attr(local_name!("form")) {
            // An explicit unresolved form does not fall back to an ancestor.
            return TreeTraverser::new_with_root(self, root).find(|candidate| {
                self.get_node(*candidate)
                    .and_then(|n| n.element_data())
                    .is_some_and(|e| {
                        e.name.local == local_name!("form")
                            && e.attr(local_name!("id")) == Some(name)
                    })
            });
        }
        let mut cursor = self.get_node(id)?.parent;
        while let Some(id) = cursor {
            let node = self.get_node(id)?;
            if node.data.is_element_with_tag_name(&local_name!("form")) {
                return Some(id);
            }
            cursor = node.parent;
        }
        None
    }
    /// Members in current physical tree order, including disabled members. An
    /// empty/missing name is a singleton; tree and form owners isolate groups.
    /// Only the live host tree is traversed, never an application's data source.
    pub fn radio_group(&self, id: NodeId) -> Vec<NodeId> {
        if self.check_kind(id) != Some("radio") {
            return Vec::new();
        }
        let Some(name) = self
            .get_node(id)
            .and_then(|n| n.attr(local_name!("name")))
            .filter(|s| !s.is_empty())
        else {
            return vec![id];
        };
        let root = self.radio_root(id).unwrap();
        let form = self.radio_form(id, root);
        TreeTraverser::new_with_root(self, root)
            .filter(|peer| {
                self.check_kind(*peer) == Some("radio")
                    && self
                        .get_node(*peer)
                        .and_then(|n| n.attr(local_name!("name")))
                        == Some(name)
                    && self.radio_form(*peer, root) == form
            })
            .collect()
    }
    fn live_checked(&self, id: NodeId) -> bool {
        let element = self.get_node(id).unwrap().element_data().unwrap();
        element
            .checkbox_input_checked()
            .unwrap_or_else(|| element.element_state.contains(ElementState::CHECKED))
    }
    fn write_checked(&mut self, id: NodeId, checked: bool) -> bool {
        let before = self.live_checked(id);
        let initialized = self
            .get_node(id)
            .unwrap()
            .element_data()
            .unwrap()
            .checkbox_input_checked()
            .is_some();
        if before == checked && initialized {
            return false;
        }
        self.snapshot_node_and(id, ElementState::CHECKED, |node| {
            let element = node.element_data_mut().unwrap();
            // Property writes before initial layout must survive construction.
            if !matches!(element.special_data, SpecialElementData::CheckboxInput(_)) {
                element.special_data = SpecialElementData::CheckboxInput(checked);
            }
            element.set_checkbox_input_checked(checked);
            node.mark_ancestors_dirty();
            node.insert_damage(crate::layout::damage::ALL_DAMAGE);
        });
        true
    }
    /// The live checked property; does not rewrite the default checked attribute.
    /// Reasserting true on a radio also withdraws its peers without input events.
    pub fn set_input_checked(&mut self, id: NodeId, checked: bool) -> bool {
        let Some(kind) = self.check_kind(id) else {
            return false;
        };
        let peers = if kind == "radio" && checked {
            self.radio_group(id)
        } else {
            Vec::new()
        };
        let mut changed = self.write_checked(id, checked);
        for peer in peers {
            if peer != id && self.live_checked(peer) {
                changed |= self.write_checked(peer, false);
            }
        }
        if changed {
            self.shell_provider.request_redraw();
        }
        true
    }
    /// None is unsupported; false is an already-selected radio. Native input
    /// events belong only to the newly selected target, never its old peers.
    pub fn activate_radio(&mut self, id: NodeId) -> Option<bool> {
        if self.check_kind(id) != Some("radio") {
            return None;
        }
        let changed = !self.live_checked(id);
        self.set_input_checked(id, true);
        Some(changed)
    }
    pub(crate) fn reconcile_radio(&mut self, id: NodeId) {
        if self.check_kind(id) == Some("radio") && self.live_checked(id) {
            self.set_input_checked(id, true);
        }
    }
    pub(crate) fn reconcile_radio_subtree(&mut self, id: NodeId) {
        let radios: Vec<_> = TreeTraverser::new_with_root(self, id)
            .filter(|id| self.check_kind(*id) == Some("radio") && self.live_checked(*id))
            .collect();
        for id in radios {
            self.reconcile_radio(id);
        }
    }
}

use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;
use crate::style::Style;
use crate::style::resolution::ResolvedStyle;

impl Document {
    /// Set the style merged into a node's resolved style while it is being pressed.
    ///
    /// Active style merges on top of focus style, so a focused node that is being
    /// pressed shows both, with active winning on conflicting properties.
    pub fn set_active_style(&self, node: NodeId, style: &Style) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;
        lock::mutex(&self.inner.pseudo_styles)
            .entry(node)
            .or_default()
            .active = Some(style.clone());
        if self.active() == Some(node) {
            self.refresh_pseudo_style_effect(node)?;
        }
        Ok(())
    }

    /// Clear a node's active style.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn clear_active_style(&self, node: NodeId) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;
        self.clear_pseudo_style(node, |pseudo| pseudo.active = None);
        if self.active() == Some(node) {
            self.refresh_pseudo_style_effect(node)?;
        }
        Ok(())
    }

    /// The node currently being pressed, if any.
    pub fn active(&self) -> Option<NodeId> {
        *lock::mutex(&self.inner.active_node)
    }

    /// Mark a node as pressed, or clear the pressed node when `active` is false.
    ///
    /// The engine drives this from mouse down/up. Set it manually for activation the
    /// engine cannot see, such as pressing Enter on a button — tuidom has no button
    /// concept, so that policy belongs downstream.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn set_active(&self, node: NodeId, active: bool) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;
        if active && self.blocks_interaction(node) {
            return Ok(());
        }
        let target = if active { Some(node) } else { None };
        self.set_active_node(target)
    }

    /// Set or clear the active node, refreshing the resolved style of both the
    /// previously active node and the new one.
    pub(crate) fn set_active_node(&self, node: Option<NodeId>) -> Result<()> {
        let previous = {
            let mut active = lock::mutex(&self.inner.active_node);
            if *active == node {
                return Ok(());
            }
            let previous = *active;
            *active = node;
            previous
        };

        if let Some(previous) = previous {
            self.refresh_pseudo_style_effect(previous)?;
        }
        if let Some(node) = node {
            self.refresh_pseudo_style_effect(node)?;
        }
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Set the style merged into a node's resolved style while it is effectively disabled.
    ///
    /// A node merges its own disabled style whenever it or an ancestor is disabled, so
    /// disabling a container restyles descendants that define a disabled style and
    /// leaves the rest untouched.
    pub fn set_disabled_style(&self, node: NodeId, style: &Style) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;
        lock::mutex(&self.inner.pseudo_styles)
            .entry(node)
            .or_default()
            .disabled = Some(style.clone());
        if self.is_effectively_disabled_unlocked(node) {
            self.refresh_pseudo_style_effect(node)?;
        }
        Ok(())
    }

    /// Clear a node's disabled style.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn clear_disabled_style(&self, node: NodeId) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;
        self.clear_pseudo_style(node, |pseudo| pseudo.disabled = None);
        if self.is_effectively_disabled_unlocked(node) {
            self.refresh_pseudo_style_effect(node)?;
        }
        Ok(())
    }

    /// Whether this node is itself marked disabled, ignoring its ancestors.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn is_disabled(&self, node: NodeId) -> Result<bool> {
        self.ensure_pseudo_node_exists(node)?;
        Ok(lock::mutex(&self.inner.disabled_nodes).contains(&node))
    }

    /// Whether this node is disabled, either directly or through an ancestor.
    ///
    /// Effectively disabled nodes cannot be focused, are skipped by tab and spatial
    /// navigation, and swallow targeted events rather than bubbling them to enabled
    /// ancestors.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn is_effectively_disabled(&self, node: NodeId) -> Result<bool> {
        self.ensure_pseudo_node_exists(node)?;
        Ok(self.is_effectively_disabled_unlocked(node))
    }

    /// Disable or re-enable a node and, with it, its whole subtree.
    ///
    /// Disabling blurs the node or a focused descendant and clears a pressed node inside
    /// the subtree, so a disabled node never retains focus or an active state.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn set_disabled(&self, node: NodeId, disabled: bool) -> Result<()> {
        self.ensure_pseudo_node_exists(node)?;

        // Effective disabled state changes for the whole subtree, so any descendant
        // with transition configs may be about to restyle. Snapshot their merged
        // styles before the flip, so the diff sees the pre-change values even where
        // the cache is cold.
        let snapshots = self.transition_style_snapshots(node);

        let changed = {
            let mut disabled_nodes = lock::mutex(&self.inner.disabled_nodes);
            if disabled {
                disabled_nodes.insert(node)
            } else {
                disabled_nodes.remove(&node)
            }
        };
        if !changed {
            return Ok(());
        }

        if disabled {
            if let Some(focused) = self.focused()
                && self.is_self_or_descendant(node, focused)
            {
                self.blur();
            }
            if let Some(active) = self.active()
                && self.is_self_or_descendant(node, active)
            {
                self.set_active_node(None)?;
            }
        }

        // Effective disabled state changed for the whole subtree, so every descendant may
        // now merge or drop its disabled style.
        self.invalidate_resolved_style(node);
        self.sync_layout_subtree_styles(node)?;
        for (id, old_resolved) in snapshots {
            self.signal_animation(id, &old_resolved)?;
        }
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Merged resolved styles for every node in the subtree that has transition
    /// configs — the nodes whose restyle could start a transition.
    fn transition_style_snapshots(&self, node: NodeId) -> Vec<(NodeId, ResolvedStyle)> {
        let mut snapshots = Vec::new();
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            let Some(data) = self.inner.nodes.get(&id) else {
                continue;
            };
            let has_configs = !data.transition_configs.is_empty();
            stack.extend(data.children.iter().copied());
            drop(data);
            if has_configs && let Ok(resolved) = self.resolved_base_style(id) {
                snapshots.push((id, resolved));
            }
        }
        snapshots
    }

    pub(crate) fn is_effectively_disabled_unlocked(&self, node: NodeId) -> bool {
        let disabled_nodes = lock::mutex(&self.inner.disabled_nodes);
        if disabled_nodes.is_empty() {
            return false;
        }

        let mut current = Some(node);
        while let Some(id) = current {
            if disabled_nodes.contains(&id) {
                return true;
            }
            current = self.get_parent_unlocked(id);
        }
        false
    }

    /// Whether `candidate` is `ancestor` itself or sits below it in the tree.
    ///
    /// Reads parents without taking the tree lock, so this stays callable from paths that
    /// already hold it — and cannot invert lock order against the focus context stack.
    pub(super) fn is_self_or_descendant(&self, ancestor: NodeId, candidate: NodeId) -> bool {
        let mut current = Some(candidate);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.get_parent_unlocked(id);
        }
        false
    }

    pub(super) fn refresh_pseudo_style_effect(&self, node: NodeId) -> Result<()> {
        if !self.inner.nodes.contains_key(&node) {
            return Ok(());
        }
        // The cache still holds the pre-change merged style; capturing it before
        // invalidating lets the animation driver diff the pseudo-state change like
        // any other style change.
        let old_resolved = self.resolved_base_style(node)?;
        self.invalidate_resolved_style(node);
        self.sync_layout_subtree_styles(node)?;
        self.signal_animation(node, &old_resolved)?;
        self.inner.notify.notify_one();
        Ok(())
    }

    fn ensure_pseudo_node_exists(&self, node: NodeId) -> Result<()> {
        if self.inner.nodes.contains_key(&node) {
            Ok(())
        } else {
            Err(TuidomError::NodeNotFound { id: node })
        }
    }
}

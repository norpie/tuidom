use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;
use crate::style::Style;

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

    pub(super) fn refresh_pseudo_style_effect(&self, node: NodeId) -> Result<()> {
        if !self.inner.nodes.contains_key(&node) {
            return Ok(());
        }
        self.invalidate_resolved_style(node);
        self.sync_layout_subtree_styles(node)?;
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

use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::event::FocusKeys;
use crate::id::NodeId;
use crate::lock;

impl Document {
    /// Set whether a node can receive focus.
    ///
    /// If focusability is removed from the currently focused node, focus is
    /// blurred and blur listeners are dispatched.
    pub fn set_focusable(&self, node: NodeId, focusable: bool) -> Result<()> {
        self.ensure_focus_node_exists(node)?;

        if focusable {
            lock::mutex(&self.inner.focusable_nodes).insert(node);
        } else {
            lock::mutex(&self.inner.focusable_nodes).remove(&node);
            if self.focused() == Some(node) {
                self.blur();
            }
        }

        Ok(())
    }

    /// Return whether a node can receive focus.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn is_focusable(&self, node: NodeId) -> Result<bool> {
        self.ensure_focus_node_exists(node)?;
        Ok(lock::mutex(&self.inner.focusable_nodes).contains(&node))
    }

    /// Move focus to a focusable node.
    ///
    /// Dispatches blur listeners for the previously focused node, followed by
    /// focus listeners for `node`. Calling this for the already-focused node is
    /// a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document or is not focusable.
    pub fn focus(&self, node: NodeId) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        if !lock::mutex(&self.inner.focusable_nodes).contains(&node) {
            return Err(TuidomError::NodeNotFocusable { id: node });
        }

        let previous = {
            let mut focused = lock::mutex(&self.inner.focused_node);
            if *focused == Some(node) {
                return Ok(());
            }

            let previous = *focused;
            *focused = Some(node);
            previous
        };

        if let Some(previous) = previous {
            self.dispatch_blur_to(previous);
        }
        self.dispatch_focus_to(node);
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Clear the current focus, if any.
    ///
    /// Dispatches blur listeners for the previously focused node.
    pub fn blur(&self) {
        let previous = lock::mutex(&self.inner.focused_node).take();
        if let Some(previous) = previous {
            self.dispatch_blur_to(previous);
            self.inner.notify.notify_one();
        }
    }

    /// Return the currently focused node, if one exists.
    pub fn focused(&self) -> Option<NodeId> {
        *lock::mutex(&self.inner.focused_node)
    }

    /// Replace the document-level focus key bindings.
    pub fn set_focus_keys(&self, keys: FocusKeys) {
        *lock::mutex(&self.inner.focus_keys) = keys;
    }

    /// Return the document-level focus key bindings.
    pub fn focus_keys(&self) -> FocusKeys {
        lock::mutex(&self.inner.focus_keys).clone()
    }

    pub(super) fn remove_focus_side_state(&self, node: NodeId) {
        lock::mutex(&self.inner.focusable_nodes).remove(&node);
        let removed_focus = {
            let mut focused = lock::mutex(&self.inner.focused_node);
            if *focused == Some(node) {
                *focused = None;
                true
            } else {
                false
            }
        };

        if removed_focus {
            self.inner.notify.notify_one();
        }
    }

    fn ensure_focus_node_exists(&self, node: NodeId) -> Result<()> {
        if self.inner.nodes.contains_key(&node) {
            Ok(())
        } else {
            Err(TuidomError::NodeNotFound { id: node })
        }
    }
}

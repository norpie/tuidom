use std::collections::HashSet;

use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;

impl Document {
    /// Append a child to the end of `parent`'s children list.
    ///
    /// If `child` already has a parent, it is detached from that parent first.
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` or `child` does not exist, or if the
    /// operation would create a cycle.
    pub fn append_child(&self, parent: NodeId, child: NodeId) -> Result<()> {
        self.insert_child(parent, child, None)
    }

    /// Insert `child` into `parent`'s children list before `before_sibling`.
    ///
    /// If `child` already has a parent, it is detached from that parent first.
    /// If `before_sibling` is not found in `parent`'s children, the child is
    /// appended at the end.
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` or `child` does not exist, or if the
    /// operation would create a cycle.
    pub fn insert_before(
        &self,
        parent: NodeId,
        child: NodeId,
        before_sibling: NodeId,
    ) -> Result<()> {
        self.insert_child(parent, child, Some(before_sibling))
    }

    /// Remove `child` from `parent` and delete the entire subtree rooted at
    /// `child` from the arena.
    ///
    /// Does nothing if `child` is not actually a child of `parent`.
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` or `child` does not exist.
    pub fn remove_child(&self, parent: NodeId, child: NodeId) -> Result<()> {
        let tree_guard = lock::rw_write(&self.inner.tree_mutation);
        self.ensure_node_exists(parent)?;
        self.ensure_node_exists(child)?;
        if child == self.root() {
            return Err(TuidomError::CannotRemoveRoot { id: child });
        }

        let parent_contains_child = self
            .inner
            .nodes
            .get(&parent)
            .is_some_and(|node| node.children.contains(&child));
        let child_points_to_parent = self
            .inner
            .nodes
            .get(&child)
            .is_some_and(|node| node.parent == Some(parent));

        if !(parent_contains_child && child_points_to_parent) {
            return Ok(());
        }

        if let Some(mut parent_data) = self.inner.nodes.get_mut(&parent) {
            parent_data.children.retain(|&c| c != child);
        } else {
            return Err(TuidomError::NodeNotFound { id: parent });
        }

        self.remove_subtree(child)?;
        let parent_still_exists = self.inner.nodes.contains_key(&parent);
        drop(tree_guard);

        // Focus handlers may touch the tree, so contexts settle only once the tree lock is
        // released.
        self.settle_focus_contexts();

        if parent_still_exists {
            self.sync_layout_children(parent)?;
        }
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Move `child` from its current parent to `new_parent`, inserting it
    /// before `before_sibling` in the new parent's children list.
    ///
    /// If `before_sibling` is not found in `new_parent`'s children, the child
    /// is appended at the end.
    ///
    /// # Errors
    ///
    /// Returns an error if `new_parent` or `child` does not exist, or if the
    /// operation would create a cycle.
    pub fn move_child(
        &self,
        new_parent: NodeId,
        child: NodeId,
        before_sibling: NodeId,
    ) -> Result<()> {
        self.insert_child(new_parent, child, Some(before_sibling))
    }

    fn insert_child(
        &self,
        parent: NodeId,
        child: NodeId,
        before_sibling: Option<NodeId>,
    ) -> Result<()> {
        let tree_guard = lock::rw_write(&self.inner.tree_mutation);
        self.validate_reparent(parent, child)?;

        let old_parent = self.detach_from_current_parent(child);
        self.insert_child_reference(parent, child, before_sibling)?;
        self.set_parent(child, parent)?;

        drop(tree_guard);

        if let Some(old_parent) = old_parent {
            self.sync_layout_children(old_parent)?;
        }
        self.sync_layout_children(parent)?;
        self.invalidate_resolved_style(child);
        self.sync_layout_subtree_styles(child)?;
        self.inner.notify.notify_one();
        Ok(())
    }

    fn validate_reparent(&self, parent: NodeId, child: NodeId) -> Result<()> {
        self.ensure_node_exists(parent)?;
        self.ensure_node_exists(child)?;

        if child == self.root() {
            return Err(TuidomError::CannotReparentRoot { id: child });
        }

        if parent == child || self.is_descendant_of_unlocked(parent, child) {
            return Err(TuidomError::TreeCycle { parent, child });
        }

        Ok(())
    }

    fn ensure_node_exists(&self, id: NodeId) -> Result<()> {
        if self.inner.nodes.contains_key(&id) {
            Ok(())
        } else {
            Err(TuidomError::NodeNotFound { id })
        }
    }

    fn detach_from_current_parent(&self, child: NodeId) -> Option<NodeId> {
        let old_parent = self.get_parent_unlocked(child);
        if let Some(old_parent) = old_parent
            && let Some(mut old_parent_data) = self.inner.nodes.get_mut(&old_parent)
        {
            old_parent_data.children.retain(|&c| c != child);
        }
        old_parent
    }

    fn insert_child_reference(
        &self,
        parent: NodeId,
        child: NodeId,
        before_sibling: Option<NodeId>,
    ) -> Result<()> {
        let Some(mut parent_data) = self.inner.nodes.get_mut(&parent) else {
            return Err(TuidomError::NodeNotFound { id: parent });
        };

        parent_data.children.retain(|&c| c != child);

        if let Some(before_sibling) = before_sibling
            && let Some(pos) = parent_data
                .children
                .iter()
                .position(|&c| c == before_sibling)
        {
            parent_data.children.insert(pos, child);
        } else {
            parent_data.children.push(child);
        }

        Ok(())
    }

    fn set_parent(&self, child: NodeId, parent: NodeId) -> Result<()> {
        let Some(mut child_data) = self.inner.nodes.get_mut(&child) else {
            return Err(TuidomError::NodeNotFound { id: child });
        };

        child_data.parent = Some(parent);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Tree queries
    // ------------------------------------------------------------------

    /// Get the parent of a node, if any.
    pub fn get_parent(&self, id: NodeId) -> Option<NodeId> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.get_parent_unlocked(id)
    }

    pub(super) fn get_parent_unlocked(&self, id: NodeId) -> Option<NodeId> {
        self.inner.nodes.get(&id).and_then(|r| r.parent)
    }

    /// Get the children of a node.
    ///
    /// Returns an empty vector if the node does not exist.
    pub fn get_children(&self, id: NodeId) -> Vec<NodeId> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.get_children_unlocked(id)
    }

    pub(crate) fn get_children_unlocked(&self, id: NodeId) -> Vec<NodeId> {
        self.inner
            .nodes
            .get(&id)
            .map(|r| r.children.clone())
            .unwrap_or_default()
    }

    /// Check whether `id` is a descendant of `ancestor`.
    pub fn is_descendant_of(&self, id: NodeId, ancestor: NodeId) -> bool {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.is_descendant_of_unlocked(id, ancestor)
    }

    fn is_descendant_of_unlocked(&self, id: NodeId, ancestor: NodeId) -> bool {
        if id == ancestor {
            return false;
        }

        let mut seen = HashSet::new();
        let mut current = id;
        while let Some(parent) = self.get_parent_unlocked(current) {
            if parent == ancestor {
                return true;
            }
            if !seen.insert(parent) {
                return false;
            }
            current = parent;
        }

        false
    }

    /// Remove a node and its entire subtree from the arena.
    fn remove_subtree(&self, id: NodeId) -> Result<()> {
        if id == self.root() {
            return Ok(());
        }

        let children = self.get_children_unlocked(id);
        for child in children {
            self.remove_subtree(child)?;
        }

        self.remove_node_side_state(id)?;
        self.inner.nodes.remove(&id);
        Ok(())
    }
}

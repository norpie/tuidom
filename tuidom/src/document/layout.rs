use crate::document::Document;
use crate::document::scroll::scrollable_max;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::layout::compute_layout as compute_document_layout;
use crate::lock;
use crate::node::{LayoutView, NodeLayout, NodeView};
use crate::paint_order::entry_at;
use crate::style::resolution::ResolvedStyle;

#[cfg(test)]
use taffy::prelude::NodeId as TaffyNodeId;

impl Document {
    /// Compute layout for all nodes in the DOM tree.
    ///
    /// Resolves styles, builds a taffy layout tree, computes positions and
    /// sizes, and stores the results on each node. Nodes with `display: None`
    /// are skipped.
    pub fn compute_layout(&self, screen_width: u16, screen_height: u16) -> Result<()> {
        self.sync_animating_layout_styles();
        compute_document_layout(self, screen_width, screen_height)
    }

    /// Feed the interpolated style of every layout-animating node to the layout
    /// engine, so this layout pass sees the in-flight values.
    ///
    /// The synced base styles only change on explicit style updates; a transition
    /// on a layout-affecting property moves between them, so each frame's values
    /// are pushed here. With no such transition active this is a no-op, and
    /// layout stays as passive as before.
    fn sync_animating_layout_styles(&self) {
        let animating = lock::mutex(&self.inner.animation).layout_animating_nodes();
        for id in animating {
            let Ok(resolved) = self.resolved_style(id) else {
                continue;
            };
            let _ = lock::mutex(&self.inner.layout).set_style(id, &resolved);
        }
    }

    // ------------------------------------------------------------------
    // Node inspection
    // ------------------------------------------------------------------

    /// Get a read-only snapshot of a node's public state.
    ///
    /// Returns `None` if the node does not exist.
    pub fn get_node(&self, id: NodeId) -> Option<NodeView> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.get_node_unlocked(id)
    }

    /// Get the topmost node at the given screen coordinate.
    ///
    /// Uses the latest committed layout snapshot. If layout has not been
    /// computed yet, or no visible node contains the coordinate, returns
    /// `None`.
    pub fn node_at(&self, x: i32, y: i32) -> Option<NodeId> {
        entry_at(self, x, y).map(|entry| entry.id)
    }

    fn get_node_unlocked(&self, id: NodeId) -> Option<NodeView> {
        let layout = lock::rw_read(&self.inner.layout_snapshot)
            .get(&id)
            .copied()
            .and_then(|layout| self.layout_view(id, layout));
        self.inner.nodes.get(&id).map(|r| NodeView {
            id,
            kind: r.kind.to_view(self.now()),
            parent: r.parent,
            children: r.children.clone(),
            layout,
            attrs: r.attrs.clone(),
        })
    }

    /// Build the public view of one snapshot entry.
    ///
    /// The scrollport needs the node's border and max scroll its overflow, both from
    /// the resolved style; a node whose style cannot resolve reports no layout.
    fn layout_view(&self, id: NodeId, layout: NodeLayout) -> Option<LayoutView> {
        let resolved = self.resolved_style_unlocked(id).ok()?;
        Some(LayoutView {
            rect: layout.rect,
            scrollport: layout.rect.padding_box(&resolved),
            max_scroll_x: scrollable_max(resolved.overflow_x, layout.max_scroll_x),
            max_scroll_y: scrollable_max(resolved.overflow_y, layout.max_scroll_y),
        })
    }

    // ------------------------------------------------------------------
    // Layout engine synchronization
    // ------------------------------------------------------------------

    pub(super) fn register_layout_node(&self, id: NodeId) -> Result<()> {
        let resolved = self.resolved_base_style(id)?;
        let Some(kind) = self.inner.nodes.get(&id).map(|data| data.kind.clone()) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        lock::mutex(&self.inner.layout).insert_node(id, &kind, &resolved)
    }

    fn remove_layout_node(&self, id: NodeId) -> Result<()> {
        lock::mutex(&self.inner.layout).remove_node(id)?;
        lock::rw_write(&self.inner.layout_snapshot).remove(&id);
        Ok(())
    }

    pub(super) fn remove_node_side_state(&self, id: NodeId) -> Result<()> {
        self.remove_layout_node(id)?;
        self.remove_focus_side_state(id);
        lock::mutex(&self.inner.targeted_listeners).retain(|(node, _), _| *node != id);
        lock::mutex(&self.inner.animation).remove_node(id);
        lock::mutex(&self.inner.scroll_offsets).remove(&id);
        Ok(())
    }

    pub(super) fn sync_layout_children(&self, parent: NodeId) -> Result<()> {
        let children = self.get_children(parent);
        lock::mutex(&self.inner.layout).sync_children(parent, &children)
    }

    pub(super) fn sync_layout_subtree_styles(&self, id: NodeId) -> Result<()> {
        let mut updates = Vec::new();
        self.collect_layout_style_updates(id, &mut updates)?;

        let mut layout = lock::mutex(&self.inner.layout);
        for (node_id, resolved) in updates {
            layout.set_style(node_id, &resolved)?;
        }
        Ok(())
    }

    fn collect_layout_style_updates(
        &self,
        id: NodeId,
        updates: &mut Vec<(NodeId, ResolvedStyle)>,
    ) -> Result<()> {
        let resolved = self.resolved_base_style(id)?;
        updates.push((id, resolved));

        for child in self.get_children(id) {
            self.collect_layout_style_updates(child, updates)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn layout_node_count(&self) -> usize {
        lock::mutex(&self.inner.layout).mapped_node_count()
    }

    #[cfg(test)]
    pub(super) fn layout_mapping_snapshot(&self) -> Vec<(NodeId, TaffyNodeId)> {
        lock::mutex(&self.inner.layout).mapping_snapshot()
    }

    #[cfg(test)]
    pub(super) fn layout_children(&self, parent: NodeId) -> Vec<NodeId> {
        lock::mutex(&self.inner.layout).dom_children(parent)
    }

    #[cfg(test)]
    pub(super) fn remove_layout_mapping_for_test(&self, id: NodeId) {
        lock::mutex(&self.inner.layout).remove_node(id).unwrap();
    }
}


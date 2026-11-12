use std::sync::Arc;

use crate::document::Document;
use crate::document::scroll::scrollable_max;
use crate::error::{Result, TuidomError};
use crate::event::ScrollEvent;
use crate::id::NodeId;
use crate::layout::compute_layout as compute_document_layout;
use crate::lock;
use crate::node::{LayoutRect, LayoutView, NodeLayout, NodeView};
use crate::paint_order::{DrawContext, entry_at};
use crate::runtime_event::RuntimeEvent;
use crate::style::resolution::ResolvedStyle;
use crate::style::{Display, Overflow};

#[cfg(test)]
use taffy::prelude::NodeId as TaffyNodeId;

impl Document {
    /// Compute layout for all nodes in the DOM tree.
    ///
    /// Resolves styles, builds a taffy layout tree, computes positions and
    /// sizes, and stores the results on each node. Nodes with `display: None`
    /// are skipped.
    ///
    /// A scroll offset the new layout re-clamps is reported to its `on_scroll` listeners, and a
    /// changed content size to its `on_size_change` listeners, before this returns.
    pub fn compute_layout(&self, screen_width: u16, screen_height: u16) -> Result<()> {
        for event in self.compute_layout_for_frame(screen_width, screen_height)? {
            match event {
                RuntimeEvent::ScrollClamped { node, offset } => {
                    self.dispatch_scroll_to(node, &mut ScrollEvent::new(offset.x, offset.y));
                }
                RuntimeEvent::SizeChange {
                    node,
                    width,
                    height,
                } => self.dispatch_size_change_to(node, width, height),
                _ => {}
            }
        }
        Ok(())
    }

    /// Compute layout for a frame, returning the scroll and size change events it owes instead
    /// of dispatching them, since a frame is laid out on the render task, which never runs user
    /// code.
    pub(crate) fn compute_layout_for_frame(
        &self,
        screen_width: u16,
        screen_height: u16,
    ) -> Result<Vec<RuntimeEvent>> {
        self.sync_animating_layout_styles();
        self.sync_cell_pixels()?;
        let reclamped = compute_document_layout(self, screen_width, screen_height)?;
        let mut events: Vec<RuntimeEvent> = reclamped
            .into_iter()
            .map(|(node, offset)| RuntimeEvent::ScrollClamped { node, offset })
            .collect();
        events.extend(self.size_changes());
        Ok(events)
    }

    fn sync_cell_pixels(&self) -> Result<()> {
        lock::mutex(&self.inner.layout).set_cell_pixels(self.cell_pixels())
    }

    /// Compare each listened node's content size against the one last reported for it.
    ///
    /// Runs even when layout skipped its pass, so a listener added since the last frame gets
    /// its first report. A node outside the snapshot measures zero by zero.
    fn size_changes(&self) -> Vec<RuntimeEvent> {
        let reported: Vec<(NodeId, Option<(u16, u16)>)> = lock::mutex(&self.inner.reported_sizes)
            .iter()
            .map(|(node, size)| (*node, *size))
            .collect();
        if reported.is_empty() {
            return Vec::new();
        }
        let rects: Vec<Option<LayoutRect>> = {
            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            reported
                .iter()
                .map(|(node, _)| snapshot.get(node).map(|layout| layout.rect))
                .collect()
        };

        let mut changed = Vec::new();
        for ((node, last), rect) in reported.into_iter().zip(rects) {
            let size = rect
                .and_then(|rect| {
                    let resolved = self.resolved_style_arc(node).ok()?;
                    let content = rect.content_rect(&resolved);
                    Some((content.width, content.height))
                })
                .unwrap_or((0, 0));
            if last != Some(size) {
                changed.push((node, size));
            }
        }

        let mut reported = lock::mutex(&self.inner.reported_sizes);
        changed
            .into_iter()
            .filter_map(|(node, (width, height))| {
                // A listener removed while this ran leaves nothing to report to.
                let entry = reported.get_mut(&node)?;
                *entry = Some((width, height));
                Some(RuntimeEvent::SizeChange {
                    node,
                    width,
                    height,
                })
            })
            .collect()
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

    /// Where a node is drawn: its layout rect shifted by every scrolling ancestor's offset.
    ///
    /// [`LayoutView::rect`] is where layout placed the node, and scrolling never reruns layout,
    /// so inside a scrolled container that rect stays put while the node moves on screen. This
    /// applies the shift paint does: each ancestor moves its descendants by its scroll offset on
    /// every axis whose overflow is [`Overflow::Scroll`]. It reports position only, so a node
    /// scrolled out of its container's viewport gets a rect outside that viewport.
    ///
    /// Returns `None` if the node does not exist or has not been laid out yet.
    pub fn screen_rect(&self, id: NodeId) -> Option<LayoutRect> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        let mut rect = lock::rw_read(&self.inner.layout_snapshot).get(&id)?.rect;
        let mut ancestor = self.inner.nodes.get(&id)?.parent;
        while let Some(node) = ancestor {
            let resolved = self.resolved_style_unlocked(node).ok()?;
            let offset = self.scroll_offset(node);
            if resolved.overflow_x == Overflow::Scroll {
                rect.x -= i32::from(offset.x);
            }
            if resolved.overflow_y == Overflow::Scroll {
                rect.y -= i32::from(offset.y);
            }
            ancestor = self.inner.nodes.get(&node)?.parent;
        }
        Some(rect)
    }

    /// The part of a node that reaches the screen: its [`screen_rect`](Self::screen_rect)
    /// clipped the way paint clips it.
    ///
    /// Each ancestor whose overflow on an axis is [`Overflow::Scroll`] or [`Overflow::Clip`]
    /// bounds its descendants to its padding box on that axis, and the terminal bounds
    /// everything. A node equals its screen rect when fully visible, and is smaller when
    /// partly clipped.
    ///
    /// Returns `None` when no cell of the node is drawn: it is clipped or scrolled out
    /// entirely, off the terminal, zero-sized, inside a `display: None` subtree, or has an
    /// opacity of zero, itself or on an ancestor. Also `None` when the node does not exist or
    /// has not been laid out. Nodes painted over it do not count, so a node under a modal is
    /// still visible.
    pub fn visible_rect(&self, id: NodeId) -> Option<LayoutRect> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        let (width, height) = lock::mutex(&self.inner.layout).screen_size()?;

        let mut chain = vec![id];
        let mut ancestor = self.inner.nodes.get(&id)?.parent;
        while let Some(node) = ancestor {
            chain.push(node);
            ancestor = self.inner.nodes.get(&node)?.parent;
        }
        let rects = {
            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            chain
                .iter()
                .map(|node| snapshot.get(node).map(|layout| layout.rect))
                .collect::<Option<Vec<_>>>()?
        };

        let mut context = DrawContext::screen(width, height);
        for (&node, rect) in chain.iter().zip(rects).rev() {
            let resolved = self.resolved_style_unlocked(node).ok()?;
            if resolved.display == Display::None || resolved.opacity <= 0.0 {
                return None;
            }
            let drawn = context.place(rect);
            if node == id {
                return context.clip.visible_part(drawn);
            }
            context = context.children_of(drawn, &resolved, || self.scroll_offset(node));
        }
        None
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
    pub(super) fn layout_view(&self, id: NodeId, layout: NodeLayout) -> Option<LayoutView> {
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
        lock::mutex(&self.inner.reported_sizes).remove(&id);
        lock::mutex(&self.inner.half_block_images).remove(&id);
        lock::mutex(&self.inner.image_ids).0.remove(&id);
        lock::mutex(&self.inner.scroll_offsets).remove(&id);
        lock::mutex(&self.inner.scroll_activity).remove(&id);
        {
            let mut grab = lock::mutex(&self.inner.scrollbar_grab);
            if *grab == Some(id) {
                *grab = None;
            }
        }
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
        updates: &mut Vec<(NodeId, Arc<ResolvedStyle>)>,
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

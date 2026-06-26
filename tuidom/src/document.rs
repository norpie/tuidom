//! The [`Document`] type — the public API surface for tuidom.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::Notify;

use crate::animation::TransitionConfig;
use crate::animation::driver::AnimationDriver;
use crate::debug::DebugOverlay;
use crate::error::{Result, TuidomError};
use crate::event::{Event, Listener, ListenerHandle};
use crate::id::{NodeId, next_document_id};
use crate::inner::DocumentInner;
use crate::lock;
use crate::node::{NodeData, NodeView};
use crate::style::Style;
use crate::style::resolution::{ResolvedStyle, StyleDefaults};

/// The root container and public API surface for tuidom.
///
/// Wraps an `Arc<DocumentInner>` for cheap cloning. All methods take `&self`
/// and use interior mutability — the document is `Send + Sync` and can be
/// shared across threads. Every document owns a permanent root node that acts
/// as the layout, rendering, and runtime-event entry point.
///
/// # Example
///
/// ```ignore
/// let doc = Document::new()?;
/// let container = doc.create_box()?;
/// doc.append_child(doc.root(), container)?;
/// // ... build tree, register handlers, then:
/// doc.run().await;
/// ```
#[derive(Clone)]
pub struct Document {
    pub(crate) inner: Arc<DocumentInner>,
}

impl Document {
    /// Create a new document with a permanent root node.
    pub fn new() -> Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let document_id = next_document_id();
        let root = NodeId::scoped(document_id, 0);
        let nodes = dashmap::DashMap::new();
        nodes.insert(root, NodeData::box_node());

        let document = Self {
            inner: Arc::new(DocumentInner {
                nodes,
                document_id,
                next_id: std::sync::atomic::AtomicU64::new(1),
                next_listener_id: std::sync::atomic::AtomicU64::new(0),
                root,
                tree_mutation: RwLock::new(()),
                notify: tokio::sync::Notify::new(),
                shutdown: std::sync::RwLock::new(false),
                event_tx,
                event_rx: tokio::sync::Mutex::new(event_rx),
                pending_resize: Mutex::new(None),
                resize_notify: Notify::new(),
                shutdown_notify: Arc::new(Notify::new()),
                animation: Arc::new(Mutex::new(AnimationDriver::new())),
                anim_config_changed: Arc::new(Notify::new()),
                max_frame_interval: std::sync::RwLock::new(None),
                layout: Mutex::new(crate::layout::LayoutEngine::new()),
                layout_rects: RwLock::new(std::collections::HashMap::new()),
                debug_overlay: Mutex::new(DebugOverlay::new()),
                listeners: Mutex::new(Vec::new()),
            }),
        };
        document.register_layout_node(root)?;
        Ok(document)
    }

    // ------------------------------------------------------------------
    // Node creation
    // ------------------------------------------------------------------

    /// Create a new box (generic container) node.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_box(&self) -> Result<NodeId> {
        let id = self.inner.alloc(NodeData::box_node());
        if let Err(err) = self.register_layout_node(id) {
            self.inner.nodes.remove(&id);
            return Err(err);
        }
        Ok(id)
    }

    /// Create a new text node with the given content.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_text(&self, content: impl Into<String>) -> Result<NodeId> {
        let id = self.inner.alloc(NodeData::text(content));
        if let Err(err) = self.register_layout_node(id) {
            self.inner.nodes.remove(&id);
            return Err(err);
        }
        Ok(id)
    }

    // ------------------------------------------------------------------
    // Root
    // ------------------------------------------------------------------

    /// Get the permanent document root node.
    ///
    /// The root is created by [`Document::new`], always exists, cannot be
    /// reparented or removed, and is the entry point for layout, rendering, and
    /// runtime events.
    pub fn root(&self) -> NodeId {
        self.inner.root
    }

    /// Trigger shutdown of the render loop.
    pub fn quit(&self) {
        *lock::rw_write(&self.inner.shutdown) = true;
        self.inner.notify.notify_waiters();
        self.inner.resize_notify.notify_waiters();
        self.inner.anim_config_changed.notify_waiters();
        self.inner.shutdown_notify.notify_waiters();
    }

    /// Toggle the debug overlay on/off.
    pub fn toggle_debug_overlay(&self) {
        let mut overlay = lock::mutex(&self.inner.debug_overlay);
        overlay.enabled = !overlay.enabled;
        self.inner.notify.notify_one();
    }

    /// Register an event listener on a node.
    ///
    /// The handler is called synchronously when an event is dispatched to this
    /// node. Until focus and hit-testing are implemented, runtime events target
    /// the document root. For async work, spawn a task inside the handler.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        if !self.inner.nodes.contains_key(&node) {
            return Err(TuidomError::NodeNotFound { id: node });
        }

        let id = self
            .inner
            .next_listener_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let listener = Listener {
            id,
            node,
            handler: Arc::new(handler),
        };

        lock::mutex(&self.inner.listeners).push(listener);
        Ok(ListenerHandle::new(self.inner.document_id, id))
    }

    /// Remove an event listener.
    ///
    /// Returns `true` if a listener was removed, or `false` if the handle was
    /// unknown or had already been removed.
    pub fn remove_listener(&self, handle: ListenerHandle) -> bool {
        let mut listeners = lock::mutex(&self.inner.listeners);
        let old_len = listeners.len();
        listeners.retain(|listener| {
            listener.id != handle.id || handle.document_id != self.inner.document_id
        });
        listeners.len() != old_len
    }

    /// Dispatch an event to the document root-targeted event path.
    pub(crate) fn dispatch_event(&self, event: Event) {
        self.dispatch_event_to(self.root(), event);
    }

    fn dispatch_event_to(&self, target: NodeId, event: Event) {
        let listeners = lock::mutex(&self.inner.listeners).clone();

        for listener in listeners
            .into_iter()
            .filter(|listener| listener.node == target)
        {
            let result = catch_unwind(AssertUnwindSafe(|| {
                (listener.handler)(&event);
            }));

            if result.is_err() {
                log::error!("event listener {} panicked", listener.id);
            }
        }
    }

    /// Record rendering metrics for the debug overlay.
    pub(crate) fn record_frame_metrics(
        &self,
        frame: std::time::Duration,
        layout: std::time::Duration,
        stats: crate::render::RenderStats,
    ) {
        let mut overlay = lock::mutex(&self.inner.debug_overlay);
        overlay.record(frame, layout, stats);
    }

    /// Run the render + event loop until [`quit`](Self::quit) is called.
    ///
    /// Consumes the document. Clone it first if you need to keep a handle
    /// for event handlers or other tasks.
    pub async fn run(self) -> std::io::Result<()> {
        crate::event_loop::run(self).await
    }

    // ------------------------------------------------------------------
    // Transitions
    // ------------------------------------------------------------------

    /// Set a transition configuration for a node.
    ///
    /// When the given property changes (via [`update_style`] or [`set_style`]),
    /// the engine will animate the change over the specified duration and easing.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn set_transition(&self, id: NodeId, config: TransitionConfig) -> Result<()> {
        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };

        data.transition_configs.insert(config.property, config);
        Ok(())
    }

    /// Set a document-wide maximum frame rate.
    ///
    /// `None` disables the cap, which is the default. Non-finite or non-positive
    /// values are treated as `None`.
    pub fn set_max_fps(&self, fps: Option<f64>) {
        let interval = fps
            .filter(|fps| fps.is_finite() && *fps > 0.0)
            .and_then(|fps| Duration::try_from_secs_f64(1.0 / fps).ok());
        *lock::rw_write(&self.inner.max_frame_interval) = interval;
        self.inner.notify.notify_one();
    }

    // ------------------------------------------------------------------
    // Style
    // ------------------------------------------------------------------

    /// Set the inline style for a node.
    ///
    /// This replaces any previously set style, invalidates the resolved
    /// style cache, and signals the animation driver if any transitionable
    /// properties changed.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn set_style(&self, id: NodeId, style: &Style) -> Result<()> {
        let old_resolved = self.resolved_base_style(id)?;

        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        data.style = style.clone();
        drop(data);

        self.invalidate_resolved_style(id);
        self.sync_layout_subtree_styles(id)?;
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved)
    }

    /// Update a node's style in-place via a closure.
    ///
    /// Invalidates the resolved style cache, triggers a re-render, and signals
    /// the animation driver if any transitionable properties changed.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn update_style(&self, id: NodeId, f: impl FnOnce(&mut Style)) -> Result<()> {
        // Capture old resolved values before the mutation
        let old_resolved = self.resolved_base_style(id)?;

        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        let mut style = data.style.clone();
        let result = catch_unwind(AssertUnwindSafe(|| f(&mut style)));
        if let Err(payload) = result {
            log::error!("style update callback panicked for {id:?}");
            resume_unwind(payload);
        }
        data.style = style;
        drop(data);

        self.invalidate_resolved_style(id);
        self.sync_layout_subtree_styles(id)?;
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved)
    }

    /// Get the fully resolved style for a node, including animation overrides.
    ///
    /// Returns the cached value if available, otherwise computes it by
    /// applying explicit values, explicit inheritance, and document defaults.
    ///
    /// During active animations, property values are overridden with the
    /// interpolated animation value.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn resolved_style(&self, id: NodeId) -> Result<ResolvedStyle> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_style_unlocked(id)
    }

    pub(crate) fn resolved_style_unlocked(&self, id: NodeId) -> Result<ResolvedStyle> {
        let mut resolved = self.resolved_base_style_unlocked(id)?;

        // Apply animation overrides
        {
            let driver = lock::mutex(&self.inner.animation);
            for (prop, val) in driver.overrides_for(id) {
                match prop {
                    crate::animation::TransitionProperty::Opacity => resolved.opacity = val,
                }
            }
        }

        Ok(resolved)
    }

    /// Get the base resolved style without animation overrides.
    ///
    /// Used internally by the animation driver to read target values.
    pub(crate) fn resolved_base_style(&self, id: NodeId) -> Result<ResolvedStyle> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_base_style_unlocked(id)
    }

    pub(crate) fn resolved_base_style_unlocked(&self, id: NodeId) -> Result<ResolvedStyle> {
        // Check cache
        {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            if let Some(resolved) = &*lock::rw_read(&node.resolved_style) {
                return Ok(resolved.clone());
            }
        }

        // Cache miss — compute
        let parent = self.get_parent_unlocked(id);
        let parent_resolved = parent
            .map(|pid| self.resolved_base_style_unlocked(pid))
            .transpose()?;

        let Some(node) = self.inner.nodes.get(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        let resolved = if id == self.root() {
            ResolvedStyle::compute_with_defaults(
                &node,
                parent_resolved.as_ref(),
                &StyleDefaults::root(),
            )
        } else {
            ResolvedStyle::compute(&node, parent_resolved.as_ref())
        };

        *lock::rw_write(&node.resolved_style) = Some(resolved.clone());
        Ok(resolved)
    }

    /// Signal the animation driver about a style change and spawn tick task if needed.
    fn signal_animation(&self, id: NodeId, old_resolved: &ResolvedStyle) -> Result<()> {
        // Read transition configs before locking the driver
        let configs = {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            node.transition_configs.clone()
        };

        // Compute the new resolved value BEFORE locking the driver
        let new_resolved = self.resolved_base_style(id)?;

        let mut driver = lock::mutex(&self.inner.animation);
        driver.style_changed(id, old_resolved, &new_resolved, &configs);
        drop(driver);

        self.inner.anim_config_changed.notify_one();

        Ok(())
    }

    /// Invalidate the resolved style cache for a node and all descendants.
    pub(crate) fn invalidate_resolved_style(&self, id: NodeId) {
        if let Some(node) = self.inner.nodes.get(&id) {
            *lock::rw_write(&node.resolved_style) = None;
            let children = node.children.clone();
            drop(node); // release lock before recursing
            for child in children {
                self.invalidate_resolved_style(child);
            }
        }
    }

    // ------------------------------------------------------------------
    // Tree mutation
    // ------------------------------------------------------------------

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

    fn get_parent_unlocked(&self, id: NodeId) -> Option<NodeId> {
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

    // ------------------------------------------------------------------
    // Layout
    // ------------------------------------------------------------------

    /// Compute layout for all nodes in the DOM tree.
    ///
    /// Resolves styles, builds a taffy layout tree, computes positions and
    /// sizes, and stores the results on each node. Nodes with `display: None`
    /// are skipped.
    pub fn compute_layout(&self, screen_width: u16, screen_height: u16) -> Result<()> {
        crate::layout::compute_layout(self, screen_width, screen_height)
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

    fn get_node_unlocked(&self, id: NodeId) -> Option<NodeView> {
        let layout = lock::rw_read(&self.inner.layout_rects).get(&id).copied();
        self.inner.nodes.get(&id).map(|r| NodeView {
            id,
            kind: r.kind.to_view(),
            parent: r.parent,
            children: r.children.clone(),
            layout,
            attrs: r.attrs.clone(),
        })
    }

    // ------------------------------------------------------------------
    // Layout engine synchronization
    // ------------------------------------------------------------------

    fn register_layout_node(&self, id: NodeId) -> Result<()> {
        let resolved = self.resolved_base_style(id)?;
        let Some(kind) = self.inner.nodes.get(&id).map(|data| data.kind.clone()) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        lock::mutex(&self.inner.layout).insert_node(id, &kind, &resolved)
    }

    fn remove_layout_node(&self, id: NodeId) -> Result<()> {
        lock::mutex(&self.inner.layout).remove_node(id)?;
        lock::rw_write(&self.inner.layout_rects).remove(&id);
        Ok(())
    }

    fn remove_node_side_state(&self, id: NodeId) -> Result<()> {
        self.remove_layout_node(id)?;
        lock::mutex(&self.inner.listeners).retain(|listener| listener.node != id);
        lock::mutex(&self.inner.animation).remove_node(id);
        Ok(())
    }

    fn sync_layout_children(&self, parent: NodeId) -> Result<()> {
        let children = self.get_children(parent);
        lock::mutex(&self.inner.layout).sync_children(parent, &children)
    }

    fn sync_layout_subtree_styles(&self, id: NodeId) -> Result<()> {
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
    fn layout_node_count(&self) -> usize {
        lock::mutex(&self.inner.layout).mapped_node_count()
    }

    #[cfg(test)]
    fn layout_mapping_snapshot(&self) -> Vec<(NodeId, taffy::prelude::NodeId)> {
        lock::mutex(&self.inner.layout).mapping_snapshot()
    }

    #[cfg(test)]
    fn layout_children(&self, parent: NodeId) -> Vec<NodeId> {
        lock::mutex(&self.inner.layout).dom_children(parent)
    }

    #[cfg(test)]
    fn remove_layout_mapping_for_test(&self, id: NodeId) {
        lock::mutex(&self.inner.layout).remove_node(id).unwrap();
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::event::{Event, KeyCode, KeyEvent};
    use crate::style::{Color, Length};

    #[test]
    fn create_nodes() {
        let doc = Document::new().unwrap();
        let box_id = doc.create_box().unwrap();
        let text_id = doc.create_text("hello").unwrap();

        let box_view = doc.get_node(box_id).unwrap();
        let text_view = doc.get_node(text_id).unwrap();

        assert!(matches!(box_view.kind, crate::node::NodeKindView::Box));
        assert!(matches!(
            text_view.kind,
            crate::node::NodeKindView::Text { .. }
        ));

        assert!(doc.get_node(NodeId::new(999)).is_none());
    }

    #[test]
    fn node_ids_are_scoped_to_their_document() {
        let first = Document::new().unwrap();
        let second = Document::new().unwrap();
        let first_root = first.root();
        let second_root = second.root();

        assert_ne!(first_root, second_root);
        assert!(second.get_node(first_root).is_none());

        let mut style = Style::new();
        style.width(Length::Pixels(1));
        assert_eq!(
            second.set_style(first_root, &style),
            Err(TuidomError::NodeNotFound { id: first_root })
        );
        assert!(second.get_node(second_root).is_some());
    }

    #[test]
    fn creating_dom_nodes_creates_persistent_layout_nodes() {
        let doc = Document::new().unwrap();
        let root = doc.create_box().unwrap();
        let text = doc.create_text("hello").unwrap();

        assert_eq!(doc.layout_node_count(), 3);
        assert_eq!(doc.layout_mapping_snapshot().len(), 3);
        assert!(
            doc.layout_mapping_snapshot()
                .iter()
                .any(|(id, _)| *id == root)
        );
        assert!(
            doc.layout_mapping_snapshot()
                .iter()
                .any(|(id, _)| *id == text)
        );
    }

    #[test]
    fn repeated_layout_uses_same_taffy_nodes() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_text("hello").unwrap();
        doc.append_child(root, child).unwrap();

        let before = doc.layout_mapping_snapshot();
        doc.compute_layout(20, 5).unwrap();
        doc.compute_layout(20, 5).unwrap();
        let after = doc.layout_mapping_snapshot();

        assert_eq!(before, after);
    }

    #[test]
    fn failed_layout_preserves_previous_snapshot() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_box().unwrap();

        let mut child_style = Style::new();
        child_style.width(Length::Pixels(7));
        child_style.height(Length::Pixels(1));
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(root, child).unwrap();
        doc.compute_layout(20, 5).unwrap();

        let before = doc.get_node(child).unwrap().layout.unwrap();

        doc.remove_layout_mapping_for_test(child);

        assert_eq!(
            doc.compute_layout(20, 5),
            Err(TuidomError::LayoutMappingMissing { id: child })
        );
        let after = doc.get_node(child).unwrap().layout.unwrap();
        assert_eq!(after.x, before.x);
        assert_eq!(after.y, before.y);
        assert_eq!(after.width, before.width);
        assert_eq!(after.height, before.height);
    }

    #[test]
    fn reparenting_syncs_taffy_child_order() {
        let doc = Document::new().unwrap();
        let first_parent = doc.create_box().unwrap();
        let second_parent = doc.create_box().unwrap();
        let first = doc.create_text("first").unwrap();
        let second = doc.create_text("second").unwrap();
        let third = doc.create_text("third").unwrap();

        doc.append_child(first_parent, first).unwrap();
        doc.append_child(first_parent, second).unwrap();
        doc.insert_before(first_parent, third, second).unwrap();
        assert_eq!(
            doc.layout_children(first_parent),
            vec![first, third, second]
        );

        doc.move_child(second_parent, third, first).unwrap();
        assert_eq!(doc.layout_children(first_parent), vec![first, second]);
        assert_eq!(doc.layout_children(second_parent), vec![third]);
    }

    #[test]
    fn inherited_style_change_updates_layout_without_recreating_taffy_nodes() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_box().unwrap();

        let mut root_style = Style::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        let mut child_style = Style::new();
        child_style.inherit_width();
        child_style.height(Length::Pixels(1));
        doc.set_style(child, &child_style).unwrap();

        doc.append_child(root, child).unwrap();
        let before = doc.layout_mapping_snapshot();

        doc.compute_layout(100, 10).unwrap();
        assert_eq!(doc.get_node(child).unwrap().layout.unwrap().width, 10);

        doc.update_style(root, |style| style.width(Length::Pixels(20)))
            .unwrap();
        doc.compute_layout(100, 10).unwrap();

        assert_eq!(doc.layout_mapping_snapshot(), before);
        assert_eq!(doc.get_node(child).unwrap().layout.unwrap().width, 20);
    }

    #[test]
    fn removing_subtree_removes_layout_nodes() {
        let doc = Document::new().unwrap();
        let root = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        let grandchild = doc.create_text("deep").unwrap();

        doc.append_child(root, child).unwrap();
        doc.append_child(child, grandchild).unwrap();
        assert_eq!(doc.layout_node_count(), 4);

        doc.remove_child(root, child).unwrap();

        assert_eq!(doc.layout_node_count(), 2);
        assert_eq!(doc.layout_children(root), Vec::<NodeId>::new());
    }

    fn key_event() -> Event {
        Event::KeyPress(KeyEvent {
            code: KeyCode::Char('x'),
        })
    }

    #[test]
    fn max_fps_defaults_to_uncapped_and_can_be_configured() {
        let doc = Document::new().unwrap();

        assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());

        doc.set_max_fps(Some(120.0));
        assert_eq!(
            *lock::rw_read(&doc.inner.max_frame_interval),
            Some(Duration::try_from_secs_f64(1.0 / 120.0).unwrap())
        );

        doc.set_max_fps(None);
        assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());
    }

    #[test]
    fn invalid_max_fps_values_disable_the_cap() {
        let doc = Document::new().unwrap();

        for fps in [
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::MIN_POSITIVE),
        ] {
            doc.set_max_fps(fps);
            assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());
        }
    }

    #[test]
    fn listener_handle_removes_registered_listener() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();

        let handle = doc
            .on(root, move |_| {
                calls_for_handler.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        assert!(doc.remove_listener(handle));
        assert!(!doc.remove_listener(handle));

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn listener_handles_are_scoped_to_their_document() {
        let first = Document::new().unwrap();
        let second = Document::new().unwrap();
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));

        let first_calls_for_handler = first_calls.clone();
        let first_handle = first
            .on(first.root(), move |_| {
                first_calls_for_handler.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();

        let second_calls_for_handler = second_calls.clone();
        second
            .on(second.root(), move |_| {
                second_calls_for_handler.fetch_add(1, Ordering::Relaxed);
            })
            .unwrap();

        assert!(!second.remove_listener(first_handle));

        first.dispatch_event(key_event());
        second.dispatch_event(key_event());
        assert_eq!(first_calls.load(Ordering::Relaxed), 1);
        assert_eq!(second_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn listener_can_register_listener_during_dispatch() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let calls = Arc::new(AtomicUsize::new(0));
        let doc_for_handler = doc.clone();
        let calls_for_handler = calls.clone();

        doc.on(root, move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
            let calls_for_new_handler = calls_for_handler.clone();
            doc_for_handler
                .on(root, move |_| {
                    calls_for_new_handler.fetch_add(10, Ordering::Relaxed);
                })
                .unwrap();
        })
        .unwrap();

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn listener_panic_is_caught_and_later_listeners_still_run() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();

        doc.on(root, |_| panic!("listener boom")).unwrap();
        doc.on(root, move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_targets_root_listeners_only_for_now() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_box().unwrap();
        doc.append_child(root, child).unwrap();

        let root_calls = Arc::new(AtomicUsize::new(0));
        let child_calls = Arc::new(AtomicUsize::new(0));

        let root_calls_for_handler = root_calls.clone();
        doc.on(root, move |_| {
            root_calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        let child_calls_for_handler = child_calls.clone();
        doc.on(child, move |_| {
            child_calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        doc.dispatch_event(key_event());

        assert_eq!(root_calls.load(Ordering::Relaxed), 1);
        assert_eq!(child_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn registering_listener_on_missing_node_returns_error() {
        let doc = Document::new().unwrap();
        let result = doc.on(NodeId::new(999), |_| {});
        assert!(matches!(result, Err(TuidomError::NodeNotFound { .. })));
    }

    #[test]
    fn tree_ops() {
        let doc = Document::new().unwrap();

        let root = doc.create_box().unwrap();
        let child1 = doc.create_text("one").unwrap();
        let child2 = doc.create_text("two").unwrap();
        let child3 = doc.create_text("three").unwrap();

        // append
        doc.append_child(root, child1).unwrap();
        doc.append_child(root, child2).unwrap();
        assert_eq!(doc.get_children(root), vec![child1, child2]);

        // insert_before
        doc.insert_before(root, child3, child2).unwrap();
        assert_eq!(doc.get_children(root), vec![child1, child3, child2]);

        // move_child
        let other = doc.create_box().unwrap();
        doc.move_child(other, child3, child2).unwrap(); // inserts at end since child2 isn't in other
        assert_eq!(doc.get_children(root), vec![child1, child2]);
        assert_eq!(doc.get_children(other), vec![child3]);

        assert_eq!(doc.get_parent(child3), Some(other));
    }

    #[test]
    fn append_child_reparents_without_stale_reference() {
        let doc = Document::new().unwrap();
        let first_parent = doc.create_box().unwrap();
        let second_parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();

        doc.append_child(first_parent, child).unwrap();
        doc.append_child(second_parent, child).unwrap();

        assert!(doc.get_children(first_parent).is_empty());
        assert_eq!(doc.get_children(second_parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(second_parent));
    }

    #[test]
    fn append_child_does_not_duplicate_existing_child() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();

        doc.append_child(parent, child).unwrap();
        doc.append_child(parent, child).unwrap();

        assert_eq!(doc.get_children(parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(parent));
    }

    #[test]
    fn insert_before_reorders_existing_child_without_duplicate() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let first = doc.create_text("first").unwrap();
        let second = doc.create_text("second").unwrap();
        let third = doc.create_text("third").unwrap();

        doc.append_child(parent, first).unwrap();
        doc.append_child(parent, second).unwrap();
        doc.append_child(parent, third).unwrap();
        doc.insert_before(parent, third, first).unwrap();

        assert_eq!(doc.get_children(parent), vec![third, first, second]);
        assert_eq!(doc.get_parent(third), Some(parent));
    }

    #[test]
    fn cycle_attempt_returns_error_and_does_not_mutate() {
        let doc = Document::new().unwrap();
        let ancestor = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        doc.append_child(ancestor, child).unwrap();

        let err = doc.append_child(child, ancestor).unwrap_err();
        assert_eq!(
            err,
            TuidomError::TreeCycle {
                parent: child,
                child: ancestor,
            }
        );
        assert_eq!(doc.get_children(ancestor), vec![child]);
        assert!(doc.get_children(child).is_empty());
        assert_eq!(doc.get_parent(ancestor), None);
        assert_eq!(doc.get_parent(child), Some(ancestor));
    }

    #[test]
    fn invalid_node_error_does_not_partially_mutate_tree() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();
        let missing = NodeId::new(999);

        assert_eq!(
            doc.append_child(parent, missing),
            Err(TuidomError::NodeNotFound { id: missing })
        );
        assert!(doc.get_children(parent).is_empty());

        assert_eq!(
            doc.append_child(missing, child),
            Err(TuidomError::NodeNotFound { id: missing })
        );
        assert_eq!(doc.get_parent(child), None);
    }

    #[test]
    fn move_child_invalid_parent_does_not_detach_child() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();
        let missing = NodeId::new(999);

        doc.append_child(parent, child).unwrap();

        assert_eq!(
            doc.move_child(missing, child, child),
            Err(TuidomError::NodeNotFound { id: missing })
        );
        assert_eq!(doc.get_children(parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(parent));
    }

    #[test]
    fn remove_child_noops_when_child_belongs_to_another_parent() {
        let doc = Document::new().unwrap();
        let unrelated_parent = doc.create_box().unwrap();
        let actual_parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();

        doc.append_child(actual_parent, child).unwrap();
        doc.remove_child(unrelated_parent, child).unwrap();

        assert!(doc.get_children(unrelated_parent).is_empty());
        assert_eq!(doc.get_children(actual_parent), vec![child]);
        assert!(doc.get_node(child).is_some());
        assert_eq!(doc.get_parent(child), Some(actual_parent));
    }

    #[test]
    fn remove_child_missing_node_returns_error_without_mutation() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let child = doc.create_text("child").unwrap();
        let missing = NodeId::new(999);

        doc.append_child(parent, child).unwrap();

        assert_eq!(
            doc.remove_child(parent, missing),
            Err(TuidomError::NodeNotFound { id: missing })
        );
        assert_eq!(doc.get_children(parent), vec![child]);
        assert!(doc.get_node(child).is_some());

        assert_eq!(
            doc.remove_child(missing, child),
            Err(TuidomError::NodeNotFound { id: missing })
        );
        assert_eq!(doc.get_children(parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(parent));
    }

    #[test]
    fn remove_subtree() {
        let doc = Document::new().unwrap();

        let root = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        let grandchild = doc.create_text("deep").unwrap();

        doc.append_child(root, child).unwrap();
        doc.append_child(child, grandchild).unwrap();

        doc.remove_child(root, child).unwrap();

        // grandchild is also gone
        assert!(doc.get_node(child).is_none());
        assert!(doc.get_node(grandchild).is_none());
        assert!(doc.get_children(root).is_empty());
    }

    #[test]
    fn remove_subtree_removes_attached_listeners() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_box().unwrap();
        let grandchild = doc.create_text("deep").unwrap();

        doc.append_child(root, child).unwrap();
        doc.append_child(child, grandchild).unwrap();
        doc.on(child, |_| {}).unwrap();
        doc.on(grandchild, |_| {}).unwrap();
        assert_eq!(lock::mutex(&doc.inner.listeners).len(), 2);

        doc.remove_child(root, child).unwrap();

        assert!(lock::mutex(&doc.inner.listeners).is_empty());
    }

    #[test]
    fn remove_subtree_removes_active_animations() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let child = doc.create_box().unwrap();

        doc.append_child(root, child).unwrap();
        doc.set_transition(
            child,
            TransitionConfig::opacity(Duration::from_secs(60), crate::animation::Easing::Linear),
        )
        .unwrap();
        doc.update_style(child, |style| style.opacity(0.0)).unwrap();
        assert!(lock::mutex(&doc.inner.animation).has_active());

        doc.remove_child(root, child).unwrap();

        assert!(!lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn cannot_remove_document_root() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let parent = doc.create_box().unwrap();

        assert_eq!(
            doc.remove_child(parent, root),
            Err(TuidomError::CannotRemoveRoot { id: root })
        );
        assert!(doc.get_node(root).is_some());
        assert_eq!(doc.get_parent(root), None);
    }

    #[test]
    fn is_descendant_of() {
        let doc = Document::new().unwrap();

        let a = doc.create_box().unwrap();
        let b = doc.create_box().unwrap();
        let c = doc.create_text("deep").unwrap();

        doc.append_child(a, b).unwrap();
        doc.append_child(b, c).unwrap();

        assert!(doc.is_descendant_of(c, a));
        assert!(doc.is_descendant_of(c, b));
        assert!(doc.is_descendant_of(b, a));
        assert!(!doc.is_descendant_of(a, c));
        assert!(!doc.is_descendant_of(a, a)); // not its own descendant
    }

    #[test]
    fn move_child_preserves_children() {
        let doc = Document::new().unwrap();

        let a = doc.create_box().unwrap();
        let b = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        let grandchild = doc.create_text("deep").unwrap();

        doc.append_child(a, child).unwrap();
        doc.append_child(child, grandchild).unwrap();

        // Move child (with grandchild) from a to b
        doc.move_child(b, child, b).unwrap(); // before_sibling doesn't exist → append

        assert_eq!(doc.get_parent(child), Some(b));
        assert_eq!(doc.get_parent(grandchild), Some(child));
        assert!(doc.get_children(a).is_empty());
        assert_eq!(doc.get_children(b), vec![child]);
    }

    #[test]
    fn document_has_permanent_root() {
        let doc = Document::new().unwrap();
        let root = doc.root();

        assert!(doc.get_node(root).is_some());
        assert_eq!(doc.get_parent(root), None);
        assert_eq!(doc.layout_node_count(), 1);

        let parent = doc.create_box().unwrap();
        assert_eq!(
            doc.append_child(parent, root),
            Err(TuidomError::CannotReparentRoot { id: root })
        );
        assert_eq!(doc.get_parent(root), None);
    }

    #[test]
    fn document_root_defaults_to_full_viewport_size() {
        let doc = Document::new().unwrap();
        let normal_node = doc.create_box().unwrap();

        let root_style = doc.resolved_style(doc.root()).unwrap();
        let normal_style = doc.resolved_style(normal_node).unwrap();

        assert_eq!(root_style.width, Length::Percent(100.0));
        assert_eq!(root_style.height, Length::Percent(100.0));
        assert_eq!(normal_style.width, Length::Auto);
        assert_eq!(normal_style.height, Length::Auto);
    }

    // -- Style resolution tests ---------------------------------------

    #[test]
    fn set_style_gets_resolved() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();

        let mut style = Style::new();
        style.width(Length::Pixels(42));
        doc.set_style(node, &style).unwrap();

        let resolved = doc.resolved_style(node).unwrap();
        assert_eq!(resolved.width, Length::Pixels(42));
        assert_eq!(resolved.opacity, 1.0); // Inherit → default
        assert_eq!(resolved.color, Color::white()); // Inherit → default
    }

    #[test]
    fn set_style_missing_node_returns_error() {
        let doc = Document::new().unwrap();
        let missing = NodeId::new(999);

        assert_eq!(
            doc.set_style(missing, &Style::new()),
            Err(TuidomError::NodeNotFound { id: missing })
        );
    }

    #[test]
    fn set_transition_missing_node_returns_error() {
        let doc = Document::new().unwrap();
        let missing = NodeId::new(999);
        let config =
            TransitionConfig::opacity(Duration::from_millis(100), crate::animation::Easing::Linear);

        assert_eq!(
            doc.set_transition(missing, config),
            Err(TuidomError::NodeNotFound { id: missing })
        );
    }

    #[test]
    fn update_style_invalidates_cache() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();

        let mut style = Style::new();
        style.width(Length::Pixels(10));
        doc.set_style(node, &style).unwrap();

        assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));

        doc.update_style(node, |s| {
            s.width(Length::Pixels(20));
        })
        .unwrap();

        assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(20));
    }

    #[test]
    fn panicking_update_style_does_not_partially_mutate_style() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();

        let mut style = Style::new();
        style.width(Length::Pixels(10));
        doc.set_style(node, &style).unwrap();
        assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = doc.update_style(node, |style| {
                style.width(Length::Pixels(20));
                panic!("boom");
            });
        }));

        assert!(result.is_err());
        assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));
    }

    #[test]
    fn update_style_missing_node_returns_error() {
        let doc = Document::new().unwrap();
        let missing = NodeId::new(999);

        assert_eq!(
            doc.update_style(missing, |s| s.opacity(0.5)),
            Err(TuidomError::NodeNotFound { id: missing })
        );
    }

    #[test]
    fn resolved_style_missing_node_returns_error() {
        let doc = Document::new().unwrap();
        let missing = NodeId::new(999);

        assert!(matches!(
            doc.resolved_style(missing),
            Err(TuidomError::NodeNotFound { id }) if id == missing
        ));
    }

    #[test]
    fn unset_properties_use_defaults_not_parent_values() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let mut parent_style = Style::new();
        parent_style.color(Color::red());
        doc.set_style(parent, &parent_style).unwrap();

        let child = doc.create_text("hi").unwrap();
        doc.append_child(parent, child).unwrap();

        let child_resolved = doc.resolved_style(child).unwrap();
        assert_eq!(child_resolved.color, Color::white());
        assert_eq!(child_resolved.width, Length::Auto);
    }

    #[test]
    fn explicitly_inherits_from_parent() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let mut parent_style = Style::new();
        parent_style.color(Color::red());
        doc.set_style(parent, &parent_style).unwrap();

        let child = doc.create_text("hi").unwrap();
        let mut child_style = Style::new();
        child_style.inherit_color();
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent, child).unwrap();

        let child_resolved = doc.resolved_style(child).unwrap();
        assert_eq!(child_resolved.color, Color::red());
        assert_eq!(child_resolved.width, Length::Auto);
    }

    #[test]
    fn override_breaks_inheritance() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let mut parent_style = Style::new();
        parent_style.color(Color::red());
        doc.set_style(parent, &parent_style).unwrap();

        let child = doc.create_text("hi").unwrap();
        let mut child_style = Style::new();
        child_style.color(Color::blue()); // Explicit override
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent, child).unwrap();

        let child_resolved = doc.resolved_style(child).unwrap();
        assert_eq!(child_resolved.color, Color::blue()); // Override wins
    }

    #[test]
    fn move_child_triggers_re_resolve() {
        let doc = Document::new().unwrap();

        let parent_red = doc.create_box().unwrap();
        let mut red_style = Style::new();
        red_style.color(Color::red());
        doc.set_style(parent_red, &red_style).unwrap();

        let parent_blue = doc.create_box().unwrap();
        let mut blue_style = Style::new();
        blue_style.color(Color::blue());
        doc.set_style(parent_blue, &blue_style).unwrap();

        let child = doc.create_text("movable").unwrap();
        let mut child_style = Style::new();
        child_style.inherit_color();
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent_red, child).unwrap();

        assert_eq!(doc.resolved_style(child).unwrap().color, Color::red());

        // Move to blue parent
        doc.move_child(parent_blue, child, child).unwrap();
        assert_eq!(doc.resolved_style(child).unwrap().color, Color::blue());
    }
}

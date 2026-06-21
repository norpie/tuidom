//! The [`Document`] type — the public API surface for tuidom.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use crate::animation::TransitionConfig;
use crate::animation::driver::{AnimationDriver, spawn_tick_task};
use crate::debug::DebugOverlay;
use crate::error::{Result, TuidomError};
use crate::event::{Event, Listener, ListenerHandle};
use crate::id::NodeId;
use crate::inner::DocumentInner;
use crate::lock;
use crate::node::{NodeData, NodeView};
use crate::style::Style;
use crate::style::resolution::ResolvedStyle;

/// The root container and public API surface for tuidom.
///
/// Wraps an `Arc<DocumentInner>` for cheap cloning. All methods take `&self`
/// and use interior mutability — the document is `Send + Sync` and can be
/// shared across threads.
///
/// # Example
///
/// ```ignore
/// let doc = Document::new();
/// let container = doc.create_box();
/// doc.set_root(container);
/// // ... build tree, register handlers, then:
/// doc.run().await;
/// ```
#[derive(Clone)]
pub struct Document {
    pub(crate) inner: Arc<DocumentInner>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Create a new, empty document.
    pub fn new() -> Self {
        // Initialize file-based logging. This is best-effort: logging must never
        // prevent a TUI from starting.
        if let Ok(file) = std::fs::File::create("/tmp/tuidom.log") {
            let _ = simplelog::WriteLogger::init(
                log::LevelFilter::Trace,
                simplelog::Config::default(),
                file,
            );
        }

        Self {
            inner: Arc::new(DocumentInner {
                nodes: dashmap::DashMap::new(),
                next_id: std::sync::atomic::AtomicU64::new(0),
                next_listener_id: std::sync::atomic::AtomicU64::new(0),
                root: std::sync::RwLock::new(None),
                tree_mutation: Mutex::new(()),
                notify: tokio::sync::Notify::new(),
                shutdown: std::sync::RwLock::new(false),
                animation: Arc::new(Mutex::new(AnimationDriver::new())),
                anim_config_changed: Arc::new(Notify::new()),
                anim_tick: Arc::new(Notify::new()),
                min_animation_tick: std::sync::RwLock::new(Duration::from_millis(1)),
                debug_overlay: Mutex::new(DebugOverlay::new()),
                listeners: Mutex::new(Vec::new()),
            }),
        }
    }

    // ------------------------------------------------------------------
    // Node creation
    // ------------------------------------------------------------------

    /// Create a new box (generic container) node.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_box(&self) -> NodeId {
        self.inner.alloc(NodeData::box_node())
    }

    /// Create a new text node with the given content.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_text(&self, content: impl Into<String>) -> NodeId {
        self.inner.alloc(NodeData::text(content))
    }

    // ------------------------------------------------------------------
    // Root
    // ------------------------------------------------------------------

    /// Set the root node for rendering.
    ///
    /// Only the root and its descendants are rendered. There can only be
    /// one root at a time; calling this again replaces the previous root.
    pub fn set_root(&self, id: NodeId) {
        *lock::rw_write(&self.inner.root) = Some(id);
        self.inner.notify.notify_one();
    }

    /// Get the current root node, if set.
    pub fn root(&self) -> Option<NodeId> {
        *lock::rw_read(&self.inner.root)
    }

    /// Trigger shutdown of the render loop.
    pub fn quit(&self) {
        *lock::rw_write(&self.inner.shutdown) = true;
        self.inner.notify.notify_one();
    }

    /// Toggle the debug overlay on/off.
    pub fn toggle_debug_overlay(&self) {
        let mut overlay = lock::mutex(&self.inner.debug_overlay);
        overlay.enabled = !overlay.enabled;
        self.inner.notify.notify_one();
    }

    /// Register a global event listener.
    ///
    /// The handler is called synchronously for each terminal event. For async
    /// work, spawn a task inside the handler.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on<F>(&self, handler: F) -> ListenerHandle
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        let id = self
            .inner
            .next_listener_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let listener = Listener {
            id,
            handler: Arc::new(handler),
        };

        lock::mutex(&self.inner.listeners).push(listener);
        ListenerHandle::new(id)
    }

    /// Remove a global event listener.
    ///
    /// Returns `true` if a listener was removed, or `false` if the handle was
    /// unknown or had already been removed.
    pub fn remove_listener(&self, handle: ListenerHandle) -> bool {
        let mut listeners = lock::mutex(&self.inner.listeners);
        let old_len = listeners.len();
        listeners.retain(|listener| listener.id != handle.id);
        listeners.len() != old_len
    }

    /// Dispatch an event to all registered listeners.
    pub(crate) fn dispatch_event(&self, event: Event) {
        let listeners = lock::mutex(&self.inner.listeners).clone();

        for listener in listeners {
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
    pub fn set_transition(&self, id: NodeId, config: TransitionConfig) {
        if let Some(mut data) = self.inner.nodes.get_mut(&id) {
            data.transition_configs.insert(config.property, config);
        }
    }

    /// Set the minimum interval between animation frames.
    ///
    /// Lower values = smoother but higher CPU. Default is 1ms.
    pub fn set_min_animation_tick(&self, interval: Duration) {
        *lock::rw_write(&self.inner.min_animation_tick) = interval;
    }

    // ------------------------------------------------------------------
    // Style
    // ------------------------------------------------------------------

    /// Set the inline style for a node.
    ///
    /// This replaces any previously set style, invalidates the resolved
    /// style cache, and signals the animation driver if any transitionable
    /// properties changed.
    pub fn set_style(&self, id: NodeId, style: &Style) {
        let old_resolved = self.resolved_base_style(id);

        if let Some(mut data) = self.inner.nodes.get_mut(&id) {
            data.style = style.clone();
        }
        self.invalidate_resolved_style(id);
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved);
    }

    /// Update a node's style in-place via a closure.
    ///
    /// Invalidates the resolved style cache, triggers a re-render, and signals
    /// the animation driver if any transitionable properties changed.
    ///
    /// # Panics
    ///
    /// Panics if the node does not exist.
    pub fn update_style(&self, id: NodeId, f: impl FnOnce(&mut Style)) {
        // Capture old resolved values before the mutation
        let old_resolved = self.resolved_base_style(id);

        if let Some(mut data) = self.inner.nodes.get_mut(&id) {
            f(&mut data.style);
        } else {
            panic!("update_style: node {id:?} does not exist");
        }
        self.invalidate_resolved_style(id);
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved);
    }

    /// Get the fully resolved style for a node, including animation overrides.
    ///
    /// Returns the cached value if available, otherwise computes it by
    /// walking the parent chain. The resolved style has all [`StyleValue::Inherit`]
    /// values replaced with concrete values.
    ///
    /// During active animations, property values are overridden with the
    /// interpolated animation value.
    ///
    /// # Panics
    ///
    /// Panics if the node does not exist.
    pub fn resolved_style(&self, id: NodeId) -> ResolvedStyle {
        let mut resolved = self.resolved_base_style(id);

        // Apply animation overrides
        {
            let driver = lock::mutex(&self.inner.animation);
            for (prop, val) in driver.overrides_for(id) {
                match prop {
                    crate::animation::TransitionProperty::Opacity => resolved.opacity = val,
                }
            }
        }

        resolved
    }

    /// Get the base resolved style without animation overrides.
    ///
    /// Used internally by the animation driver to read target values.
    pub(crate) fn resolved_base_style(&self, id: NodeId) -> ResolvedStyle {
        // Check cache
        {
            let node = self.inner.nodes.get(&id).expect("node not found");
            if let Some(resolved) = &*lock::rw_read(&node.resolved_style) {
                return resolved.clone();
            }
        }

        // Cache miss — compute
        let parent = self.get_parent(id);
        let parent_resolved = parent.map(|pid| self.resolved_base_style(pid));

        let node = self.inner.nodes.get(&id).expect("node not found");
        let resolved = ResolvedStyle::compute(&node, parent_resolved.as_ref());

        *lock::rw_write(&node.resolved_style) = Some(resolved.clone());
        resolved
    }

    /// Signal the animation driver about a style change and spawn tick task if needed.
    fn signal_animation(&self, id: NodeId, old_resolved: &ResolvedStyle) {
        // Read transition configs before locking the driver
        let configs = {
            let node = self.inner.nodes.get(&id);
            node.map(|n| n.transition_configs.clone())
                .unwrap_or_default()
        };

        // Compute the new resolved value BEFORE locking the driver
        let new_resolved = self.resolved_base_style(id);

        let mut driver = lock::mutex(&self.inner.animation);
        let started = driver.style_changed(id, old_resolved, &new_resolved, &configs);
        drop(driver);

        if started {
            let min_tick = *lock::rw_read(&self.inner.min_animation_tick);
            spawn_tick_task(
                self.inner.animation.clone(),
                self.inner.anim_config_changed.clone(),
                Arc::clone(&self.inner.anim_tick),
                min_tick,
            );
        } else {
            self.inner.anim_config_changed.notify_one();
        }
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
        let tree_guard = lock::mutex(&self.inner.tree_mutation);
        self.ensure_node_exists(parent)?;
        self.ensure_node_exists(child)?;

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

        self.remove_subtree(child);
        drop(tree_guard);
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
        let tree_guard = lock::mutex(&self.inner.tree_mutation);
        self.validate_reparent(parent, child)?;

        self.detach_from_current_parent(child);
        self.insert_child_reference(parent, child, before_sibling)?;
        self.set_parent(child, parent)?;

        drop(tree_guard);
        self.invalidate_resolved_style(child);
        self.inner.notify.notify_one();
        Ok(())
    }

    fn validate_reparent(&self, parent: NodeId, child: NodeId) -> Result<()> {
        self.ensure_node_exists(parent)?;
        self.ensure_node_exists(child)?;

        if parent == child || self.is_descendant_of(parent, child) {
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

    fn detach_from_current_parent(&self, child: NodeId) {
        let old_parent = self.get_parent(child);
        if let Some(old_parent) = old_parent
            && let Some(mut old_parent_data) = self.inner.nodes.get_mut(&old_parent)
        {
            old_parent_data.children.retain(|&c| c != child);
        }
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
        self.inner.nodes.get(&id).and_then(|r| r.parent)
    }

    /// Get the children of a node.
    ///
    /// Returns an empty vector if the node does not exist.
    pub fn get_children(&self, id: NodeId) -> Vec<NodeId> {
        self.inner
            .nodes
            .get(&id)
            .map(|r| r.children.clone())
            .unwrap_or_default()
    }

    /// Check whether `id` is a descendant of `ancestor`.
    pub fn is_descendant_of(&self, id: NodeId, ancestor: NodeId) -> bool {
        if id == ancestor {
            return false;
        }

        let mut seen = HashSet::new();
        let mut current = id;
        while let Some(parent) = self.get_parent(current) {
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
    pub fn compute_layout(&self, screen_width: u16, screen_height: u16) {
        crate::layout::compute_layout(self, screen_width, screen_height);
    }

    // ------------------------------------------------------------------
    // Node inspection
    // ------------------------------------------------------------------

    /// Get a read-only snapshot of a node's public state.
    ///
    /// Returns `None` if the node does not exist.
    pub fn get_node(&self, id: NodeId) -> Option<NodeView> {
        self.inner.nodes.get(&id).map(|r| NodeView {
            id,
            kind: r.kind.to_view(),
            parent: r.parent,
            children: r.children.clone(),
            layout: r.layout,
            attrs: r.attrs.clone(),
        })
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Remove a node and its entire subtree from the arena.
    fn remove_subtree(&self, id: NodeId) {
        {
            let mut root = lock::rw_write(&self.inner.root);
            if *root == Some(id) {
                *root = None;
            }
        }

        if let Some((_, data)) = self.inner.nodes.remove(&id) {
            for child in &data.children {
                self.remove_subtree(*child);
            }
        }
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
        let doc = Document::new();
        let box_id = doc.create_box();
        let text_id = doc.create_text("hello");

        let box_view = doc.get_node(box_id).unwrap();
        let text_view = doc.get_node(text_id).unwrap();

        assert!(matches!(box_view.kind, crate::node::NodeKindView::Box));
        assert!(matches!(
            text_view.kind,
            crate::node::NodeKindView::Text { .. }
        ));

        assert!(doc.get_node(NodeId::new(999)).is_none());
    }

    fn key_event() -> Event {
        Event::KeyPress(KeyEvent {
            code: KeyCode::Char('x'),
        })
    }

    #[test]
    fn listener_handle_removes_registered_listener() {
        let doc = Document::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();

        let handle = doc.on(move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        });

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        assert!(doc.remove_listener(handle));
        assert!(!doc.remove_listener(handle));

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn listener_can_register_listener_during_dispatch() {
        let doc = Document::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let doc_for_handler = doc.clone();
        let calls_for_handler = calls.clone();

        doc.on(move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
            let calls_for_new_handler = calls_for_handler.clone();
            doc_for_handler.on(move |_| {
                calls_for_new_handler.fetch_add(10, Ordering::Relaxed);
            });
        });

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn listener_panic_is_caught_and_later_listeners_still_run() {
        let doc = Document::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();

        doc.on(|_| panic!("listener boom"));
        doc.on(move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        });

        doc.dispatch_event(key_event());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn tree_ops() {
        let doc = Document::new();

        let root = doc.create_box();
        let child1 = doc.create_text("one");
        let child2 = doc.create_text("two");
        let child3 = doc.create_text("three");

        // append
        doc.append_child(root, child1).unwrap();
        doc.append_child(root, child2).unwrap();
        assert_eq!(doc.get_children(root), vec![child1, child2]);

        // insert_before
        doc.insert_before(root, child3, child2).unwrap();
        assert_eq!(doc.get_children(root), vec![child1, child3, child2]);

        // move_child
        let other = doc.create_box();
        doc.move_child(other, child3, child2).unwrap(); // inserts at end since child2 isn't in other
        assert_eq!(doc.get_children(root), vec![child1, child2]);
        assert_eq!(doc.get_children(other), vec![child3]);

        assert_eq!(doc.get_parent(child3), Some(other));
    }

    #[test]
    fn append_child_reparents_without_stale_reference() {
        let doc = Document::new();
        let first_parent = doc.create_box();
        let second_parent = doc.create_box();
        let child = doc.create_text("child");

        doc.append_child(first_parent, child).unwrap();
        doc.append_child(second_parent, child).unwrap();

        assert!(doc.get_children(first_parent).is_empty());
        assert_eq!(doc.get_children(second_parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(second_parent));
    }

    #[test]
    fn append_child_does_not_duplicate_existing_child() {
        let doc = Document::new();
        let parent = doc.create_box();
        let child = doc.create_text("child");

        doc.append_child(parent, child).unwrap();
        doc.append_child(parent, child).unwrap();

        assert_eq!(doc.get_children(parent), vec![child]);
        assert_eq!(doc.get_parent(child), Some(parent));
    }

    #[test]
    fn insert_before_reorders_existing_child_without_duplicate() {
        let doc = Document::new();
        let parent = doc.create_box();
        let first = doc.create_text("first");
        let second = doc.create_text("second");
        let third = doc.create_text("third");

        doc.append_child(parent, first).unwrap();
        doc.append_child(parent, second).unwrap();
        doc.append_child(parent, third).unwrap();
        doc.insert_before(parent, third, first).unwrap();

        assert_eq!(doc.get_children(parent), vec![third, first, second]);
        assert_eq!(doc.get_parent(third), Some(parent));
    }

    #[test]
    fn cycle_attempt_returns_error_and_does_not_mutate() {
        let doc = Document::new();
        let ancestor = doc.create_box();
        let child = doc.create_box();

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
        let doc = Document::new();
        let parent = doc.create_box();
        let child = doc.create_text("child");
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
        let doc = Document::new();
        let parent = doc.create_box();
        let child = doc.create_text("child");
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
        let doc = Document::new();
        let unrelated_parent = doc.create_box();
        let actual_parent = doc.create_box();
        let child = doc.create_text("child");

        doc.append_child(actual_parent, child).unwrap();
        doc.remove_child(unrelated_parent, child).unwrap();

        assert!(doc.get_children(unrelated_parent).is_empty());
        assert_eq!(doc.get_children(actual_parent), vec![child]);
        assert!(doc.get_node(child).is_some());
        assert_eq!(doc.get_parent(child), Some(actual_parent));
    }

    #[test]
    fn remove_child_missing_node_returns_error_without_mutation() {
        let doc = Document::new();
        let parent = doc.create_box();
        let child = doc.create_text("child");
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
        let doc = Document::new();

        let root = doc.create_box();
        let child = doc.create_box();
        let grandchild = doc.create_text("deep");

        doc.append_child(root, child).unwrap();
        doc.append_child(child, grandchild).unwrap();

        doc.remove_child(root, child).unwrap();

        // grandchild is also gone
        assert!(doc.get_node(child).is_none());
        assert!(doc.get_node(grandchild).is_none());
        assert!(doc.get_children(root).is_empty());
    }

    #[test]
    fn remove_subtree_clears_root_if_root_was_removed() {
        let doc = Document::new();

        let parent = doc.create_box();
        let child = doc.create_box();

        doc.append_child(parent, child).unwrap();
        doc.set_root(child);

        doc.remove_child(parent, child).unwrap();

        assert_eq!(doc.root(), None);
    }

    #[test]
    fn is_descendant_of() {
        let doc = Document::new();

        let a = doc.create_box();
        let b = doc.create_box();
        let c = doc.create_text("deep");

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
        let doc = Document::new();

        let a = doc.create_box();
        let b = doc.create_box();
        let child = doc.create_box();
        let grandchild = doc.create_text("deep");

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
    fn set_root() {
        let doc = Document::new();
        assert_eq!(doc.root(), None);

        let root = doc.create_box();
        doc.set_root(root);
        assert_eq!(doc.root(), Some(root));

        let new_root = doc.create_box();
        doc.set_root(new_root);
        assert_eq!(doc.root(), Some(new_root));
    }

    // -- Style resolution tests ---------------------------------------

    #[test]
    fn set_style_gets_resolved() {
        let doc = Document::new();
        let node = doc.create_box();

        let mut style = Style::new();
        style.width(Length::Pixels(42));
        doc.set_style(node, &style);

        let resolved = doc.resolved_style(node);
        assert_eq!(resolved.width, Length::Pixels(42));
        assert_eq!(resolved.opacity, 1.0); // Inherit → default
        assert_eq!(resolved.color, Color::white()); // Inherit → default
    }

    #[test]
    fn update_style_invalidates_cache() {
        let doc = Document::new();
        let node = doc.create_box();

        let mut style = Style::new();
        style.width(Length::Pixels(10));
        doc.set_style(node, &style);

        assert_eq!(doc.resolved_style(node).width, Length::Pixels(10));

        doc.update_style(node, |s| {
            s.width(Length::Pixels(20));
        });

        assert_eq!(doc.resolved_style(node).width, Length::Pixels(20));
    }

    #[test]
    fn inherits_from_parent() {
        let doc = Document::new();

        let parent = doc.create_box();
        let mut parent_style = Style::new();
        parent_style.color(Color::red());
        doc.set_style(parent, &parent_style);

        let child = doc.create_text("hi");
        // child uses default style — all Inherit
        doc.append_child(parent, child).unwrap();

        let child_resolved = doc.resolved_style(child);
        // Inherits color from parent
        assert_eq!(child_resolved.color, Color::red());
        // Own width is Inherit → default
        assert_eq!(child_resolved.width, Length::Auto);
    }

    #[test]
    fn override_breaks_inheritance() {
        let doc = Document::new();

        let parent = doc.create_box();
        let mut parent_style = Style::new();
        parent_style.color(Color::red());
        doc.set_style(parent, &parent_style);

        let child = doc.create_text("hi");
        let mut child_style = Style::new();
        child_style.color(Color::blue()); // Explicit override
        doc.set_style(child, &child_style);
        doc.append_child(parent, child).unwrap();

        let child_resolved = doc.resolved_style(child);
        assert_eq!(child_resolved.color, Color::blue()); // Override wins
    }

    #[test]
    fn move_child_triggers_re_resolve() {
        let doc = Document::new();

        let parent_red = doc.create_box();
        let mut red_style = Style::new();
        red_style.color(Color::red());
        doc.set_style(parent_red, &red_style);

        let parent_blue = doc.create_box();
        let mut blue_style = Style::new();
        blue_style.color(Color::blue());
        doc.set_style(parent_blue, &blue_style);

        let child = doc.create_text("movable");
        doc.append_child(parent_red, child).unwrap();

        assert_eq!(doc.resolved_style(child).color, Color::red());

        // Move to blue parent
        doc.move_child(parent_blue, child, child).unwrap();
        assert_eq!(doc.resolved_style(child).color, Color::blue());
    }
}

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::{Mutex as TokioMutex, Notify};

use crate::animation::driver::AnimationDriver;
use crate::error::{Result, TuidomError};
use crate::event::FocusKeys;
use crate::event_loop;
use crate::id::{NodeId, next_document_id};
use crate::inner::{DocumentInner, FocusStack};
use crate::layout::LayoutEngine;
use crate::lock;
use crate::node::{NodeData, NodeKind};
use crate::performance::{PerformanceDetail, PerformanceSnapshot, PerformanceState, RenderMetrics};
use crate::style::Color;

mod attrs;
mod events;
mod focus;
mod focus_context;
mod input;
mod layout;
mod pseudo;
mod scroll;
pub(crate) mod selection;
mod style;
mod tree;

pub use selection::SelectionPoint;

#[cfg(test)]
mod tests;

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
        let (event_tx, event_rx) = unbounded_channel();
        let document_id = next_document_id();
        let root = NodeId::scoped(document_id, 0);
        let nodes = dashmap::DashMap::new();
        nodes.insert(root, NodeData::box_node());

        let document = Self {
            inner: Arc::new(DocumentInner {
                nodes,
                document_id,
                next_id: AtomicU64::new(1),
                next_listener_id: AtomicU64::new(0),
                root,
                focus_contexts: Mutex::new(FocusStack::new(root)),
                focusable_nodes: Mutex::new(HashSet::new()),
                focus_keys: Mutex::new(FocusKeys::default()),
                pseudo_styles: Mutex::new(HashMap::new()),
                active_node: Mutex::new(None),
                disabled_nodes: Mutex::new(HashSet::new()),
                tree_mutation: RwLock::new(()),
                notify: Notify::new(),
                shutdown: RwLock::new(false),
                event_tx,
                event_rx: TokioMutex::new(event_rx),
                pending_resize: Mutex::new(None),
                resize_notify: Notify::new(),
                shutdown_notify: Arc::new(Notify::new()),
                animation: Arc::new(Mutex::new(AnimationDriver::new())),
                anim_config_changed: Arc::new(Notify::new()),
                max_frame_interval: RwLock::new(None),
                layout: Mutex::new(LayoutEngine::new()),
                layout_snapshot: RwLock::new(HashMap::new()),
                scroll_offsets: Mutex::new(HashMap::new()),
                selection: Mutex::new(None),
                selection_listeners: Mutex::new(Vec::new()),
                performance: Mutex::new(PerformanceState::new()),
                targeted_listeners: Mutex::new(HashMap::new()),
                resize_listeners: Mutex::new(Vec::new()),
                post_frame_listeners: Mutex::new(Vec::new()),
                color_vars: Mutex::new(HashMap::new()),
                terminal_background: Mutex::new(Color::black()),
            }),
        };
        document.register_layout_node(root)?;
        Ok(document)
    }

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

    /// Replace a text node's content.
    ///
    /// Setting the content a node already has is a no-op: no relayout, no
    /// re-render. This makes it safe to call unconditionally from handlers that
    /// re-render on mutation — a post-frame handler included — since an unchanged
    /// write does not schedule another frame.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not a text node.
    pub fn set_text_content(&self, node: NodeId, content: impl Into<String>) -> Result<()> {
        {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let NodeKind::Text { content: text } = &mut data.kind else {
                return Err(TuidomError::NodeNotText { id: node });
            };
            let content = content.into();
            if *text == content {
                return Ok(());
            }
            *text = content;
        }

        // A shrunk content may have orphaned selection offsets pointing past its end.
        self.clamp_selection_to_text(node);
        self.register_layout_node(node)?;
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Create a new editable input node with the given content.
    ///
    /// Returns the [`NodeId`] of the created node. Input nodes are focusable
    /// by default.
    pub fn create_input(&self, content: impl Into<String>) -> Result<NodeId> {
        let id = self.inner.alloc(NodeData::input(content));
        if let Err(err) = self.register_layout_node(id) {
            self.inner.nodes.remove(&id);
            return Err(err);
        }
        lock::mutex(&self.inner.focusable_nodes).insert(id);
        Ok(id)
    }

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

    /// Return the latest collected performance metrics.
    pub fn performance_snapshot(&self) -> PerformanceSnapshot {
        lock::mutex(&self.inner.performance).snapshot()
    }

    /// Set the amount of performance instrumentation collected while rendering.
    pub fn set_performance_detail(&self, detail: PerformanceDetail) {
        lock::mutex(&self.inner.performance).set_detail(detail);
        self.inner.notify.notify_one();
    }

    /// Record rendering metrics for the public performance API.
    pub(crate) fn record_frame_metrics(
        &self,
        frame: Duration,
        layout: Duration,
        stats: RenderMetrics,
    ) {
        lock::mutex(&self.inner.performance).record(frame, layout, stats);
    }

    /// Run the render + event loop until [`quit`](Self::quit) is called.
    ///
    /// Consumes the document. Clone it first if you need to keep a handle
    /// for event handlers or other tasks.
    pub async fn run(self) -> io::Result<()> {
        event_loop::run(self).await
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
}

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::{Mutex as TokioMutex, Notify};

use crate::animation::driver::AnimationDriver;
use crate::debug::DebugOverlay;
use crate::error::Result;
use crate::event_loop;
use crate::id::{NodeId, next_document_id};
use crate::inner::DocumentInner;
use crate::layout::LayoutEngine;
use crate::lock;
use crate::node::NodeData;
use crate::render::RenderStats;

mod events;
mod focus;
mod layout;
mod style;
mod tree;

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
                focused_node: Mutex::new(None),
                focusable_nodes: Mutex::new(HashSet::new()),
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
                layout_rects: RwLock::new(HashMap::new()),
                debug_overlay: Mutex::new(DebugOverlay::new()),
                targeted_listeners: Mutex::new(HashMap::new()),
                resize_listeners: Mutex::new(Vec::new()),
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

    /// Record rendering metrics for the debug overlay.
    pub(crate) fn record_frame_metrics(
        &self,
        frame: Duration,
        layout: Duration,
        stats: RenderStats,
    ) {
        let mut overlay = lock::mutex(&self.inner.debug_overlay);
        overlay.record(frame, layout, stats);
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

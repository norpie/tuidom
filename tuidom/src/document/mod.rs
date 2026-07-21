use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::{Mutex as TokioMutex, Notify};

use crate::animation::driver::AnimationDriver;
use crate::error::{Result, TuidomError};
use crate::event::FocusKeys;
use crate::event_loop;
use rustc_hash::FxBuildHasher;

use crate::id::{NodeId, NodeMap, NodeSet, next_document_id};
use crate::inner::{DocumentInner, FocusStack};
use crate::layout::LayoutEngine;
use crate::lock;
use crate::node::{NodeData, NodeKind};
use crate::performance::{PerformanceDetail, PerformanceSnapshot, PerformanceState, RenderMetrics};
use crate::style::Color;

mod animation;
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

/// Default interval between animation-driven frames (~60fps).
const DEFAULT_ANIMATION_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

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
        let nodes: dashmap::DashMap<NodeId, NodeData, FxBuildHasher> = dashmap::DashMap::default();
        nodes.insert(root, NodeData::box_node());

        let document = Self {
            inner: Arc::new(DocumentInner {
                nodes,
                document_id,
                next_id: AtomicU64::new(1),
                next_listener_id: AtomicU64::new(0),
                next_animation_id: AtomicU64::new(0),
                root,
                focus_contexts: Mutex::new(FocusStack::new(root)),
                focusable_nodes: Mutex::new(NodeSet::default()),
                focus_keys: Mutex::new(FocusKeys::default()),
                pseudo_styles: Mutex::new(NodeMap::default()),
                active_node: Mutex::new(None),
                disabled_nodes: Mutex::new(NodeSet::default()),
                tree_mutation: RwLock::new(()),
                notify: Notify::new(),
                shutdown: RwLock::new(false),
                event_tx,
                event_rx: TokioMutex::new(event_rx),
                pending_resize: Mutex::new(None),
                pending_bell: AtomicBool::new(false),
                resize_notify: Notify::new(),
                shutdown_notify: Arc::new(Notify::new()),
                animation: Arc::new(Mutex::new(AnimationDriver::new())),
                anim_config_changed: Arc::new(Notify::new()),
                max_frame_interval: RwLock::new(None),
                animation_frame_interval: RwLock::new(Some(DEFAULT_ANIMATION_FRAME_INTERVAL)),
                manual_now: Mutex::new(None),
                layout: Mutex::new(LayoutEngine::new()),
                layout_snapshot: RwLock::new(NodeMap::default()),
                scroll_offsets: Mutex::new(NodeMap::default()),
                scroll_activity: Mutex::new(NodeMap::default()),
                scrollbar_grab: Mutex::new(None),
                selection: Mutex::new(None),
                selection_listeners: Mutex::new(Vec::new()),
                performance: Mutex::new(PerformanceState::new()),
                targeted_listeners: Mutex::new(HashMap::new()),
                resize_listeners: Mutex::new(Vec::new()),
                post_frame_listeners: Mutex::new(Vec::new()),
                window_focus_listeners: Mutex::new(Vec::new()),
                window_blur_listeners: Mutex::new(Vec::new()),
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

    /// Create a new frames node: text content cycling on a timer.
    ///
    /// Frames render like text, but the node is measured on its largest frame,
    /// so cycling never reflows the content around it. The current frame is a
    /// function of elapsed time — a lone frames node paces rendering at its own
    /// interval rather than the animation tick rate. A single frame, or a zero
    /// interval, shows frame zero and drives no rendering at all.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_frames(
        &self,
        frames: impl IntoIterator<Item = impl Into<String>>,
        interval: Duration,
    ) -> Result<NodeId> {
        let frames: Vec<String> = frames.into_iter().map(Into::into).collect();
        let count = frames.len();
        let started = self.now();
        let id = self
            .inner
            .alloc(NodeData::frames(frames, interval, started));
        if let Err(err) = self.register_layout_node(id) {
            self.inner.nodes.remove(&id);
            return Err(err);
        }
        lock::mutex(&self.inner.animation).set_frames_schedule(id, interval, started, count);
        self.inner.anim_config_changed.notify_one();
        self.inner.notify.notify_one();
        Ok(id)
    }

    /// Replace a frames node's frame list.
    ///
    /// The cycle keeps its phase: the current index still counts from the
    /// original start instant, modulo the new frame count.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not a frames node.
    pub fn set_frames(
        &self,
        node: NodeId,
        frames: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        let new_frames: Vec<String> = frames.into_iter().map(Into::into).collect();
        self.update_frames(node, |frames, _, _| *frames = new_frames)
    }

    /// Change a frames node's flip interval.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not a frames node.
    pub fn set_frames_interval(&self, node: NodeId, new_interval: Duration) -> Result<()> {
        self.update_frames(node, |_, interval, _| *interval = new_interval)
    }

    /// The frame index a frames node is currently showing.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not a frames node.
    pub fn current_frame(&self, node: NodeId) -> Result<usize> {
        let data = self
            .inner
            .nodes
            .get(&node)
            .ok_or(TuidomError::NodeNotFound { id: node })?;
        let NodeKind::Frames {
            frames,
            interval,
            started,
        } = &data.kind
        else {
            return Err(TuidomError::NodeNotFrames { id: node });
        };
        Ok(crate::node::frames_index(
            frames.len(),
            *interval,
            *started,
            self.now(),
        ))
    }

    /// Mutate a frames node's data, then refresh measurement, schedule, and paint.
    fn update_frames(
        &self,
        node: NodeId,
        mutate: impl FnOnce(&mut Vec<String>, &mut Duration, &mut Instant),
    ) -> Result<()> {
        let (interval, started, count) = {
            let mut data = self
                .inner
                .nodes
                .get_mut(&node)
                .ok_or(TuidomError::NodeNotFound { id: node })?;
            let NodeKind::Frames {
                frames,
                interval,
                started,
            } = &mut data.kind
            else {
                return Err(TuidomError::NodeNotFrames { id: node });
            };
            mutate(frames, interval, started);
            (*interval, *started, frames.len())
        };

        // The largest frame may have changed, so the measure context must too.
        self.register_layout_node(node)?;
        lock::mutex(&self.inner.animation).set_frames_schedule(node, interval, started, count);
        self.inner.anim_config_changed.notify_one();
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

    /// Ring the terminal bell.
    ///
    /// The bell is emitted by the next flush rather than written immediately: the
    /// render task owns the output stream, and a byte written from another thread
    /// could land in the middle of an escape sequence and corrupt it. Calling this
    /// schedules a frame, so a bell still reaches the terminal when nothing on
    /// screen changed.
    ///
    /// Several calls before that frame produce one bell — which is all a terminal
    /// can make of them anyway. What the bell *does* is the terminal's choice: a
    /// sound, a visual flash, or nothing at all.
    pub fn bell(&self) {
        self.inner.pending_bell.store(true, Ordering::SeqCst);
        self.inner.notify.notify_one();
    }

    /// Claim a pending bell for the frame being flushed.
    pub(crate) fn take_pending_bell(&self) -> bool {
        self.inner.pending_bell.swap(false, Ordering::SeqCst)
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

    /// Set the rate at which active animations drive frames.
    ///
    /// Defaults to ~60fps. `None` removes the pacing entirely, rendering
    /// animation frames as fast as the runtime allows — useful as a stress
    /// test, pathological as a default. Non-finite or non-positive values are
    /// treated as `None`. The [`set_max_fps`](Self::set_max_fps) cap still
    /// applies on top if stricter.
    ///
    /// This only paces animation-driven frames: an idle document stays fully
    /// passive regardless of this setting.
    pub fn set_animation_fps(&self, fps: Option<f64>) {
        let interval = fps
            .filter(|fps| fps.is_finite() && *fps > 0.0)
            .and_then(|fps| Duration::try_from_secs_f64(1.0 / fps).ok());
        *lock::rw_write(&self.inner.animation_frame_interval) = interval;
        self.inner.anim_config_changed.notify_one();
    }

    // ------------------------------------------------------------------
    // Time
    // ------------------------------------------------------------------

    /// The document's current time — real time, or manual time when frozen.
    ///
    /// Every animation timestamp flows from here, so freezing the clock makes
    /// interpolated values exact instead of racing the wall clock.
    pub(crate) fn now(&self) -> Instant {
        lock::mutex(&self.inner.manual_now).unwrap_or_else(Instant::now)
    }

    /// Freeze the document clock at the current instant.
    ///
    /// Used by the headless runtime: from here on, time only moves through
    /// [`advance_manual_time`](Self::advance_manual_time).
    pub(crate) fn enable_manual_time(&self) {
        let mut manual = lock::mutex(&self.inner.manual_now);
        if manual.is_none() {
            *manual = Some(Instant::now());
        }
    }

    /// Advance the frozen document clock.
    pub(crate) fn advance_manual_time(&self, delta: Duration) {
        let mut manual = lock::mutex(&self.inner.manual_now);
        if let Some(now) = manual.as_mut() {
            *now += delta;
        }
    }
}

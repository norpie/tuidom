//! Internal document state behind `Arc`.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use dashmap::DashMap;
use tokio::sync::{Notify, mpsc};

use crate::animation::driver::AnimationDriver;
use crate::debug::DebugOverlay;
use crate::event::Listener;
use crate::event_loop::{RenderCommand, RuntimeEvent};
use crate::id::NodeId;
use crate::layout::LayoutEngine;
use crate::node::NodeData;

/// Internal state of a [`Document`](crate::Document).
///
/// Held behind `Arc` for cheap cloning and thread-safe sharing.
/// All fields use interior mutability — no `&mut self` needed.
pub(crate) struct DocumentInner {
    /// Arena mapping [`NodeId`] to [`NodeData`].
    pub nodes: DashMap<NodeId, NodeData>,

    /// Monotonically increasing counter for the next `NodeId::index`.
    pub next_id: AtomicU64,

    /// Monotonically increasing counter for event listener ids.
    pub next_listener_id: AtomicU64,

    /// The root node for rendering.
    pub root: RwLock<Option<NodeId>>,

    /// Serializes multi-node tree mutations so parent/child links stay consistent.
    pub tree_mutation: Mutex<()>,

    /// Notification signal — woken when DOM mutations require a re-render.
    pub notify: Notify,

    /// Shutdown signal — triggered by [`Document::quit`].
    pub shutdown: RwLock<bool>,

    /// Sender for queued runtime events.
    pub event_tx: mpsc::UnboundedSender<RuntimeEvent>,

    /// Receiver for queued runtime events, consumed sequentially by the event loop.
    pub event_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<RuntimeEvent>>,

    /// Sender for render-task commands.
    pub render_tx: mpsc::UnboundedSender<RenderCommand>,

    /// Receiver for render-task commands.
    pub render_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<RenderCommand>>,

    /// Broadcast wakeup for runtime shutdown.
    pub shutdown_notify: Arc<Notify>,

    /// Animation driver for managing transitions.
    pub animation: Arc<Mutex<AnimationDriver>>,

    /// Animation state change signal for waking the render task.
    pub anim_config_changed: Arc<Notify>,

    /// Optional document-wide frame-rate cap. `None` means uncapped.
    pub max_frame_interval: std::sync::RwLock<Option<std::time::Duration>>,

    /// Persistent taffy layout engine and DOM-to-layout-node mapping.
    pub layout: Mutex<LayoutEngine>,

    /// Debug overlay — toggled via F1, renders performance stats.
    pub debug_overlay: Mutex<DebugOverlay>,

    /// Global event listeners.
    pub listeners: Mutex<Vec<Listener>>,
}

impl DocumentInner {
    /// Allocate the next [`NodeId`] and insert node data into the arena.
    pub fn next_id(&self) -> NodeId {
        let index = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        NodeId::new(index)
    }

    /// Allocate a new node and return its [`NodeId`].
    pub fn alloc(&self, data: NodeData) -> NodeId {
        let id = self.next_id();
        self.nodes.insert(id, data);
        id
    }
}

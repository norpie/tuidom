//! Internal document state behind `Arc`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use dashmap::DashMap;
use tokio::sync::{Notify, mpsc};

use crate::animation::driver::AnimationDriver;
use crate::debug::DebugOverlay;
use crate::event::{FocusKeys, Listener, TargetedEventKind};
use crate::id::NodeId;
use crate::layout::LayoutEngine;
use crate::node::{LayoutRect, NodeData};
use crate::runtime_event::RuntimeEvent;

/// Internal state of a [`Document`](crate::Document).
///
/// Held behind `Arc` for cheap cloning and thread-safe sharing.
/// All fields use interior mutability — no `&mut self` needed.
pub(crate) struct DocumentInner {
    /// Arena mapping [`NodeId`] to [`NodeData`].
    pub nodes: DashMap<NodeId, NodeData>,

    /// Unique identity encoded into handles created by this document.
    pub document_id: u64,

    /// Monotonically increasing counter for the next `NodeId::index`.
    pub next_id: AtomicU64,

    /// Monotonically increasing counter for event listener ids.
    pub next_listener_id: AtomicU64,

    /// The permanent document root node.
    pub root: NodeId,

    /// The currently focused node, if any.
    pub focused_node: Mutex<Option<NodeId>>,

    /// Nodes that are allowed to receive focus.
    pub focusable_nodes: Mutex<HashSet<NodeId>>,

    /// Keyboard bindings for document-level focus default actions.
    pub focus_keys: Mutex<FocusKeys>,

    /// Per-node style merged when the node is focused.
    pub focus_styles: Mutex<HashMap<NodeId, crate::style::Style>>,

    /// Coordinates multi-node tree mutations with tree readers.
    pub tree_mutation: RwLock<()>,

    /// Notification signal — woken when DOM mutations require a re-render.
    pub notify: Notify,

    /// Shutdown signal — triggered by [`Document::quit`].
    pub shutdown: RwLock<bool>,

    /// Sender for queued runtime events.
    pub event_tx: mpsc::UnboundedSender<RuntimeEvent>,

    /// Receiver for queued runtime events, consumed sequentially by the event loop.
    pub event_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<RuntimeEvent>>,

    /// Latest pending terminal resize for the render task.
    pub pending_resize: Mutex<Option<(u16, u16)>>,

    /// Wakeup for render-task resize handling.
    pub resize_notify: Notify,

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

    /// Latest published layout rectangles, updated atomically after layout computation.
    pub layout_rects: RwLock<HashMap<NodeId, LayoutRect>>,

    /// Debug overlay — toggled via F1, renders performance stats.
    pub debug_overlay: Mutex<DebugOverlay>,

    /// Targeted event listeners keyed by node and event kind.
    pub targeted_listeners: Mutex<HashMap<(NodeId, TargetedEventKind), Vec<Listener>>>,

    /// Document-level resize listeners.
    pub resize_listeners: Mutex<Vec<Listener>>,
}

impl DocumentInner {
    /// Allocate the next [`NodeId`] and insert node data into the arena.
    pub fn next_id(&self) -> NodeId {
        let index = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        NodeId::scoped(self.document_id, index)
    }

    /// Allocate a new node and return its [`NodeId`].
    pub fn alloc(&self, data: NodeData) -> NodeId {
        let id = self.next_id();
        self.nodes.insert(id, data);
        id
    }
}

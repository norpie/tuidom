//! Internal document state behind `Arc`.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use dashmap::DashMap;
use tokio::sync::Notify;

use crate::animation::driver::AnimationDriver;
use crate::debug::DebugOverlay;
use crate::event::Event;
use crate::id::NodeId;
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

    /// The root node for rendering.
    pub root: RwLock<Option<NodeId>>,

    /// Notification signal — woken when DOM mutations require a re-render.
    pub notify: Notify,

    /// Shutdown signal — triggered by [`Document::quit`].
    pub shutdown: RwLock<bool>,

    /// Animation driver for managing transitions.
    pub animation: Arc<Mutex<AnimationDriver>>,

    /// Configuration change signal for the animation tick task.
    pub anim_config_changed: Arc<Notify>,

    /// Minimum interval between animation ticks, to prevent busy-waiting.
    /// Defaults to 1ms (render as fast as possible).
    pub min_animation_tick: std::sync::RwLock<std::time::Duration>,

    /// Animation tick signal — the render loop `select!`s on this.
    /// Woken by the tick task each frame. When no tick task runs,
    /// this never fires (passive idle).
    pub anim_tick: Arc<Notify>,

    /// Debug overlay — toggled via F1, renders performance stats.
    pub debug_overlay: Mutex<DebugOverlay>,

    /// Global event listeners.
    pub listeners: Mutex<Vec<Box<dyn Fn(&Event) + Send + Sync>>>,
}

impl DocumentInner {
    /// Allocate the next [`NodeId`] and insert node data into the arena.
    pub fn next_id(&self) -> NodeId {
        let index = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        NodeId::new(index)
    }

    /// Allocate a new node and return its [`NodeId`].
    pub fn alloc(&self, data: NodeData) -> NodeId {
        let id = self.next_id();
        self.nodes.insert(id, data);
        id
    }
}

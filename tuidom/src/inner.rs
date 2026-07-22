//! Internal document state behind `Arc`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};

use dashmap::DashMap;
use rustc_hash::FxBuildHasher;
use tokio::sync::{Notify, mpsc};

use crate::animation::driver::AnimationDriver;
use crate::document::selection::SelectionState;
use crate::event::{FocusKeys, Listener, ScrollKeys, TargetedEventKind};
use crate::id::{NodeId, NodeMap, NodeSet};
use crate::layout::LayoutEngine;
use crate::node::{NodeData, NodeLayout, ScrollOffset};
use crate::performance::{PerformanceState, StyleCounters};
use crate::runtime_event::RuntimeEvent;
use crate::style::{Color, Style};

/// Styles merged into a node's resolved style while it is in a pseudo-state.
///
/// One entry per node keeps pseudo-state lookup and cleanup on a single path as
/// more states are added.
/// Merge order is base → focus → active → disabled, so disabled wins on conflict.
#[derive(Debug, Default, Clone)]
pub(crate) struct PseudoStyles {
    pub focus: Option<Style>,
    pub active: Option<Style>,
    pub disabled: Option<Style>,
}

impl PseudoStyles {
    /// Whether no pseudo-style remains, so the entry can be dropped.
    pub fn is_empty(&self) -> bool {
        self.focus.is_none() && self.active.is_none() && self.disabled.is_none()
    }
}

/// One level of the focus context stack.
///
/// A focus context traps focus inside one subtree. Each level remembers its own focused
/// node, so restoring focus when a modal-like context closes is a stack pop rather than a
/// separate bookkeeping path.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FocusContext {
    /// The node whose subtree traps focus.
    pub context: NodeId,

    /// The focused node within this context, if any.
    pub focused: Option<NodeId>,
}

impl FocusContext {
    /// Create a context with nothing focused inside it yet.
    fn new(context: NodeId) -> Self {
        Self {
            context,
            focused: None,
        }
    }
}

/// The stack of focus contexts, innermost last.
///
/// The root context is stored outside the vector so the stack can never be empty and the
/// active context is always reachable without a fallible lookup.
#[derive(Debug)]
pub(crate) struct FocusStack {
    root: FocusContext,
    nested: Vec<FocusContext>,
}

impl FocusStack {
    /// Create a stack holding only the document root context.
    pub fn new(root: NodeId) -> Self {
        Self {
            root: FocusContext::new(root),
            nested: Vec::new(),
        }
    }

    /// The innermost context — the one that currently traps focus.
    pub fn active(&self) -> &FocusContext {
        self.nested.last().unwrap_or(&self.root)
    }

    /// The innermost context, mutably.
    pub fn active_mut(&mut self) -> &mut FocusContext {
        self.nested.last_mut().unwrap_or(&mut self.root)
    }

    /// Push a nested context, which becomes the active one.
    pub fn push(&mut self, context: NodeId) {
        self.nested.push(FocusContext::new(context));
    }

    /// Pop the innermost nested context. Returns `None` for the root context, which is
    /// permanent.
    pub fn pop(&mut self) -> Option<FocusContext> {
        self.nested.pop()
    }

    /// Number of contexts on the stack, counting the permanent root context.
    pub fn depth(&self) -> usize {
        self.nested.len() + 1
    }

    /// Whether `context` is a nested context on the stack.
    pub fn contains(&self, context: NodeId) -> bool {
        self.nested.iter().any(|entry| entry.context == context)
    }

    /// Drop nested contexts whose node no longer satisfies `exists`, innermost first.
    ///
    /// The root context is permanent and is never pruned.
    pub fn prune(&mut self, exists: impl Fn(NodeId) -> bool) {
        while let Some(active) = self.nested.last() {
            if exists(active.context) {
                break;
            }
            self.nested.pop();
        }
    }

    /// Every context on the stack, root first.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FocusContext> {
        std::iter::once(&mut self.root).chain(self.nested.iter_mut())
    }
}

/// Internal state of a [`Document`](crate::Document).
///
/// Held behind `Arc` for cheap cloning and thread-safe sharing.
/// All fields use interior mutability — no `&mut self` needed.
pub(crate) struct DocumentInner {
    /// Arena mapping [`NodeId`] to [`NodeData`].
    pub nodes: DashMap<NodeId, NodeData, FxBuildHasher>,

    /// Unique identity encoded into handles created by this document.
    pub document_id: u64,

    /// Monotonically increasing counter for the next `NodeId::index`.
    pub next_id: AtomicU64,

    /// Monotonically increasing counter for event listener ids.
    pub next_listener_id: AtomicU64,

    /// Monotonically increasing counter for keyframe animation ids.
    pub next_animation_id: AtomicU64,

    /// The permanent document root node.
    pub root: NodeId,

    /// The focus context stack. Focus lives in the innermost context.
    pub focus_contexts: Mutex<FocusStack>,

    /// Nodes that are allowed to receive focus.
    pub focusable_nodes: Mutex<NodeSet>,

    /// Keyboard bindings for document-level focus default actions.
    pub focus_keys: Mutex<FocusKeys>,

    /// Keyboard bindings for document-level scroll default actions.
    pub scroll_keys: Mutex<ScrollKeys>,

    /// The last screen cell the pointer was reported at, if it has ever moved.
    ///
    /// A position rather than the node under it: a cached `NodeId` goes stale when the
    /// tree changes beneath a stationary pointer, while re-hit-testing a position at the
    /// moment it is read cannot.
    pub last_pointer: Mutex<Option<(i32, i32)>>,

    /// Per-node styles merged when the node enters a pseudo-state.
    pub pseudo_styles: Mutex<NodeMap<PseudoStyles>>,

    /// The node currently being pressed, if any.
    pub active_node: Mutex<Option<NodeId>>,

    /// Nodes explicitly marked disabled. A node is *effectively* disabled when it or
    /// any ancestor appears here.
    pub disabled_nodes: Mutex<NodeSet>,

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

    /// Whether a bell is waiting to be emitted by the next flush.
    ///
    /// A flag rather than a count: the flush path owns the output stream, so a
    /// bell rides along with the next frame instead of racing it, and several
    /// bells within one frame are one beep — which is all a terminal can make of
    /// them anyway.
    pub pending_bell: AtomicBool,

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

    /// Interval between animation-driven frames. `None` means unlimited.
    pub animation_frame_interval: std::sync::RwLock<Option<std::time::Duration>>,

    /// Manual time source. `None` means real time; the headless runtime freezes
    /// time here so animation tests advance it explicitly.
    pub manual_now: Mutex<Option<std::time::Instant>>,

    /// Persistent taffy layout engine and DOM-to-layout-node mapping.
    pub layout: Mutex<LayoutEngine>,

    /// Latest published layout snapshot, updated atomically after layout computation.
    pub layout_snapshot: RwLock<NodeMap<NodeLayout>>,

    /// Current scroll offset per scroll container. Absent means `(0, 0)`.
    pub scroll_offsets: Mutex<NodeMap<ScrollOffset>>,

    /// Last scroll activity per `WhenScrolling` container, for auto-hide timing.
    ///
    /// Recorded only for containers whose resolved `scrollbar_show` is
    /// `WhenScrolling`, and pruned once their bars have fully faded, so the map
    /// holds only bars that are visible or on their way out.
    pub scroll_activity: Mutex<NodeMap<std::time::Instant>>,

    /// The container whose scrollbar is currently grabbed, if any.
    ///
    /// A grabbed `WhenScrolling` bar stays fully visible however long the grip is
    /// held; its fade countdown restarts on release.
    pub scrollbar_grab: Mutex<Option<NodeId>>,

    /// Current document text selection, if any.
    pub selection: Mutex<Option<SelectionState>>,

    /// Document-level selection change listeners.
    pub selection_listeners: Mutex<Vec<Listener>>,

    /// Collected runtime performance metrics.
    pub performance: Mutex<PerformanceState>,

    /// Targeted event listeners keyed by node and event kind.
    pub targeted_listeners: Mutex<HashMap<(NodeId, TargetedEventKind), Vec<Listener>>>,

    /// Document-level resize listeners.
    pub resize_listeners: Mutex<Vec<Listener>>,

    /// Document-level post-frame listeners.
    pub post_frame_listeners: Mutex<Vec<Listener>>,

    /// Document-level terminal window focus listeners.
    pub window_focus_listeners: Mutex<Vec<Listener>>,

    /// Document-level terminal window blur listeners.
    pub window_blur_listeners: Mutex<Vec<Listener>>,

    /// Color variables declared on the document — the root of every node's variable scope.
    pub color_vars: Mutex<HashMap<String, Color>>,

    /// Style-resolution counters, read and reset once per frame.
    ///
    /// Outside [`Self::performance`]'s mutex on purpose — style resolution runs thousands
    /// of times a frame, and locking at that rate would cost more than it measures.
    pub style_counters: StyleCounters,

    /// The terminal background color the document assumes.
    ///
    /// The real one is unknowable without querying the terminal, so it is declared rather than
    /// detected. It is an assumption used for resolving colors and blending translucent ones —
    /// never a color that gets painted.
    pub terminal_background: Mutex<Color>,
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

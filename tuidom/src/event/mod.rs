//! Event types for DOM event handling.
//!
//! Events flow from the terminal through the render loop to registered handlers.

use std::sync::Arc;

mod key;

pub(crate) use key::convert_key_event;
pub use key::{KeyCode, MediaKeyCode, ModifierKeyCode};

use crate::id::NodeId;

/// Opaque handle returned when registering an event listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerHandle {
    pub(crate) document_id: u64,
    pub(crate) id: u64,
}

impl ListenerHandle {
    pub(crate) fn new(document_id: u64, id: u64) -> Self {
        Self { document_id, id }
    }
}

/// The current dispatch phase for a targeted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    /// Dispatch is invoking listeners on the target node.
    Target,
    /// Dispatch is invoking listeners on an ancestor of the target node.
    Bubble,
}

/// Relation between the focused/blurred target and the listener currently receiving the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusEventRelation {
    /// The current listener node is the node that gained or lost focus.
    SelfNode,
    /// The current listener node is an ancestor of the node that gained or lost focus.
    Descendant,
}

/// Mouse buttons reported by terminal mouse input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Primary mouse button.
    Left,
    /// Secondary mouse button.
    Right,
    /// Middle mouse button.
    Middle,
}

#[derive(Debug, Clone, Copy)]
struct TargetedMetadata {
    target: NodeId,
    current_target: NodeId,
    phase: EventPhase,
    propagation_stopped: bool,
}

impl TargetedMetadata {
    fn pending() -> Self {
        let pending = NodeId::scoped(0, 0);
        Self {
            target: pending,
            current_target: pending,
            phase: EventPhase::Target,
            propagation_stopped: false,
        }
    }

    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.target = target;
        self.current_target = current_target;
        self.phase = phase;
    }
}

/// A keyboard event.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// The key that was pressed.
    pub code: KeyCode,
    metadata: TargetedMetadata,
    default_prevented: bool,
}

impl KeyEvent {
    pub(crate) fn new(code: KeyCode) -> Self {
        Self {
            code,
            metadata: TargetedMetadata::pending(),
            default_prevented: false,
        }
    }

    /// The node this event originally targeted.
    pub fn target(&self) -> NodeId {
        self.metadata.target
    }

    /// The node whose listeners are currently being invoked.
    pub fn current_target(&self) -> NodeId {
        self.metadata.current_target
    }

    /// The current dispatch phase.
    pub fn phase(&self) -> EventPhase {
        self.metadata.phase
    }

    /// Stop this event from bubbling to ancestor nodes.
    pub fn stop_propagation(&mut self) {
        self.metadata.propagation_stopped = true;
    }

    /// Whether propagation to ancestor nodes has been stopped.
    pub fn propagation_stopped(&self) -> bool {
        self.metadata.propagation_stopped
    }

    /// Prevent document-level default handling for this key press.
    ///
    /// This does not stop propagation. Use [`stop_propagation`](Self::stop_propagation)
    /// when ancestor listeners should not receive the event.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// Whether document-level default handling has been prevented.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }
}

/// A focus or blur event.
#[derive(Debug, Clone)]
pub struct FocusEvent {
    metadata: TargetedMetadata,
    relation: FocusEventRelation,
}

impl FocusEvent {
    pub(crate) fn new() -> Self {
        Self {
            metadata: TargetedMetadata::pending(),
            relation: FocusEventRelation::SelfNode,
        }
    }

    /// The node that gained or lost focus.
    pub fn target(&self) -> NodeId {
        self.metadata.target
    }

    /// The node whose listeners are currently being invoked.
    pub fn current_target(&self) -> NodeId {
        self.metadata.current_target
    }

    /// The current dispatch phase.
    pub fn phase(&self) -> EventPhase {
        self.metadata.phase
    }

    /// Whether the current listener node is the focused/blurred node or an ancestor.
    pub fn relation(&self) -> FocusEventRelation {
        self.relation
    }

    /// Stop this event from bubbling to ancestor nodes.
    pub fn stop_propagation(&mut self) {
        self.metadata.propagation_stopped = true;
    }

    /// Whether propagation to ancestor nodes has been stopped.
    pub fn propagation_stopped(&self) -> bool {
        self.metadata.propagation_stopped
    }
}

/// A mouse button event.
#[derive(Debug, Clone)]
pub struct MouseEvent {
    /// X coordinate in terminal cells.
    pub x: i32,
    /// Y coordinate in terminal cells.
    pub y: i32,
    /// Mouse button involved in the event.
    pub button: MouseButton,
    metadata: TargetedMetadata,
}

impl MouseEvent {
    /// Create a mouse button event.
    pub fn new(x: i32, y: i32, button: MouseButton) -> Self {
        Self {
            x,
            y,
            button,
            metadata: TargetedMetadata::pending(),
        }
    }

    /// The node this event originally targeted.
    pub fn target(&self) -> NodeId {
        self.metadata.target
    }

    /// The node whose listeners are currently being invoked.
    pub fn current_target(&self) -> NodeId {
        self.metadata.current_target
    }

    /// The current dispatch phase.
    pub fn phase(&self) -> EventPhase {
        self.metadata.phase
    }

    /// Stop this event from bubbling to ancestor nodes.
    pub fn stop_propagation(&mut self) {
        self.metadata.propagation_stopped = true;
    }

    /// Whether propagation to ancestor nodes has been stopped.
    pub fn propagation_stopped(&self) -> bool {
        self.metadata.propagation_stopped
    }
}

/// A mouse wheel event.
#[derive(Debug, Clone)]
pub struct WheelEvent {
    /// X coordinate in terminal cells.
    pub x: i32,
    /// Y coordinate in terminal cells.
    pub y: i32,
    /// Signed wheel delta. Positive values move upward; negative values move downward.
    pub delta: i16,
    metadata: TargetedMetadata,
}

impl WheelEvent {
    /// Create a wheel event.
    pub fn new(x: i32, y: i32, delta: i16) -> Self {
        Self {
            x,
            y,
            delta,
            metadata: TargetedMetadata::pending(),
        }
    }

    /// The node this event originally targeted.
    pub fn target(&self) -> NodeId {
        self.metadata.target
    }

    /// The node whose listeners are currently being invoked.
    pub fn current_target(&self) -> NodeId {
        self.metadata.current_target
    }

    /// The current dispatch phase.
    pub fn phase(&self) -> EventPhase {
        self.metadata.phase
    }

    /// Stop this event from bubbling to ancestor nodes.
    pub fn stop_propagation(&mut self) {
        self.metadata.propagation_stopped = true;
    }

    /// Whether propagation to ancestor nodes has been stopped.
    pub fn propagation_stopped(&self) -> bool {
        self.metadata.propagation_stopped
    }
}

/// A terminal resize event.
#[derive(Debug, Clone)]
pub struct ResizeEvent {
    /// New width in terminal cells.
    pub width: u16,
    /// New height in terminal cells.
    pub height: u16,
}

pub(crate) trait TargetedEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase);
    fn propagation_stopped(&self) -> bool;
}

impl TargetedEvent for KeyEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.metadata
            .set_dispatch_state(target, current_target, phase);
    }

    fn propagation_stopped(&self) -> bool {
        self.propagation_stopped()
    }
}

impl TargetedEvent for FocusEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.metadata
            .set_dispatch_state(target, current_target, phase);
        self.relation = if current_target == target {
            FocusEventRelation::SelfNode
        } else {
            FocusEventRelation::Descendant
        };
    }

    fn propagation_stopped(&self) -> bool {
        self.propagation_stopped()
    }
}

impl TargetedEvent for MouseEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.metadata
            .set_dispatch_state(target, current_target, phase);
    }

    fn propagation_stopped(&self) -> bool {
        self.propagation_stopped()
    }
}

impl TargetedEvent for WheelEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.metadata
            .set_dispatch_state(target, current_target, phase);
    }

    fn propagation_stopped(&self) -> bool {
        self.propagation_stopped()
    }
}

pub(crate) type KeyEventHandler = Arc<dyn Fn(&mut KeyEvent) + Send + Sync + 'static>;
pub(crate) type FocusEventHandler = Arc<dyn Fn(&mut FocusEvent) + Send + Sync + 'static>;
pub(crate) type MouseEventHandler = Arc<dyn Fn(&mut MouseEvent) + Send + Sync + 'static>;
pub(crate) type WheelEventHandler = Arc<dyn Fn(&mut WheelEvent) + Send + Sync + 'static>;
pub(crate) type ResizeEventHandler = Arc<dyn Fn(&mut ResizeEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TargetedEventKind {
    KeyPress,
    Focus,
    Blur,
    MouseDown,
    MouseUp,
    Click,
    Wheel,
}

/// Registered event listener callback.
#[derive(Clone)]
pub(crate) enum ListenerKind {
    /// Key press listener.
    KeyPress(KeyEventHandler),
    /// Focus listener.
    Focus(FocusEventHandler),
    /// Blur listener.
    Blur(FocusEventHandler),
    /// Mouse down listener.
    MouseDown(MouseEventHandler),
    /// Mouse up listener.
    MouseUp(MouseEventHandler),
    /// Mouse click listener.
    Click(MouseEventHandler),
    /// Mouse wheel listener.
    Wheel(WheelEventHandler),
    /// Terminal resize listener.
    Resize(ResizeEventHandler),
}

/// Registered event listener.
#[derive(Clone)]
pub(crate) struct Listener {
    /// Stable listener id used for removal.
    pub id: u64,
    /// Callback invoked for matching events.
    pub kind: ListenerKind,
}

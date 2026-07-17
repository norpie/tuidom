//! Event types for DOM event handling.
//!
//! Events flow from the terminal through the render loop to registered handlers.

use std::sync::Arc;

mod key;

pub(crate) use key::convert_key_event;
pub use key::{KeyCode, MediaKeyCode, ModifierKeyCode};

use crate::id::NodeId;
use crate::performance::FrameMetrics;

/// Keyboard bindings used by document-level focus default actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusKeys {
    /// Keys that move focus to the next focusable node in DOM order.
    pub next: Vec<KeyCode>,
    /// Keys that move focus to the previous focusable node in DOM order.
    pub previous: Vec<KeyCode>,
    /// Keys that move focus spatially upward.
    pub up: Vec<KeyCode>,
    /// Keys that move focus spatially downward.
    pub down: Vec<KeyCode>,
    /// Keys that move focus spatially left.
    pub left: Vec<KeyCode>,
    /// Keys that move focus spatially right.
    pub right: Vec<KeyCode>,
    /// Keys that clear the current focus.
    pub blur: Vec<KeyCode>,
}

impl Default for FocusKeys {
    fn default() -> Self {
        Self {
            next: vec![KeyCode::Tab],
            previous: vec![KeyCode::BackTab],
            up: vec![KeyCode::Up],
            down: vec![KeyCode::Down],
            left: vec![KeyCode::Left],
            right: vec![KeyCode::Right],
            blur: vec![KeyCode::Esc],
        }
    }
}

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
    default_prevented: bool,
}

impl MouseEvent {
    /// Create a mouse button event.
    pub fn new(x: i32, y: i32, button: MouseButton) -> Self {
        Self {
            x,
            y,
            button,
            metadata: TargetedMetadata::pending(),
            default_prevented: false,
        }
    }

    /// Prevent document-level default handling for this mouse event.
    ///
    /// The only mouse default action is starting a text selection on left mouse
    /// down; preventing it also keeps the existing selection instead of clearing it.
    /// This does not stop propagation. Use [`stop_propagation`](Self::stop_propagation)
    /// when ancestor listeners should not receive the event.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// Whether document-level default handling has been prevented.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
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

/// The axis a wheel event scrolls along.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WheelAxis {
    /// Vertical scrolling — the common mouse wheel.
    #[default]
    Vertical,
    /// Horizontal scrolling — tilt wheels and trackpads, where the terminal reports them.
    Horizontal,
}

/// A mouse wheel event.
#[derive(Debug, Clone)]
pub struct WheelEvent {
    /// X coordinate in terminal cells.
    pub x: i32,
    /// Y coordinate in terminal cells.
    pub y: i32,
    /// Signed wheel delta. Positive values move toward the start of the axis — up or
    /// left; negative values move toward the end — down or right.
    pub delta: i16,
    /// The axis this event scrolls along.
    pub axis: WheelAxis,
    metadata: TargetedMetadata,
    default_prevented: bool,
}

impl WheelEvent {
    /// Create a vertical wheel event.
    pub fn new(x: i32, y: i32, delta: i16) -> Self {
        Self {
            x,
            y,
            delta,
            axis: WheelAxis::Vertical,
            metadata: TargetedMetadata::pending(),
            default_prevented: false,
        }
    }

    /// Create a horizontal wheel event.
    pub fn horizontal(x: i32, y: i32, delta: i16) -> Self {
        Self {
            axis: WheelAxis::Horizontal,
            ..Self::new(x, y, delta)
        }
    }

    /// Prevent the default scroll this wheel would apply to a scrollable ancestor.
    ///
    /// This does not stop propagation. Use [`stop_propagation`](Self::stop_propagation)
    /// when ancestor listeners should not receive the event.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// Whether the default scroll has been prevented.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
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

/// A scroll offset change on an overflow container.
///
/// Fires on the container whose offset changed — from wheel input and imperative
/// scrolling alike. It does not bubble: scrolling is high-frequency, and like the DOM's
/// `scroll` event it reports a state change the engine has already applied, so it is
/// also delivered when the container is inert or disabled.
#[derive(Debug, Clone)]
pub struct ScrollEvent {
    /// The container's new horizontal scroll offset in terminal cells.
    pub x: u16,
    /// The container's new vertical scroll offset in terminal cells.
    pub y: u16,
    metadata: TargetedMetadata,
}

impl ScrollEvent {
    pub(crate) fn new(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            metadata: TargetedMetadata::pending(),
        }
    }

    /// The container whose scroll offset changed.
    pub fn target(&self) -> NodeId {
        self.metadata.target
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

/// A completed frame, dispatched after it was flushed to the screen.
///
/// Post-frame is document-level like resize: a frame has no target node, so the
/// event does not bubble. It carries the metrics recorded for the frame that just
/// finished, so a handler reads them without a separate snapshot call.
///
/// Mutating the DOM from a post-frame handler schedules another frame, whose own
/// post-frame event fires in turn — the requestAnimationFrame contract. A handler
/// that mutates on every event therefore keeps the renderer permanently active;
/// pace the mutations (skip the write when nothing visible changed, or throttle
/// on time) to let the renderer go idle again.
#[derive(Debug, Clone)]
pub struct PostFrameEvent {
    /// Metrics recorded for the frame that just finished.
    pub metrics: FrameMetrics,
    /// Frames per second as of this frame.
    pub fps: f64,
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

impl TargetedEvent for ScrollEvent {
    fn set_dispatch_state(&mut self, target: NodeId, current_target: NodeId, phase: EventPhase) {
        self.metadata
            .set_dispatch_state(target, current_target, phase);
    }

    // Scroll events do not bubble, so propagation can never be observed mid-flight.
    fn propagation_stopped(&self) -> bool {
        false
    }
}

pub(crate) type KeyEventHandler = Arc<dyn Fn(&mut KeyEvent) + Send + Sync + 'static>;
pub(crate) type FocusEventHandler = Arc<dyn Fn(&mut FocusEvent) + Send + Sync + 'static>;
pub(crate) type MouseEventHandler = Arc<dyn Fn(&mut MouseEvent) + Send + Sync + 'static>;
pub(crate) type WheelEventHandler = Arc<dyn Fn(&mut WheelEvent) + Send + Sync + 'static>;
pub(crate) type ScrollEventHandler = Arc<dyn Fn(&mut ScrollEvent) + Send + Sync + 'static>;
pub(crate) type ResizeEventHandler = Arc<dyn Fn(&mut ResizeEvent) + Send + Sync + 'static>;
pub(crate) type PostFrameEventHandler = Arc<dyn Fn(&mut PostFrameEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TargetedEventKind {
    KeyPress,
    Focus,
    Blur,
    MouseDown,
    MouseUp,
    Click,
    Wheel,
    Scroll,
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
    /// Scroll offset change listener.
    Scroll(ScrollEventHandler),
    /// Terminal resize listener.
    Resize(ResizeEventHandler),
    /// Post-frame listener.
    PostFrame(PostFrameEventHandler),
}

/// Registered event listener.
#[derive(Clone)]
pub(crate) struct Listener {
    /// Stable listener id used for removal.
    pub id: u64,
    /// Callback invoked for matching events.
    pub kind: ListenerKind,
}

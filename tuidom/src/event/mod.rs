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

/// Shared event callback type used internally for snapshot dispatch.
pub(crate) type EventHandler = Arc<dyn Fn(&Event) + Send + Sync + 'static>;

/// Registered event listener.
#[derive(Clone)]
pub(crate) struct Listener {
    /// Stable listener id used for removal.
    pub id: u64,
    /// Node this listener is attached to.
    pub node: NodeId,
    /// Callback invoked for matching events.
    pub handler: EventHandler,
}

/// A terminal event dispatched to user handlers.
#[derive(Debug, Clone)]
pub enum Event {
    /// A key was pressed.
    KeyPress(KeyEvent),
    /// The terminal was resized.
    Resize(ResizeEvent),
}

/// A keyboard event.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// The key that was pressed.
    pub code: KeyCode,
}

/// A terminal resize event.
#[derive(Debug, Clone)]
pub struct ResizeEvent {
    /// New width in terminal cells.
    pub width: u16,
    /// New height in terminal cells.
    pub height: u16,
}

//! Event types for DOM event handling.
//!
//! Events flow from the terminal through the render loop to registered handlers.

use std::sync::Arc;

/// Opaque handle returned when registering an event listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerHandle {
    pub(crate) id: u64,
}

impl ListenerHandle {
    pub(crate) fn new(id: u64) -> Self {
        Self { id }
    }
}

/// Shared event callback type used internally for snapshot dispatch.
pub(crate) type EventHandler = Arc<dyn Fn(&Event) + Send + Sync + 'static>;

/// Registered event listener.
#[derive(Clone)]
pub(crate) struct Listener {
    /// Stable listener id used for removal.
    pub id: u64,
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

/// Key codes for keyboard events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    /// A character key.
    Char(char),
    /// Escape key.
    Esc,
    /// Function key (F1, F2, …).
    F(u8),
}

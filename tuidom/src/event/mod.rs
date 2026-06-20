//! Event types for DOM event handling.
//!
//! Events flow from the terminal through the render loop to registered handlers.

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

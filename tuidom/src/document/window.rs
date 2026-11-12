use std::time::{Duration, Instant};

use crate::document::Document;
use crate::lock;

/// How long the cursor is shown, and then hidden, in each half of a blink.
const BLINK_PHASE: Duration = Duration::from_millis(500);

/// How long the cursor blinks after the window regains focus. A whole number of on-off
/// pairs, so the blink ends with the cursor shown.
const BLINK_DURATION: Duration = Duration::from_secs(3);

/// Whether the terminal window holds OS focus, and when the cursor's current blink began.
#[derive(Debug)]
pub(crate) struct WindowFocus {
    focused: bool,
    blink_start: Option<Instant>,
}

impl Default for WindowFocus {
    fn default() -> Self {
        Self {
            focused: true,
            blink_start: None,
        }
    }
}

/// Where a blinking cursor is right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CursorBlink {
    /// Whether the cursor is shown.
    pub on: bool,
    /// When it next flips, or when the blink ends.
    pub next: Instant,
}

impl Document {
    /// Whether the terminal window holds OS focus.
    ///
    /// Assumed true until the terminal reports otherwise, so a terminal without focus
    /// reporting always reads as focused. It is set before `on_window_focus` and
    /// `on_window_blur` listeners run.
    pub fn window_focused(&self) -> bool {
        lock::mutex(&self.inner.window_focus).focused
    }

    /// Record a window focus change. Gaining focus starts the cursor blinking; losing it
    /// ends any blink.
    pub(crate) fn set_window_focused(&self, focused: bool) {
        let now = self.now();
        let mut window = lock::mutex(&self.inner.window_focus);
        window.focused = focused;
        window.blink_start = focused.then_some(now);
        drop(window);
        self.inner.notify.notify_one();
    }

    /// Stop the cursor's blink, if it has one.
    pub(crate) fn end_cursor_blink(&self) {
        lock::mutex(&self.inner.window_focus).blink_start = None;
    }

    /// The cursor's blink at `now`, or `None` once it has ended or when there is none.
    pub(crate) fn cursor_blink(&self, now: Instant) -> Option<CursorBlink> {
        let mut window = lock::mutex(&self.inner.window_focus);
        let start = window.blink_start?;
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= BLINK_DURATION {
            window.blink_start = None;
            return None;
        }
        let phase = elapsed.as_nanos() / BLINK_PHASE.as_nanos();
        let next = (phase + 1) as u32;
        Some(CursorBlink {
            on: phase.is_multiple_of(2),
            next: start + BLINK_PHASE * next,
        })
    }
}

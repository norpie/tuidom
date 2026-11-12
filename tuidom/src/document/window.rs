use std::time::{Duration, Instant};

use crate::document::Document;
use crate::id::NodeId;
use crate::lock;
use crate::node::NodeKind;

/// How long the cursor is shown, and then hidden, in each half of a blink.
const BLINK_PHASE: Duration = Duration::from_millis(500);

/// How long the cursor blinks after the window or an input gains focus. A whole number of on-off
/// pairs, so the blink ends with the cursor shown.
const BLINK_DURATION: Duration = Duration::from_secs(5);

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

    /// Record a window focus change. Gaining focus starts the cursor blinking if an input is
    /// focused; losing it ends any blink.
    pub(crate) fn set_window_focused(&self, focused: bool) {
        lock::mutex(&self.inner.window_focus).focused = focused;
        self.restart_cursor_blink(self.focused());
        self.inner.notify.notify_one();
    }

    /// Start the cursor blinking afresh when `focused` is an input in a focused window, and
    /// end any blink otherwise, since there is then no cursor to blink.
    pub(crate) fn restart_cursor_blink(&self, focused: Option<NodeId>) {
        let input = focused.is_some_and(|node| {
            self.inner
                .nodes
                .get(&node)
                .is_some_and(|data| matches!(data.kind, NodeKind::Input { .. }))
        });
        let now = self.now();
        let mut window = lock::mutex(&self.inner.window_focus);
        window.blink_start = (input && window.focused).then_some(now);
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

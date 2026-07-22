use std::time::Instant;

use crate::document::Document;
use crate::error::Result;
use crate::event::{ScrollEvent, WheelAxis, WheelEvent};
use crate::id::NodeId;
use crate::lock;
use crate::node::ScrollOffset;
use crate::style::{Overflow, ScrollbarShow};

/// What `WhenScrolling` bars need from the render loop right now.
///
/// `fading` asks for smooth animation ticks; `next_deadline` asks for one wake at
/// the instant the earliest fully visible bar starts fading. Both absent means the
/// bars need no frames at all.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollbarFadeSchedule {
    /// Whether any bar is mid-fade and needs smooth repainting.
    pub fading: bool,
    /// When the earliest fully visible bar starts to fade, if any is waiting.
    pub next_deadline: Option<Instant>,
}

impl ScrollbarFadeSchedule {
    /// Whether the render loop has any scrollbar-driven frame to schedule.
    pub fn is_active(&self) -> bool {
        self.fading || self.next_deadline.is_some()
    }
}

impl Document {
    /// Get a node's current scroll offset.
    ///
    /// Returns `(0, 0)` for anything that has never been scrolled.
    pub fn scroll_offset(&self, node: NodeId) -> ScrollOffset {
        lock::mutex(&self.inner.scroll_offsets)
            .get(&node)
            .copied()
            .unwrap_or_default()
    }

    /// Scroll a node to an absolute offset, clamped to its scrollable range.
    ///
    /// An axis is scrollable only when the node's overflow on it is [`Overflow::Scroll`]
    /// and the latest layout measured content beyond the box; on any other axis the
    /// offset clamps to zero. Returns an error if the node does not exist.
    pub fn scroll_to(&self, node: NodeId, x: u16, y: u16) -> Result<()> {
        let resolved = self.resolved_style(node)?;
        let (max_x, max_y) = {
            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            match snapshot.get(&node) {
                Some(layout) => (
                    scrollable_max(resolved.overflow_x, layout.max_scroll_x),
                    scrollable_max(resolved.overflow_y, layout.max_scroll_y),
                ),
                None => (0, 0),
            }
        };

        let clamped = ScrollOffset {
            x: x.min(max_x),
            y: y.min(max_y),
        };

        let mut offsets = lock::mutex(&self.inner.scroll_offsets);
        let current = offsets.get(&node).copied().unwrap_or_default();
        if clamped == current {
            return Ok(());
        }
        if clamped == ScrollOffset::default() {
            offsets.remove(&node);
        } else {
            offsets.insert(node, clamped);
        }
        drop(offsets);

        if resolved.scrollbar_show == ScrollbarShow::WhenScrolling {
            self.record_scroll_activity(node);
        }
        self.inner.notify.notify_one();
        let mut event = ScrollEvent::new(clamped.x, clamped.y);
        self.dispatch_scroll_to(node, &mut event);
        Ok(())
    }

    /// Scroll a node by a relative amount, clamped to its scrollable range.
    ///
    /// Deltas are signed terminal cells; positive scrolls content further out of view
    /// past the top/left edge. Returns an error if the node does not exist.
    pub fn scroll_by(&self, node: NodeId, dx: i32, dy: i32) -> Result<()> {
        let current = self.scroll_offset(node);
        self.scroll_to(node, offset_by(current.x, dx), offset_by(current.y, dy))
    }

    /// Scroll the nearest scrollable ancestor a wheel event can move.
    ///
    /// Runs after wheel dispatch unless a listener prevented the default.
    pub(crate) fn apply_wheel_default_action(&self, target: NodeId, event: &WheelEvent) {
        let Some(container) = self.wheel_scroll_target(target, event) else {
            return;
        };
        // A positive delta moves the view toward the start of the axis, i.e. decreases
        // the offset. One cell per delta unit: terminals already send one event per
        // wheel notch, most sending several notches per physical tick.
        let step = -i32::from(event.delta);
        let (dx, dy) = match event.axis {
            WheelAxis::Vertical => (0, step),
            WheelAxis::Horizontal => (step, 0),
        };
        if let Err(err) = self.scroll_by(container, dx, dy) {
            tracing::error!("wheel default scroll failed: {err}");
        }
    }

    /// Restart a `WhenScrolling` container's auto-hide countdown from now.
    pub(crate) fn record_scroll_activity(&self, node: NodeId) {
        lock::mutex(&self.inner.scroll_activity).insert(node, self.now());
    }

    /// The frame-scheduling needs of `WhenScrolling` bars at `now`, pruning as it goes.
    ///
    /// Activity entries whose bars have fully faded, or whose containers no longer
    /// resolve to `WhenScrolling`, are removed here — this runs on every render-loop
    /// turn, so the map only ever holds bars that are visible or on their way out.
    /// A grabbed bar is pinned visible and schedules nothing; releasing it records
    /// fresh activity and wakes the renderer, which re-enters this query.
    pub(crate) fn scrollbar_fade_schedule(&self, now: Instant) -> ScrollbarFadeSchedule {
        let grabbed = *lock::mutex(&self.inner.scrollbar_grab);
        let entries: Vec<(NodeId, Instant)> = lock::mutex(&self.inner.scroll_activity)
            .iter()
            .map(|(node, at)| (*node, *at))
            .collect();

        let mut fading = false;
        let mut next_deadline: Option<Instant> = None;
        let mut stale = Vec::new();
        for (node, activity) in entries {
            if grabbed == Some(node) {
                continue;
            }
            let keep = self
                .resolved_style(node)
                .ok()
                .filter(|resolved| resolved.scrollbar_show == ScrollbarShow::WhenScrolling)
                .map(|resolved| {
                    let fade_start = activity + resolved.scrollbar_hide_delay;
                    if now < fade_start {
                        next_deadline =
                            Some(next_deadline.map_or(fade_start, |d| d.min(fade_start)));
                        true
                    } else if now < fade_start + resolved.scrollbar_fade_duration {
                        fading = true;
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            if !keep {
                stale.push(node);
            }
        }

        if !stale.is_empty() {
            let mut activity = lock::mutex(&self.inner.scroll_activity);
            for node in stale {
                activity.remove(&node);
            }
        }
        ScrollbarFadeSchedule {
            fading,
            next_deadline,
        }
    }

    /// Mark a container's scrollbar as grabbed, or release it with `None`.
    ///
    /// Grab and release both restart the released container's fade countdown and wake
    /// the renderer: a grabbed `WhenScrolling` bar must stay visible past its delay,
    /// and a released one must get its fade scheduled even when the release itself
    /// changed no offset.
    pub(crate) fn set_scrollbar_grab(&self, grab: Option<NodeId>) {
        let previous = {
            let mut grabbed = lock::mutex(&self.inner.scrollbar_grab);
            std::mem::replace(&mut *grabbed, grab)
        };
        if previous == grab {
            return;
        }
        for node in [previous, grab].into_iter().flatten() {
            if self
                .resolved_style(node)
                .is_ok_and(|resolved| resolved.scrollbar_show == ScrollbarShow::WhenScrolling)
            {
                self.record_scroll_activity(node);
            }
        }
        self.inner.notify.notify_one();
    }

    /// The nearest ancestor (or the target itself) the wheel can actually move.
    ///
    /// A container scrollable on the wheel's axis but already at the end the wheel pushes
    /// toward is skipped, so the scroll chains outward — matching how browsers hand a
    /// wheel to the next scrollable ancestor. Inert and disabled nodes are skipped the
    /// same way they swallow the wheel event itself.
    fn wheel_scroll_target(&self, target: NodeId, event: &WheelEvent) -> Option<NodeId> {
        if event.delta == 0 || self.blocks_interaction(target) {
            return None;
        }

        self.event_path(target).into_iter().find(|&node| {
            if self.blocks_interaction(node) {
                return false;
            }
            let Ok(resolved) = self.resolved_style(node) else {
                return false;
            };
            let overflow = match event.axis {
                WheelAxis::Vertical => resolved.overflow_y,
                WheelAxis::Horizontal => resolved.overflow_x,
            };
            if overflow != Overflow::Scroll {
                return false;
            }

            let offset = self.scroll_offset(node);
            let current = match event.axis {
                WheelAxis::Vertical => offset.y,
                WheelAxis::Horizontal => offset.x,
            };
            if event.delta > 0 {
                return current > 0;
            }

            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            let Some(layout) = snapshot.get(&node) else {
                return false;
            };
            let max = match event.axis {
                WheelAxis::Vertical => layout.max_scroll_y,
                WheelAxis::Horizontal => layout.max_scroll_x,
            };
            current < max
        })
    }
}

/// The scroll range an axis actually offers: taffy's measured overhang, gated on the
/// axis opting into scrolling.
pub(super) fn scrollable_max(overflow: Overflow, max_scroll: u16) -> u16 {
    match overflow {
        Overflow::Scroll => max_scroll,
        Overflow::Visible | Overflow::Clip => 0,
    }
}

fn offset_by(current: u16, delta: i32) -> u16 {
    i64::from(current)
        .saturating_add(i64::from(delta))
        .clamp(0, i64::from(u16::MAX)) as u16
}

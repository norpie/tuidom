use std::time::Instant;

use crate::document::Document;
use crate::error::Result;
use crate::event::{KeyCode, KeyModifiers, ScrollEvent, ScrollKeys, WheelAxis, WheelEvent};
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

/// What a bound scroll key does, once its binding has matched.
#[derive(Debug, Clone, Copy)]
enum ScrollAction {
    /// Move a fixed number of cells along one axis.
    By {
        /// The axis the movement is on.
        axis: WheelAxis,
        /// Cells to move, signed toward the end of the axis.
        delta: i32,
    },
    /// Move one visible page vertically, sized on the container that takes it.
    Page {
        /// `-1` toward the start of the axis, `1` toward its end.
        direction: i32,
    },
    /// Move to one end of the vertical range.
    ToExtreme {
        /// `-1` toward the start of the axis, `1` toward its end.
        direction: i32,
    },
}

impl ScrollAction {
    fn by(axis: WheelAxis, delta: i32) -> Self {
        ScrollAction::By { axis, delta }
    }

    /// The axis this action moves along.
    fn axis(self) -> WheelAxis {
        match self {
            ScrollAction::By { axis, .. } => axis,
            // Paging and jumping to an extreme are vertical by definition: they are sized
            // and named after rows, and a horizontal spelling would need its own keys.
            ScrollAction::Page { .. } | ScrollAction::ToExtreme { .. } => WheelAxis::Vertical,
        }
    }

    /// A signed delta standing in for this action while routing.
    ///
    /// Routing only needs the direction, and asking for the real distance first would
    /// mean sizing a page against a container that has not been chosen yet.
    fn probe_delta(self) -> i32 {
        match self {
            ScrollAction::By { delta, .. } => delta,
            ScrollAction::Page { direction } | ScrollAction::ToExtreme { direction } => direction,
        }
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

    /// Replace the document-level scroll key bindings.
    pub fn set_scroll_keys(&self, keys: ScrollKeys) {
        *lock::mutex(&self.inner.scroll_keys) = keys;
    }

    /// Return the document-level scroll key bindings.
    pub fn scroll_keys(&self) -> ScrollKeys {
        lock::mutex(&self.inner.scroll_keys).clone()
    }

    /// Scroll the nearest container that can move, from the document's keyboard target.
    ///
    /// Runs after the focus default action, so an existing focus binding still wins if a
    /// downstream rebinding makes the two sets overlap.
    pub(crate) fn apply_scroll_default_action(&self, code: KeyCode, modifiers: KeyModifiers) {
        let Some(action) = self.scroll_action_for_key(code, modifiers) else {
            return;
        };

        // Routing has to precede sizing: a page is a page of whichever container ends up
        // taking it, not of the node the key was aimed at.
        let Some(container) = self
            .scroll_route_start()
            .into_iter()
            .find_map(|start| self.scroll_target(start, action.axis(), action.probe_delta()))
        else {
            return;
        };

        let (dx, dy) = match action {
            ScrollAction::By { axis, delta } => match axis {
                WheelAxis::Vertical => (0, delta),
                WheelAxis::Horizontal => (delta, 0),
            },
            ScrollAction::Page { direction } => {
                let rows = i32::from(self.scroll_page_rows(container));
                (0, rows * direction)
            }
            // The clamp in `scroll_by` does the work: no max is read here, so an extreme
            // needs no layout of its own and cannot disagree with one.
            ScrollAction::ToExtreme { direction } => (0, i32::MAX * direction),
        };

        if let Err(err) = self.scroll_by(container, dx, dy) {
            tracing::error!("keyboard scroll default action failed: {err}");
        }
    }

    /// Where keyboard scrolling starts its rootward walk, in preference order.
    ///
    /// Focus first, then the node under the pointer, then the active focus context. The
    /// pointer is a *fallback* rather than a priority on purpose: hovering a focusable
    /// node already focuses it, so the two only disagree over a subtree with nothing
    /// focusable in it — and letting a parked mouse outrank a deliberate Tab would make
    /// the key depend on something a keyboard user has no reason to think about.
    fn scroll_route_start(&self) -> Vec<NodeId> {
        let hovered = (*lock::mutex(&self.inner.last_pointer))
            .and_then(|(x, y)| self.node_at(x, y))
            .filter(|node| Some(*node) != self.focused());

        [self.focused(), hovered, Some(self.active_focus_context())]
            .into_iter()
            .flatten()
            .collect()
    }

    /// How many rows one page covers in a container, per the last layout.
    ///
    /// One row of overlap is kept, matching page motion inside an input, so a page leaves
    /// a shared line to read against. Falls back to a single row when the container has
    /// not been laid out, rather than declining to move at all.
    fn scroll_page_rows(&self, node: NodeId) -> u16 {
        // The scrollport rather than the rect: content slides through the padding but
        // never over the border, so the border's rows are not part of a page.
        self.get_node(node)
            .and_then(|view| view.layout)
            .map(|layout| layout.scrollport.height.saturating_sub(1).max(1))
            .unwrap_or(1)
    }

    fn scroll_action_for_key(
        &self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<ScrollAction> {
        // Modifiers match exactly, as focus bindings do, so PageDown and Ctrl+PageDown
        // stay distinct rather than one shadowing the other.
        let binding = (code, modifiers);
        let keys = self.scroll_keys();
        for (bindings, action) in [
            (&keys.up, ScrollAction::by(WheelAxis::Vertical, -1)),
            (&keys.down, ScrollAction::by(WheelAxis::Vertical, 1)),
            (&keys.left, ScrollAction::by(WheelAxis::Horizontal, -1)),
            (&keys.right, ScrollAction::by(WheelAxis::Horizontal, 1)),
            (&keys.page_up, ScrollAction::Page { direction: -1 }),
            (&keys.page_down, ScrollAction::Page { direction: 1 }),
            (&keys.start, ScrollAction::ToExtreme { direction: -1 }),
            (&keys.end, ScrollAction::ToExtreme { direction: 1 }),
        ] {
            if bindings.contains(&binding) {
                return Some(action);
            }
        }
        None
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
        if event.delta == 0 {
            return None;
        }
        // A wheel's delta is positive toward the start of the axis, the opposite of a
        // scroll offset's direction.
        self.scroll_target(target, event.axis, -i32::from(event.delta))
    }

    /// The nearest container rootward of `target` that can still scroll `delta` on `axis`.
    ///
    /// This is scroll chaining: a container at the end of its range hands the movement to
    /// the ancestor beyond it rather than swallowing it. Shared by the wheel and by
    /// keyboard scrolling, so both route by exactly the same rule.
    pub(crate) fn scroll_target(
        &self,
        target: NodeId,
        axis: WheelAxis,
        delta: i32,
    ) -> Option<NodeId> {
        if delta == 0 || self.blocks_interaction(target) {
            return None;
        }

        self.event_path(target).into_iter().find(|&node| {
            if self.blocks_interaction(node) {
                return false;
            }
            let Ok(resolved) = self.resolved_style(node) else {
                return false;
            };
            let overflow = match axis {
                WheelAxis::Vertical => resolved.overflow_y,
                WheelAxis::Horizontal => resolved.overflow_x,
            };
            if overflow != Overflow::Scroll {
                return false;
            }

            let offset = self.scroll_offset(node);
            let current = match axis {
                WheelAxis::Vertical => offset.y,
                WheelAxis::Horizontal => offset.x,
            };
            if delta < 0 {
                return current > 0;
            }

            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            let Some(layout) = snapshot.get(&node) else {
                return false;
            };
            let max = match axis {
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

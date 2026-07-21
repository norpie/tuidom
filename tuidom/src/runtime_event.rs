use tokio::sync::mpsc;

use crate::animation::{AnimationHandle, TransitionProperty};
use crate::document::Document;
use crate::document::selection::PendingSelection;
use crate::event::{KeyEvent, MouseButton, MouseEvent, PostFrameEvent, ResizeEvent, WheelEvent};
use crate::id::NodeId;
use crate::lock;
use crate::node::LayoutRect;
use crate::paint_order::{PaintEntry, entry_at, offset_for_thumb, scrollbar_strip_of};

/// Internal runtime event used by runtime event queues and simulated input.
///
/// This is separate from public event structs so runtime coordination can evolve
/// without exposing renderer/input-loop details as user-facing API.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    /// A key press from the input stream.
    KeyPress(KeyEvent),
    /// A mouse button press from the input stream.
    MouseDown(MouseEvent),
    /// A mouse pointer movement from the input stream. `held` carries the button a
    /// drag is holding down; a plain move holds none.
    MouseMove {
        x: i32,
        y: i32,
        held: Option<MouseButton>,
    },
    /// A mouse button release from the input stream.
    MouseUp(MouseEvent),
    /// A mouse wheel movement from the input stream.
    Wheel(WheelEvent),
    /// Runtime resize requiring both public dispatch and renderer resize.
    Resize(ResizeEvent),
    /// The terminal window gained OS focus.
    WindowFocus,
    /// The terminal window lost OS focus.
    WindowBlur,
    /// A rendered frame, queued by the render task for post-frame dispatch.
    /// Boxed so the frame's metrics don't dominate the size of every queued event.
    PostFrame(Box<PostFrameEvent>),
    /// A completed transition, queued by animation upkeep for end-event dispatch.
    TransitionEnd {
        node: NodeId,
        property: TransitionProperty,
    },
    /// A completed keyframe animation, queued by animation upkeep.
    AnimationEnd {
        node: NodeId,
        handle: AnimationHandle,
    },
    /// A keyframe animation crossing an iteration boundary, queued by animation upkeep.
    AnimationIteration {
        node: NodeId,
        handle: AnimationHandle,
        iteration: u64,
    },
}

/// Collapse redundant runs in a drained batch, leaving order otherwise intact.
///
/// Only *adjacent* events merge, and only the two kinds where every value but the
/// last is dead weight: pointer movement and resize. Everything else — keys,
/// presses, releases, wheel ticks, frame and animation events — passes through
/// untouched, because each one carries information the next does not.
///
/// Adjacency is the load-bearing rule. Hover-to-focus makes the pointer's position
/// decide which node a key press targets, so merging movement *across* a key would
/// deliver that key to the wrong node. A press between two runs is a boundary for
/// the same reason: a drag starts where the button went down, not where the pointer
/// finished.
///
/// This runs only over what was already queued, so a batch of one — the common
/// case — is returned unchanged. Nothing is ever collapsed to reduce latency; it
/// is collapsed because the event task was already behind.
pub(crate) fn coalesce(batch: &mut Vec<RuntimeEvent>) {
    if batch.len() < 2 {
        return;
    }

    let mut kept = 0;
    for index in 0..batch.len() {
        if index + 1 < batch.len() && superseded_by(&batch[index], &batch[index + 1]) {
            continue;
        }
        batch.swap(kept, index);
        kept += 1;
    }
    batch.truncate(kept);
}

/// Whether `event` carries nothing that `next` does not already say.
fn superseded_by(event: &RuntimeEvent, next: &RuntimeEvent) -> bool {
    match (event, next) {
        // A drag and a hover are different gestures, so a change of held button
        // ends the run even though both are movement.
        (RuntimeEvent::MouseMove { held: from, .. }, RuntimeEvent::MouseMove { held: to, .. }) => {
            from == to
        }
        (RuntimeEvent::Resize(_), RuntimeEvent::Resize(_)) => true,
        _ => false,
    }
}

/// Take the next event plus everything already queued behind it, coalesced.
///
/// Never waits for more: the drain is `try_recv` only, so this adds no latency
/// and degrades to "process one event" whenever the task is keeping up.
pub(crate) fn drain_and_coalesce(
    first: RuntimeEvent,
    rx: &mut mpsc::UnboundedReceiver<RuntimeEvent>,
) -> Vec<RuntimeEvent> {
    let mut batch = vec![first];
    while let Ok(event) = rx.try_recv() {
        batch.push(event);
    }
    coalesce(&mut batch);
    batch
}

#[derive(Debug, Clone, Copy)]
struct ClickCandidate {
    target: NodeId,
    x: i32,
    y: i32,
    button: MouseButton,
}

/// An in-flight scrollbar drag: which bar is grabbed and where within the thumb.
///
/// Strip geometry is deliberately not stored — every update reads it fresh from
/// paint order, so a relayout mid-drag cannot desync the thumb from the cursor.
#[derive(Debug, Clone, Copy)]
struct ScrollbarDrag {
    container: NodeId,
    vertical: bool,
    /// Cells from the thumb's start to where the press grabbed it.
    grab: u16,
}

/// Stateful runtime-event processing data shared by terminal and headless runtimes.
#[derive(Debug, Default)]
pub(crate) struct RuntimeEventState {
    pending_click: Option<ClickCandidate>,
    pending_selection: Option<PendingSelection>,
    scrollbar_drag: Option<ScrollbarDrag>,
}

/// Process one queued or simulated runtime event.
pub(crate) fn process_runtime_event(
    doc: &Document,
    event: RuntimeEvent,
    state: &mut RuntimeEventState,
) {
    match event {
        RuntimeEvent::KeyPress(key) => {
            doc.dispatch_key_press(key);
        }
        RuntimeEvent::MouseDown(mut mouse) => {
            let hit_entry = entry_at(doc, mouse.x, mouse.y);
            let target = hit_entry
                .as_ref()
                .map_or_else(|| doc.root(), |entry| entry.id);
            // The pressed node is the node that would take focus from this hit, so
            // "being pressed" and "would be focused" never disagree.
            set_active_node(doc, doc.focus_target_from_hit(target));
            doc.dispatch_mouse_down_to(target, &mut mouse);

            state.pending_selection = None;
            state.scrollbar_drag = None;

            // A left press on a scrollbar strip has grabbing the bar as its default
            // action, in place of selection and click: a track press jumps the thumb
            // under the cursor, a thumb press grabs it where it is. Preventing the
            // default keeps the press an ordinary container press.
            if mouse.button == MouseButton::Left && !mouse.default_prevented() {
                state.scrollbar_drag = hit_entry
                    .filter(|entry| entry.scrollbar.is_some())
                    .and_then(|entry| begin_scrollbar_grab(doc, &entry, mouse.x, mouse.y));
            }
            if let Some(drag) = state.scrollbar_drag {
                doc.set_scrollbar_grab(Some(drag.container));
                state.pending_click = None;
            } else {
                state.pending_click = Some(ClickCandidate {
                    target,
                    x: mouse.x,
                    y: mouse.y,
                    button: mouse.button,
                });

                // The mouse default action: a left press clears the selection and arms a
                // new drag from this point. Preventing it keeps the selection too — the
                // press was claimed for something other than selecting.
                if mouse.button == MouseButton::Left && !mouse.default_prevented() {
                    doc.clear_selection();
                    state.pending_selection = doc.begin_selection_drag(mouse.x, mouse.y, target);
                }
            }
        }
        RuntimeEvent::MouseMove { x, y, held } => {
            // A drag must not yank focus from node to node as it crosses the screen —
            // hover-to-focus applies only to unpressed movement.
            if held.is_none() {
                focus_hover_target(doc, x, y);
            } else if held == Some(MouseButton::Left) {
                if let Some(drag) = state.scrollbar_drag {
                    update_scrollbar_drag(doc, drag, x, y);
                } else if let Some(pending) = &state.pending_selection {
                    doc.update_selection_drag(pending, x, y);
                }
            }
        }
        RuntimeEvent::MouseUp(mut mouse) => {
            let target = mouse_target(doc, mouse.x, mouse.y);
            // Release clears the pressed state wherever the cursor ended up, so dragging
            // off a node before releasing leaves nothing stuck active.
            set_active_node(doc, None);
            state.pending_selection = None;
            state.scrollbar_drag = None;
            doc.set_scrollbar_grab(None);
            doc.dispatch_mouse_up_to(target, &mut mouse);

            if state.pending_click.is_some_and(|down| {
                down.target == target
                    && down.x == mouse.x
                    && down.y == mouse.y
                    && down.button == mouse.button
            }) {
                let mut click = MouseEvent::new(mouse.x, mouse.y, mouse.button);
                doc.dispatch_click_to(target, &mut click);
            }
            state.pending_click = None;
        }
        RuntimeEvent::Wheel(mut wheel) => {
            let target = mouse_target(doc, wheel.x, wheel.y);
            doc.dispatch_wheel_to(target, &mut wheel);
            if !wheel.default_prevented() {
                doc.apply_wheel_default_action(target, &wheel);
            }
            state.pending_click = None;
        }
        RuntimeEvent::Resize(resize) => {
            doc.dispatch_resize(resize.clone());
            set_pending_resize(doc, resize);
        }
        // Window focus is the OS window, not the DOM. It deliberately leaves the
        // focused node and the focus stack alone: alt-tabbing away and back must
        // return the user to the node they left, not to nothing.
        RuntimeEvent::WindowFocus => {
            doc.dispatch_window_focus();
        }
        RuntimeEvent::WindowBlur => {
            doc.dispatch_window_blur();
        }
        // Not input: a frame between a mouse down and up must not suppress the click.
        RuntimeEvent::PostFrame(mut event) => {
            doc.dispatch_post_frame(&mut event);
        }
        // Not input either: a transition finishing mid-click must not suppress it.
        // A node removed after its transition finished has an empty event path, so
        // dispatch is naturally a no-op.
        RuntimeEvent::TransitionEnd { node, property } => {
            doc.dispatch_transition_end_to(node, property);
        }
        RuntimeEvent::AnimationEnd { node, handle } => {
            doc.dispatch_animation_end_to(node, handle);
        }
        RuntimeEvent::AnimationIteration {
            node,
            handle,
            iteration,
        } => {
            doc.dispatch_animation_iteration_to(node, handle, iteration);
        }
    }
}

fn mouse_target(doc: &Document, x: i32, y: i32) -> NodeId {
    doc.node_at(x, y).unwrap_or_else(|| doc.root())
}

/// Start a scrollbar drag from a press on a strip.
///
/// A press on the thumb grabs it in place without scrolling, so an unmoved press
/// never perturbs the offset through rounding. A press on the track jumps the
/// thumb's center under the cursor and continues grabbed there.
fn begin_scrollbar_grab(
    doc: &Document,
    entry: &PaintEntry,
    x: i32,
    y: i32,
) -> Option<ScrollbarDrag> {
    let bar = entry.scrollbar?;
    let pos = strip_position(&entry.layout, bar.vertical, x, y);
    let span = strip_span(&entry.layout, bar.vertical);
    let range = i32::from(span.saturating_sub(bar.thumb_len));

    let thumb_start = i32::from(bar.thumb_start);
    let on_thumb = pos >= thumb_start && pos < thumb_start + i32::from(bar.thumb_len);
    let (grab, jump) = if on_thumb {
        (pos - thumb_start, false)
    } else {
        let new_start = (pos - i32::from(bar.thumb_len / 2)).clamp(0, range);
        (pos - new_start, true)
    };

    let drag = ScrollbarDrag {
        container: entry.id,
        vertical: bar.vertical,
        grab: u16::try_from(grab).ok()?,
    };
    if jump {
        update_scrollbar_drag(doc, drag, x, y);
    }
    Some(drag)
}

/// Move a grabbed scrollbar to follow the cursor.
///
/// Geometry is read fresh from paint order and the layout snapshot each time, so
/// the drag stays true to what is on screen across relayouts. Only the cursor's
/// position along the strip axis matters; cross-axis movement is ignored. A bar
/// that is no longer shown leaves the offset where it was.
fn update_scrollbar_drag(doc: &Document, drag: ScrollbarDrag, x: i32, y: i32) {
    let Some(entry) = scrollbar_strip_of(doc, drag.container, drag.vertical) else {
        return;
    };
    let Some(bar) = entry.scrollbar else {
        return;
    };
    let span = strip_span(&entry.layout, drag.vertical);
    let range = i32::from(span.saturating_sub(bar.thumb_len));
    let pos = strip_position(&entry.layout, drag.vertical, x, y);
    let new_start = (pos - i32::from(drag.grab)).clamp(0, range) as u16;

    let Some(view) = doc.get_node(drag.container).and_then(|view| view.layout) else {
        return;
    };
    let (viewport, max_scroll) = if drag.vertical {
        (view.scrollport.height, view.max_scroll_y)
    } else {
        (view.scrollport.width, view.max_scroll_x)
    };

    let offset = offset_for_thumb(span, viewport, max_scroll, new_start);
    let current = doc.scroll_offset(drag.container);
    let (to_x, to_y) = if drag.vertical {
        (current.x, offset)
    } else {
        (offset, current.y)
    };
    if let Err(err) = doc.scroll_to(drag.container, to_x, to_y) {
        log::error!("scrollbar drag scroll failed: {err}");
    }
}

fn strip_position(layout: &LayoutRect, vertical: bool, x: i32, y: i32) -> i32 {
    if vertical { y - layout.y } else { x - layout.x }
}

fn strip_span(layout: &LayoutRect, vertical: bool) -> u16 {
    if vertical {
        layout.height
    } else {
        layout.width
    }
}

fn set_active_node(doc: &Document, node: Option<NodeId>) {
    if let Err(err) = doc.set_active_node(node) {
        log::error!("active state update failed: {err}");
    }
}

fn focus_hover_target(doc: &Document, x: i32, y: i32) {
    let Some(hit) = doc.node_at(x, y) else {
        return;
    };
    let Some(target) = doc.focus_target_from_hit(hit) else {
        return;
    };
    if doc.focused() != Some(target)
        && let Err(err) = doc.focus(target)
    {
        log::error!("hover focus failed: {err}");
    }
}

fn set_pending_resize(doc: &Document, resize: ResizeEvent) {
    *lock::mutex(&doc.inner.pending_resize) = Some((resize.width, resize.height));
    doc.inner.resize_notify.notify_one();
}

/// Take the latest pending resize dimensions for the terminal render task.
pub(crate) fn take_pending_resize(doc: &Document) -> Option<(u16, u16)> {
    lock::mutex(&doc.inner.pending_resize).take()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    use crate::event::KeyCode;

    fn moved(x: i32, held: Option<MouseButton>) -> RuntimeEvent {
        RuntimeEvent::MouseMove { x, y: 0, held }
    }

    fn resized(width: u16) -> RuntimeEvent {
        RuntimeEvent::Resize(ResizeEvent { width, height: 24 })
    }

    fn coalesced(events: Vec<RuntimeEvent>) -> Vec<RuntimeEvent> {
        let mut batch = events;
        coalesce(&mut batch);
        batch
    }

    fn move_xs(events: &[RuntimeEvent]) -> Vec<i32> {
        events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::MouseMove { x, .. } => Some(*x),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_single_event_batch_is_left_alone() {
        let batch = coalesced(vec![moved(3, None)]);
        assert_eq!(move_xs(&batch), vec![3]);
    }

    #[test]
    fn a_run_of_moves_collapses_to_the_last_position() {
        let batch = coalesced(vec![moved(1, None), moved(2, None), moved(3, None)]);
        assert_eq!(batch.len(), 1);
        assert_eq!(move_xs(&batch), vec![3]);
    }

    #[test]
    fn a_key_between_move_runs_breaks_them() {
        let batch = coalesced(vec![
            moved(1, None),
            moved(2, None),
            RuntimeEvent::KeyPress(KeyEvent::new(KeyCode::Char('a'))),
            moved(8, None),
            moved(9, None),
        ]);

        // Hover decides which node the key targets, so the pointer must be at 2
        // when the key runs — not fast-forwarded to 9.
        assert_eq!(batch.len(), 3);
        assert_eq!(move_xs(&batch), vec![2, 9]);
        assert!(matches!(batch[1], RuntimeEvent::KeyPress(_)));
    }

    #[test]
    fn moves_do_not_merge_across_a_held_button_change() {
        let batch = coalesced(vec![
            moved(1, None),
            moved(2, None),
            moved(5, Some(MouseButton::Left)),
            moved(6, Some(MouseButton::Left)),
        ]);

        assert_eq!(
            move_xs(&batch),
            vec![2, 6],
            "hover and drag are different gestures"
        );
    }

    #[test]
    fn a_press_between_move_runs_breaks_them() {
        let batch = coalesced(vec![
            moved(1, None),
            moved(2, None),
            RuntimeEvent::MouseDown(MouseEvent::new(2, 0, MouseButton::Left)),
            moved(7, Some(MouseButton::Left)),
            moved(8, Some(MouseButton::Left)),
        ]);

        assert_eq!(batch.len(), 3);
        assert_eq!(move_xs(&batch), vec![2, 8]);
    }

    #[test]
    fn a_run_of_resizes_collapses_to_the_final_size() {
        let batch = coalesced(vec![resized(80), resized(100), resized(120)]);

        assert_eq!(batch.len(), 1);
        match &batch[0] {
            RuntimeEvent::Resize(resize) => assert_eq!(resize.width, 120),
            other => panic!("expected a resize, got {other:?}"),
        }
    }

    #[test]
    fn presses_releases_and_wheel_ticks_are_never_dropped() {
        let batch = coalesced(vec![
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            RuntimeEvent::MouseUp(MouseEvent::new(1, 2, MouseButton::Left)),
            RuntimeEvent::Wheel(WheelEvent::new(1, 2, 1)),
            RuntimeEvent::Wheel(WheelEvent::new(1, 2, 1)),
            RuntimeEvent::Wheel(WheelEvent::new(1, 2, 1)),
        ]);

        // Wheel deltas stay separate: scroll chaining decides per event whether a
        // container can still move before passing to its ancestor.
        assert_eq!(batch.len(), 5);
    }

    #[test]
    fn interleaved_moves_and_keys_keep_their_order() {
        let batch = coalesced(vec![
            moved(1, None),
            RuntimeEvent::KeyPress(KeyEvent::new(KeyCode::Char('a'))),
            moved(2, None),
            RuntimeEvent::KeyPress(KeyEvent::new(KeyCode::Char('b'))),
        ]);

        assert_eq!(batch.len(), 4, "nothing here is adjacent to its own kind");
        assert_eq!(move_xs(&batch), vec![1, 2]);
    }

    #[test]
    fn draining_takes_everything_already_queued_and_coalesces_it() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        for x in 1..=5 {
            tx.send(moved(x, None)).unwrap();
        }
        tx.send(RuntimeEvent::KeyPress(KeyEvent::new(KeyCode::Char('z'))))
            .unwrap();

        let first = rx.try_recv().unwrap();
        let batch = drain_and_coalesce(first, &mut rx);

        assert_eq!(move_xs(&batch), vec![5]);
        assert_eq!(batch.len(), 2);
        assert!(rx.try_recv().is_err(), "the drain empties the queue");
    }

    #[test]
    fn draining_an_empty_queue_yields_just_the_first_event() {
        let (_tx, mut rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let batch = drain_and_coalesce(moved(4, None), &mut rx);
        assert_eq!(move_xs(&batch), vec![4]);
    }

    /// The invariant collapsing must not break: a click is synthesized from a
    /// matching down/up pair, and the movement between them is irrelevant to it.
    #[test]
    fn a_click_survives_a_collapsed_move_run() {
        let doc = Document::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();
        doc.on_click(doc.root(), move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        let batch = coalesced(vec![
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            moved(1, Some(MouseButton::Left)),
            moved(1, Some(MouseButton::Left)),
            RuntimeEvent::MouseUp(MouseEvent::new(1, 2, MouseButton::Left)),
        ]);
        assert_eq!(batch.len(), 3, "only the move run collapses");

        let mut state = RuntimeEventState::default();
        for event in batch {
            process_runtime_event(&doc, event, &mut state);
        }

        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pending_resize_keeps_latest_size() {
        let doc = Document::new().unwrap();

        process_runtime_event(
            &doc,
            RuntimeEvent::Resize(ResizeEvent {
                width: 80,
                height: 24,
            }),
            &mut RuntimeEventState::default(),
        );
        process_runtime_event(
            &doc,
            RuntimeEvent::Resize(ResizeEvent {
                width: 120,
                height: 40,
            }),
            &mut RuntimeEventState::default(),
        );

        assert_eq!(take_pending_resize(&doc), Some((120, 40)));
        assert_eq!(take_pending_resize(&doc), None);
    }

    #[test]
    fn click_is_generated_from_matching_down_and_up() {
        let doc = Document::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();
        doc.on_click(doc.root(), move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        let mut state = RuntimeEventState::default();
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut state,
        );
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseUp(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut state,
        );

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(state.pending_click.is_none());
    }

    #[test]
    fn click_is_not_generated_when_up_cell_differs() {
        let doc = Document::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();
        doc.on_click(doc.root(), move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

        let mut state = RuntimeEventState::default();
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut state,
        );
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseUp(MouseEvent::new(2, 2, MouseButton::Left)),
            &mut state,
        );

        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(state.pending_click.is_none());
    }
}

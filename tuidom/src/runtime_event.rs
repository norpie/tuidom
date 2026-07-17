use crate::animation::TransitionProperty;
use crate::document::Document;
use crate::document::selection::PendingSelection;
use crate::event::{KeyEvent, MouseButton, MouseEvent, PostFrameEvent, ResizeEvent, WheelEvent};
use crate::id::NodeId;
use crate::lock;

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
    /// A rendered frame, queued by the render task for post-frame dispatch.
    /// Boxed so the frame's metrics don't dominate the size of every queued event.
    PostFrame(Box<PostFrameEvent>),
    /// A completed transition, queued by animation upkeep for end-event dispatch.
    TransitionEnd {
        node: NodeId,
        property: TransitionProperty,
    },
}

#[derive(Debug, Clone, Copy)]
struct ClickCandidate {
    target: NodeId,
    x: i32,
    y: i32,
    button: MouseButton,
}

/// Stateful runtime-event processing data shared by terminal and headless runtimes.
#[derive(Debug, Default)]
pub(crate) struct RuntimeEventState {
    pending_click: Option<ClickCandidate>,
    pending_selection: Option<PendingSelection>,
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
            let target = mouse_target(doc, mouse.x, mouse.y);
            // The pressed node is the node that would take focus from this hit, so
            // "being pressed" and "would be focused" never disagree.
            set_active_node(doc, doc.focus_target_from_hit(target));
            doc.dispatch_mouse_down_to(target, &mut mouse);
            state.pending_click = Some(ClickCandidate {
                target,
                x: mouse.x,
                y: mouse.y,
                button: mouse.button,
            });

            // The mouse default action: a left press clears the selection and arms a
            // new drag from this point. Preventing it keeps the selection too — the
            // press was claimed for something other than selecting.
            state.pending_selection = None;
            if mouse.button == MouseButton::Left && !mouse.default_prevented() {
                doc.clear_selection();
                state.pending_selection = doc.begin_selection_drag(mouse.x, mouse.y, target);
            }
        }
        RuntimeEvent::MouseMove { x, y, held } => {
            // A drag must not yank focus from node to node as it crosses the screen —
            // hover-to-focus applies only to unpressed movement.
            if held.is_none() {
                focus_hover_target(doc, x, y);
            } else if held == Some(MouseButton::Left)
                && let Some(pending) = &state.pending_selection
            {
                doc.update_selection_drag(pending, x, y);
            }
        }
        RuntimeEvent::MouseUp(mut mouse) => {
            let target = mouse_target(doc, mouse.x, mouse.y);
            // Release clears the pressed state wherever the cursor ended up, so dragging
            // off a node before releasing leaves nothing stuck active.
            set_active_node(doc, None);
            state.pending_selection = None;
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
    }
}

fn mouse_target(doc: &Document, x: i32, y: i32) -> NodeId {
    doc.node_at(x, y).unwrap_or_else(|| doc.root())
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

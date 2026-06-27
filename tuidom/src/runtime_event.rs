use crate::document::Document;
use crate::event::{KeyEvent, MouseButton, MouseEvent, ResizeEvent, WheelEvent};
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
    /// A mouse pointer movement from the input stream.
    MouseMove { x: i32, y: i32 },
    /// A mouse button release from the input stream.
    MouseUp(MouseEvent),
    /// A mouse wheel movement from the input stream.
    Wheel(WheelEvent),
    /// Runtime resize requiring both public dispatch and renderer resize.
    Resize(ResizeEvent),
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
            doc.dispatch_mouse_down_to(target, &mut mouse);
            state.pending_click = Some(ClickCandidate {
                target,
                x: mouse.x,
                y: mouse.y,
                button: mouse.button,
            });
        }
        RuntimeEvent::MouseMove { x, y } => {
            focus_hover_target(doc, x, y);
        }
        RuntimeEvent::MouseUp(mut mouse) => {
            let target = mouse_target(doc, mouse.x, mouse.y);
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
            state.pending_click = None;
        }
        RuntimeEvent::Resize(resize) => {
            doc.dispatch_resize(resize.clone());
            set_pending_resize(doc, resize);
        }
    }
}

fn mouse_target(doc: &Document, x: i32, y: i32) -> NodeId {
    doc.node_at(x, y).unwrap_or_else(|| doc.root())
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

//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::time::Instant;

use crossterm::event::KeyEventKind;
use crossterm::event::{
    Event as CrosstermEvent, EventStream, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind,
};
use tokio::task::JoinSet;
use tokio::time::{Instant as TokioInstant, sleep_until};
use tokio_stream::StreamExt;

use crate::document::Document;
use crate::event::{KeyEvent, MouseButton, MouseEvent, ResizeEvent, WheelEvent, convert_key_event};
use crate::lock;
use crate::render::Renderer;

/// Internal runtime event used by the event queue.
///
/// This is separate from public event structs so runtime coordination can evolve
/// without exposing renderer/input-loop details as user-facing API.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    /// A key press from the terminal input stream.
    KeyPress(KeyEvent),
    /// A mouse button press from the terminal input stream.
    MouseDown(MouseEvent),
    /// A mouse button release from the terminal input stream.
    MouseUp(MouseEvent),
    /// A mouse wheel movement from the terminal input stream.
    Wheel(WheelEvent),
    /// Terminal resize requiring both public dispatch and renderer resize.
    Resize(ResizeEvent),
}

#[derive(Debug, Clone, Copy)]
struct ClickCandidate {
    target: crate::id::NodeId,
    x: i32,
    y: i32,
    button: MouseButton,
}

/// Run the runtime tasks until [`Document::quit`] is called or a critical task errors.
pub(crate) async fn run(doc: Document) -> io::Result<()> {
    let mut tasks = JoinSet::new();

    tasks.spawn(input_task(doc.clone()));
    tasks.spawn(event_task(doc.clone()));
    tasks.spawn(render_task(doc.clone()));

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {
                if !is_shutdown(&doc) {
                    doc.quit();
                }
            }
            Ok(Err(err)) => {
                doc.quit();
                tasks.abort_all();
                return Err(err);
            }
            Err(err) => {
                doc.quit();
                tasks.abort_all();
                return Err(io::Error::other(err));
            }
        }
    }

    Ok(())
}

async fn input_task(doc: Document) -> io::Result<()> {
    let mut event_stream = EventStream::new();

    loop {
        if is_shutdown(&doc) {
            break;
        }

        tokio::select! {
            _ = doc.inner.shutdown_notify.notified() => break,
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(crossterm_event)) => {
                        if let Some(runtime_event) = convert_terminal_event(crossterm_event) {
                            enqueue_runtime_event(&doc, runtime_event)?;
                        }
                    }
                    Some(Err(err)) => return Err(err),
                    None => {
                        if is_shutdown(&doc) {
                            break;
                        }
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "terminal event stream ended",
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

async fn event_task(doc: Document) -> io::Result<()> {
    let mut pending_click = None;

    loop {
        if is_shutdown(&doc) {
            break;
        }

        tokio::select! {
            _ = doc.inner.shutdown_notify.notified() => break,
            maybe_event = recv_runtime_event(&doc) => {
                let Some(event) = maybe_event else {
                    if is_shutdown(&doc) {
                        break;
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "runtime event queue closed",
                    ));
                };
                process_runtime_event(&doc, event, &mut pending_click);
            }
        }
    }

    Ok(())
}

async fn render_task(doc: Document) -> io::Result<()> {
    let (mut screen_w, mut screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(screen_w, screen_h)?;
    let mut next_frame_at = None;

    render_frame_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at).await?;

    loop {
        if is_shutdown(&doc) {
            break;
        }

        let animation_frame_needed = animations_active(&doc);

        tokio::select! {
            _ = doc.inner.shutdown_notify.notified() => break,

            _ = doc.inner.resize_notify.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                if let Some((width, height)) = take_pending_resize(&doc) {
                    screen_w = width;
                    screen_h = height;
                    renderer.resize(width, height);
                    render_full_timed_capped(
                        &doc,
                        &mut renderer,
                        screen_w,
                        screen_h,
                        &mut next_frame_at,
                    )
                    .await?;
                }
            }

            _ = doc.inner.notify.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                render_frame_timed_capped(
                    &doc,
                    &mut renderer,
                    screen_w,
                    screen_h,
                    &mut next_frame_at,
                )
                .await?;
            }

            _ = doc.inner.anim_config_changed.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                if animations_active(&doc) {
                    render_frame_timed_capped(
                        &doc,
                        &mut renderer,
                        screen_w,
                        screen_h,
                        &mut next_frame_at,
                    )
                    .await?;
                }
            }

            _ = tokio::task::yield_now(), if animation_frame_needed => {
                cleanup_animations(&doc);
                render_frame_timed_capped(
                    &doc,
                    &mut renderer,
                    screen_w,
                    screen_h,
                    &mut next_frame_at,
                )
                .await?;
            }
        }
    }

    Ok(())
}

fn is_shutdown(doc: &Document) -> bool {
    *lock::rw_read(&doc.inner.shutdown)
}

/// Enqueue a runtime event for sequential processing.
fn enqueue_runtime_event(doc: &Document, event: RuntimeEvent) -> io::Result<()> {
    doc.inner.event_tx.send(event).map_err(|_| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "runtime event queue receiver is closed",
        )
    })
}

/// Receive the next queued runtime event.
async fn recv_runtime_event(doc: &Document) -> Option<RuntimeEvent> {
    doc.inner.event_rx.lock().await.recv().await
}

/// Process one queued runtime event.
fn process_runtime_event(
    doc: &Document,
    event: RuntimeEvent,
    pending_click: &mut Option<ClickCandidate>,
) {
    match event {
        RuntimeEvent::KeyPress(key) => {
            doc.dispatch_key_press(key);
        }
        RuntimeEvent::MouseDown(mut mouse) => {
            let target = mouse_target(doc, mouse.x, mouse.y);
            doc.dispatch_mouse_down_to(target, &mut mouse);
            *pending_click = Some(ClickCandidate {
                target,
                x: mouse.x,
                y: mouse.y,
                button: mouse.button,
            });
        }
        RuntimeEvent::MouseUp(mut mouse) => {
            let target = mouse_target(doc, mouse.x, mouse.y);
            doc.dispatch_mouse_up_to(target, &mut mouse);

            if pending_click.is_some_and(|down| {
                down.target == target
                    && down.x == mouse.x
                    && down.y == mouse.y
                    && down.button == mouse.button
            }) {
                let mut click = MouseEvent::new(mouse.x, mouse.y, mouse.button);
                doc.dispatch_click_to(target, &mut click);
            }
            *pending_click = None;
        }
        RuntimeEvent::Wheel(mut wheel) => {
            let target = mouse_target(doc, wheel.x, wheel.y);
            doc.dispatch_wheel_to(target, &mut wheel);
            *pending_click = None;
        }
        RuntimeEvent::Resize(resize) => {
            doc.dispatch_resize(resize.clone());
            set_pending_resize(doc, resize);
        }
    }
}

fn mouse_target(doc: &Document, x: i32, y: i32) -> crate::id::NodeId {
    doc.node_at(x, y).unwrap_or_else(|| doc.root())
}

fn set_pending_resize(doc: &Document, resize: ResizeEvent) {
    *lock::mutex(&doc.inner.pending_resize) = Some((resize.width, resize.height));
    doc.inner.resize_notify.notify_one();
}

fn take_pending_resize(doc: &Document) -> Option<(u16, u16)> {
    lock::mutex(&doc.inner.pending_resize).take()
}

/// Render a diffed frame with timing for the debug overlay.
fn render_frame_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) -> io::Result<()> {
    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh).map_err(io::Error::other)?;
    let layout_time = layout_start.elapsed();

    let stats = renderer.render_frame(doc)?;

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, stats);
    Ok(())
}

/// Render a full redraw with timing for the debug overlay.
fn render_full_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) -> io::Result<()> {
    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh).map_err(io::Error::other)?;
    let layout_time = layout_start.elapsed();

    let stats = renderer.render_full(doc)?;

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, stats);
    Ok(())
}

async fn render_frame_timed_capped(
    doc: &Document,
    renderer: &mut Renderer,
    sw: u16,
    sh: u16,
    next_frame_at: &mut Option<TokioInstant>,
) -> io::Result<()> {
    if !wait_for_frame_slot(doc, *next_frame_at).await {
        return Ok(());
    }

    let started_at = TokioInstant::now();
    render_frame_timed(doc, renderer, sw, sh)?;
    advance_frame_slot(doc, next_frame_at, started_at);
    Ok(())
}

async fn render_full_timed_capped(
    doc: &Document,
    renderer: &mut Renderer,
    sw: u16,
    sh: u16,
    next_frame_at: &mut Option<TokioInstant>,
) -> io::Result<()> {
    if !wait_for_frame_slot(doc, *next_frame_at).await {
        return Ok(());
    }

    let started_at = TokioInstant::now();
    render_full_timed(doc, renderer, sw, sh)?;
    advance_frame_slot(doc, next_frame_at, started_at);
    Ok(())
}

async fn wait_for_frame_slot(doc: &Document, next_frame_at: Option<TokioInstant>) -> bool {
    if lock::rw_read(&doc.inner.max_frame_interval).is_none() {
        return true;
    }
    let Some(deadline) = next_frame_at else {
        return true;
    };

    if TokioInstant::now() >= deadline {
        return true;
    }

    tokio::select! {
        _ = doc.inner.shutdown_notify.notified() => false,
        _ = sleep_until(deadline) => true,
    }
}

fn advance_frame_slot(
    doc: &Document,
    next_frame_at: &mut Option<TokioInstant>,
    started_at: TokioInstant,
) {
    let interval = *lock::rw_read(&doc.inner.max_frame_interval);
    let Some(interval) = interval else {
        *next_frame_at = None;
        return;
    };

    let mut next = next_frame_at.map_or(started_at + interval, |deadline| deadline + interval);
    while next <= started_at {
        next += interval;
    }
    *next_frame_at = Some(next);
}

fn animations_active(doc: &Document) -> bool {
    lock::mutex(&doc.inner.animation).has_active()
}

fn cleanup_animations(doc: &Document) -> bool {
    lock::mutex(&doc.inner.animation).cleanup()
}

/// Convert a crossterm event into an internal runtime event.
fn convert_terminal_event(event: CrosstermEvent) -> Option<RuntimeEvent> {
    match event {
        CrosstermEvent::Resize(width, height) => {
            Some(RuntimeEvent::Resize(ResizeEvent { width, height }))
        }
        CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
            Some(RuntimeEvent::KeyPress(convert_key_event(key)))
        }
        CrosstermEvent::Mouse(mouse) => convert_mouse_event(mouse),
        _ => None,
    }
}

fn convert_mouse_event(mouse: CrosstermMouseEvent) -> Option<RuntimeEvent> {
    match mouse.kind {
        MouseEventKind::Down(button) => convert_mouse_button(button).map(|button| {
            RuntimeEvent::MouseDown(MouseEvent::new(
                i32::from(mouse.column),
                i32::from(mouse.row),
                button,
            ))
        }),
        MouseEventKind::Up(button) => convert_mouse_button(button).map(|button| {
            RuntimeEvent::MouseUp(MouseEvent::new(
                i32::from(mouse.column),
                i32::from(mouse.row),
                button,
            ))
        }),
        MouseEventKind::ScrollUp => Some(RuntimeEvent::Wheel(WheelEvent::new(
            i32::from(mouse.column),
            i32::from(mouse.row),
            1,
        ))),
        MouseEventKind::ScrollDown => Some(RuntimeEvent::Wheel(WheelEvent::new(
            i32::from(mouse.column),
            i32::from(mouse.row),
            -1,
        ))),
        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => None,
        MouseEventKind::Drag(_) | MouseEventKind::Moved => None,
    }
}

fn convert_mouse_button(button: CrosstermMouseButton) -> Option<MouseButton> {
    match button {
        CrosstermMouseButton::Left => Some(MouseButton::Left),
        CrosstermMouseButton::Right => Some(MouseButton::Right),
        CrosstermMouseButton::Middle => Some(MouseButton::Middle),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn pending_resize_keeps_latest_size() {
        let doc = Document::new().unwrap();

        set_pending_resize(
            &doc,
            ResizeEvent {
                width: 80,
                height: 24,
            },
        );
        set_pending_resize(
            &doc,
            ResizeEvent {
                width: 120,
                height: 40,
            },
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

        let mut pending_click = None;
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut pending_click,
        );
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseUp(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut pending_click,
        );

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(pending_click.is_none());
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

        let mut pending_click = None;
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseDown(MouseEvent::new(1, 2, MouseButton::Left)),
            &mut pending_click,
        );
        process_runtime_event(
            &doc,
            RuntimeEvent::MouseUp(MouseEvent::new(2, 2, MouseButton::Left)),
            &mut pending_click,
        );

        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(pending_click.is_none());
    }
}

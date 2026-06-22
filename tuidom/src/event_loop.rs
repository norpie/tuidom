//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::time::Instant;

use crossterm::event::KeyEventKind;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode as CrosstermKeyCode};
use tokio_stream::StreamExt;

use crate::document::Document;
use crate::event::{Event, KeyCode, KeyEvent, ResizeEvent};
use crate::lock;
use crate::render::Renderer;

/// Internal runtime event used by the event queue.
///
/// This is separate from public [`Event`] so runtime coordination can evolve
/// without exposing renderer/input-loop details as user-facing API.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    /// A key press from the terminal input stream.
    KeyPress(KeyEvent),
    /// Terminal resize requiring both public dispatch and renderer resize.
    Resize(ResizeEvent),
}

/// Run the main loop: layout, paint, diff, flush, and event dispatch.
///
/// Blocks until [`Document::quit`] is called from a handler.
pub(crate) async fn run(doc: Document) -> io::Result<()> {
    let (mut screen_w, mut screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(screen_w, screen_h)?;
    let mut event_stream = EventStream::new();
    let inner = doc.inner.clone();

    // Initial render
    render_frame_timed(&doc, &mut renderer, screen_w, screen_h)?;

    loop {
        if *lock::rw_read(&inner.shutdown) {
            break;
        }

        tokio::select! {
            // DOM mutations → re-render
            _ = inner.notify.notified() => {
                if *lock::rw_read(&inner.shutdown) {
                    break;
                }
                render_frame_timed(&doc, &mut renderer, screen_w, screen_h)?;
            }

            // Terminal events (resize, keyboard) → runtime event queue
            maybe_event = event_stream.next() => {
                if let Some(Ok(crossterm_event)) = maybe_event
                    && let Some(runtime_event) = convert_terminal_event(crossterm_event)
                {
                    enqueue_runtime_event(&doc, runtime_event);
                }
            }

            // Runtime event queue → sequential dispatch / renderer coordination
            maybe_event = recv_runtime_event(&doc) => {
                let Some(event) = maybe_event else {
                    break;
                };
                process_runtime_event(
                    &doc,
                    &mut renderer,
                    &mut screen_w,
                    &mut screen_h,
                    event,
                )?;
            }

            // Animation tick → re-render
            _ = inner.anim_tick.notified() => {
                if *lock::rw_read(&inner.shutdown) {
                    break;
                }
                render_frame_timed(&doc, &mut renderer, screen_w, screen_h)?;
            }
        }
    }

    Ok(())
}

/// Render a diffed frame with timing for the debug overlay.
fn render_frame_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) -> io::Result<()> {
    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh);
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
    doc.compute_layout(sw, sh);
    let layout_time = layout_start.elapsed();

    let stats = renderer.render_full(doc)?;

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, stats);
    Ok(())
}

/// Enqueue a runtime event for sequential processing.
fn enqueue_runtime_event(doc: &Document, event: RuntimeEvent) {
    if doc.inner.event_tx.send(event).is_err() {
        log::error!("runtime event queue receiver is closed");
    }
}

/// Receive the next queued runtime event.
async fn recv_runtime_event(doc: &Document) -> Option<RuntimeEvent> {
    doc.inner.event_rx.lock().await.recv().await
}

/// Process one queued runtime event.
fn process_runtime_event(
    doc: &Document,
    renderer: &mut Renderer,
    screen_w: &mut u16,
    screen_h: &mut u16,
    event: RuntimeEvent,
) -> io::Result<()> {
    match event {
        RuntimeEvent::KeyPress(key) => {
            doc.dispatch_event(Event::KeyPress(key));
            Ok(())
        }
        RuntimeEvent::Resize(resize) => {
            *screen_w = resize.width;
            *screen_h = resize.height;
            renderer.resize(resize.width, resize.height);

            doc.dispatch_event(Event::Resize(resize));
            render_full_timed(doc, renderer, *screen_w, *screen_h)
        }
    }
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
        _ => None,
    }
}

/// Convert a crossterm key event to a tuidom key event.
fn convert_key_event(key: crossterm::event::KeyEvent) -> KeyEvent {
    let code = match key.code {
        CrosstermKeyCode::Char(c) => KeyCode::Char(c),
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::F(n) => KeyCode::F(n),
        _ => KeyCode::Char('?'), // unhandled keys → '?'
    };

    KeyEvent { code }
}

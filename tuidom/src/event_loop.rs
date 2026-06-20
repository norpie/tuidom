//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::time::Instant;

use crossterm::event::KeyEventKind;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode as CrosstermKeyCode};
use tokio_stream::StreamExt;

use crate::document::Document;
use crate::render::Renderer;

/// Run the main loop: layout, paint, diff, flush, and event dispatch.
///
/// Blocks until [`Document::quit`] is called from a handler.
pub(crate) async fn run(doc: Document) -> io::Result<()> {
    let (mut screen_w, mut screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(screen_w, screen_h)?;
    let mut event_stream = EventStream::new();
    let inner = doc.inner.clone();

    // Initial render
    render_frame_timed(&doc, &mut renderer, screen_w, screen_h);

    loop {
        if *inner.shutdown.read().expect("lock poisoned") {
            break;
        }

        tokio::select! {
            // DOM mutations → re-render
            _ = inner.notify.notified() => {
                if *inner.shutdown.read().expect("lock poisoned") {
                    break;
                }
                render_frame_timed(&doc, &mut renderer, screen_w, screen_h);
            }

            // Terminal events (resize, keyboard)
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Resize(w, h))) => {
                        screen_w = w;
                        screen_h = h;
                        renderer.resize(w, h);
                        doc.compute_layout(w, h);
                        let _ = renderer.render_full(&doc);
                    }
                    Some(Ok(CrosstermEvent::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            let event = convert_key_event(key);
                            doc.dispatch_event(event);
                        }
                    }
                    _ => {}
                }
            }

            // Animation tick → re-render
            _ = inner.anim_tick.notified() => {
                if *inner.shutdown.read().expect("lock poisoned") {
                    break;
                }
                render_frame_timed(&doc, &mut renderer, screen_w, screen_h);
            }
        }
    }

    Ok(())
}

/// Render a frame with timing for the debug overlay.
fn render_frame_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) {
    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh);
    let layout_time = layout_start.elapsed();

    let render_start = Instant::now();
    let cells = renderer.render_frame(doc).unwrap_or(0);
    let render_time = render_start.elapsed();

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, render_time, cells);
}

/// Convert a crossterm key event to a tuidom [`Event`].
fn convert_key_event(key: crossterm::event::KeyEvent) -> crate::event::Event {
    use crate::event::{Event, KeyCode, KeyEvent};

    let code = match key.code {
        CrosstermKeyCode::Char(c) => KeyCode::Char(c),
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::F(n) => KeyCode::F(n),
        _ => KeyCode::Char('?'), // unhandled keys → '?'
    };

    Event::KeyPress(KeyEvent { code })
}

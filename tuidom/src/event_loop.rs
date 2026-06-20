//! Render + event loop — the main `Document::run()` implementation.

use std::io;

use crossterm::event::{Event, EventStream, KeyEventKind};
use tokio_stream::StreamExt;

use crate::document::Document;
use crate::render::Renderer;

/// Run the main loop: layout, paint, diff, flush, and event dispatch.
///
/// Blocks until [`Document::quit`] is called from a handler.
pub(crate) async fn run(doc: Document) -> io::Result<()> {
    let (screen_w, screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(screen_w, screen_h)?;
    let mut event_stream = EventStream::new();
    let inner = doc.inner.clone();

    // Initial render
    doc.compute_layout(screen_w, screen_h);
    renderer.render_frame(&doc)?;

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
                doc.compute_layout(screen_w, screen_h);
                renderer.render_frame(&doc)?;
            }

            // Terminal events (resize, keyboard)
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(Event::Resize(w, h))) => {
                        renderer.resize(w, h);
                        inner.notify.notify_one();
                    }
                    Some(Ok(Event::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            // TODO: event dispatch (phase 8 — debug overlay, smoke test)
                            let _ = key;
                        }
                    }
                    _ => {}
                }
            }

            // Animation tick → re-render
            // Fires only while animations are active (tick task periodically
            // calls notify_one). When idle, this branch never fires.
            _ = inner.anim_tick.notified() => {
                if *inner.shutdown.read().expect("lock poisoned") {
                    break;
                }
                doc.compute_layout(screen_w, screen_h);
                renderer.render_frame(&doc)?;
            }
        }
    }

    Ok(())
}

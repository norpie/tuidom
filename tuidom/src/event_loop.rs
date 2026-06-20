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

    // Initial render
    doc.compute_layout(screen_w, screen_h);
    renderer.render_frame(&doc)?;

    loop {
        // Check shutdown before blocking
        if *doc.inner.shutdown.read().expect("lock poisoned") {
            break;
        }

        tokio::select! {
            // DOM mutations → re-render
            _ = doc.inner.notify.notified() => {
                if *doc.inner.shutdown.read().expect("lock poisoned") {
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
                        doc.inner.notify.notify_one();
                    }
                    Some(Ok(Event::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            // TODO(phase 8): dispatch to registered handlers
                            let _ = key;
                        }
                    }
                    _ => {}
                }
            }

            // Animation tick arm — added in phase 7
        }
    }

    Ok(())
}

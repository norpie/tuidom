//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::time::Instant;

use crossterm::event::KeyEventKind;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode as CrosstermKeyCode};
use tokio::task::JoinSet;
use tokio::time::{Instant as TokioInstant, sleep_until};
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

/// Internal render-task command.
#[derive(Debug, Clone)]
pub(crate) enum RenderCommand {
    /// Resize terminal buffers and perform a full redraw.
    Resize { width: u16, height: u16 },
    /// Stop the render task.
    Shutdown,
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
                process_runtime_event(&doc, event)?;
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

            maybe_command = recv_render_command(&doc) => {
                let Some(command) = maybe_command else {
                    if is_shutdown(&doc) {
                        break;
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "render command queue closed",
                    ));
                };

                match command {
                    RenderCommand::Resize { width, height } => {
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
                    RenderCommand::Shutdown => break,
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

async fn recv_render_command(doc: &Document) -> Option<RenderCommand> {
    doc.inner.render_rx.lock().await.recv().await
}

/// Process one queued runtime event.
fn process_runtime_event(doc: &Document, event: RuntimeEvent) -> io::Result<()> {
    match event {
        RuntimeEvent::KeyPress(key) => {
            doc.dispatch_event(Event::KeyPress(key));
            Ok(())
        }
        RuntimeEvent::Resize(resize) => {
            doc.dispatch_event(Event::Resize(resize.clone()));
            doc.inner
                .render_tx
                .send(RenderCommand::Resize {
                    width: resize.width,
                    height: resize.height,
                })
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "render command queue receiver is closed",
                    )
                })
        }
    }
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

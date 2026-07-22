//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::time::Instant;

use crossterm::event::KeyEventKind;
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyModifiers, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind,
};
use tokio::task::JoinSet;
use tokio::time::{Instant as TokioInstant, sleep_until};
use tokio_stream::StreamExt;

use crate::document::Document;
use crate::event::{
    KeyModifiers as TuidomKeyModifiers, MouseButton, MouseEvent, ResizeEvent, WheelEvent,
    convert_key_event,
};
use crate::lock;
use crate::render::Renderer;
use crate::runtime_event::{
    RuntimeEvent, RuntimeEventState, drain_and_coalesce, process_runtime_event, take_pending_resize,
};

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
    let mut event_state = RuntimeEventState::default();

    loop {
        if is_shutdown(&doc) {
            break;
        }

        tokio::select! {
            _ = doc.inner.shutdown_notify.notified() => break,
            maybe_batch = recv_runtime_batch(&doc) => {
                let Some(batch) = maybe_batch else {
                    if is_shutdown(&doc) {
                        break;
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "runtime event queue closed",
                    ));
                };
                for event in batch {
                    process_runtime_event(&doc, event, &mut event_state);
                }
            }
        }
    }

    Ok(())
}

async fn render_task(doc: Document) -> io::Result<()> {
    let (mut screen_w, mut screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(screen_w, screen_h)?;
    let mut next_frame_at = None;
    let mut next_anim_frame_at = None;

    render_frame_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at).await?;

    loop {
        if is_shutdown(&doc) {
            break;
        }

        let anim_frame_needed = animation_frame_needed(&doc);
        if !anim_frame_needed {
            next_anim_frame_at = None;
        }

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
                    render_full_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at)
                        .await?;
                }
            }

            _ = doc.inner.notify.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                render_frame_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at)
                    .await?;
            }

            _ = doc.inner.anim_config_changed.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                if animation_frame_needed(&doc) {
                    render_frame_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at)
                        .await?;
                }
            }

            _ = wait_for_anim_tick(&doc, &mut next_anim_frame_at), if anim_frame_needed => {
                cleanup_animations(&doc);
                render_frame_timed_capped(&doc, &mut renderer, screen_w, screen_h, &mut next_frame_at)
                    .await?;
            }
        }
    }

    Ok(())
}

/// Wait until the next animation-driven frame is due, then advance the deadline.
///
/// Transitions and keyframe animations pace frames at the document's animation
/// tick rate; an unlimited rate (`None`) degrades to a yield, rendering as fast
/// as the runtime allows. Missed slots are skipped rather than replayed, so a
/// slow frame does not cause a burst. The deadline lives with the caller and is
/// reset when animations go idle, so a fresh animation never inherits a stale
/// deadline.
///
/// With only frames nodes or waiting `WhenScrolling` bars active, the wait is a
/// plain deadline instead: the next frame flip, or the instant a bar starts its
/// fade. A mid-fade bar ticks smoothly like a transition.
async fn wait_for_anim_tick(doc: &Document, next_anim_frame_at: &mut Option<TokioInstant>) {
    let schedule = doc.scrollbar_fade_schedule(doc.now());
    let (smooth, next_flip) = {
        let driver = lock::mutex(&doc.inner.animation);
        (
            driver.has_smooth_active() || schedule.fading,
            driver.next_frames_flip(doc.now()),
        )
    };

    if !smooth {
        *next_anim_frame_at = None;
        let deadline = [next_flip, schedule.next_deadline]
            .into_iter()
            .flatten()
            .min();
        match deadline {
            Some(at) => sleep_until(TokioInstant::from_std(at)).await,
            // Guarded by animation_frame_needed, so something exists; a schedule
            // that stopped cycling between checks just yields once.
            None => tokio::task::yield_now().await,
        }
        return;
    }

    let Some(interval) = *lock::rw_read(&doc.inner.animation_frame_interval) else {
        *next_anim_frame_at = None;
        tokio::task::yield_now().await;
        return;
    };

    let deadline = *next_anim_frame_at.get_or_insert_with(|| TokioInstant::now() + interval);
    sleep_until(deadline).await;

    let now = TokioInstant::now();
    let mut next = deadline + interval;
    while next <= now {
        next += interval;
    }
    *next_anim_frame_at = Some(next);
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

/// Receive the next queued runtime event, plus everything already behind it.
///
/// The lock is held for the whole batch so a drain cannot interleave with another
/// consumer, and the drain itself never blocks — see [`drain_and_coalesce`].
async fn recv_runtime_batch(doc: &Document) -> Option<Vec<RuntimeEvent>> {
    let mut rx = doc.inner.event_rx.lock().await;
    let first = rx.recv().await?;
    Some(drain_and_coalesce(first, &mut rx))
}

/// Render a diffed frame with timing for the performance API.
fn render_frame_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) -> io::Result<()> {
    // The frame span lives here rather than in `render/`, because a frame is layout *then*
    // paint and the renderer is only entered for the second half. This is the one place
    // both are in scope, so it is the only place that can parent them.
    let _span = tracing::debug_span!("frame", mode = "diffed").entered();

    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh).map_err(io::Error::other)?;
    let layout_time = layout_start.elapsed();

    let stats = renderer.render_frame(doc)?;

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, stats);
    queue_post_frame(doc);
    Ok(())
}

/// Render a full redraw with timing for the performance API.
fn render_full_timed(doc: &Document, renderer: &mut Renderer, sw: u16, sh: u16) -> io::Result<()> {
    let _span = tracing::debug_span!("frame", mode = "full").entered();

    let frame_start = Instant::now();

    let layout_start = Instant::now();
    doc.compute_layout(sw, sh).map_err(io::Error::other)?;
    let layout_time = layout_start.elapsed();

    let stats = renderer.render_full(doc)?;

    let frame_time = frame_start.elapsed();
    doc.record_frame_metrics(frame_time, layout_time, stats);
    queue_post_frame(doc);
    Ok(())
}

/// Queue the post-frame event for the frame that was just recorded.
///
/// The render task never runs user code: the event goes through the runtime queue
/// so handlers run on the event task, in order with input handlers. A send failure
/// means the runtime is shutting down and is safe to ignore.
fn queue_post_frame(doc: &Document) {
    if let Some(event) = doc.pending_post_frame_event() {
        let _ = doc
            .inner
            .event_tx
            .send(RuntimeEvent::PostFrame(Box::new(event)));
    }
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

/// Whether the render task has any animation-driven frame to schedule: an active
/// transition, keyframe animation, or frames node — or a `WhenScrolling` bar that
/// is waiting to fade or mid-fade.
fn animation_frame_needed(doc: &Document) -> bool {
    animations_active(doc) || doc.scrollbar_fade_schedule(doc.now()).is_active()
}

/// Remove finished transitions and animations and queue their events.
///
/// The render task never runs user code: like post-frame, the events go through
/// the runtime queue so handlers run on the event task, ordered with input. A
/// send failure means the runtime is shutting down and is safe to ignore.
fn cleanup_animations(doc: &Document) {
    for event in doc.run_animation_upkeep() {
        let _ = doc.inner.event_tx.send(event);
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
        CrosstermEvent::Mouse(mouse) => convert_mouse_event(mouse),
        CrosstermEvent::FocusGained => Some(RuntimeEvent::WindowFocus),
        CrosstermEvent::FocusLost => Some(RuntimeEvent::WindowBlur),
        _ => None,
    }
}

fn convert_mouse_event(mouse: CrosstermMouseEvent) -> Option<RuntimeEvent> {
    let modifiers = TuidomKeyModifiers::from_crossterm(mouse.modifiers);
    // Shift picks the axis on the two translating arms below, so it is spent there rather
    // than reported. Everything else passes through.
    let beyond_axis = modifiers.without(TuidomKeyModifiers::SHIFT);
    match mouse.kind {
        MouseEventKind::Down(button) => convert_mouse_button(button).map(|button| {
            RuntimeEvent::MouseDown(MouseEvent::with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                button,
                modifiers,
            ))
        }),
        MouseEventKind::Up(button) => convert_mouse_button(button).map(|button| {
            RuntimeEvent::MouseUp(MouseEvent::with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                button,
                modifiers,
            ))
        }),
        // Shift+wheel is the conventional horizontal scroll: terminals that forward it
        // send a vertical scroll with the shift modifier set, not ScrollLeft/ScrollRight.
        // Shift+up scrolls toward the start (left), matching the unshifted sign.
        MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(RuntimeEvent::Wheel(WheelEvent::horizontal_with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                1,
                beyond_axis,
            )))
        }
        MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(RuntimeEvent::Wheel(WheelEvent::horizontal_with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                -1,
                beyond_axis,
            )))
        }
        MouseEventKind::ScrollUp => Some(RuntimeEvent::Wheel(WheelEvent::with_modifiers(
            i32::from(mouse.column),
            i32::from(mouse.row),
            1,
            modifiers,
        ))),
        MouseEventKind::ScrollDown => Some(RuntimeEvent::Wheel(WheelEvent::with_modifiers(
            i32::from(mouse.column),
            i32::from(mouse.row),
            -1,
            modifiers,
        ))),
        MouseEventKind::Moved => Some(RuntimeEvent::MouseMove {
            x: i32::from(mouse.column),
            y: i32::from(mouse.row),
            held: None,
        }),
        MouseEventKind::Drag(button) => Some(RuntimeEvent::MouseMove {
            x: i32::from(mouse.column),
            y: i32::from(mouse.row),
            held: convert_mouse_button(button),
        }),
        // Nothing was spent picking the axis here, so shift is a real modifier and stays.
        MouseEventKind::ScrollLeft => {
            Some(RuntimeEvent::Wheel(WheelEvent::horizontal_with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                1,
                modifiers,
            )))
        }
        MouseEventKind::ScrollRight => {
            Some(RuntimeEvent::Wheel(WheelEvent::horizontal_with_modifiers(
                i32::from(mouse.column),
                i32::from(mouse.row),
                -1,
                modifiers,
            )))
        }
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
    use super::*;
    use crate::event::WheelAxis;

    fn scroll_event(kind: MouseEventKind, modifiers: KeyModifiers) -> CrosstermMouseEvent {
        CrosstermMouseEvent {
            kind,
            column: 3,
            row: 2,
            modifiers,
        }
    }

    fn converted_wheel(kind: MouseEventKind, modifiers: KeyModifiers) -> WheelEvent {
        match convert_mouse_event(scroll_event(kind, modifiers)) {
            Some(RuntimeEvent::Wheel(wheel)) => wheel,
            other => panic!("expected a wheel event, got {other:?}"),
        }
    }

    #[test]
    fn shift_wheel_converts_to_horizontal() {
        let wheel = converted_wheel(MouseEventKind::ScrollUp, KeyModifiers::SHIFT);
        assert_eq!(wheel.axis, WheelAxis::Horizontal);
        assert_eq!(wheel.delta, 1);

        let wheel = converted_wheel(MouseEventKind::ScrollDown, KeyModifiers::SHIFT);
        assert_eq!(wheel.axis, WheelAxis::Horizontal);
        assert_eq!(wheel.delta, -1);
    }

    #[test]
    fn unshifted_wheel_stays_vertical() {
        let wheel = converted_wheel(MouseEventKind::ScrollUp, KeyModifiers::NONE);
        assert_eq!(wheel.axis, WheelAxis::Vertical);
        assert_eq!(wheel.delta, 1);
    }

    /// The two spellings of a horizontal scroll must report the same modifiers, or
    /// downstream reading them gets a terminal-dependent answer for one gesture.
    #[test]
    fn a_shift_spent_picking_the_axis_is_not_reported() {
        let translated = converted_wheel(MouseEventKind::ScrollUp, KeyModifiers::SHIFT);
        let native = converted_wheel(MouseEventKind::ScrollLeft, KeyModifiers::NONE);

        assert_eq!(translated.axis, WheelAxis::Horizontal);
        assert!(translated.modifiers.is_empty());
        assert_eq!(translated.modifiers, native.modifiers);
    }

    #[test]
    fn modifiers_beyond_the_axis_survive_translation() {
        let wheel = converted_wheel(
            MouseEventKind::ScrollDown,
            KeyModifiers::SHIFT | KeyModifiers::CONTROL,
        );

        assert_eq!(wheel.axis, WheelAxis::Horizontal);
        assert_eq!(wheel.modifiers, TuidomKeyModifiers::CONTROL);
    }

    /// Shift on a wheel the terminal already spells horizontally was not spent on the
    /// axis, so it is a real modifier and stays.
    #[test]
    fn shift_on_a_native_horizontal_wheel_is_kept() {
        let wheel = converted_wheel(MouseEventKind::ScrollRight, KeyModifiers::SHIFT);

        assert_eq!(wheel.axis, WheelAxis::Horizontal);
        assert_eq!(wheel.modifiers, TuidomKeyModifiers::SHIFT);
    }

    #[test]
    fn vertical_wheel_keeps_its_modifiers() {
        let wheel = converted_wheel(MouseEventKind::ScrollUp, KeyModifiers::CONTROL);

        assert_eq!(wheel.axis, WheelAxis::Vertical);
        assert_eq!(wheel.modifiers, TuidomKeyModifiers::CONTROL);
    }

    #[test]
    fn mouse_buttons_carry_their_modifiers() {
        let event = CrosstermMouseEvent {
            kind: MouseEventKind::Down(CrosstermMouseButton::Left),
            column: 3,
            row: 2,
            modifiers: KeyModifiers::CONTROL,
        };

        match convert_mouse_event(event) {
            Some(RuntimeEvent::MouseDown(mouse)) => {
                assert_eq!(mouse.modifiers, TuidomKeyModifiers::CONTROL);
            }
            other => panic!("expected a mouse down event, got {other:?}"),
        }
    }
}

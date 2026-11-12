//! Render + event loop — the main `Document::run()` implementation.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::KeyEventKind;
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyModifiers, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind,
};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::{Instant as TokioInstant, sleep_until};
use tokio_stream::StreamExt;
use tracing::Instrument;

use crate::document::Document;
use crate::event::{
    KeyModifiers as TuidomKeyModifiers, MouseButton, MouseEvent, ResizeEvent, WheelEvent,
    convert_key_event,
};
use crate::lock;
use crate::render::{Renderer, Terminal};
use crate::runtime_event::{
    RuntimeEvent, RuntimeEventState, SIZE_CHANGE_ROUNDS, drain_and_coalesce, process_runtime_event,
    take_pending_resize, take_size_changes,
};
use crate::terminal::{self, TerminalInfo};

/// Run the runtime tasks until [`Document::quit`] is called or a critical task errors.
pub(crate) async fn run(doc: Document) -> io::Result<()> {
    let terminal = Terminal::new()?;
    // Before any task starts: the probe reads the terminal's replies itself, and crossterm's
    // reader would take them for input.
    let info = terminal::probe().await;
    enqueue_runtime_event(&doc, RuntimeEvent::TerminalInfo(Box::new(info.clone())))?;

    let mut tasks = JoinSet::new();

    tasks.spawn(input_task(doc.clone(), info));
    tasks.spawn(event_task(doc.clone()));
    tasks.spawn(render_task(doc.clone(), terminal));

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

async fn input_task(doc: Document, mut info: TerminalInfo) -> io::Result<()> {
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
                        let resized = matches!(crossterm_event, CrosstermEvent::Resize(..));
                        if let Some(runtime_event) = convert_terminal_event(crossterm_event) {
                            enqueue_runtime_event(&doc, runtime_event)?;
                        }
                        if let Some(updated) =
                            resized.then(|| info.with_pixel_size(terminal::pixel_size())).flatten()
                        {
                            info = updated.clone();
                            enqueue_runtime_event(&doc, RuntimeEvent::TerminalInfo(Box::new(updated)))?;
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
                // Gated per event rather than per batch, so a burst lets frames in between.
                for event in batch {
                    process_gated(&doc, event, &mut event_state).await;
                }
            }
        }
    }

    Ok(())
}

async fn render_task(doc: Document, terminal: Terminal) -> io::Result<()> {
    let (mut screen_w, mut screen_h) = crossterm::terminal::size()?;
    let mut renderer = Renderer::new(terminal, screen_w, screen_h);
    let mut next_frame_at = None;
    let mut next_anim_frame_at = None;

    render_frame_capped(
        &doc,
        &mut renderer,
        (screen_w, screen_h),
        FrameMode::Diffed,
        &mut next_frame_at,
    )
    .await?;

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
                    render_frame_capped(&doc, &mut renderer, (screen_w, screen_h), FrameMode::Full, &mut next_frame_at)
                        .await?;
                }
            }

            _ = doc.inner.notify.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                render_frame_capped(&doc, &mut renderer, (screen_w, screen_h), FrameMode::Diffed, &mut next_frame_at)
                    .await?;
            }

            _ = doc.inner.anim_config_changed.notified() => {
                if is_shutdown(&doc) {
                    break;
                }
                if animation_frame_needed(&doc) {
                    render_frame_capped(&doc, &mut renderer, (screen_w, screen_h), FrameMode::Diffed, &mut next_frame_at)
                        .await?;
                }
            }

            _ = wait_for_anim_tick(&doc, &mut next_anim_frame_at), if anim_frame_needed => {
                cleanup_animations(&doc);
                render_frame_capped(&doc, &mut renderer, (screen_w, screen_h), FrameMode::Diffed, &mut next_frame_at)
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
/// With nothing animating smoothly, the wait is a plain deadline instead, from
/// [`idle_deadline`]. A mid-fade bar ticks smoothly like a transition.
async fn wait_for_anim_tick(doc: &Document, next_anim_frame_at: &mut Option<TokioInstant>) {
    let now = doc.now();
    let smooth = doc.scrollbar_fade_schedule(now).fading
        || lock::mutex(&doc.inner.animation).has_smooth_active();

    if !smooth {
        *next_anim_frame_at = None;
        match idle_deadline(doc, now) {
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

/// When the next frame is due while nothing animates smoothly: a frames node's
/// flip, a `WhenScrolling` bar starting its fade, or the cursor's blink flipping or
/// ending.
fn idle_deadline(doc: &Document, now: Instant) -> Option<Instant> {
    let next_flip = lock::mutex(&doc.inner.animation).next_frames_flip(now);
    let fade = doc.scrollbar_fade_schedule(now).next_deadline;
    let blink = doc.cursor_blink(now).map(|blink| blink.next);
    [next_flip, fade, blink].into_iter().flatten().min()
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

/// Process one runtime event while holding the frame gate, so no frame is laid out or painted
/// while its handlers are partway through their changes.
async fn process_gated(doc: &Document, event: RuntimeEvent, state: &mut RuntimeEventState) {
    let _gate = doc.inner.frame_gate.lock().await;
    process_runtime_event(doc, event, state);
}

/// Whether a frame is diffed against the previous one or redrawn whole.
#[derive(Debug, Clone, Copy)]
enum FrameMode {
    Diffed,
    Full,
}

/// How long a frame waits for the event task to handle the size changes it handed over. A
/// listener slower than this has its result shown a frame later instead of holding up the
/// screen.
const SIZE_CHANGE_WAIT: Duration = Duration::from_millis(50);

/// A frame laid out and painted, waiting to be flushed.
struct Painted<T> {
    output: T,
    started: Instant,
    /// Every layout pass the frame made, without the waits between them.
    layout_time: Duration,
    /// Events the layout owes but the frame did not wait for, for the event task to dispatch.
    events: Vec<RuntimeEvent>,
}

/// Lay out and paint one frame while holding the frame gate, so it shows the document as it
/// stands between two runtime events, never partway through one.
///
/// Size changes are handled before painting: the gate is released, the event task runs their
/// listeners, and layout runs again, so a node that redraws for its new size shows the redraw
/// in this frame. That repeats at most [`SIZE_CHANGE_ROUNDS`] times.
async fn lay_out_and_paint<T>(
    doc: &Document,
    (width, height): (u16, u16),
    paint: impl FnOnce() -> T,
) -> io::Result<Painted<T>> {
    let started = Instant::now();
    let mut layout_time = Duration::ZERO;
    let mut deferred = Vec::new();
    let mut rounds = 0;
    loop {
        let gate = doc.inner.frame_gate.lock().await;
        let layout_started = Instant::now();
        let mut events = doc
            .compute_layout_for_frame(width, height)
            .map_err(io::Error::other)?;
        layout_time += layout_started.elapsed();
        let size_changes = take_size_changes(&mut events);
        deferred.extend(events);

        if size_changes.is_empty() || rounds == SIZE_CHANGE_ROUNDS {
            deferred.extend(size_changes);
            let output = paint();
            drop(gate);
            return Ok(Painted {
                output,
                started,
                layout_time,
                events: deferred,
            });
        }
        rounds += 1;
        drop(gate);

        let reached = Arc::new(Notify::new());
        for event in size_changes {
            let _ = doc.inner.event_tx.send(event);
        }
        let _ = doc
            .inner
            .event_tx
            .send(RuntimeEvent::FrameBarrier(reached.clone()));
        // On a timeout the listeners still run when the event task reaches them; this frame
        // lays out once more and paints without them.
        let _ = tokio::time::timeout(SIZE_CHANGE_WAIT, reached.notified()).await;
    }
}

/// Render a frame with timing for the performance API.
async fn render_frame_timed(
    doc: &Document,
    renderer: &mut Renderer,
    size: (u16, u16),
    mode: FrameMode,
) -> io::Result<()> {
    // The frame span lives here rather than in `render/`, because a frame is layout *then*
    // paint and the renderer is only entered for the second half. This is the one place
    // both are in scope, so it is the only place that can parent them.
    let span = match mode {
        FrameMode::Diffed => tracing::debug_span!("frame", mode = "diffed"),
        FrameMode::Full => tracing::debug_span!("frame", mode = "full"),
    };
    async {
        let painted = lay_out_and_paint(doc, size, || renderer.paint(doc)).await?;
        for event in painted.events {
            let _ = doc.inner.event_tx.send(event);
        }
        let stats = match mode {
            FrameMode::Diffed => renderer.flush_diffed(doc, painted.output)?,
            FrameMode::Full => renderer.flush_whole(doc, painted.output)?,
        };
        doc.record_frame_metrics(painted.started.elapsed(), painted.layout_time, stats);
        queue_post_frame(doc);
        Ok(())
    }
    .instrument(span)
    .await
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

async fn render_frame_capped(
    doc: &Document,
    renderer: &mut Renderer,
    size: (u16, u16),
    mode: FrameMode,
    next_frame_at: &mut Option<TokioInstant>,
) -> io::Result<()> {
    if !wait_for_frame_slot(doc, *next_frame_at).await {
        return Ok(());
    }

    let started_at = TokioInstant::now();
    render_frame_timed(doc, renderer, size, mode).await?;
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
/// transition, keyframe animation, or frames node, a `WhenScrolling` bar that is
/// waiting to fade or mid-fade, or a blinking cursor.
fn animation_frame_needed(doc: &Document) -> bool {
    let now = doc.now();
    animations_active(doc)
        || doc.scrollbar_fade_schedule(now).is_active()
        || doc.cursor_blink(now).is_some()
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
    use crate::headless::HeadlessRuntime;

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

    #[test]
    fn a_cursor_blink_wakes_the_render_task_at_each_flip_and_its_end() {
        let doc = Document::new().unwrap();
        let input = doc.create_input("abc").unwrap();
        doc.append_child(doc.root(), input).unwrap();
        let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 1);
        assert!(!animation_frame_needed(&doc));

        doc.focus(input).unwrap();
        let start = doc.now();
        for ms in (1..=10).map(|phase| phase * 500) {
            assert!(animation_frame_needed(&doc));
            let due = start + Duration::from_millis(ms);
            assert_eq!(idle_deadline(&doc, doc.now()), Some(due));
            runtime.advance_time(Duration::from_millis(500));
        }
        assert!(!animation_frame_needed(&doc), "idle once the blink is over");
        assert_eq!(idle_deadline(&doc, doc.now()), None);
    }
}

#[cfg(test)]
mod gate_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::canvas::{CanvasBuffer, CanvasStyle};
    use crate::render::grid::Grid;
    use crate::render::render_into_grid;
    use crate::style::Style;
    use crate::style::color::RgbCache;

    const WORDS: [&str; 3] = ["one", "two", "three"];

    fn screen_text(grid: &Grid) -> String {
        grid.cells
            .iter()
            .flatten()
            .map(|cell| cell.terminal_text())
            .collect()
    }

    // The event appends three text nodes with pauses between them, while frames are painted as
    // fast as they can be on another thread. Without the gate, frames land between the appends.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_frame_never_shows_an_event_partway_through() {
        let doc = Document::new().unwrap();
        let writer = doc.clone();
        doc.set_runtime_flush_hook(Arc::new(move || {
            for word in WORDS {
                let text = writer.create_text(word).unwrap();
                writer.append_child(writer.root(), text).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }));

        let done = Arc::new(AtomicBool::new(false));
        let events = tokio::spawn({
            let doc = doc.clone();
            let done = done.clone();
            async move {
                let mut state = RuntimeEventState::default();
                process_gated(&doc, RuntimeEvent::Flush, &mut state).await;
                done.store(true, Ordering::SeqCst);
            }
        });

        let mut grid = Grid::new(20, 3);
        let mut rgb_cache = RgbCache::new();
        let mut frames = Vec::new();
        loop {
            let finished = done.load(Ordering::SeqCst);
            let painted = lay_out_and_paint(&doc, (20, 3), || {
                render_into_grid(&doc, &mut grid, &mut rgb_cache);
                screen_text(&grid)
            })
            .await
            .unwrap();
            frames.push(painted.output);
            if finished {
                break;
            }
            tokio::task::yield_now().await;
        }
        events.await.unwrap();

        for frame in &frames {
            let shown = WORDS.iter().filter(|word| frame.contains(*word)).count();
            assert!(shown == 0 || shown == WORDS.len(), "torn frame: {frame:?}");
        }
        assert!(frames.last().unwrap().contains("three"));
    }

    /// A canvas filling the screen that redraws itself full of `#` at every size it reports.
    fn self_sizing_canvas() -> Document {
        let doc = Document::new().unwrap();
        let canvas = doc.create_canvas().unwrap();
        let mut style = Style::new();
        style.flex_grow(1.0);
        doc.set_style(canvas, &style).unwrap();
        doc.append_child(doc.root(), canvas).unwrap();
        let d = doc.clone();
        doc.on_size_change(canvas, move |event| {
            let mut buffer = CanvasBuffer::new(event.width, event.height);
            for y in 0..event.height {
                buffer.text(
                    0,
                    y,
                    &"#".repeat(usize::from(event.width)),
                    CanvasStyle::default(),
                );
            }
            d.set_canvas(canvas, buffer).unwrap();
        })
        .unwrap();
        doc
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn size_changes_are_handled_before_the_frame_paints() {
        let doc = self_sizing_canvas();
        let events = tokio::spawn(event_task(doc.clone()));

        let mut grid = Grid::new(6, 2);
        let mut rgb_cache = RgbCache::new();
        let painted = lay_out_and_paint(&doc, (6, 2), || {
            render_into_grid(&doc, &mut grid, &mut rgb_cache);
            screen_text(&grid)
        })
        .await
        .unwrap();
        assert_eq!(painted.output, "#".repeat(12));
        assert!(painted.events.is_empty());

        doc.quit();
        events.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_frame_stops_waiting_for_size_changes_nobody_handles() {
        let doc = self_sizing_canvas();
        let started = Instant::now();
        let mut grid = Grid::new(6, 2);
        let mut rgb_cache = RgbCache::new();
        let painted = lay_out_and_paint(&doc, (6, 2), || {
            render_into_grid(&doc, &mut grid, &mut rgb_cache);
            screen_text(&grid)
        })
        .await
        .unwrap();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(painted.output, " ".repeat(12));
        // The size change went to the queue before the wait, so the frame owes nothing more.
        assert!(painted.events.is_empty());
    }
}

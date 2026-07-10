//! Smoke test exercising the full pipeline:
//! Box + Text/Input rendering, keyboard events, focus, focus styles,
//! configurable focus keys, targeted mouse events, bubbling, stop_propagation,
//! wheel events, transitions, and the performance metrics API.
//!
//! Tab / Shift-Tab      — move focus in DOM order
//! Arrows / hjkl        — move focus spatially, or move input cursor
//! Esc                  — blur focused node
//! Hover text/input     — focus node
//! Type in inputs       — edit text / masked password input
//! Space outside input  — toggle text opacity (fade in/out)
//! Click first text     — toggle text background, stop propagation
//! Click background     — toggle container background
//! Wheel anywhere       — adjust text opacity via container wheel handler
//! q outside input      — quit

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig};
use tuidom::event::{FocusEventRelation, FocusKeys, KeyCode};
use tuidom::style::{AlignItems, Color, CursorShape, JustifyContent, Length, Style};

fn init_logging() {
    // Best-effort file logging for the smoke test.
    if let Ok(file) = std::fs::File::create("/tmp/tuidom.log") {
        let _ = simplelog::WriteLogger::init(
            log::LevelFilter::Trace,
            simplelog::Config::default(),
            file,
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let doc = tuidom::Document::new()?;

    // --- Styles -------------------------------------------------------

    let mut container_style = Style::new();
    container_style.width(Length::Percent(100.0));
    container_style.height(Length::Percent(100.0));
    container_style.justify_content(JustifyContent::Center);
    container_style.align_items(AlignItems::Center);
    container_style.background(Color::oklch(0.3, 0.1, 50.0));

    let mut text_style = Style::new();
    text_style.width(Length::Auto);
    text_style.height(Length::Auto);
    text_style.color(Color::white());
    text_style.background(Color::blue());

    let mut secondary_text_style = text_style.clone();
    secondary_text_style.background(Color::oklch(0.35, 0.12, 250.0));

    let mut focus_style = Style::new();
    focus_style.color(Color::black());
    focus_style.background(Color::yellow());

    let mut input_style = Style::new();
    input_style.width(Length::Pixels(24));
    input_style.height(Length::Pixels(1));
    input_style.color(Color::white());
    input_style.background(Color::oklch(0.18, 0.04, 260.0));
    input_style.cursor_shape(CursorShape::Bar);

    let mut password_style = input_style.clone();
    password_style.cursor_shape(CursorShape::Block);

    // --- DOM ----------------------------------------------------------

    let container = doc.create_box()?;
    doc.set_style(container, &container_style)?;

    let perf_counter = doc.create_text("FPS: --  Frame: --")?;
    let mut perf_style = Style::new();
    perf_style.color(Color::oklch(0.85, 0.16, 145.0));
    perf_style.background(Color::black());
    doc.set_style(perf_counter, &perf_style)?;

    let text = doc.create_text("Hello, tuidom!  Click / wheel me")?;
    doc.set_style(text, &text_style)?;
    doc.set_focusable(text, true)?;
    doc.set_focus_style(text, &focus_style)?;

    let second = doc.create_text("  Focus target 2  ")?;
    doc.set_style(second, &secondary_text_style)?;
    doc.set_focusable(second, true)?;
    doc.set_focus_style(second, &focus_style)?;

    let third = doc.create_text("  Focus target 3  ")?;
    doc.set_style(third, &secondary_text_style)?;
    doc.set_focusable(third, true)?;
    doc.set_focus_style(third, &focus_style)?;

    let editable = doc.create_input("edit me")?;
    doc.set_style(editable, &input_style)?;
    doc.set_focus_style(editable, &focus_style)?;

    let password = doc.create_input("secret")?;
    doc.set_style(password, &password_style)?;
    doc.set_input_mask(password, Some('•'))?;
    doc.set_focus_style(password, &focus_style)?;

    doc.append_child(container, perf_counter)?;
    doc.append_child(container, text)?;
    doc.append_child(container, second)?;
    doc.append_child(container, third)?;
    doc.append_child(container, editable)?;
    doc.append_child(container, password)?;
    doc.append_child(doc.root(), container)?;

    let mut focus_keys = FocusKeys::default();
    focus_keys.up.push(KeyCode::Char('k'));
    focus_keys.down.push(KeyCode::Char('j'));
    focus_keys.left.push(KeyCode::Char('h'));
    focus_keys.right.push(KeyCode::Char('l'));
    doc.set_focus_keys(focus_keys);

    // --- Transition config — opacity changes animate over 1000ms -------

    doc.set_transition(
        text,
        TransitionConfig::opacity(Duration::from_millis(1000), Easing::EaseInOut),
    )?;

    // --- Shared state -------------------------------------------------

    let opacity_visible = Arc::new(AtomicBool::new(true));
    let text_background_alt = Arc::new(AtomicBool::new(false));
    let container_background_alt = Arc::new(AtomicBool::new(false));
    let text_opacity = Arc::new(Mutex::new(1.0));

    update_perf_counter(&doc, perf_counter);

    // --- Keyboard handler --------------------------------------------

    let d = doc.clone();
    let ov = opacity_visible.clone();
    let opacity_for_key = text_opacity.clone();

    doc.on_key_press(doc.root(), move |key| {
        if matches!(d.focused(), Some(node) if node == editable || node == password) {
            return;
        }

        match key.code {
            KeyCode::Char(' ') => {
                update_perf_counter(&d, perf_counter);
                let was_visible = ov.fetch_not(Ordering::Relaxed);
                let target = if !was_visible { 1.0 } else { 0.0 };
                if let Ok(mut opacity) = opacity_for_key.lock() {
                    *opacity = target;
                }
                let _ = d.update_style(text, |s| s.opacity(target));
            }
            KeyCode::Char('q') => d.quit(),
            _ => {}
        }
    })?;

    // --- Focus handlers ----------------------------------------------

    let d = doc.clone();
    doc.on_focus(container, move |event| {
        update_perf_counter(&d, perf_counter);
        if event.relation() == FocusEventRelation::Descendant {
            let _ = d.update_style(container, |s| s.background(Color::oklch(0.38, 0.16, 95.0)));
        }
    })?;

    let d = doc.clone();
    let container_bg = container_background_alt.clone();
    doc.on_blur(container, move |event| {
        update_perf_counter(&d, perf_counter);
        if event.relation() == FocusEventRelation::Descendant {
            let color = if container_bg.load(Ordering::Relaxed) {
                Color::oklch(0.25, 0.12, 260.0)
            } else {
                Color::oklch(0.3, 0.1, 50.0)
            };
            let _ = d.update_style(container, |s| s.background(color));
        }
    })?;

    // --- Mouse handlers ----------------------------------------------

    let d = doc.clone();
    let text_bg = text_background_alt.clone();
    doc.on_click(text, move |event| {
        update_perf_counter(&d, perf_counter);
        event.stop_propagation();
        let use_alt = text_bg.fetch_not(Ordering::Relaxed);
        let color = if use_alt {
            Color::blue()
        } else {
            Color::oklch(0.65, 0.2, 140.0)
        };
        let _ = d.update_style(text, |s| s.background(color));
    })?;

    let d = doc.clone();
    let container_bg = container_background_alt.clone();
    doc.on_click(container, move |_| {
        update_perf_counter(&d, perf_counter);
        let use_alt = container_bg.fetch_not(Ordering::Relaxed);
        let color = if use_alt {
            Color::oklch(0.3, 0.1, 50.0)
        } else {
            Color::oklch(0.25, 0.12, 260.0)
        };
        let _ = d.update_style(container, |s| s.background(color));
    })?;

    let d = doc.clone();
    let opacity_for_wheel = text_opacity.clone();
    doc.on_wheel(container, move |event| {
        update_perf_counter(&d, perf_counter);
        if let Ok(mut opacity) = opacity_for_wheel.lock() {
            let direction = if event.delta > 0 { 1.0 } else { -1.0 };
            *opacity = (*opacity + direction * 0.1).clamp(0.1, 1.0);
            let target = *opacity;
            let _ = d.update_style(text, |s| s.opacity(target));
        }
    })?;

    // --- Run ----------------------------------------------------------

    doc.run().await?;
    Ok(())
}

fn update_perf_counter(doc: &tuidom::Document, node: tuidom::NodeId) {
    let snapshot = doc.performance_snapshot();
    let Some(frame) = snapshot.latest else {
        return;
    };

    let text = format!(
        "FPS: {:.0}  Frame: {:.3}ms",
        snapshot.fps,
        frame.frame_time.as_secs_f64() * 1000.0
    );
    let _ = doc.set_text_content(node, text);
}

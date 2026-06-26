//! Smoke test exercising the full pipeline:
//! Box + Text rendering, keyboard events, targeted mouse events, bubbling,
//! stop_propagation, wheel events, transitions, and debug overlay.
//!
//! Space             — toggle text opacity (fade in/out)
//! Click text        — toggle text background, stop propagation
//! Click background  — toggle container background
//! Wheel anywhere    — adjust text opacity via container wheel handler
//! q/Esc             — quit

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig};
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Color, JustifyContent, Length, Style};

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

    // --- DOM ----------------------------------------------------------

    let container = doc.create_box()?;
    doc.set_style(container, &container_style)?;

    let text = doc.create_text("Hello, tuidom!  Click / wheel me")?;
    doc.set_style(text, &text_style)?;

    doc.append_child(container, text)?;
    doc.append_child(doc.root(), container)?;

    // Always show debug overlay.
    doc.toggle_debug_overlay();

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

    // --- Keyboard handler --------------------------------------------

    let d = doc.clone();
    let ov = opacity_visible.clone();
    let opacity_for_key = text_opacity.clone();

    doc.on_key_press(doc.root(), move |key| match key.code {
        KeyCode::Char(' ') => {
            let was_visible = ov.fetch_not(Ordering::Relaxed);
            let target = if !was_visible { 1.0 } else { 0.0 };
            if let Ok(mut opacity) = opacity_for_key.lock() {
                *opacity = target;
            }
            let _ = d.update_style(text, |s| s.opacity(target));
        }
        KeyCode::Char('q') | KeyCode::Esc => d.quit(),
        _ => {}
    })?;

    // --- Mouse handlers ----------------------------------------------

    let d = doc.clone();
    let text_bg = text_background_alt.clone();
    doc.on_click(text, move |event| {
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

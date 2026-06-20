//! Smoke test exercising the full pipeline:
//! Box + Text rendering, toggleable animation, debug overlay.
//!
//! Space  — toggle text opacity (fade in/out)
//! F1     — toggle debug overlay
//! q/Esc  — quit

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig};
use tuidom::event::{Event, KeyCode};
use tuidom::style::{AlignItems, Color, JustifyContent, Length, Style};

#[tokio::main]
async fn main() {
    let doc = tuidom::Document::new();

    // --- Styles -------------------------------------------------------

    let container_style = Style::new()
        .width(Length::Percent(100.0))
        .height(Length::Percent(100.0))
        .justify_content(JustifyContent::Center)
        .align_items(AlignItems::Center);

    let text_style = Style::new()
        .color(Color::white())
        .background(Color::blue());

    // --- DOM ----------------------------------------------------------

    let container = doc.create_box();
    doc.set_style(container, container_style);

    let text = doc.create_text("Hello, tuidom!");
    doc.set_style(text, text_style);

    doc.append_child(container, text);
    doc.set_root(container);

    // --- Transition config — opacity changes animate over 400ms -------

    doc.set_transition(
        text,
        TransitionConfig::opacity(Duration::from_millis(400), Easing::EaseInOut),
    );

    // --- Shared state -------------------------------------------------

    let opacity_visible = Arc::new(AtomicBool::new(true));

    // --- Event handler ------------------------------------------------

    let d = doc.clone();
    let ov = opacity_visible.clone();

    doc.on(move |event: &Event| match event {
        Event::KeyPress(key) => match key.code {
            // Toggle opacity — engine picks up style change and animates
            KeyCode::Char(' ') => {
                let was_visible = ov.fetch_not(Ordering::Relaxed);
                let target = if !was_visible { 1.0 } else { 0.0 };
                d.update_style(text, |s| s.opacity(target));
            }
            // Toggle debug overlay
            KeyCode::F(1) => d.toggle_debug_overlay(),
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => d.quit(),
            _ => {}
        },
        Event::Resize(size) => {
            // Engine handles relayout automatically.
            // React here if needed, e.g.: reposition absolute elements.
            let _ = (d.clone(), size);
        }
        _ => {}
    });

    // --- Run ----------------------------------------------------------

    doc.run().await;
}

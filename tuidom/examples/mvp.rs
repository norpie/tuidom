//! Smoke test exercising the full pipeline:
//! Box + Text rendering, toggleable animation, debug overlay.
//!
//! Space  — toggle text opacity (fade in/out)
//! F1     — toggle debug overlay
//! q/Esc  — quit

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig};
use tuidom::event::{Event, KeyCode};
use tuidom::style::{AlignItems, Color, JustifyContent, Length, Style};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = tuidom::Document::new();

    // --- Styles -------------------------------------------------------

    let mut container_style = Style::new();
    container_style.width(Length::Percent(100.0));
    container_style.height(Length::Percent(100.0));
    container_style.justify_content(JustifyContent::Center);
    container_style.align_items(AlignItems::Center);
    container_style.background(Color::oklch(0.3, 0.1, 50.0)); // dark red

    let mut text_style = Style::new();
    text_style.width(Length::Auto);
    text_style.height(Length::Auto);
    text_style.color(Color::white());
    text_style.background(Color::blue());

    // --- DOM ----------------------------------------------------------

    let container = doc.create_box();
    doc.set_style(container, &container_style)?;

    let text = doc.create_text("Hello, tuidom!");
    doc.set_style(text, &text_style)?;

    doc.append_child(container, text)?;
    doc.set_root(container);

    // Always show debug overlay
    doc.toggle_debug_overlay();

    // --- Transition config — opacity changes animate over 400ms -------

    doc.set_transition(
        text,
        TransitionConfig::opacity(Duration::from_millis(2000), Easing::EaseInOut),
    );

    // --- Shared state -------------------------------------------------

    let opacity_visible = Arc::new(AtomicBool::new(true));

    // --- Event handler ------------------------------------------------

    let d = doc.clone();
    let ov = opacity_visible.clone();

    doc.on(move |event: &Event| {
        let Event::KeyPress(key) = event else {
            return;
        };

        match key.code {
            // Toggle opacity — engine picks up style change and animates
            KeyCode::Char(' ') => {
                let was_visible = ov.fetch_not(Ordering::Relaxed);
                let target = if !was_visible { 1.0 } else { 0.0 };
                let _ = d.update_style(text, |s| s.opacity(target));
            }
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => d.quit(),
            _ => {}
        }
    });

    // --- Run ----------------------------------------------------------

    doc.run().await?;
    Ok(())
}

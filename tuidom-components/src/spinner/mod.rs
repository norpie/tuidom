#[cfg(test)]
mod tests;

use std::time::Duration;

use tuidom::style::{Color, Style};
use tuidom_ui::{View, component, view};

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const INTERVAL: Duration = Duration::from_millis(80);

/// A braille spinner in `--primary`, followed by `label` when there is one.
///
/// It turns on the engine clock, repainting at its own 80ms pace only while it is mounted.
/// `style` styles the row holding it.
#[component]
pub fn Spinner(#[prop(default)] label: String, #[prop(default)] style: Style) -> View {
    view! {
        box {
            style: style.get(),
            frames(FRAMES, INTERVAL) { style: primary() },
            if !label.get().is_empty() {
                text(" "),
                text(label.get()),
            },
        }
    }
}

fn primary() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--primary"));
    style
}

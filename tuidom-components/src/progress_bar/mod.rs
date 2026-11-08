#[cfg(test)]
mod tests;

use tuidom::style::{Color, Length, Style};
use tuidom_ui::{View, component, derived, view};

use crate::tone::Tone;

/// A one-row bar on a `--muted` track, filled from the left to `value`, from 0 to 1, in the
/// `tone`'s color.
///
/// The fill moves in whole cells. It fills its container's width unless `style` sets one, and
/// `style` is merged over the track's own style.
#[component]
pub fn ProgressBar(value: f64, #[prop(default)] tone: Tone, #[prop(default)] style: Style) -> View {
    let track = derived(move || {
        let mut track = Style::new();
        track.width(Length::Percent(100.0));
        track.height(Length::Cells(1));
        track.background(Color::var("--muted"));
        track.merge(&style.get());
        track
    });
    let share = derived(move || value.get().clamp(0.0, 1.0));
    let fill = derived(move || {
        let mut fill = segment(share.get());
        fill.background(tone.get().color());
        fill
    });

    view! {
        box {
            style: track.get(),
            box { style: fill.get() },
            box { style: segment(1.0 - share.get()) },
        }
    }
}

fn segment(share: f64) -> Style {
    let mut style = Style::new();
    style.flex_basis(Length::Cells(0));
    style.flex_grow(share as f32);
    style
}

#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Border, BorderCharset, Color, Length, Sides, Style};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};

use crate::theme::{FocusMark, use_shape};

/// A horizontal track for picking `value` between `min` and `max` in increments of `step`, read
/// and written.
///
/// The track is filled in `--primary` up to the thumb and drawn in `--muted` after it. While it is
/// focused, Left and Right move the value by one step and Home and End jump to the ends, in place
/// of moving focus; Up and Down still navigate. A click on the track jumps the value to that
/// point. Values are snapped to the nearest step from `min` and kept inside the range. It fills its
/// container's width unless `style` sets one. `disabled` blocks focus and changes and mutes it.
///
/// `style` is merged over the track's own style.
#[component]
pub fn Slider(
    value: Signal<f64>,
    #[prop(default = 0.0)] min: f64,
    #[prop(default = 100.0)] max: f64,
    #[prop(default = 1.0)] step: f64,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let track = node_ref();
    let range = derived(move || Range {
        min: min.get(),
        max: max.get(),
        step: step.get(),
    });
    let fraction = derived(move || range.get().fraction(value.get()));
    let frame = derived(move || {
        let mut frame = Style::new();
        frame.width(Length::Percent(100.0));
        frame.height(Length::Cells(1));
        frame.align_items(AlignItems::FlexStart);
        frame.merge(&style.get());
        frame
    });
    let filled = derived(move || {
        if disabled.get() {
            "--muted-foreground"
        } else {
            "--primary"
        }
    });
    let thumb = derived(move || {
        let mut thumb = Style::new();
        let color = filled.get();
        thumb.color(Color::var(color));
        if focused.get() {
            match shape.get().focus {
                FocusMark::Ring => thumb.color(Color::var("--ring")),
                FocusMark::Underline => thumb.underline(true),
                FocusMark::Reverse => {
                    thumb.background(Color::var(color));
                    thumb.color(Color::var("--background"));
                }
            }
        }
        thumb
    });
    let set = move |next: f64| value.set(range.get().snap(next));

    view! {
        box {
            ref: track,
            focusable: true,
            disabled: disabled.get(),
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_key_press: move |ev| {
                let Range { min, max, step } = range.get();
                match ev.code {
                    KeyCode::Left => set(value.get() - step),
                    KeyCode::Right => set(value.get() + step),
                    KeyCode::Home => set(min),
                    KeyCode::End => set(max),
                    _ => return,
                }
                ev.prevent_default();
                ev.stop_propagation();
            },
            on_click: move |ev| {
                if let Some(rect) = track.screen_rect() {
                    let span = f64::from(rect.width.saturating_sub(1).max(1));
                    let at = (f64::from(ev.x - rect.x) / span).clamp(0.0, 1.0);
                    let Range { min, max, .. } = range.get();
                    set(min + at * (max - min));
                }
            },
            box { style: segment(fraction.get(), BorderCharset::thick(), filled.get()) },
            text("●") { style: thumb.get() },
            box { style: segment(1.0 - fraction.get(), shape.get().border, "--muted") },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
struct Range {
    min: f64,
    max: f64,
    step: f64,
}

impl Range {
    /// How far along the range `value` is, from 0 to 1.
    fn fraction(&self, value: f64) -> f64 {
        if self.max > self.min {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    fn snap(&self, value: f64) -> f64 {
        let stepped = if self.step > 0.0 {
            self.min + ((value - self.min) / self.step).round() * self.step
        } else {
            value
        };
        stepped.clamp(self.min, self.max.max(self.min))
    }
}

/// One side of the track: a line taking `share` of the cells beside the thumb.
fn segment(share: f64, charset: BorderCharset, color: &'static str) -> Style {
    let mut style = Style::new();
    style.flex_basis(Length::Cells(0));
    style.flex_grow(share as f32);
    style.border(Border::new(charset).with_sides(Sides::new(true, false, false, false)));
    style.border_color(Color::var(color));
    style
}

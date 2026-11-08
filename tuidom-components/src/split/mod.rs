#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Border, Color, FlexDirection, Length, Overflow, Sides, Style};
use tuidom_ui::{Children, Signal, View, component, derived, node_ref, state, view};

use crate::theme::use_shape;

/// How far one key press moves a [`Split`]'s divider, as a share of the space.
const KEY_STEP: f64 = 0.05;

/// Two panes, `first` and `second`, side by side or `stacked`, with a divider between them.
///
/// `ratio` is the first pane's share of the space the panes divide, from 0 to 1, read and written.
/// Dragging the divider moves it to the pointer. While the divider is focused, the arrow keys on
/// the split's axis move it by a twentieth, and once it is at an end they move focus as usual.
/// The divider is a line in `--border` from the theme's charset, `--ring` while focused. The
/// split fills its container, and each pane clips what does not fit.
///
/// `style` is merged over the split's own style.
#[component]
pub fn Split(
    ratio: Signal<f64>,
    first: Children,
    second: Children,
    #[prop(default)] stacked: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let split = node_ref();
    let frame = derived(move || {
        let mut frame = Style::new();
        frame.width(Length::Percent(100.0));
        frame.height(Length::Percent(100.0));
        let direction = if stacked.get() {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        frame.flex_direction(direction);
        frame.merge(&style.get());
        frame
    });
    let share = derived(move || ratio.get().clamp(0.0, 1.0));
    let divider = derived(move || {
        let mut divider = Style::new();
        let sides = if stacked.get() {
            Sides::new(true, false, false, false)
        } else {
            Sides::new(false, false, false, true)
        };
        divider.border(Border::new(shape.get().border).with_sides(sides));
        let color = if focused.get() { "--ring" } else { "--border" };
        divider.border_color(Color::var(color));
        divider.flex_shrink(0.0);
        divider
    });
    let set = move |next: f64| {
        let next = next.clamp(0.0, 1.0);
        let moved = next != share.get();
        ratio.set(next);
        moved
    };

    view! {
        box {
            ref: split,
            style: frame.get(),
            box { style: pane(share.get()), { first } },
            box {
                focusable: true,
                style: divider.get(),
                on_focus: move |_| focused.set(true),
                on_blur: move |_| focused.set(false),
                on_drag: move |ev| {
                    let Some(rect) = split.screen_rect() else {
                        return;
                    };
                    let (at, start, length) = if stacked.get() {
                        (ev.y, rect.y, rect.height)
                    } else {
                        (ev.x, rect.x, rect.width)
                    };
                    let space = f64::from(length.saturating_sub(1).max(1));
                    set(f64::from(at - start) / space);
                },
                on_key_press: move |ev| {
                    let step = match (stacked.get(), ev.code) {
                        (false, KeyCode::Left) | (true, KeyCode::Up) => -KEY_STEP,
                        (false, KeyCode::Right) | (true, KeyCode::Down) => KEY_STEP,
                        _ => return,
                    };
                    if set(share.get() + step) {
                        ev.prevent_default();
                        ev.stop_propagation();
                    }
                },
            },
            box { style: pane(1.0 - share.get()), { second } },
        }
    }
}

/// A pane taking `share` of the space beside the divider.
fn pane(share: f64) -> Style {
    let mut style = Style::new();
    style.flex_basis(Length::Cells(0));
    style.flex_grow(share as f32);
    style.overflow(Overflow::Clip);
    style
}

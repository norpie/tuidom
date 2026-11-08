#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::Style;
use tuidom_ui::{Signal, View, component, derived, state, view};

use crate::choice::ChoiceLook;
use crate::theme::use_shape;

/// A focusable `[✓] label` row bound to `checked`, toggled by a click, or by Enter or Space while
/// focused.
///
/// The brackets are drawn in `--input` and the mark in `--primary`. While focused, the theme's
/// focus mark applies: a ring recolors the brackets to `--ring`. `disabled` blocks focus and
/// toggling and mutes the row.
///
/// `style` is merged over the row's own style.
#[component]
pub fn Checkbox(
    checked: Signal<bool>,
    #[prop(default)] label: String,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let look = derived(move || ChoiceLook {
        disabled: disabled.get(),
        focused: focused.get(),
        focus: shape.get().focus,
    });
    let row = derived(move || {
        let mut row = look.get().row();
        row.merge(&style.get());
        row
    });
    let toggle = move || checked.update(|on| *on = !*on);

    view! {
        box {
            focusable: true,
            disabled: disabled.get(),
            style: row.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_click: move |_| toggle(),
            on_key_press: move |ev| {
                if matches!(ev.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    toggle();
                }
            },
            text("[") { style: look.get().bracket() },
            text(if checked.get() { "✓" } else { " " }) { style: look.get().mark() },
            text("]") { style: look.get().bracket() },
            if !label.get().is_empty() {
                text(" "),
                text(label.get()) { style: look.get().label() },
            },
        }
    }
}

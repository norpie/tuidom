#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Style};
use tuidom_ui::{Signal, View, component, derived, state, view};

use crate::choice::ChoiceLook;
use crate::theme::use_shape;

/// A column of `(•) option` rows, one of which is selected: `selected` is the index into
/// `options`, read and written.
///
/// The group is a single focus stop. While it is focused, Up and Down move the selection; at the
/// first or last option they move focus out of the group instead, as spatial navigation would.
/// Clicking an option selects it. The focus mark is drawn on the selected row. An index past the
/// end selects nothing. `disabled` blocks focus and selection and mutes every row.
///
/// `style` is merged over the column's own style.
#[component]
pub fn RadioGroup(
    options: Vec<String>,
    selected: Signal<usize>,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let look = move |index: usize| ChoiceLook {
        disabled: disabled.get(),
        focused: focused.get() && selected.get() == index,
        focus: shape.get().focus,
    };
    let column = derived(move || {
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.merge(&style.get());
        column
    });

    view! {
        box {
            focusable: true,
            disabled: disabled.get(),
            style: column.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_key_press: move |ev| {
                let last = options.get().len().saturating_sub(1);
                let current = selected.get().min(last);
                let next = match ev.code {
                    KeyCode::Up if current > 0 => Some(current - 1),
                    KeyCode::Down if current < last => Some(current + 1),
                    _ => None,
                };
                if let Some(next) = next {
                    ev.prevent_default();
                    ev.stop_propagation();
                    selected.set(next);
                }
            },
            each i, option in options.get() key = (i, option.clone()) {
                box {
                    style: look(i).row(),
                    on_click: move || selected.set(i),
                    text("(") { style: look(i).bracket() },
                    text(if selected.get() == i { "•" } else { " " }) { style: look(i).mark() },
                    text(")") { style: look(i).bracket() },
                    text(" "),
                    text(option.clone()) { style: look(i).label() },
                }
            },
        }
    }
}

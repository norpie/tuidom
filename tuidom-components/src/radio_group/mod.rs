#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Style};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};

use crate::choice::ChoiceLook;
use crate::roving::focus_item;
use crate::theme::use_shape;

/// A column of `(•) option` rows, one of which is selected: `selected` is the index into
/// `options`, read and written.
///
/// The group is a single tab stop, on the selected option. Each option takes focus on its own, so
/// hovering one focuses it and the focus mark is drawn on it. Up and Down move focus and the
/// selection together; at the first or last option they move focus out of the group instead, as
/// spatial navigation would. Space, Enter, or a click selects an option. An index past the end
/// selects nothing. `disabled` blocks focus and selection and mutes every row.
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
    let focused = state(None::<usize>);
    let group = node_ref();
    let look = move |index: usize| ChoiceLook {
        disabled: disabled.get(),
        focused: focused.get() == Some(index),
        focus: shape.get().focus,
    };
    let column = derived(move || {
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.selectable(false);
        column.merge(&style.get());
        column
    });
    let tab_stop = move |index: usize| {
        let last = options.get().len().saturating_sub(1);
        selected.get().min(last) == index
    };

    view! {
        box {
            ref: group,
            disabled: disabled.get(),
            style: column.get(),
            on_key_press: move |ev| {
                let Some(current) = focused.get() else {
                    return;
                };
                let last = options.get().len().saturating_sub(1);
                let chosen = match ev.code {
                    KeyCode::Up if current > 0 => current - 1,
                    KeyCode::Down if current < last => current + 1,
                    KeyCode::Enter | KeyCode::Char(' ') => current,
                    _ => return,
                };
                ev.prevent_default();
                ev.stop_propagation();
                selected.set(chosen);
                focus_item(group, chosen);
            },
            each i, option in options.get() key = (i, option.clone()) {
                box {
                    item: i,
                    focusable: true,
                    tab_stop: tab_stop(i),
                    style: look(i).row(),
                    on_focus: move |_| focused.set(Some(i)),
                    on_blur: move |_| focused.set(None),
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

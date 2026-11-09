#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Color, Style};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};
use unicode_width::UnicodeWidthStr;

use crate::field::FieldLook;
use crate::popup_list::{PopupList, PopupListProps, PopupRow, padded};
use crate::theme::use_shape;

/// A field showing the selected option, which opens a list of `options` to pick from: `selected`
/// is the index into `options`, read and written.
///
/// A click, or Enter or Space while focused, opens the list below the field, drawn over
/// everything else in `--popover`. While it is open it holds the keyboard: Up and Down, or
/// hovering, move a highlight in `--accent`, Enter or Space picks the highlighted option and closes it, and Escape
/// or a press anywhere outside closes it without changing the selection. The field is framed like
/// a [`TextInput`](crate::TextInput), and it and the list are both as wide as the widest option,
/// so a field stretched wider than that is wider than its list. `disabled` keeps it closed and
/// mutes it.
///
/// `style` is merged over the field's frame.
#[component]
pub fn Select(
    options: Vec<String>,
    selected: Signal<usize>,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let open = state(false);
    let trigger = node_ref();

    let look = derived(move || FieldLook {
        disabled: disabled.get(),
        focused: focused.get(),
        shape: shape.get(),
    });
    let frame = derived(move || {
        let mut frame = look.get().frame();
        frame.selectable(false);
        frame.merge(&style.get());
        frame
    });
    let widest = derived(move || options.get().iter().map(|o| o.width()).max().unwrap_or(0));
    let current = derived(move || {
        let current = options
            .get()
            .get(selected.get())
            .cloned()
            .unwrap_or_default();
        padded(&current, widest.get())
    });

    let show = move || {
        if !options.get().is_empty() {
            open.set(true);
        }
    };

    view! {
        box {
            ref: trigger,
            focusable: true,
            disabled: disabled.get(),
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_click: move |_| show(),
            on_key_press: move |ev| {
                if matches!(ev.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    show();
                }
            },
            text(current.get()) { style: look.get().text() },
            text(" ▾") { style: chevron() },
            if open.get() {
                portal {
                    PopupList {
                        rows: rows(&options.get(), selected.get()),
                        anchor: trigger.node(),
                        initial: selected.get(),
                        on_choose: move |index| {
                            selected.set(index);
                            open.set(false);
                        },
                        on_close: move |_| open.set(false),
                    }
                }
            },
        }
    }
}

/// The options as list rows, with a check beside the selected one.
fn rows(options: &[String], selected: usize) -> Vec<PopupRow> {
    options
        .iter()
        .enumerate()
        .map(|(i, option)| PopupRow {
            label: option.clone(),
            detail: if i == selected { " ✓" } else { "  " }.to_string(),
            ..PopupRow::default()
        })
        .collect()
}

fn chevron() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--muted-foreground"));
    style
}

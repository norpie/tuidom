#[cfg(test)]
mod tests;

use tuidom::style::Style;
use tuidom_ui::{Callback, View, component, node_ref, state, view};

use crate::button::{Button, ButtonProps, ButtonVariant};
use crate::popup_list::{PopupList, PopupListProps, PopupRow};

/// One entry in a [`Menu`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuItem {
    /// The text shown for the item.
    pub label: String,
    /// A shortcut shown right-aligned after the label in `--muted-foreground`. It is only
    /// drawn; binding the key is up to the caller.
    pub hint: String,
    /// Draws the item muted, keeps the highlight off it, and makes it unchoosable.
    pub disabled: bool,
    /// Draws a line above the item. The line is not an entry, so it shifts no index.
    pub separated: bool,
}

impl MenuItem {
    /// An enabled item showing `label`, with no hint and no line above it.
    pub fn new(label: impl Into<String>) -> Self {
        MenuItem {
            label: label.into(),
            ..MenuItem::default()
        }
    }

    /// Shows `hint` after the label.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Makes the item disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Draws a line above the item.
    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }
}

/// A [`Button`] showing `label ▾`, which opens a list of `items` below it, drawn over
/// everything else in `--popover`.
///
/// Pressing the button opens the list. While it is open it holds the keyboard: Up and Down, or
/// hovering, move a highlight in `--accent` over the enabled items, and Enter or Space, or a click on an item,
/// closes the list and calls `on_select` with the item's index in `items`. Escape or a press
/// anywhere outside closes it without selecting. Focus goes back to the button either way.
/// `variant` is the button's, [`Ghost`](ButtonVariant::Ghost) unless given; `disabled` keeps the
/// menu closed and mutes the button. `style` is merged over the button's own style.
#[component]
pub fn Menu(
    label: String,
    items: Vec<MenuItem>,
    #[prop(default)] on_select: Callback<usize>,
    #[prop(default = ButtonVariant::Ghost)] variant: ButtonVariant,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let open = state(false);
    let trigger = node_ref();
    let on_select = state(on_select);

    view! {
        box {
            ref: trigger,
            Button {
                label: format!("{} ▾", label.get()),
                variant: variant.get(),
                disabled: disabled.get(),
                style: style.get(),
                on_press: move |_| {
                    if !items.get().is_empty() {
                        open.set(true);
                    }
                },
            },
            if open.get() {
                portal {
                    PopupList {
                        rows: rows(&items.get()),
                        anchor: trigger.node(),
                        on_choose: move |index| {
                            open.set(false);
                            if let Some(on_select) = on_select.try_get() {
                                on_select.call(index);
                            }
                        },
                        on_close: move |_| open.set(false),
                    }
                }
            },
        }
    }
}

fn rows(items: &[MenuItem]) -> Vec<PopupRow> {
    items
        .iter()
        .map(|item| PopupRow {
            label: item.label.clone(),
            detail: if item.hint.is_empty() {
                String::new()
            } else {
                format!("  {}", item.hint)
            },
            muted_detail: true,
            disabled: item.disabled,
            separated: item.separated,
        })
        .collect()
}

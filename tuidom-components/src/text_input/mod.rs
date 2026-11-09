#[cfg(test)]
mod tests;

use tuidom::style::{Color, Length, Position, Style};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};

use crate::field::FieldLook;
use crate::theme::use_shape;

/// A single-line text field bound to `value`: typing writes the caller's signal, and writing the
/// signal updates the field.
///
/// `placeholder` shows in `--muted-foreground` while the value is empty. `mask` draws every
/// character as the given one, for passwords. `disabled` blocks focus and editing and mutes the
/// text. The frame follows the theme's [`Shape`](crate::Shape): edged in `--input`, which turns
/// `--ring` while focused, and filled with `--input` where the surface has no border. A field
/// with no edge at all tints its fill toward `--ring` while focused instead. It fills its
/// container's width unless `style` sets one, and scrolls a value too long to show. Stretched
/// taller by its container, it keeps its text on the middle row.
///
/// `style` is merged over the field's frame.
#[component]
pub fn TextInput(
    value: Signal<String>,
    #[prop(default)] placeholder: String,
    mask: Option<char>,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let field = node_ref();
    let look = derived(move || FieldLook {
        disabled: disabled.get(),
        focused: focused.get(),
        shape: shape.get(),
    });
    let frame = derived(move || {
        let mut frame = look.get().frame();
        frame.width(Length::Percent(100.0));
        frame.merge(&style.get());
        frame
    });
    let input_style = derived(move || {
        let mut input = look.get().text();
        input.merge(&overlay());
        input
    });

    view! {
        box {
            style: frame.get(),
            on_click: move || {
                if !disabled.get() {
                    field.focus();
                }
            },
            box {
                style: line(),
                if value.get().is_empty() {
                    text(placeholder.get()) { style: placeholder_style() }
                },
                input {
                    ref: field,
                    bind_value: value,
                    mask: mask.get(),
                    disabled: disabled.get(),
                    style: input_style.get(),
                    on_focus: move |_| focused.set(true),
                    on_blur: move |_| focused.set(false),
                },
            },
        }
    }
}

/// The one-row strip holding the placeholder and, laid over it, the input.
fn line() -> Style {
    let mut style = Style::new();
    style.flex_grow(1.0);
    style.height(Length::Cells(1));
    style
}

fn placeholder_style() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--muted-foreground"));
    style
}

/// The input covers the whole strip. Out of flow, so a long value scrolls instead of widening the
/// field, and an empty one paints nothing over the placeholder beneath it.
fn overlay() -> Style {
    let mut style = Style::new();
    style.position(Position::Absolute { x: 0, y: 0 });
    style.width(Length::Percent(100.0));
    style.height(Length::Cells(1));
    style
}

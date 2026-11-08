#[cfg(test)]
mod tests;

use tuidom::style::{AlignItems, Border, Color, EdgeInsets, Length, Position, Sides, Style};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};

use crate::theme::{Density, FocusMark, Shape, Surface, use_shape};

/// A single-line text field bound to `value`: typing writes the caller's signal, and writing the
/// signal updates the field.
///
/// `placeholder` shows in `--muted-foreground` while the value is empty. `mask` draws every
/// character as the given one, for passwords. `disabled` blocks focus and editing and mutes the
/// text. The frame follows the theme's [`Shape`]: edged in `--input`, which turns `--ring` while
/// focused, and filled with `--input` where the surface has no border. It fills its container's
/// width unless `style` sets one, and scrolls a value too long to show. Stretched taller by its
/// container, it keeps its text on the middle row.
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
    let appearance = derived(move || Appearance {
        disabled: disabled.get(),
        focused: focused.get(),
        shape: shape.get(),
    });
    let frame = derived(move || {
        let mut frame = appearance.get().frame();
        frame.merge(&style.get());
        frame
    });
    let input_style = derived(move || appearance.get().input());

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

#[derive(Clone, Default)]
struct Appearance {
    disabled: bool,
    focused: bool,
    shape: Shape,
}

impl Appearance {
    /// The focus mark actually drawn. A flat field has no edge to ring, so it reverses instead.
    fn mark(&self) -> Option<FocusMark> {
        if !self.focused {
            return None;
        }
        match (self.shape.focus, self.shape.surface) {
            (FocusMark::Ring, Surface::Flat) => Some(FocusMark::Reverse),
            (mark, _) => Some(mark),
        }
    }

    /// Background and foreground variable names, after a reverse focus mark swaps them.
    fn fill_and_text(&self) -> (Option<&'static str>, &'static str) {
        let fill = match self.shape.surface {
            Surface::Bordered => None,
            Surface::HalfBlock | Surface::Flat => Some("--input"),
        };
        let text = if self.disabled {
            "--muted-foreground"
        } else {
            "--foreground"
        };
        match self.mark() {
            Some(FocusMark::Reverse) => (Some(text), fill.unwrap_or("--background")),
            _ => (fill, text),
        }
    }

    fn edge(&self) -> Color {
        if self.mark() == Some(FocusMark::Ring) {
            Color::var("--ring")
        } else if self.disabled {
            Color::var("--border")
        } else {
            Color::var("--input")
        }
    }

    fn frame(&self) -> Style {
        let mut style = Style::new();
        style.width(Length::Percent(100.0));
        if let (Some(fill), _) = self.fill_and_text() {
            style.background(Color::var(fill));
        }
        let padding = match self.shape.density {
            Density::Compact => 0,
            Density::Comfortable => 1,
        };
        style.padding(EdgeInsets::symmetric(padding, 0));
        style.align_items(AlignItems::Center);
        match self.shape.surface {
            Surface::Bordered => {
                style.border(Border::new(self.shape.border));
                style.border_color(self.edge());
            }
            Surface::HalfBlock => {
                style.half_block_edges(Sides::new(false, true, false, true));
                if self.mark() == Some(FocusMark::Ring) {
                    style.half_block_inner_color(self.edge());
                }
            }
            Surface::Flat => {}
        }
        style
    }

    /// The input covers the whole strip. Out of flow, so a long value scrolls instead of widening
    /// the field, and an empty one paints nothing over the placeholder beneath it.
    fn input(&self) -> Style {
        let mut style = Style::new();
        style.position(Position::Absolute { x: 0, y: 0 });
        style.width(Length::Percent(100.0));
        style.height(Length::Cells(1));
        style.color(Color::var(self.fill_and_text().1));
        if self.mark() == Some(FocusMark::Underline) {
            style.underline(true);
        }
        style
    }
}

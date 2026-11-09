#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Border, Color, EdgeInsets, JustifyContent, Sides, Style};
use tuidom_ui::{Callback, View, component, derived, state, view};

use crate::theme::{Density, FocusMark, Shape, Surface, use_shape};

/// What a [`Button`] is for, which decides its colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The main action: filled with `--primary`.
    #[default]
    Primary,
    /// A lower-emphasis action: filled with `--secondary`.
    Secondary,
    /// A dangerous action: filled with `--destructive`.
    Destructive,
    /// Unfilled, framed in `--input`.
    Outline,
    /// Unfilled and unframed until focused.
    Ghost,
}

/// A focusable button showing `label`, which calls `on_press` when clicked, or on Enter or
/// Space while focused.
///
/// `disabled` blocks focus and pressing and draws it muted. Its frame comes from the theme's
/// [`Shape`]: a [`Flat`](Surface::Flat) button is one row tall, the others three. Like
/// any flex item it stretches across its container's cross axis unless `style` says otherwise,
/// and keeps its label centered when it does.
///
/// `style` is merged over the button's own style. The other three style props are the engine's
/// pseudo-state styles, merged over its look while focused, pressed, or disabled.
#[component]
pub fn Button(
    label: String,
    #[prop(default)] variant: ButtonVariant,
    #[prop(default)] disabled: bool,
    #[prop(default)] on_press: Callback<()>,
    #[prop(default)] style: Style,
    #[prop(default)] focus_style: Style,
    #[prop(default)] active_style: Style,
    #[prop(default)] disabled_style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let appearance = derived(move || Appearance {
        variant: variant.get(),
        disabled: disabled.get(),
        focused: focused.get(),
        shape: shape.get(),
    });
    let frame = derived(move || {
        let mut frame = appearance.get().frame();
        frame.merge(&style.get());
        frame
    });
    let pressed = derived(move || {
        let mut pressed = appearance.get().pressed();
        pressed.merge(&active_style.get());
        pressed
    });
    let label_style = derived(move || appearance.get().label());
    let click = on_press.clone();

    view! {
        box {
            focusable: true,
            disabled: disabled.get(),
            style: frame.get(),
            focus_style: focus_style.get(),
            active_style: pressed.get(),
            disabled_style: disabled_style.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_click: move |_| click.call(()),
            on_key_press: move |ev| {
                if matches!(ev.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    on_press.call(());
                }
            },
            text(label.get()) { style: label_style.get() },
        }
    }
}

#[derive(Clone, Default)]
struct Appearance {
    variant: ButtonVariant,
    disabled: bool,
    focused: bool,
    shape: Shape,
}

/// How far a focused half-block button's fill moves toward its text color.
const FOCUS_TINT: f64 = 0.2;

/// The colors a button draws with, as variable names.
struct Colors {
    fill: Option<&'static str>,
    text: &'static str,
}

impl Appearance {
    fn colors(&self) -> Colors {
        let filled = |fill, text| Colors {
            fill: Some(fill),
            text,
        };
        let unfilled = |text| Colors { fill: None, text };
        let colors = match (self.variant, self.disabled) {
            (ButtonVariant::Outline | ButtonVariant::Ghost, true) => unfilled("--muted-foreground"),
            (_, true) => filled("--muted", "--muted-foreground"),
            (ButtonVariant::Primary, false) => filled("--primary", "--primary-foreground"),
            (ButtonVariant::Secondary, false) => filled("--secondary", "--secondary-foreground"),
            (ButtonVariant::Destructive, false) => filled("--destructive", "--foreground"),
            (ButtonVariant::Outline | ButtonVariant::Ghost, false) => unfilled("--foreground"),
        };
        match colors.fill {
            None if self.focused => filled("--accent", "--accent-foreground"),
            _ => colors,
        }
    }

    /// The focus mark actually drawn. A flat button has no edge to ring, so it reverses instead.
    fn mark(&self) -> Option<FocusMark> {
        if !self.focused {
            return None;
        }
        match (self.shape.focus, self.shape.button) {
            (FocusMark::Ring, Surface::Flat) => Some(FocusMark::Reverse),
            (mark, _) => Some(mark),
        }
    }

    /// Background and foreground, after the focus mark changes them. Half-block edges take the
    /// fill's color, so a ring there shifts the fill toward the text instead.
    fn fill_and_text(&self) -> (Option<Color>, Color) {
        let Colors { fill, text } = self.colors();
        let filled = matches!(
            self.variant,
            ButtonVariant::Primary | ButtonVariant::Secondary | ButtonVariant::Destructive
        );
        match (self.mark(), fill) {
            (Some(FocusMark::Reverse), Some(fill)) => (Some(Color::var(text)), Color::var(fill)),
            (Some(FocusMark::Ring), Some(fill))
                if filled && self.shape.button == Surface::HalfBlock =>
            {
                (
                    Some(Color::var(fill).mix(Color::var(text), FOCUS_TINT)),
                    Color::var(text),
                )
            }
            _ => (fill.map(Color::var), Color::var(text)),
        }
    }

    /// The color of the border or half-block edges, where it differs from the fill.
    fn edge(&self) -> Option<Color> {
        match self.shape.button {
            Surface::Bordered if self.mark() == Some(FocusMark::Ring) => {
                return Some(Color::var("--ring"));
            }
            Surface::HalfBlock if self.focused => return None,
            _ => {}
        }
        match self.variant {
            ButtonVariant::Outline if self.disabled => Some(Color::var("--border")),
            ButtonVariant::Outline => Some(Color::var("--input")),
            _ => None,
        }
    }

    fn frame(&self) -> Style {
        let mut style = Style::new();
        style.selectable(false);
        let (fill, _) = self.fill_and_text();
        if let Some(fill) = fill {
            style.background(fill);
        }
        let padding = match self.shape.density {
            Density::Compact => 1,
            Density::Comfortable => 2,
        };
        // Half-block edges sit in the padding, so they need a row above and below the label.
        let rows = u16::from(self.shape.button == Surface::HalfBlock);
        style.padding(EdgeInsets::symmetric(padding, rows));
        style.justify_content(JustifyContent::Center);
        style.align_items(AlignItems::Center);
        match self.shape.button {
            Surface::Bordered => {
                style.border(Border::new(self.shape.border));
                // With no edge color of its own, the border is drawn in the color it sits on.
                style.border_color(self.edge().unwrap_or(Color::CurrentBg));
            }
            Surface::HalfBlock => {
                style.half_block_edges(Sides::new(true, false, true, false));
                if let Some(edge) = self.edge() {
                    style.half_block_inner_color(edge);
                }
            }
            Surface::Flat => {}
        }
        style
    }

    fn pressed(&self) -> Style {
        let mut style = Style::new();
        if let (Some(fill), _) = self.fill_and_text() {
            style.background(fill.darken(0.1));
        }
        style
    }

    fn label(&self) -> Style {
        let mut style = Style::new();
        style.color(self.fill_and_text().1);
        if self.mark() == Some(FocusMark::Underline) {
            style.underline(true);
        }
        style
    }
}

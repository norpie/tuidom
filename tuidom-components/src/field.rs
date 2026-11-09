//! The frame a single-line field draws, shared by the text input and the select trigger.

use tuidom::style::{AlignItems, Border, Color, EdgeInsets, Sides, Style};

use crate::theme::{Density, FocusMark, Shape, Surface};

/// What decides a field's frame and text colors.
#[derive(Clone, Default)]
pub(crate) struct FieldLook {
    pub(crate) disabled: bool,
    pub(crate) focused: bool,
    pub(crate) shape: Shape,
}

/// How far a focused flat field's fill moves toward `--ring`.
const FOCUS_TINT: f64 = 0.3;

impl FieldLook {
    fn mark(&self) -> Option<FocusMark> {
        self.focused.then_some(self.shape.focus)
    }

    /// Background and foreground, after the focus mark changes them. A flat field has no edge to
    /// ring, so a ring tints its fill instead.
    fn fill_and_text(&self) -> (Option<Color>, Color) {
        let fill = match self.shape.field {
            Surface::Bordered => None,
            Surface::HalfBlock | Surface::Flat => Some("--input"),
        };
        let text = if self.disabled {
            "--muted-foreground"
        } else {
            "--foreground"
        };
        match (self.mark(), self.shape.field) {
            (Some(FocusMark::Reverse), _) => (
                Some(Color::var(text)),
                Color::var(fill.unwrap_or("--background")),
            ),
            (Some(FocusMark::Ring), Surface::Flat) => (
                fill.map(|fill| Color::var(fill).mix(Color::var("--ring"), FOCUS_TINT)),
                Color::var(text),
            ),
            _ => (fill.map(Color::var), Color::var(text)),
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

    /// Fill, edge, and padding. Stretched taller than its content, it centers it vertically.
    pub(crate) fn frame(&self) -> Style {
        let mut style = Style::new();
        if let (Some(fill), _) = self.fill_and_text() {
            style.background(fill);
        }
        let padding = match self.shape.density {
            Density::Compact => 0,
            Density::Comfortable => 1,
        };
        style.padding(EdgeInsets::symmetric(padding, 0));
        style.align_items(AlignItems::Center);
        match self.shape.field {
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

    /// The color of the field's text, and its underline when that is the focus mark.
    pub(crate) fn text(&self) -> Style {
        let mut style = Style::new();
        style.color(self.fill_and_text().1);
        if self.mark() == Some(FocusMark::Underline) {
            style.underline(true);
        }
        style
    }
}

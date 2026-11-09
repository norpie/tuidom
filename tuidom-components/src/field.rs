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

impl FieldLook {
    /// The focus mark actually drawn. A flat field has no edge to ring, so it reverses instead.
    fn mark(&self) -> Option<FocusMark> {
        if !self.focused {
            return None;
        }
        match (self.shape.focus, self.shape.field) {
            (FocusMark::Ring, Surface::Flat) => Some(FocusMark::Reverse),
            (mark, _) => Some(mark),
        }
    }

    /// Background and foreground variable names, after a reverse focus mark swaps them.
    fn fill_and_text(&self) -> (Option<&'static str>, &'static str) {
        let fill = match self.shape.field {
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

    /// Fill, edge, and padding. Stretched taller than its content, it centers it vertically.
    pub(crate) fn frame(&self) -> Style {
        let mut style = Style::new();
        if let (Some(fill), _) = self.fill_and_text() {
            style.background(Color::var(fill));
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
        style.color(Color::var(self.fill_and_text().1));
        if self.mark() == Some(FocusMark::Underline) {
            style.underline(true);
        }
        style
    }
}

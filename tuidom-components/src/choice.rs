//! The row a checkbox or a radio option draws: a bracketed mark and a label.

use tuidom::style::{Color, FlexDirection, Style};

use crate::theme::FocusMark;

/// What decides a choice row's colors.
#[derive(Clone, Copy, Default)]
pub(crate) struct ChoiceLook {
    pub(crate) disabled: bool,
    /// Whether this row carries the focus mark.
    pub(crate) focused: bool,
    pub(crate) focus: FocusMark,
}

impl ChoiceLook {
    fn reversed(&self) -> bool {
        self.focused && self.focus == FocusMark::Reverse
    }

    fn colored(&self, color: &'static str) -> Style {
        let mut style = Style::new();
        let color = if self.reversed() {
            "--background"
        } else {
            color
        };
        style.color(Color::var(color));
        style
    }

    pub(crate) fn row(&self) -> Style {
        let mut style = Style::new();
        style.flex_direction(FlexDirection::Row);
        if self.reversed() {
            style.background(Color::var("--foreground"));
        }
        style
    }

    pub(crate) fn bracket(&self) -> Style {
        self.colored(if self.disabled {
            "--border"
        } else if self.focused && self.focus == FocusMark::Ring {
            "--ring"
        } else {
            "--input"
        })
    }

    pub(crate) fn mark(&self) -> Style {
        self.colored(if self.disabled {
            "--muted-foreground"
        } else {
            "--primary"
        })
    }

    pub(crate) fn label(&self) -> Style {
        let mut style = self.colored(if self.disabled {
            "--muted-foreground"
        } else {
            "--foreground"
        });
        if self.focused && self.focus == FocusMark::Underline {
            style.underline(true);
        }
        style
    }
}

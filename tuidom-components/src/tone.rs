use tuidom::style::Color;

/// What an accent means, which decides its color: a [`ProgressBar`](crate::ProgressBar)'s fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    /// Filled with `--primary`.
    #[default]
    Default,
    /// Filled with `--success`.
    Success,
    /// Filled with `--warning`.
    Warning,
    /// Filled with `--destructive`.
    Destructive,
}

impl Tone {
    pub(crate) fn color(self) -> Color {
        Color::var(match self {
            Tone::Default => "--primary",
            Tone::Success => "--success",
            Tone::Warning => "--warning",
            Tone::Destructive => "--destructive",
        })
    }
}

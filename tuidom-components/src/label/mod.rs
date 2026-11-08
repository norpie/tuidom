#[cfg(test)]
mod tests;

use tuidom::style::{Color, Style};
use tuidom_ui::{View, component, derived, view};

/// How much a [`Label`] stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelTone {
    /// Ordinary text, in `--foreground`.
    #[default]
    Default,
    /// Hints and secondary text, in `--muted-foreground`.
    Muted,
}

/// A line of themed text.
///
/// It is a single text node with no box around it, so it adds no flex level. `style` is merged
/// over the tone's color, and is how a label is made bold, italic, or underlined.
#[component]
pub fn Label(
    text: String,
    #[prop(default)] tone: LabelTone,
    #[prop(default)] style: Style,
) -> View {
    let styled = derived(move || {
        let mut styled = Style::new();
        styled.color(Color::var(match tone.get() {
            LabelTone::Default => "--foreground",
            LabelTone::Muted => "--muted-foreground",
        }));
        styled.merge(&style.get());
        styled
    });
    view! { text(text.get()) { style: styled.get() } }
}

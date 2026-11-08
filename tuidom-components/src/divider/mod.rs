#[cfg(test)]
mod tests;

use tuidom::style::{AlignSelf, Border, Color, Sides, Style};
use tuidom_ui::{View, component, derived, view};

use crate::theme::use_shape;

/// Which way a [`Divider`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// Left to right.
    #[default]
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// A one-cell line in `--border`, drawn with the theme's border charset so it meets the
/// borders around it.
///
/// It stretches across its container's cross axis, so a horizontal divider belongs in a column
/// and a vertical one in a row. `style` is merged over its own.
#[component]
pub fn Divider(#[prop(default)] orientation: Orientation, #[prop(default)] style: Style) -> View {
    let shape = use_shape();
    let line = derived(move || {
        let sides = match orientation.get() {
            Orientation::Horizontal => Sides::new(true, false, false, false),
            Orientation::Vertical => Sides::new(false, false, false, true),
        };
        let mut line = Style::new();
        line.border(Border::new(shape.get().border).with_sides(sides));
        line.border_color(Color::var("--border"));
        line.align_self(AlignSelf::Stretch);
        line.flex_shrink(0.0);
        line.merge(&style.get());
        line
    });
    view! { box { style: line.get() } }
}

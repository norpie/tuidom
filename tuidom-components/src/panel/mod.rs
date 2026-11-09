#[cfg(test)]
mod tests;

use tuidom::style::{Border, Color, EdgeInsets, FlexDirection, Position, Sides, Style};
use tuidom_ui::{Children, View, component, derived, view};

use crate::theme::{Density, Shape, Surface, use_shape};

/// A surface in `--card` holding `children` in a column, with an optional `title`.
///
/// The edge follows the theme's [`Shape`](crate::Shape): a border in `--border`, half-block
/// edges, or none. A bordered panel draws its title on the top border; the others draw it as a
/// bold first row. A title on the border takes no width, so one longer than the content runs
/// past the corner unless `style` sizes the panel. Plain text inside takes `--card-foreground`,
/// since the panel points `--foreground` at it for its subtree.
///
/// `style` is merged over the panel's own style.
#[component]
pub fn Panel(
    #[prop(default)] title: String,
    #[prop(default)] style: Style,
    children: Children,
) -> View {
    let shape = use_shape();
    let frame = derived(move || {
        let mut frame = surface(&shape.get());
        frame.merge(&style.get());
        frame
    });
    let heading = derived(move || heading(&shape.get()));

    view! {
        box {
            style: frame.get(),
            if !title.get().is_empty() {
                text(title_text(&title.get(), &shape.get())) { style: heading.get() }
            },
            { children },
        }
    }
}

fn surface(shape: &Shape) -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.background(Color::var("--card"));
    style.color_var("--foreground", Color::var("--card-foreground"));
    let padding = match shape.density {
        Density::Compact => 1,
        Density::Comfortable => 2,
    };
    match shape.container {
        Surface::Bordered => {
            style.border(Border::new(shape.border));
            style.border_color(Color::var("--border"));
            style.padding(EdgeInsets::symmetric(padding, 0));
        }
        Surface::HalfBlock => {
            style.half_block_edges(Sides::new(true, false, true, false));
            style.padding(EdgeInsets::symmetric(padding, 1));
        }
        Surface::Flat => style.padding(EdgeInsets::symmetric(padding, 0)),
    }
    style
}

/// On a border the title is set off from the line by a space each side.
fn title_text(title: &str, shape: &Shape) -> String {
    match shape.container {
        Surface::Bordered => format!(" {title} "),
        Surface::HalfBlock | Surface::Flat => title.to_string(),
    }
}

fn heading(shape: &Shape) -> Style {
    let mut style = Style::new();
    style.bold(true);
    // Absolute offsets count from inside the border, so this lands on the top border row.
    if shape.container == Surface::Bordered {
        style.position(Position::Absolute { x: 1, y: -1 });
    }
    style
}

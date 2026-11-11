#[cfg(test)]
mod tests;

use tuidom::style::{AlignItems, Color, EdgeInsets, FlexGap, JustifyContent, Length, Style};
use tuidom_ui::{Children, View, component, derived, view};

use crate::theme::{Density, use_shape};

/// A bar in `--muted` across its container, with `left` at its start and `right` at its end.
///
/// Plain text inside takes `--muted-foreground`, since the bar points `--foreground` at it for
/// its subtree. Each side lays its content out in a row, so several items passed as a
/// [`fragment`](tuidom_ui::fragment) sit two cells apart. The bar does not pin
/// itself anywhere: make it the last child of a full-height column to keep it at the bottom.
///
/// `style` is merged over the bar's own style.
#[component]
pub fn StatusBar(
    #[prop(default)] left: Children,
    #[prop(default)] right: Children,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let frame = derived(move || {
        let mut frame = Style::new();
        frame.width(Length::Percent(100.0));
        frame.background(Color::var("--muted"));
        frame.color_var("--foreground", Color::var("--muted-foreground"));
        frame.justify_content(JustifyContent::SpaceBetween);
        frame.align_items(AlignItems::Center);
        frame.flex_shrink(0.0);
        let padding = match shape.get().density {
            Density::Compact => 1,
            Density::Comfortable => 2,
        };
        frame.padding(EdgeInsets::symmetric(padding, 0));
        frame.merge(&style.get());
        frame
    });

    view! {
        box {
            style: frame.get(),
            box { style: side(), { left } },
            box { style: side(), { right } },
        }
    }
}

/// A shortcut hint for a [`StatusBar`]: `key` in bold `--primary`, then `label`.
///
/// It only draws the hint; binding the key is up to the caller. `style` is merged over its
/// box, which has no style of its own.
#[component]
pub fn KeyHint(key: String, label: String, #[prop(default)] style: Style) -> View {
    view! {
        box {
            style: style.get(),
            text(key.get()) { style: key_style() },
            text(format!(" {}", label.get())),
        }
    }
}

fn side() -> Style {
    let mut style = Style::new();
    style.gap(FlexGap::new(0, 2));
    style.align_items(AlignItems::Center);
    style
}

fn key_style() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--primary"));
    style.bold(true);
    style
}

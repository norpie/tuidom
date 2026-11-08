#[cfg(test)]
mod tests;

use tuidom::NodeId;
use tuidom::style::{AnchorAxis, AnchorEdge, Color, EdgeInsets, Position, Style};
use tuidom_ui::{Children, Signal, View, component, derived, node_ref, state, view};

use crate::theme::{Theme, use_theme};

/// Which side of its content a [`Tooltip`] shows on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// Above the content, left edges lined up.
    #[default]
    Above,
    /// Below the content, left edges lined up.
    Below,
}

/// `children`, with `text` shown beside them in `--popover` while focus is inside them.
///
/// Hovering focuses, so the tip also shows on hover. It is drawn over everything else and takes
/// no space. It does not move to the other side at the screen edge, so pick `side` for where the
/// content sits.
#[component]
pub fn Tooltip(text: String, #[prop(default)] side: Side, children: Children) -> View {
    let theme = use_theme();
    let shown = state(false);
    let content = node_ref();
    let tip = derived(move || tip_style(content.node(), side.get(), theme));

    view! {
        box {
            ref: content,
            on_focus: move |_| shown.set(true),
            on_blur: move |_| shown.set(false),
            { children },
            if shown.get() {
                portal {
                    box { style: tip.get(), text(text.get()) { style: tip_text() } }
                }
            },
        }
    }
}

/// The tip's frame, beside the content. The palette is declared on it because the portal mounts
/// it outside any `ThemeScope` the content sits in.
fn tip_style(content: Option<NodeId>, side: Side, theme: Option<Signal<Theme>>) -> Style {
    let mut style = Style::new();
    if let Some(theme) = theme {
        theme.get().palette.declare_on(&mut style);
    }
    style.background(Color::var("--popover"));
    style.padding(EdgeInsets::symmetric(1, 0));
    if let Some(content) = content {
        let y = match side {
            Side::Above => AnchorAxis::new(AnchorEdge::Start, AnchorEdge::End),
            Side::Below => AnchorAxis::new(AnchorEdge::End, AnchorEdge::Start),
        };
        style.position(Position::Anchored {
            anchor: content,
            x: AnchorAxis::new(AnchorEdge::Start, AnchorEdge::Start),
            y,
        });
    }
    style
}

fn tip_text() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--popover-foreground"));
    style
}

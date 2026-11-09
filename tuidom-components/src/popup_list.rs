use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{
    AnchorAxis, AnchorEdge, Border, Color, EdgeInsets, FlexDirection, Position, Sides, Style,
};
use tuidom_ui::{Callback, Signal, View, component, derived, state, view};
use unicode_width::UnicodeWidthStr;

use crate::theme::{Density, Shape, Surface, Theme, use_shape, use_theme};

/// One row of a [`PopupList`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct PopupRow {
    pub(crate) label: String,
    /// Drawn after the label, right-aligned with the other rows' details.
    pub(crate) detail: String,
    /// Draws the detail in `--muted-foreground` while the row is not highlighted.
    pub(crate) muted_detail: bool,
    pub(crate) disabled: bool,
    /// Draws a line in `--border` above the row.
    pub(crate) separated: bool,
}

/// `rows` drawn below `anchor` over everything else, holding the keyboard while mounted.
///
/// Up and Down move a highlight in `--accent` over the enabled rows, starting from the first
/// enabled row at or after `initial`. Enter or Space, or a click on an enabled row, calls
/// `on_choose` with the row's index; Escape or a press outside calls `on_close`.
#[component]
pub(crate) fn PopupList(
    rows: Vec<PopupRow>,
    anchor: Option<NodeId>,
    #[prop(default)] initial: usize,
    on_choose: Callback<usize>,
    on_close: Callback<()>,
) -> View {
    let shape = use_shape();
    let theme = use_theme();
    // Callbacks are not Copy; in signals, every row's handler can share them.
    let on_choose = state(on_choose);
    let on_close = state(on_close);
    let choose = move |index: usize| {
        if let Some(on_choose) = on_choose.try_get() {
            on_choose.call(index);
        }
    };
    let close = move || {
        if let Some(on_close) = on_close.try_get() {
            on_close.call(());
        }
    };

    let enabled = move |i: usize| rows.get().get(i).is_some_and(|row| !row.disabled);
    let highlight = state(first_enabled(&rows.get(), initial.get()));
    let step = move |forward: bool| {
        let Some(from) = highlight.get() else {
            return;
        };
        let count = rows.get().len();
        let next = if forward {
            (from + 1..count).find(|&i| enabled(i))
        } else {
            (0..from).rev().find(|&i| enabled(i))
        };
        if next.is_some() {
            highlight.set(next);
        }
    };
    let widest = derived(move || rows.get().iter().map(|r| r.label.width()).max());
    let widest_detail = derived(move || rows.get().iter().map(|r| r.detail.width()).max());
    let frame = derived(move || list_style(anchor.get(), &shape.get(), theme));

    view! {
        box {
            trap_focus: true,
            style: frame.get(),
            on_press_outside: move |_| close(),
            on_key_press: move |ev| {
                match ev.code {
                    KeyCode::Up => step(false),
                    KeyCode::Down => step(true),
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(index) = highlight.get().filter(|&i| enabled(i)) {
                            choose(index);
                        }
                    }
                    KeyCode::Esc => close(),
                    _ => return,
                }
                ev.prevent_default();
                ev.stop_propagation();
            },
            each i, row in rows.get() key = (i, row.clone()) {
                if row.separated {
                    box { style: separator(&shape.get()) }
                },
                box {
                    style: row_style(highlight.get() == Some(i), &shape.get()),
                    on_click: move |_| {
                        if !row.disabled {
                            choose(i);
                        }
                    },
                    text(padded(&row.label, widest.get().unwrap_or(0))) {
                        style: label_text(row.disabled, highlight.get() == Some(i))
                    },
                    text(aligned_right(&row.detail, widest_detail.get().unwrap_or(0))) {
                        style: detail_text(row.disabled, row.muted_detail, highlight.get() == Some(i))
                    },
                }
            },
        }
    }
}

fn first_enabled(rows: &[PopupRow], from: usize) -> Option<usize> {
    let from = from.min(rows.len().saturating_sub(1));
    (from..rows.len())
        .chain(0..from)
        .find(|&i| !rows[i].disabled)
}

/// `label` followed by spaces out to `width` columns.
pub(crate) fn padded(label: &str, width: usize) -> String {
    let fill = width.saturating_sub(label.width());
    format!("{label}{}", " ".repeat(fill))
}

fn aligned_right(text: &str, width: usize) -> String {
    let fill = width.saturating_sub(text.width());
    format!("{}{text}", " ".repeat(fill))
}

/// The list's frame: below the anchor, in the popover colors, with the theme's surface. The
/// palette is declared on it because a portal mounts it outside any `ThemeScope` its anchor
/// sits in.
fn list_style(anchor: Option<NodeId>, shape: &Shape, theme: Option<Signal<Theme>>) -> Style {
    let mut style = Style::new();
    if let Some(theme) = theme {
        theme.get().palette.declare_on(&mut style);
    }
    style.stacking_context(true);
    style.selectable(false);
    style.flex_direction(FlexDirection::Column);
    style.background(Color::var("--popover"));
    if let Some(anchor) = anchor {
        style.position(Position::Anchored {
            anchor,
            x: AnchorAxis::new(AnchorEdge::Start, AnchorEdge::Start),
            y: AnchorAxis::new(AnchorEdge::End, AnchorEdge::Start),
        });
    }
    match shape.surface {
        Surface::Bordered => {
            style.border(Border::new(shape.border));
            style.border_color(Color::var("--border"));
        }
        Surface::HalfBlock => style.half_block_edges(Sides::new(false, true, false, true)),
        Surface::Flat => {}
    }
    style
}

/// A row, padded like a field so a select's options line up with it.
fn row_style(highlighted: bool, shape: &Shape) -> Style {
    let mut style = Style::new();
    let padding = match shape.density {
        Density::Compact => 0,
        Density::Comfortable => 1,
    };
    style.padding(EdgeInsets::symmetric(padding, 0));
    if highlighted {
        style.background(Color::var("--accent"));
    }
    style
}

/// A line across the list, drawn as a top border so it takes the theme's charset.
fn separator(shape: &Shape) -> Style {
    let mut style = Style::new();
    style.border(Border::new(shape.border).with_sides(Sides::new(true, false, false, false)));
    style.border_color(Color::var("--border"));
    style
}

fn label_text(disabled: bool, highlighted: bool) -> Style {
    let mut style = Style::new();
    style.color(Color::var(if disabled {
        "--muted-foreground"
    } else if highlighted {
        "--accent-foreground"
    } else {
        "--popover-foreground"
    }));
    style
}

fn detail_text(disabled: bool, muted: bool, highlighted: bool) -> Style {
    if muted && !highlighted {
        let mut style = Style::new();
        style.color(Color::var("--muted-foreground"));
        return style;
    }
    label_text(disabled, highlighted)
}

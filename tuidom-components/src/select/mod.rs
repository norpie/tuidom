#[cfg(test)]
mod tests;

use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{
    AnchorAxis, AnchorEdge, Border, Color, EdgeInsets, FlexDirection, Position, Sides, Style,
};
use tuidom_ui::{Signal, View, component, derived, node_ref, state, view};
use unicode_width::UnicodeWidthStr;

use crate::field::FieldLook;
use crate::theme::{Density, Shape, Surface, Theme, use_shape, use_theme};

/// A field showing the selected option, which opens a list of `options` to pick from: `selected`
/// is the index into `options`, read and written.
///
/// A click, or Enter or Space while focused, opens the list below the field, drawn over
/// everything else in `--popover`. While it is open it holds the keyboard: Up and Down move a
/// highlight in `--accent`, Enter or Space picks the highlighted option and closes it, and Escape
/// or a press anywhere outside closes it without changing the selection. The field is framed like
/// a [`TextInput`](crate::TextInput), and it and the list are both as wide as the widest option,
/// so a field stretched wider than that is wider than its list. `disabled` keeps it closed and
/// mutes it.
///
/// `style` is merged over the field's frame.
#[component]
pub fn Select(
    options: Vec<String>,
    selected: Signal<usize>,
    #[prop(default)] disabled: bool,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let theme = use_theme();
    let focused = state(false);
    let open = state(false);
    let highlight = state(0_usize);
    let trigger = node_ref();

    let look = derived(move || FieldLook {
        disabled: disabled.get(),
        focused: focused.get(),
        shape: shape.get(),
    });
    let frame = derived(move || {
        let mut frame = look.get().frame();
        frame.merge(&style.get());
        frame
    });
    let widest = derived(move || options.get().iter().map(|o| o.width()).max().unwrap_or(0));
    let current = derived(move || {
        let current = options
            .get()
            .get(selected.get())
            .cloned()
            .unwrap_or_default();
        padded(&current, widest.get())
    });
    let list = derived(move || list_style(trigger.node(), &shape.get(), theme));

    let show = move || {
        let count = options.get().len();
        if count > 0 {
            highlight.set(selected.get().min(count - 1));
            open.set(true);
        }
    };
    let choose = move |index: usize| {
        selected.set(index);
        open.set(false);
    };

    view! {
        box {
            ref: trigger,
            focusable: true,
            disabled: disabled.get(),
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_click: move |_| show(),
            on_key_press: move |ev| {
                if matches!(ev.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    ev.prevent_default();
                    ev.stop_propagation();
                    show();
                }
            },
            text(current.get()) { style: look.get().text() },
            text(" ▾") { style: chevron() },
            if open.get() {
                portal {
                    box {
                        trap_focus: true,
                        style: list.get(),
                        on_press_outside: move |_| open.set(false),
                        on_key_press: move |ev| {
                            let last = options.get().len().saturating_sub(1);
                            match ev.code {
                                KeyCode::Up => highlight.update(|h| *h = h.saturating_sub(1)),
                                KeyCode::Down => highlight.update(|h| *h = (*h + 1).min(last)),
                                KeyCode::Enter | KeyCode::Char(' ') => choose(highlight.get()),
                                KeyCode::Esc => open.set(false),
                                _ => return,
                            }
                            ev.prevent_default();
                            ev.stop_propagation();
                        },
                        each i, option in options.get() key = (i, option.clone()) {
                            box {
                                style: row_style(highlight.get() == i, shape.get().density),
                                on_click: move |_| choose(i),
                                text(padded(&option, widest.get())) {
                                    style: row_text(highlight.get() == i)
                                },
                                text(if selected.get() == i { " ✓" } else { "  " }) {
                                    style: row_text(highlight.get() == i)
                                },
                            }
                        },
                    }
                }
            },
        }
    }
}

/// `label` followed by spaces out to `width` columns.
fn padded(label: &str, width: usize) -> String {
    let fill = width.saturating_sub(label.width());
    format!("{label}{}", " ".repeat(fill))
}

fn chevron() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--muted-foreground"));
    style
}

/// The list's frame: below the field, in the popover colors, with the theme's surface. The
/// palette is declared on it because the portal mounts it outside any `ThemeScope` the field
/// sits in.
fn list_style(trigger: Option<NodeId>, shape: &Shape, theme: Option<Signal<Theme>>) -> Style {
    let mut style = Style::new();
    if let Some(theme) = theme {
        theme.get().palette.declare_on(&mut style);
    }
    style.stacking_context(true);
    style.flex_direction(FlexDirection::Column);
    style.background(Color::var("--popover"));
    if let Some(trigger) = trigger {
        style.position(Position::Anchored {
            anchor: trigger,
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

/// An option's row, padded like the field so the two line up.
fn row_style(highlighted: bool, density: Density) -> Style {
    let mut style = Style::new();
    let padding = match density {
        Density::Compact => 0,
        Density::Comfortable => 1,
    };
    style.padding(EdgeInsets::symmetric(padding, 0));
    if highlighted {
        style.background(Color::var("--accent"));
    }
    style
}

fn row_text(highlighted: bool) -> Style {
    let mut style = Style::new();
    style.color(Color::var(if highlighted {
        "--accent-foreground"
    } else {
        "--popover-foreground"
    }));
    style
}

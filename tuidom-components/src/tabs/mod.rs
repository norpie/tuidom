#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Color, EdgeInsets, FlexDirection, Style};
use tuidom_ui::{Children, Signal, View, component, derived, node_ref, state, view};

use crate::roving::focus_item;
use crate::theme::{Density, FocusMark, Shape, use_shape};

/// A strip of `tabs` above `children`, with `selected` the index of the active tab, read and
/// written.
///
/// The strip is drawn in `--muted` with the active tab in `--background`. It is a single tab stop,
/// on the active tab, but each tab takes focus on its own, so hovering one focuses it and the
/// theme's focus mark is drawn on it. Left and Right move focus and switch tabs together, falling
/// through to spatial navigation at the first and last tab. Enter, Space, or a click activates a
/// tab. The children are placed below the strip whatever the tab; pick the pane in them
/// with a `match` on `selected`.
///
/// `style` is merged over the outer column's own style.
#[component]
pub fn Tabs(
    tabs: Vec<String>,
    selected: Signal<usize>,
    #[prop(default)] style: Style,
    children: Children,
) -> View {
    let shape = use_shape();
    let focused = state(None::<usize>);
    let strip_ref = node_ref();
    let column = derived(move || {
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.merge(&style.get());
        column
    });
    let tab = move |index: usize| TabLook {
        active: selected.get() == index,
        focused: focused.get() == Some(index),
        shape: shape.get(),
    };
    let tab_stop = move |index: usize| {
        let last = tabs.get().len().saturating_sub(1);
        selected.get().min(last) == index
    };

    view! {
        box {
            style: column.get(),
            box {
                ref: strip_ref,
                style: strip(),
                on_key_press: move |ev| {
                    let Some(current) = focused.get() else {
                        return;
                    };
                    let last = tabs.get().len().saturating_sub(1);
                    let chosen = match ev.code {
                        KeyCode::Left if current > 0 => current - 1,
                        KeyCode::Right if current < last => current + 1,
                        KeyCode::Enter | KeyCode::Char(' ') => current,
                        _ => return,
                    };
                    ev.prevent_default();
                    ev.stop_propagation();
                    selected.set(chosen);
                    focus_item(strip_ref, chosen);
                },
                each i, name in tabs.get() key = (i, name.clone()) {
                    box {
                        item: i,
                        focusable: true,
                        tab_stop: tab_stop(i),
                        style: tab(i).frame(),
                        on_focus: move |_| focused.set(Some(i)),
                        on_blur: move |_| focused.set(None),
                        on_click: move || selected.set(i),
                        text(name.clone()) { style: tab(i).label() },
                    }
                },
            },
            { children },
        }
    }
}

fn strip() -> Style {
    let mut style = Style::new();
    style.selectable(false);
    style.background(Color::var("--muted"));
    style
}

#[derive(Clone, Default)]
struct TabLook {
    active: bool,
    focused: bool,
    shape: Shape,
}

impl TabLook {
    fn mark(&self) -> Option<FocusMark> {
        self.focused.then_some(self.shape.focus)
    }

    fn frame(&self) -> Style {
        let mut style = Style::new();
        let padding = match self.shape.density {
            Density::Compact => 1,
            Density::Comfortable => 2,
        };
        style.padding(EdgeInsets::symmetric(padding, 0));
        match self.mark() {
            Some(FocusMark::Reverse) => style.background(Color::var("--foreground")),
            _ if self.active => style.background(Color::var("--background")),
            _ => {}
        }
        style
    }

    fn label(&self) -> Style {
        let mut style = Style::new();
        let color = match self.mark() {
            Some(FocusMark::Ring) => "--ring",
            Some(FocusMark::Reverse) => "--background",
            _ if self.active => "--foreground",
            _ => "--muted-foreground",
        };
        style.color(Color::var(color));
        if self.mark() == Some(FocusMark::Underline) {
            style.underline(true);
        }
        style
    }
}

#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Color, EdgeInsets, FlexDirection, Style};
use tuidom_ui::{Children, Signal, View, component, derived, state, view};

use crate::theme::{Density, FocusMark, Shape, use_shape};

/// A strip of `tabs` above `children`, with `selected` the index of the active tab, read and
/// written.
///
/// The strip is drawn in `--muted` with the active tab in `--background`. It is a single focus
/// stop: while it is focused, Left and Right switch tabs, falling through to spatial navigation
/// at the first and last tab. Clicking a tab activates it. The theme's focus mark is drawn on the
/// active tab. The children are placed below the strip whatever the tab; pick the pane in them
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
    let focused = state(false);
    let column = derived(move || {
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.merge(&style.get());
        column
    });
    let tab = move |index: usize| TabLook {
        active: selected.get() == index,
        focused: focused.get(),
        shape: shape.get(),
    };

    view! {
        box {
            style: column.get(),
            box {
                focusable: true,
                style: strip(),
                on_focus: move |_| focused.set(true),
                on_blur: move |_| focused.set(false),
                on_key_press: move |ev| {
                    let last = tabs.get().len().saturating_sub(1);
                    let current = selected.get().min(last);
                    let next = match ev.code {
                        KeyCode::Left if current > 0 => Some(current - 1),
                        KeyCode::Right if current < last => Some(current + 1),
                        _ => None,
                    };
                    if let Some(next) = next {
                        ev.prevent_default();
                        ev.stop_propagation();
                        selected.set(next);
                    }
                },
                each i, name in tabs.get() key = (i, name.clone()) {
                    box {
                        style: tab(i).frame(),
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
    /// Whether the strip holds focus.
    focused: bool,
    shape: Shape,
}

impl TabLook {
    fn mark(&self) -> Option<FocusMark> {
        (self.active && self.focused).then_some(self.shape.focus)
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

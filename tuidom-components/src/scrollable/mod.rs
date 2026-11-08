#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Color, FlexDirection, Overflow, Style};
use tuidom_ui::{Children, View, component, derived, node_ref, state, view};

use crate::theme::{FocusMark, use_shape};

/// Which way a [`Scrollable`] scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollAxis {
    /// Up and down, with the children in a column.
    #[default]
    Vertical,
    /// Left and right, with the children in a row.
    Horizontal,
    /// Both ways, with the children in a column.
    Both,
}

/// A focusable area that scrolls `children` along `axis`, with scrollbars in `--muted` and
/// `--muted-foreground`.
///
/// While the area itself is focused, the arrow keys on its axis scroll it one cell, and once it
/// can scroll no further that way they move focus as usual. A focused node inside keeps its
/// arrows for navigation. Page Up, Page Down, Home, and End scroll it through the engine's own
/// keyboard scrolling either way. Focus recolors the thumb to `--ring`. It shrinks below its
/// content on the scrolling axis, so give it a size through `style` or its container.
///
/// `style` is merged over the area's own style.
#[component]
pub fn Scrollable(
    #[prop(default)] axis: ScrollAxis,
    #[prop(default)] style: Style,
    children: Children,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let area = node_ref();
    let frame = derived(move || {
        let mut frame = Style::new();
        let (direction, x, y) = match axis.get() {
            ScrollAxis::Vertical => (FlexDirection::Column, Overflow::Visible, Overflow::Scroll),
            ScrollAxis::Horizontal => (FlexDirection::Row, Overflow::Scroll, Overflow::Visible),
            ScrollAxis::Both => (FlexDirection::Column, Overflow::Scroll, Overflow::Scroll),
        };
        frame.flex_direction(direction);
        frame.overflow_x(x);
        frame.overflow_y(y);
        frame.scrollbar_track_color(Color::var("--muted"));
        let thumb = match (focused.get(), shape.get().focus) {
            (true, FocusMark::Ring) => "--ring",
            (true, _) => "--foreground",
            (false, _) => "--muted-foreground",
        };
        frame.scrollbar_thumb_color(Color::var(thumb));
        frame.merge(&style.get());
        frame
    });

    view! {
        box {
            ref: area,
            focusable: true,
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_key_press: move |ev| {
                if Some(ev.target()) != area.node() {
                    return;
                }
                // An axis that does not scroll cannot move, so its arrows fall through below.
                let (dx, dy) = match ev.code {
                    KeyCode::Up => (0, -1),
                    KeyCode::Down => (0, 1),
                    KeyCode::Left => (-1, 0),
                    KeyCode::Right => (1, 0),
                    _ => return,
                };
                let before = area.scroll_offset();
                area.scroll_by(dx, dy);
                if area.scroll_offset() != before {
                    ev.prevent_default();
                    ev.stop_propagation();
                }
            },
            { children },
        }
    }
}

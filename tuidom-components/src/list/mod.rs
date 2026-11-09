#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Color, FlexDirection, Length, Overflow, Style};
use tuidom_ui::{
    Callback, Children, Signal, View, component, derived, node_ref, state, view, virtual_list,
    virtual_list_ref,
};

/// A scrolling list of `count` rows, each built by `render` from its index, with one row
/// selected: `selected` is that index, read and written.
///
/// Only the rows in view exist, so `count` can be large. Each row is `row_height` cells tall,
/// one unless given, and `render` runs when a row scrolls into view. While the list is focused,
/// Up and Down move the selection a row, PageUp and PageDown a screenful, and Home and End to
/// either end, scrolling as little as needed to keep it in view; at the first or last row, Up
/// or Down moves focus on as usual. Enter calls `on_activate` with the selected index, and a
/// click selects a row. The selected row is filled with `--accent` while the list is focused
/// and `--muted` otherwise; plain text in it takes the matching foreground.
///
/// `style` is merged over the list's own and must bound its height, a fixed height or flex
/// within a bounded parent, since the list scrolls within whatever height it is given.
#[component]
pub fn List(
    count: usize,
    render: Callback<usize, View>,
    selected: Signal<usize>,
    #[prop(default)] on_activate: Callback<usize>,
    #[prop(default = 1)] row_height: u16,
    #[prop(default)] style: Style,
) -> View {
    let focused = state(false);
    let frame_ref = node_ref();
    let list = virtual_list_ref();
    let frame = derived(move || {
        let mut frame = Style::new();
        frame.flex_direction(FlexDirection::Column);
        // Without it the frame floors at the full scroll height and outgrows a bounded parent.
        frame.overflow_y(Overflow::Clip);
        frame.merge(&style.get());
        frame
    });

    let select = move |index: usize| {
        selected.set(index);
        list.scroll_into_view(index);
    };
    let page = move || {
        let height = frame_ref
            .layout()
            .map_or(1, |layout| layout.scrollport.height);
        usize::from((height / row_height.get().max(1)).max(1))
    };

    let rows = Children::new(move || {
        let render = render.clone();
        virtual_list(
            list_style(),
            row_height.get().max(1),
            2,
            move || count.get(),
            move |i| {
                let content = Children::new({
                    let render = render.clone();
                    move || render.call(i)
                });
                view! {
                    box {
                        style: row_style(selected.get() == i, focused.get(), row_height.get()),
                        on_click: move |_| {
                            select(i);
                            frame_ref.focus();
                        },
                        { content }
                    }
                }
            },
        )
        .list_ref(list)
        .into()
    });

    view! {
        box {
            ref: frame_ref,
            focusable: true,
            style: frame.get(),
            on_focus: move |_| focused.set(true),
            on_blur: move |_| focused.set(false),
            on_key_press: move |ev| {
                let count = count.get();
                if count == 0 {
                    return;
                }
                let last = count - 1;
                let current = selected.get().min(last);
                match moved(ev.code, current, last, page()) {
                    Some(next) => select(next),
                    None if ev.code == KeyCode::Enter => on_activate.call(current),
                    None => return,
                }
                ev.prevent_default();
                ev.stop_propagation();
            },
            { rows }
        }
    }
}

/// Where a key moves a selection at `current` among rows `0..=last`, `page` rows to a screenful.
/// `None` for a key that does not move it, including Up or Down at an end, so focus moves on.
pub(crate) fn moved(code: KeyCode, current: usize, last: usize, page: usize) -> Option<usize> {
    match code {
        KeyCode::Up if current > 0 => Some(current - 1),
        KeyCode::Down if current < last => Some(current + 1),
        KeyCode::PageUp => Some(current.saturating_sub(page)),
        KeyCode::PageDown => Some((current + page).min(last)),
        KeyCode::Home => Some(0),
        KeyCode::End => Some(last),
        _ => None,
    }
}

/// The scroll container fills the list's frame.
pub(crate) fn list_style() -> Style {
    let mut style = Style::new();
    style.flex_grow(1.0);
    style.flex_shrink(1.0);
    style
}

pub(crate) fn row_style(selected: bool, focused: bool, height: u16) -> Style {
    let mut style = Style::new();
    style.height(Length::Cells(height.max(1)));
    style.flex_shrink(0.0);
    if selected {
        let (fill, text) = if focused {
            ("--accent", "--accent-foreground")
        } else {
            ("--muted", "--muted-foreground")
        };
        style.background(Color::var(fill));
        style.color_var("--foreground", Color::var(text));
    }
    style
}

#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{Border, Color, FlexDirection, Length, Overflow, ScrollbarShow, Sides, Style};
use tuidom_ui::{
    Callback, Children, Signal, View, component, derived, node_ref, on_cleanup, on_mount, state,
    use_document, view, virtual_list, virtual_list_ref,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::list::{list_style, moved, row_style};
use crate::popup_list::padded;
use crate::theme::{Shape, use_shape};

/// One column of a [`Table`]: the `title` in its header and its `width` in cells.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Column {
    /// The text in the column's header.
    pub title: String,
    /// How many cells the column takes. Text wider than that is cut short with `…`.
    pub width: u16,
}

impl Column {
    /// A column titled `title`, `width` cells wide.
    pub fn new(title: impl Into<String>, width: u16) -> Self {
        Column {
            title: title.into(),
            width,
        }
    }
}

/// A scrolling table of `count` rows under a header naming its `columns`, with one row
/// selected: `selected` is that index, read and written.
///
/// `cell` gives the text at a row and column index. Only the rows in view exist, so `count` can
/// be large; a cell's text is asked for when its row scrolls into view, and again when a signal
/// it read changes. Columns are separated by a line in `--border`, and a line under the bold
/// header divides it from the rows. The header stays put while the rows scroll, and follows them
/// sideways when the columns are wider than the table. Selecting, scrolling and activating work
/// as in a [`List`](crate::List).
///
/// `style` is merged over the table's own and must bound its height, as a list's must.
#[component]
pub fn Table(
    columns: Vec<Column>,
    count: usize,
    cell: Callback<(usize, usize), String>,
    selected: Signal<usize>,
    #[prop(default)] on_activate: Callback<usize>,
    #[prop(default)] style: Style,
) -> View {
    let shape = use_shape();
    let focused = state(false);
    let frame_ref = node_ref();
    let header = node_ref();
    let list = virtual_list_ref();
    // Callbacks are not Copy; in a signal, every cell's binding can share it.
    let cell = state(cell);
    let text_at = move |row: usize, column: usize| {
        cell.try_get()
            .map(|cell| cell.call((row, column)))
            .unwrap_or_default()
    };
    let width = derived(move || total_width(&columns.get()));
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
        let body = frame_ref
            .document()
            .zip(list.node())
            .and_then(|(doc, body)| doc.get_node(body)?.layout);
        usize::from(body.map_or(1, |layout| layout.scrollport.height).max(1))
    };

    // Scroll events do not bubble, so the header follows the body through a listener on the
    // body's own scroll container.
    on_mount(move || {
        let (Some(doc), Some(body)) = (use_document(), list.node()) else {
            return;
        };
        match doc.on_scroll(body, move |ev| header.scroll_to(ev.x, 0)) {
            Ok(handle) => on_cleanup(move || {
                doc.remove_listener(handle);
            }),
            Err(err) => tracing::warn!(?body, "table header cannot follow its rows: {err}"),
        }
    });

    let body = Children::new(move || {
        virtual_list(
            body_style(),
            1,
            2,
            move || count.get(),
            move |i| {
                view! {
                    box {
                        style: {
                            let mut row = row_style(selected.get() == i, focused.get(), 1);
                            row.width(Length::Cells(width.get()));
                            row
                        },
                        on_click: move |_| {
                            select(i);
                            frame_ref.focus();
                        },
                        each c, column in columns.get() key = (c, column.clone()) {
                            if c > 0 {
                                text(separator(&shape.get())) { style: border_text() }
                            },
                            text(fit(&text_at(i, c), column.width)),
                        },
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
            box {
                ref: header,
                style: header_style(&shape.get()),
                on_scroll: move |ev| {
                    // Scrolled on its own, by a wheel over it: take the body along.
                    if let (Some(doc), Some(body)) = (frame_ref.document(), list.node()) {
                        let _ = doc.scroll_to(body, ev.x, doc.scroll_offset(body).y);
                    }
                },
                box {
                    style: {
                        let mut row = Style::new();
                        row.width(Length::Cells(width.get()));
                        row.flex_shrink(0.0);
                        row
                    },
                    each c, column in columns.get() key = (c, column.clone()) {
                        if c > 0 {
                            text(separator(&shape.get())) { style: border_text() }
                        },
                        text(fit(&column.title, column.width)) { style: title() },
                    },
                },
            },
            { body }
        }
    }
}

const SEPARATOR_WIDTH: u16 = 3;

/// Every column's width plus the separators between them.
fn total_width(columns: &[Column]) -> u16 {
    let cells: u16 = columns.iter().fold(0, |sum, c| sum.saturating_add(c.width));
    let gaps = u16::try_from(columns.len().saturating_sub(1)).unwrap_or(u16::MAX);
    cells.saturating_add(gaps.saturating_mul(SEPARATOR_WIDTH))
}

/// `text` in exactly `width` columns: padded, or cut short with `…`.
fn fit(text: &str, width: u16) -> String {
    let width = usize::from(width);
    if text.width() <= width {
        return padded(text, width);
    }
    let Some(room) = width.checked_sub(1) else {
        return String::new();
    };
    let mut cut = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        cut.push(ch);
        used += w;
    }
    cut.push('…');
    padded(&cut, width)
}

fn separator(shape: &Shape) -> String {
    format!(" {} ", shape.border.left)
}

fn border_text() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--border"));
    style
}

fn title() -> Style {
    let mut style = Style::new();
    style.bold(true);
    style
}

/// The header scrolls only to follow the body, so it shows no bar of its own.
fn header_style(shape: &Shape) -> Style {
    let mut style = Style::new();
    style.flex_shrink(0.0);
    style.overflow_x(Overflow::Scroll);
    style.scrollbar_show(ScrollbarShow::Never);
    style.border(Border::new(shape.border).with_sides(Sides::new(false, false, true, false)));
    style.border_color(Color::var("--border"));
    style
}

fn body_style() -> Style {
    let mut style = list_style();
    style.overflow_x(Overflow::Scroll);
    style
}

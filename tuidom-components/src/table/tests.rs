use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::theme::{Theme, install_theme};

fn columns() -> Vec<Column> {
    vec![Column::new("Name", 8), Column::new("Size", 4)]
}

/// A 50-row table `width` cells wide and six tall, its first row's name read from `first`.
fn harness(
    width: u16,
    first: Signal<String>,
    selected: Signal<usize>,
    activated: Signal<Vec<usize>>,
) -> Harness {
    Harness::new(width, 6, move || {
        install_theme(Theme::dark());
        let mut style = Style::new();
        style.width(Length::Cells(width));
        style.height(Length::Cells(6));
        view! {
            box {
                test_id: "host",
                Table {
                    columns: columns(),
                    count: 50,
                    cell: move |(row, column): (usize, usize)| match (row, column) {
                        (0, 0) => first.get(),
                        (row, 0) => format!("file_{row}"),
                        (row, _) => format!("{}", row * 10),
                    },
                    selected: selected,
                    on_activate: move |i| activated.update(|a| a.push(i)),
                    style: style.clone(),
                },
            }
        }
    })
}

fn wide() -> Harness {
    harness(24, state("file_0".to_string()), state(0), state(Vec::new()))
}

fn table(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    h.document().get_children(host)[0]
}

/// The header box and the body's scroll container.
fn parts(h: &mut Harness) -> (NodeId, NodeId) {
    let table = table(h);
    let children = h.document().get_children(table);
    (children[0], children[1])
}

/// Body row `y` without the scrollbar in the last column.
fn body_row(h: &mut Harness, y: u16) -> String {
    let row = h.row(y);
    let count = row.chars().count();
    let row: String = row.chars().take(count - 1).collect();
    row.trim_end().to_string()
}

#[test]
fn a_header_and_a_line_sit_above_the_rows() {
    let mut h = wide();
    assert_eq!(h.row(0).trim_end(), "Name     │ Size");
    assert_eq!(h.row(1), "─".repeat(24));
    assert_eq!(body_row(&mut h, 2), "file_0   │ 0");
    assert_eq!(body_row(&mut h, 3), "file_1   │ 10");
}

#[test]
fn text_wider_than_its_column_is_cut_short() {
    let first = state("a_very_long_name".to_string());
    let mut h = harness(24, first, state(0), state(Vec::new()));
    assert_eq!(body_row(&mut h, 2), "a_very_… │ 0");
}

#[test]
fn a_cell_follows_the_signals_it_reads() {
    let first = state("before".to_string());
    let mut h = harness(24, first, state(0), state(Vec::new()));
    assert_eq!(body_row(&mut h, 2), "before   │ 0");
    first.set("after".to_string());
    assert_eq!(body_row(&mut h, 2), "after    │ 0");
}

#[test]
fn the_header_stays_while_the_selection_scrolls_the_rows() {
    let (selected, activated) = (state(0), state(Vec::new()));
    let mut h = harness(24, state("file_0".to_string()), selected, activated);
    let table = table(&mut h);
    h.focus(table);
    for _ in 0..5 {
        h.press(KeyCode::Down);
    }
    assert_eq!(selected.get(), 5);
    assert_eq!(h.row(0).trim_end(), "Name     │ Size");
    assert_eq!(body_row(&mut h, 2), "file_2   │ 20");
    assert_eq!(body_row(&mut h, 5), "file_5   │ 50");

    h.press(KeyCode::PageDown);
    assert_eq!(selected.get(), 9);
    h.press(KeyCode::Enter);
    assert_eq!(activated.get(), vec![9]);

    h.click_text("file_7");
    assert_eq!(selected.get(), 7);
}

#[test]
fn the_header_follows_the_rows_sideways_and_back() {
    let mut h = harness(10, state("file_0".to_string()), state(0), state(Vec::new()));
    let (header, body) = parts(&mut h);
    h.document().scroll_to(body, 5, 0).unwrap();
    assert_eq!(h.row(0), "    │ Size");
    assert!(h.row(2).starts_with("0   │ 0"));
    // The selected row's fill runs the whole width of the columns, not just the first screen.
    let fill = h.cell(0, 2).unwrap().bg;
    assert_eq!(h.cell(8, 2).unwrap().bg, fill);
    assert_ne!(h.cell(8, 3).unwrap().bg, fill);

    h.document().scroll_to(header, 2, 0).unwrap();
    assert_eq!(h.document().scroll_offset(body).x, 2);
    assert_eq!(h.row(0), "me     │ S");
}

#[test]
fn growing_in_a_bounded_column_stays_inside_it() {
    let mut h = Harness::new(24, 10, move || {
        install_theme(Theme::dark());
        let mut host = Style::new();
        host.flex_direction(tuidom::style::FlexDirection::Column);
        host.width(Length::Percent(100.0));
        host.height(Length::Cells(6));
        let mut grow = Style::new();
        grow.flex_grow(1.0);
        view! {
            box {
                style: host.clone(),
                Table {
                    columns: columns(),
                    count: 10_000,
                    cell: move |(row, _): (usize, usize)| format!("file_{row}"),
                    selected: state(0),
                    style: grow.clone(),
                },
            }
        }
    });
    assert!(h.row(5).contains("file_3"), "{:?}", h.row(5));
    assert_eq!(h.row(6), "");
}

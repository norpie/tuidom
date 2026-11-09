use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{Length, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, text, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn sized(height: u16) -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(12));
    style.height(Length::Cells(height));
    style
}

/// A focusable box above a five-row list of `count` items, recording activations.
fn harness(count: usize, selected: Signal<usize>, activated: Signal<Vec<usize>>) -> Harness {
    Harness::new(12, 6, move || {
        install_theme(Theme::dark());
        view! {
            box {
                style: column(),
                box { test_id: "above", focusable: true, text("above") },
                List {
                    count: count,
                    selected: selected,
                    render: move |i| text(format!("item {i}")).into(),
                    on_activate: move |i| activated.update(|a| a.push(i)),
                    style: sized(5),
                },
            }
        }
    })
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(tuidom::style::FlexDirection::Column);
    style
}

/// The list's focusable frame, below the box above it.
fn list(h: &mut Harness) -> NodeId {
    let above = h.find_attr("test_id", "above").unwrap();
    let doc = h.document();
    let column = doc.get_node(above).unwrap().parent.unwrap();
    doc.get_node(column).unwrap().children[1]
}

fn focus_list(h: &mut Harness) {
    let list = list(h);
    h.focus(list);
}

/// The row box drawing `label`.
fn row(h: &mut Harness, label: &str) -> NodeId {
    let (x, y) = h.find_text(label).unwrap();
    let doc = h.document();
    let text = doc.node_at(x, y).unwrap();
    doc.get_node(text).unwrap().parent.unwrap()
}

/// Row `y` without the scrollbar in the last column.
fn text_at(h: &mut Harness, y: u16) -> String {
    let row: String = h.row(y).chars().take(11).collect();
    row.trim_end().to_string()
}

fn fill(h: &mut Harness, label: &str) -> Option<ResolvedColor> {
    let row = row(h, label);
    h.document().resolved_style(row).unwrap().background
}

#[test]
fn only_the_rows_in_view_exist() {
    let mut h = harness(10_000, state(0), state(Vec::new()));
    assert_eq!(text_at(&mut h, 1), "item 0");
    assert_eq!(text_at(&mut h, 5), "item 4");
    assert!(h.find_text("item 40").is_none());
    let list = list(&mut h);
    let doc = h.document();
    let rows = doc.get_children(doc.get_children(doc.get_children(list)[0])[0]);
    assert!(rows.len() < 20, "{} children", rows.len());
}

#[test]
fn the_selected_row_is_in_the_accent_while_focused_and_muted_otherwise() {
    let mut h = harness(20, state(1), state(Vec::new()));
    let dark = Palette::dark();
    assert_eq!(fill(&mut h, "item 1"), Some(literal(&dark.muted)));
    assert_eq!(fill(&mut h, "item 0"), None);

    assert_eq!(
        text_color(&mut h, "item 1"),
        literal(&dark.muted_foreground)
    );

    focus_list(&mut h);
    assert_eq!(fill(&mut h, "item 1"), Some(literal(&dark.accent)));
    assert_eq!(
        text_color(&mut h, "item 1"),
        literal(&dark.accent_foreground)
    );
}

fn text_color(h: &mut Harness, label: &str) -> ResolvedColor {
    let (x, y) = h.find_text(label).unwrap();
    let doc = h.document();
    let text = doc.node_at(x, y).unwrap();
    doc.resolved_style(text).unwrap().color
}

#[test]
fn moving_the_selection_off_screen_scrolls_just_far_enough() {
    let selected = state(0);
    let mut h = harness(20, selected, state(Vec::new()));
    focus_list(&mut h);
    for _ in 0..6 {
        h.press(KeyCode::Down);
    }
    assert_eq!(selected.get(), 6);
    assert_eq!(text_at(&mut h, 1), "item 2");
    assert_eq!(text_at(&mut h, 5), "item 6");

    h.press(KeyCode::PageDown);
    assert_eq!(selected.get(), 11);
    assert_eq!(text_at(&mut h, 5), "item 11");

    h.press(KeyCode::End);
    assert_eq!(selected.get(), 19);
    assert_eq!(text_at(&mut h, 5), "item 19");

    h.press(KeyCode::PageUp);
    assert_eq!(selected.get(), 14);
    assert_eq!(text_at(&mut h, 1), "item 14");

    h.press(KeyCode::Home);
    assert_eq!(selected.get(), 0);
    assert_eq!(text_at(&mut h, 1), "item 0");
}

#[test]
fn up_on_the_first_row_moves_focus_on() {
    let selected = state(0);
    let mut h = harness(20, selected, state(Vec::new()));
    focus_list(&mut h);
    h.press(KeyCode::Up);
    assert_eq!(selected.get(), 0);
    let above = h.find_attr("test_id", "above").unwrap();
    assert_eq!(h.document().focused(), Some(above));
}

#[test]
fn enter_activates_the_selection_and_a_click_selects_and_focuses() {
    let (selected, activated) = (state(0), state(Vec::new()));
    let mut h = harness(20, selected, activated);
    h.click_text("item 3");
    assert_eq!(selected.get(), 3);
    assert_eq!(h.document().focused(), Some(list(&mut h)));
    assert!(activated.get().is_empty());
    h.press(KeyCode::Enter);
    assert_eq!(activated.get(), vec![3]);
}

#[test]
fn taller_rows_page_by_how_many_fit() {
    let selected = state(0);
    let mut h = Harness::new(12, 6, move || {
        install_theme(Theme::dark());
        view! {
            box {
                test_id: "host",
                List {
                    count: 20,
                    selected: selected,
                    render: move |i| text(format!("item {i}")).into(),
                    row_height: 2,
                    style: sized(6),
                },
            }
        }
    });
    assert_eq!(text_at(&mut h, 2), "item 1");
    let host = h.find_attr("test_id", "host").unwrap();
    let list = h.document().get_children(host)[0];
    h.focus(list);
    h.press(KeyCode::PageDown);
    assert_eq!(selected.get(), 3);
}

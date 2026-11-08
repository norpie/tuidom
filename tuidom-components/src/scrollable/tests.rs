use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

/// A three-row area over six lines, the second of them focusable, with a focusable box below.
fn harness() -> Harness {
    Harness::new(10, 4, || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.width(Length::Percent(100.0));
        let mut sized = Style::new();
        sized.height(Length::Cells(3));
        sized.flex_shrink(0.0);
        view! {
            box {
                style: column.clone(),
                Scrollable {
                    style: sized.clone(),
                    text("line 0"),
                    box { test_id: "inner", focusable: true, text("line 1") },
                    text("line 2"),
                    text("line 3"),
                    text("line 4"),
                    text("line 5"),
                },
                box { test_id: "below", focusable: true, text("below") },
            }
        }
    })
}

fn area(h: &mut Harness) -> NodeId {
    let doc = h.document();
    let column = doc.get_node(doc.root()).unwrap().children[0];
    doc.get_node(column).unwrap().children[0]
}

#[test]
fn arrows_scroll_the_focused_area_then_move_focus_at_the_end() {
    let mut h = harness();
    let area = area(&mut h);
    h.focus(area);
    h.press(KeyCode::Down);
    assert!(h.row(0).starts_with("line 1"), "{:?}", h.row(0));
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    assert!(h.row(0).starts_with("line 3"), "{:?}", h.row(0));
    assert_eq!(h.document().focused(), Some(area));

    h.press(KeyCode::Down);
    assert!(h.row(0).starts_with("line 3"), "{:?}", h.row(0));
    assert_eq!(h.document().focused(), h.find_attr("test_id", "below"));
}

#[test]
fn a_focused_node_inside_keeps_its_arrows() {
    let mut h = harness();
    let inner = h.find_attr("test_id", "inner").unwrap();
    h.focus(inner);
    h.press(KeyCode::Down);
    assert!(h.row(0).starts_with("line 0"), "{:?}", h.row(0));
    assert_eq!(h.document().focused(), h.find_attr("test_id", "below"));
}

#[test]
fn page_keys_scroll_it_through_the_engine() {
    let mut h = harness();
    let area = area(&mut h);
    h.focus(area);
    h.press(KeyCode::End);
    assert!(h.row(0).starts_with("line 3"), "{:?}", h.row(0));
    h.press(KeyCode::Home);
    assert!(h.row(0).starts_with("line 0"), "{:?}", h.row(0));
}

#[test]
fn focus_rings_the_thumb() {
    let mut h = harness();
    let area = area(&mut h);
    let thumb = |h: &Harness| {
        h.document()
            .resolved_style(area)
            .unwrap()
            .scrollbar_thumb_color
    };
    let dark = Palette::dark();
    assert_eq!(thumb(&h), Some(literal(&dark.muted_foreground)));
    h.focus(area);
    assert_eq!(thumb(&h), Some(literal(&dark.ring)));
}

#[test]
fn a_horizontal_area_takes_left_and_right() {
    let mut h = Harness::new(6, 2, || {
        install_theme(Theme::dark());
        let mut sized = Style::new();
        sized.width(Length::Cells(6));
        view! {
            box {
                Scrollable { axis: ScrollAxis::Horizontal, style: sized.clone(), text("abcdefghij") },
            }
        }
    });
    let doc = h.document();
    let area = doc
        .get_node(doc.get_node(doc.root()).unwrap().children[0])
        .unwrap()
        .children[0];
    h.focus(area);
    h.press(KeyCode::Right);
    h.press(KeyCode::Right);
    assert!(h.row(0).starts_with("cdef"), "{:?}", h.row(0));
    h.press(KeyCode::Down);
    assert!(h.row(0).starts_with("cdef"));
}

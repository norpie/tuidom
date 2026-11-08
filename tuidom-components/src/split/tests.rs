use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

/// An 11-cell split of two lettered panes, wide or tall, with a focusable box after it.
fn harness(ratio: Signal<f64>, stacked: bool) -> Harness {
    let (width, height) = if stacked { (6, 12) } else { (12, 3) };
    Harness::new(width, height, move || {
        install_theme(Theme::dark());
        let mut outer = Style::new();
        outer.width(Length::Percent(100.0));
        outer.height(Length::Percent(100.0));
        if stacked {
            outer.flex_direction(FlexDirection::Column);
        }
        let mut sized = Style::new();
        if stacked {
            sized.height(Length::Cells(11));
        } else {
            sized.width(Length::Cells(11));
        }
        view! {
            box {
                style: outer.clone(),
                Split {
                    ratio: ratio,
                    stacked: stacked,
                    style: sized.clone(),
                    first: view! { text("aaaaaaaaaaa") },
                    second: view! { text("bbbbbbbbbbb") },
                },
                box { test_id: "after", focusable: true, text("x") },
            }
        }
    })
}

fn divider(h: &mut Harness) -> NodeId {
    let doc = h.document();
    let outer = doc.get_node(doc.root()).unwrap().children[0];
    let split = doc.get_node(outer).unwrap().children[0];
    doc.get_node(split).unwrap().children[1]
}

#[test]
fn the_panes_divide_the_space_by_the_ratio() {
    let ratio = state(0.5);
    let mut h = harness(ratio, false);
    assert_eq!(h.row(0), "aaaaa│bbbbbx");
    ratio.set(0.2);
    assert_eq!(h.row(0), "aa│bbbbbbbbx");
}

#[test]
fn dragging_the_divider_moves_it_to_the_pointer() {
    let ratio = state(0.5);
    let mut h = harness(ratio, false);
    h.drag((5, 1), &[(3, 1), (8, 0)]);
    assert_eq!(ratio.get(), 0.8);
    assert_eq!(h.row(0), "aaaaaaaa│bbx");

    h.drag((8, 1), &[(-4, 1)]);
    assert_eq!(ratio.get(), 0.0);
}

#[test]
fn a_stacked_split_divides_rows_and_drags_vertically() {
    let ratio = state(0.5);
    let mut h = harness(ratio, true);
    assert_eq!(h.row(5).trim_end(), "──────");
    h.drag((0, 5), &[(0, 2)]);
    assert_eq!(ratio.get(), 0.2);
    assert_eq!(h.row(2).trim_end(), "──────");
}

#[test]
fn arrows_step_the_focused_divider_then_move_focus_at_an_end() {
    let ratio = state(0.9);
    let mut h = harness(ratio, false);
    let divider = divider(&mut h);
    h.focus(divider);
    h.press(KeyCode::Left);
    assert!((ratio.get() - 0.85).abs() < 1e-9, "{}", ratio.get());
    h.press(KeyCode::Right);
    h.press(KeyCode::Right);
    h.press(KeyCode::Right);
    assert_eq!(ratio.get(), 1.0);
    assert_eq!(h.document().focused(), Some(divider));

    h.press(KeyCode::Right);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "after"));
}

#[test]
fn focus_rings_the_divider() {
    let mut h = harness(state(0.5), false);
    let divider = divider(&mut h);
    let color = |h: &Harness| h.document().resolved_style(divider).unwrap().border_color;
    let dark = Palette::dark();
    assert_eq!(color(&h), Some(literal(&dark.border)));
    h.focus(divider);
    assert_eq!(color(&h), Some(literal(&dark.ring)));
}

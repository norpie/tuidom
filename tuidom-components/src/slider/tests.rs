use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

/// An 11-cell slider over 0 to 100 in steps of `step`, with focusable boxes beside and below it.
fn harness(value: Signal<f64>, step: f64, disabled: bool) -> Harness {
    Harness::new(12, 2, move || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.width(Length::Percent(100.0));
        column.flex_direction(FlexDirection::Column);
        let mut host = Style::new();
        host.width(Length::Percent(100.0));
        view! {
            box {
                style: column.clone(),
                box {
                    test_id: "host",
                    style: host.clone(),
                    Slider { value: value, step: step, disabled: disabled },
                    box { test_id: "beside", focusable: true, text("x") },
                },
                box { test_id: "below", focusable: true, text("below") },
            }
        }
    })
}

/// The slider's track and its three parts: filled segment, thumb, rest.
fn nodes(h: &mut Harness) -> (NodeId, [NodeId; 3]) {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let track = doc.get_node(host).unwrap().children[0];
    let parts = doc.get_node(track).unwrap().children;
    (track, [parts[0], parts[1], parts[2]])
}

#[test]
fn the_thumb_sits_at_the_values_fraction() {
    let value = state(50.0);
    let mut h = harness(value, 1.0, false);
    assert_eq!(h.row(0), "━━━━━●─────x");
    value.set(0.0);
    assert_eq!(h.row(0), "●──────────x");
    value.set(100.0);
    assert_eq!(h.row(0), "━━━━━━━━━━●x");
}

#[test]
fn left_and_right_step_while_up_and_down_still_navigate() {
    let value = state(50.0);
    let mut h = harness(value, 10.0, false);
    let (track, _) = nodes(&mut h);
    h.focus(track);
    h.press(KeyCode::Right);
    assert_eq!(value.get(), 60.0);
    h.press(KeyCode::Left);
    h.press(KeyCode::Left);
    assert_eq!(value.get(), 40.0);
    assert_eq!(h.document().focused(), Some(track));

    h.press(KeyCode::End);
    h.press(KeyCode::Right);
    assert_eq!(value.get(), 100.0);
    h.press(KeyCode::Home);
    h.press(KeyCode::Left);
    assert_eq!(value.get(), 0.0);

    h.press(KeyCode::Down);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "below"));
}

#[test]
fn a_click_jumps_to_that_point_on_the_step() {
    let value = state(0.0);
    let mut h = harness(value, 1.0, false);
    h.click(3, 0);
    assert_eq!(value.get(), 30.0);

    let value = state(0.0);
    let mut h = harness(value, 25.0, false);
    h.click(4, 0);
    assert_eq!(value.get(), 50.0);
}

#[test]
fn dragging_follows_the_pointer_and_selects_no_text() {
    let value = state(50.0);
    let mut h = harness(value, 1.0, false);
    h.drag((5, 0), &[(7, 0), (8, 0)]);
    assert_eq!(value.get(), 80.0);
    h.drag((8, 0), &[(4, 0), (-3, 0)]);
    assert_eq!(value.get(), 0.0);
    assert_eq!(h.document().selection(), None);
}

#[test]
fn the_parts_take_their_colors_and_a_ring_marks_the_thumb() {
    let mut h = harness(state(50.0), 1.0, false);
    let (track, [filled, thumb, rest]) = nodes(&mut h);
    let doc = h.document().clone();
    let dark = Palette::dark();
    let border = |node| doc.resolved_style(node).unwrap().border_color;
    assert_eq!(border(filled), Some(literal(&dark.primary)));
    assert_eq!(border(rest), Some(literal(&dark.muted)));
    assert_eq!(
        doc.resolved_style(thumb).unwrap().color,
        literal(&dark.primary)
    );

    h.focus(track);
    assert_eq!(
        doc.resolved_style(thumb).unwrap().color,
        literal(&dark.ring)
    );
}

#[test]
fn a_disabled_slider_cannot_change() {
    let value = state(50.0);
    let mut h = harness(value, 1.0, true);
    h.click(9, 0);
    let (track, [filled, ..]) = nodes(&mut h);
    assert!(h.document().focus(track).is_err() || h.document().focused() != Some(track));
    h.press(KeyCode::Right);
    assert_eq!(value.get(), 50.0);
    assert_eq!(
        h.document().resolved_style(filled).unwrap().border_color,
        Some(literal(&Palette::dark().muted_foreground))
    );
}

#[test]
fn snapping_counts_steps_from_min_and_stays_in_range() {
    let range = Range {
        min: 5.0,
        max: 20.0,
        step: 10.0,
    };
    assert_eq!(range.snap(11.0), 15.0);
    assert_eq!(range.snap(-40.0), 5.0);
    assert_eq!(range.snap(26.0), 20.0);
    assert_eq!(range.fraction(12.5), 0.5);

    let empty = Range {
        min: 3.0,
        max: 3.0,
        step: 1.0,
    };
    assert_eq!(empty.fraction(3.0), 0.0);
    assert_eq!(empty.snap(9.0), 3.0);
}

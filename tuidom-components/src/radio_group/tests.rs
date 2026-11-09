use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, FlexDirection, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn sizes() -> Vec<String> {
    ["Small", "Medium", "Large"].map(String::from).to_vec()
}

/// The group above a focusable box, so focus has somewhere to go past the last option.
fn harness(selected: Signal<usize>, disabled: bool) -> Harness {
    Harness::new(12, 4, move || {
        install_theme(Theme::dark());
        let mut host = Style::new();
        host.flex_direction(FlexDirection::Column);
        host.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: host.clone(),
                RadioGroup { options: sizes(), selected: selected, disabled: disabled },
                box { test_id: "below", focusable: true, text("next") },
            }
        }
    })
}

fn group(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    h.document().get_node(host).unwrap().children[0]
}

/// The text nodes of one option's row: open paren, mark, close paren, space, label.
fn row_parts(h: &mut Harness, index: usize) -> Vec<NodeId> {
    let group = group(h);
    let doc = h.document();
    let rows = doc.get_node(group).unwrap().children;
    doc.get_node(rows[index]).unwrap().children
}

#[test]
fn it_marks_the_selected_option() {
    let selected = state(1);
    let mut h = harness(selected, false);
    assert_eq!(h.row(0).trim_end(), "( ) Small");
    assert_eq!(h.row(1).trim_end(), "(•) Medium");
    assert_eq!(h.row(2).trim_end(), "( ) Large");

    selected.set(5);
    assert!(!h.screen().contains('•'));
}

#[test]
fn clicking_an_option_selects_it() {
    let selected = state(0);
    let mut h = harness(selected, false);
    h.click_text("Large");
    assert_eq!(selected.get(), 2);
}

#[test]
fn up_and_down_move_the_selection_then_leave_the_group_at_its_ends() {
    let selected = state(0);
    let mut h = harness(selected, false);
    let group = group(&mut h);
    h.focus(group);
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().focused(), Some(group));

    h.press(KeyCode::Down);
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "below"));

    h.focus(group);
    h.press(KeyCode::Up);
    assert_eq!(selected.get(), 1);
}

#[test]
fn the_focus_mark_is_drawn_on_the_selected_row() {
    let mut h = harness(state(1), false);
    let group = group(&mut h);
    h.focus(group);
    let ring = literal(&Palette::dark().ring);
    let input = literal(&Palette::dark().input);
    let bracket = |h: &mut Harness, index| {
        let open = row_parts(h, index)[0];
        h.document().resolved_style(open).unwrap().color
    };
    assert_eq!(bracket(&mut h, 1), ring);
    assert_eq!(bracket(&mut h, 0), input);
}

#[test]
fn a_disabled_group_cannot_change() {
    let selected = state(0);
    let mut h = harness(selected, true);
    h.click_text("Large");
    assert_eq!(selected.get(), 0);
    let group = group(&mut h);
    assert!(h.document().focus(group).is_err() || h.document().focused() != Some(group));

    let label = row_parts(&mut h, 2)[4];
    assert_eq!(
        h.document().resolved_style(label).unwrap().color,
        literal(&Palette::dark().muted_foreground)
    );
}

#[test]
fn a_changed_option_redraws() {
    let options = state(sizes());
    let mut h = Harness::new(12, 3, move || {
        install_theme(Theme::dark());
        view! { box { RadioGroup { options: options.get(), selected: state(0) } } }
    });
    options.update(|o| o[0] = "Tiny".to_string());
    assert_eq!(h.row(0).trim_end(), "(•) Tiny");
}

#[test]
fn a_click_that_wobbles_still_selects_the_option_and_no_text() {
    let selected = state(0);
    let mut h = harness(selected, false);
    crate::test_support::wobble(&mut h, "Large");
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().selection(), None);
}

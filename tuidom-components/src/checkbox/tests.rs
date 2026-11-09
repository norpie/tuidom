use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, ResolvedStyle, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{FocusMark, Palette, Shape, Theme, install_theme};

fn harness(theme: Theme, checked: Signal<bool>, disabled: bool) -> Harness {
    Harness::new(16, 2, move || {
        install_theme(theme.clone());
        let mut host = Style::new();
        host.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: host.clone(),
                Checkbox { checked: checked, label: "Remember".to_string(), disabled: disabled },
            }
        }
    })
}

/// The checkbox's row and its four text nodes: open bracket, mark, close bracket, label.
fn nodes(h: &mut Harness) -> (NodeId, [NodeId; 4]) {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let row = doc.get_node(host).unwrap().children[0];
    let children = doc.get_node(row).unwrap().children;
    // The label sits in a dynamic block: a space, the label, then the block's marker.
    (row, [children[0], children[1], children[2], children[4]])
}

fn resolved(h: &Harness, node: NodeId) -> ResolvedStyle {
    h.document().resolved_style(node).unwrap()
}

fn with_focus(focus: FocusMark) -> Theme {
    Theme {
        shape: Shape {
            focus,
            ..Shape::default()
        },
        ..Theme::dark()
    }
}

#[test]
fn it_draws_its_state_and_label() {
    let checked = state(false);
    let mut h = harness(Theme::dark(), checked, false);
    assert_eq!(h.row(0).trim_end(), "[ ] Remember");
    checked.set(true);
    assert_eq!(h.row(0).trim_end(), "[✓] Remember");
}

#[test]
fn a_click_enter_or_space_toggles_it() {
    let checked = state(false);
    let mut h = harness(Theme::dark(), checked, false);
    h.click_text("Remember");
    assert!(checked.get());

    let (row, _) = nodes(&mut h);
    h.focus(row);
    h.press(KeyCode::Enter);
    assert!(!checked.get());
    h.press(KeyCode::Char(' '));
    assert!(checked.get());
    h.press(KeyCode::Char('x'));
    assert!(checked.get());
}

#[test]
fn its_parts_take_their_colors() {
    let mut h = harness(Theme::dark(), state(true), false);
    let (_, [open, mark, close, label]) = nodes(&mut h);
    let dark = Palette::dark();
    assert_eq!(resolved(&h, open).color, literal(&dark.input));
    assert_eq!(resolved(&h, close).color, literal(&dark.input));
    assert_eq!(resolved(&h, mark).color, literal(&dark.primary));
    assert_eq!(resolved(&h, label).color, literal(&dark.foreground));
}

#[test]
fn a_ring_recolors_the_brackets_while_focused() {
    let mut h = harness(Theme::dark(), state(false), false);
    let (row, [open, _, close, _]) = nodes(&mut h);
    h.focus(row);
    let ring = literal(&Palette::dark().ring);
    assert_eq!(resolved(&h, open).color, ring);
    assert_eq!(resolved(&h, close).color, ring);
}

#[test]
fn an_underline_mark_underlines_the_label_and_a_reverse_mark_reverses_the_row() {
    let mut h = harness(with_focus(FocusMark::Underline), state(false), false);
    let (row, [open, _, _, label]) = nodes(&mut h);
    h.focus(row);
    assert!(resolved(&h, label).underline);
    assert!(!resolved(&h, open).underline);

    let mut h = harness(with_focus(FocusMark::Reverse), state(false), false);
    let (row, [_, mark, _, label]) = nodes(&mut h);
    h.focus(row);
    let dark = Palette::dark();
    assert_eq!(
        resolved(&h, row).background,
        Some(literal(&dark.foreground))
    );
    assert_eq!(resolved(&h, mark).color, literal(&dark.background));
    assert_eq!(resolved(&h, label).color, literal(&dark.background));
}

#[test]
fn a_disabled_checkbox_cannot_be_toggled_or_focused() {
    let checked = state(false);
    let mut h = harness(Theme::dark(), checked, true);
    h.click_text("Remember");
    h.press(KeyCode::Char(' '));
    assert!(!checked.get());
    assert_eq!(h.document().focused(), None);

    let (_, [open, mark, _, label]) = nodes(&mut h);
    let dark = Palette::dark();
    assert_eq!(resolved(&h, open).color, literal(&dark.border));
    assert_eq!(resolved(&h, mark).color, literal(&dark.muted_foreground));
    assert_eq!(resolved(&h, label).color, literal(&dark.muted_foreground));
}

#[test]
fn without_a_label_it_is_just_the_box() {
    let mut h = Harness::new(8, 1, || {
        install_theme(Theme::dark());
        view! { box { Checkbox { checked: state(true) } } }
    });
    assert_eq!(h.row(0), "[✓]");
}

#[test]
fn a_click_that_wobbles_still_toggles_and_selects_nothing() {
    let checked = state(false);
    let mut h = harness(Theme::dark(), checked, false);
    crate::test_support::wobble(&mut h, "Remember");
    assert!(checked.get());
    assert_eq!(h.document().selection(), None);
}

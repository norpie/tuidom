use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{FlexDirection, ResolvedStyle, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn names() -> Vec<String> {
    ["One", "Two", "Three"].map(String::from).to_vec()
}

/// Tabs whose pane names the active tab, with focusable boxes beside and below.
fn harness(selected: Signal<usize>) -> Harness {
    harness_with(state(names()), selected)
}

fn harness_with(tabs: Signal<Vec<String>>, selected: Signal<usize>) -> Harness {
    Harness::new(30, 3, move || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        view! {
            box {
                style: column.clone(),
                box {
                    Tabs {
                        tabs: tabs.get(),
                        selected: selected,
                        match selected.get() {
                            0 => text("pane one"),
                            1 => text("pane two"),
                            _ => text("pane three"),
                        }
                    },
                    box { test_id: "beside", focusable: true, text("|") },
                },
                box { test_id: "below", focusable: true, text("below") },
            }
        }
    })
}

/// The strip and the box of each tab in it.
fn strip(h: &mut Harness) -> (NodeId, Vec<NodeId>) {
    let doc = h.document();
    let mut node = doc.root();
    // root → column → row → Tabs column → strip
    for _ in 0..4 {
        node = doc.get_node(node).unwrap().children[0];
    }
    let tabs = doc.get_node(node).unwrap().children;
    (node, tabs)
}

fn resolved(h: &Harness, node: NodeId) -> ResolvedStyle {
    h.document().resolved_style(node).unwrap()
}

fn label(h: &Harness, tab: NodeId) -> ResolvedStyle {
    let text = h.document().get_node(tab).unwrap().children[0];
    resolved(h, text)
}

#[test]
fn the_strip_shows_every_tab_and_the_pane_follows_the_selection() {
    let selected = state(0);
    let mut h = harness(selected);
    assert_eq!(h.row(0).trim_end(), "  One    Two    Three  |");
    assert_eq!(h.row(1).trim_end(), "pane one");
    selected.set(2);
    assert_eq!(h.row(1).trim_end(), "pane three");
}

#[test]
fn the_active_tab_takes_the_background_and_the_rest_are_muted() {
    let mut h = harness(state(1));
    let (strip, tabs) = strip(&mut h);
    let dark = Palette::dark();
    assert_eq!(resolved(&h, strip).background, Some(literal(&dark.muted)));
    assert_eq!(
        resolved(&h, tabs[1]).background,
        Some(literal(&dark.background))
    );
    assert_eq!(resolved(&h, tabs[0]).background, None);
    assert_eq!(label(&h, tabs[1]).color, literal(&dark.foreground));
    assert_eq!(label(&h, tabs[0]).color, literal(&dark.muted_foreground));
}

#[test]
fn tab_reaches_the_active_tab_and_left_and_right_move_focus_and_selection_together() {
    let selected = state(0);
    let mut h = harness(selected);
    let (_, tabs) = strip(&mut h);
    h.press(KeyCode::Tab);
    assert_eq!(h.document().focused(), Some(tabs[0]));
    h.press(KeyCode::Right);
    h.press(KeyCode::Right);
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().focused(), Some(tabs[2]));

    h.press(KeyCode::Right);
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "beside"));

    h.press(KeyCode::BackTab);
    assert_eq!(h.document().focused(), Some(tabs[2]));
    h.press(KeyCode::Left);
    assert_eq!(selected.get(), 1);
    h.press(KeyCode::Down);
    assert_eq!(h.document().focused(), h.find_attr("test_id", "below"));
}
#[test]
fn a_renamed_tab_redraws() {
    let tabs = state(names());
    let mut h = harness_with(tabs, state(0));
    tabs.update(|t| t[1] = "Deux".to_string());
    assert_eq!(h.row(0).trim_end(), "  One    Deux    Three  |");
}

#[test]
fn clicking_a_tab_activates_it() {
    let selected = state(0);
    let mut h = harness(selected);
    h.click_text("Three");
    assert_eq!(selected.get(), 2);
    assert_eq!(h.row(1).trim_end(), "pane three");
}

#[test]
fn hovering_a_tab_focuses_and_rings_it_and_enter_activates_it() {
    let selected = state(0);
    let mut h = harness(selected);
    let (_, tabs) = strip(&mut h);
    let (x, y) = h.find_text("Two").unwrap();
    h.runtime().simulate_mouse_move(x, y);
    assert_eq!(h.document().focused(), Some(tabs[1]));
    assert_eq!(label(&h, tabs[1]).color, literal(&Palette::dark().ring));
    assert_eq!(
        label(&h, tabs[0]).color,
        literal(&Palette::dark().foreground)
    );
    assert_eq!(selected.get(), 0);

    h.press(KeyCode::Enter);
    assert_eq!(selected.get(), 1);
}
#[test]
fn a_click_that_wobbles_still_activates_the_tab_and_selects_nothing() {
    let selected = state(0);
    let mut h = harness(selected);
    crate::test_support::wobble(&mut h, "Three");
    assert_eq!(selected.get(), 2);
    assert_eq!(h.document().selection(), None);
}

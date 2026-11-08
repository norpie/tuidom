use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Color, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, ThemeScope, ThemeScopeProps, install_theme};

fn sizes() -> Vec<String> {
    ["Small", "Medium", "Large"].map(String::from).to_vec()
}

/// The select at its own width, with a clickable label beside it.
fn harness(selected: Signal<usize>, clicks: Signal<u32>) -> Harness {
    Harness::new(20, 8, move || {
        install_theme(Theme::dark());
        let mut host = Style::new();
        host.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: host.clone(),
                Select { options: sizes(), selected: selected },
                box {
                    test_id: "beside",
                    on_click: move || clicks.update(|n| *n += 1),
                    text("beside"),
                },
            }
        }
    })
}

fn trigger(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    h.document().get_node(host).unwrap().children[0]
}

/// The open list: the last child of the root, where the portal put it.
fn list(h: &mut Harness) -> Option<NodeId> {
    let doc = h.document();
    let last = *doc.get_node(doc.root()).unwrap().children.last().unwrap();
    let style = doc.resolved_style(last).unwrap();
    style.stacking_context.then_some(last)
}

fn open(h: &mut Harness) {
    let trigger = trigger(h);
    h.focus(trigger);
    h.press(KeyCode::Enter);
}

#[test]
fn the_field_shows_the_selection_at_the_widest_options_width() {
    let selected = state(0);
    let mut h = harness(selected, state(0));
    assert_eq!(h.row(0).trim_end(), "╭──────────╮beside");
    assert_eq!(h.row(1).trim_end(), "│ Small  ▾ │");
    assert_eq!(h.row(2).trim_end(), "╰──────────╯");

    selected.set(1);
    assert_eq!(h.row(1).trim_end(), "│ Medium ▾ │");
}

#[test]
fn opening_draws_the_list_below_the_field_over_what_is_there() {
    let mut h = harness(state(1), state(0));
    open(&mut h);
    assert!(list(&mut h).is_some());
    assert_eq!(h.row(3).trim_end(), "╭──────────╮");
    assert_eq!(h.row(4).trim_end(), "│ Small    │");
    assert_eq!(h.row(5).trim_end(), "│ Medium ✓ │");
    assert_eq!(h.row(6).trim_end(), "│ Large    │");
    assert_eq!(h.row(7).trim_end(), "╰──────────╯");
}

#[test]
fn a_click_opens_it_too() {
    let mut h = harness(state(0), state(0));
    h.click_text("Small");
    assert!(list(&mut h).is_some());
}

#[test]
fn up_down_and_enter_pick_an_option_and_give_focus_back() {
    let selected = state(0);
    let mut h = harness(selected, state(0));
    open(&mut h);
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    h.press(KeyCode::Up);
    assert_eq!(selected.get(), 0);
    h.press(KeyCode::Enter);
    assert_eq!(selected.get(), 1);
    assert!(list(&mut h).is_none());
    assert_eq!(h.document().focused(), Some(trigger(&mut h)));
}

#[test]
fn the_highlight_starts_on_the_selection_in_the_accent() {
    let mut h = harness(state(2), state(0));
    open(&mut h);
    let list = list(&mut h).unwrap();
    let doc = h.document();
    let rows = doc.get_node(list).unwrap().children;
    let background = |row| doc.resolved_style(row).unwrap().background;
    let dark = Palette::dark();
    assert_eq!(background(rows[2]), Some(literal(&dark.accent)));
    assert_eq!(background(rows[0]), None);
}

#[test]
fn escape_closes_it_without_changing_the_selection() {
    let selected = state(0);
    let mut h = harness(selected, state(0));
    open(&mut h);
    h.press(KeyCode::Down);
    h.press(KeyCode::Esc);
    assert!(list(&mut h).is_none());
    assert_eq!(selected.get(), 0);
}

#[test]
fn clicking_an_option_picks_it() {
    let selected = state(0);
    let mut h = harness(selected, state(0));
    open(&mut h);
    h.click_text("Large");
    assert_eq!(selected.get(), 2);
    assert!(list(&mut h).is_none());
}

#[test]
fn a_press_outside_closes_it_and_reaches_nothing_else() {
    let (selected, clicks) = (state(0), state(0));
    let mut h = harness(selected, clicks);
    open(&mut h);
    h.press(KeyCode::Down);
    h.click(19, 7);
    assert!(list(&mut h).is_none());
    assert_eq!(selected.get(), 0);

    // On the field itself, the press closes the list rather than reopening it on release.
    open(&mut h);
    h.click_text("Small");
    assert!(list(&mut h).is_none());

    // Beside the field, the press closes the list without clicking the label underneath.
    open(&mut h);
    h.click_text("beside");
    assert!(list(&mut h).is_none());
    assert_eq!(clicks.get(), 0);
    h.click_text("beside");
    assert_eq!(clicks.get(), 1);
}

#[test]
fn a_disabled_select_stays_closed() {
    let mut h = Harness::new(16, 6, || {
        install_theme(Theme::dark());
        view! { box { Select { options: sizes(), selected: state(0), disabled: true } } }
    });
    h.click_text("Small");
    assert!(list(&mut h).is_none());
}

#[test]
fn the_list_takes_the_theme_the_field_is_in() {
    let mut light = Palette::light();
    light.popover = Color::red();
    let theme = Theme {
        palette: light,
        ..Theme::light()
    };
    let mut h = Harness::new(16, 8, move || {
        install_theme(Theme::dark());
        view! {
            box {
                ThemeScope {
                    theme: theme.clone(),
                    box { test_id: "host", Select { options: sizes(), selected: state(0) } },
                },
            }
        }
    });
    open(&mut h);
    let list = list(&mut h).unwrap();
    assert_eq!(
        h.document().resolved_style(list).unwrap().background,
        Some(ResolvedColor::red())
    );
}

#[test]
fn the_list_covers_content_that_comes_after_the_field() {
    let mut h = Harness::new(16, 8, || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(tuidom::style::FlexDirection::Column);
        column.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: column.clone(),
                Select { options: sizes(), selected: state(0) },
                text("xxxxxxxxxxxxxxxx"),
            }
        }
    });
    open(&mut h);
    assert_eq!(h.row(3), "╭──────────╮xxxx");
}

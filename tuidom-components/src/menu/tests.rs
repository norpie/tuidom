use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn items() -> Vec<MenuItem> {
    vec![
        MenuItem::new("Open").hint("o"),
        MenuItem::new("Export").hint("e").disabled(),
        MenuItem::new("Save").hint("s"),
        MenuItem::new("Quit").hint("q").separated(),
    ]
}

/// A file menu with a clickable label beside it, recording each pick.
fn harness(picks: Signal<Vec<usize>>, clicks: Signal<u32>) -> Harness {
    Harness::new(24, 12, move || {
        install_theme(Theme::dark());
        let mut host = Style::new();
        host.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: host.clone(),
                Menu {
                    label: "File".to_string(),
                    items: items(),
                    on_select: move |i| picks.update(|p| p.push(i)),
                },
                box {
                    on_click: move || clicks.update(|n| *n += 1),
                    text("beside"),
                },
            }
        }
    })
}

/// The menu's button, inside the box the list anchors to.
fn trigger(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let menu = doc.get_node(host).unwrap().children[0];
    doc.get_node(menu).unwrap().children[0]
}

/// The open list: the last child of the root, where the portal put it.
fn list(h: &mut Harness) -> Option<NodeId> {
    let doc = h.document();
    let last = *doc.get_node(doc.root()).unwrap().children.last().unwrap();
    let style = doc.resolved_style(last).unwrap();
    style.stacking_context.then_some(last)
}

/// The list's item rows, leaving out separators and the markers of dynamic blocks.
fn item_rows(h: &mut Harness) -> Vec<NodeId> {
    let list = list(h).unwrap();
    let doc = h.document();
    doc.get_node(list)
        .unwrap()
        .children
        .into_iter()
        .filter(|&row| !doc.get_node(row).unwrap().children.is_empty())
        .collect()
}

fn open(h: &mut Harness) {
    let trigger = trigger(h);
    h.focus(trigger);
    h.press(KeyCode::Enter);
}

#[test]
fn the_list_opens_below_the_button_with_hints_and_separators() {
    let mut h = harness(state(Vec::new()), state(0));
    assert_eq!(h.row(1).trim_end(), "│  File ▾  │");
    open(&mut h);
    assert!(list(&mut h).is_some());
    for (y, expected) in [
        (3, "╭───────────╮"),
        (4, "│ Open    o │"),
        (5, "│ Export  e │"),
        (6, "│ Save    s │"),
        (7, "│───────────│"),
        (8, "│ Quit    q │"),
        (9, "╰───────────╯"),
    ] {
        assert_eq!(h.row(y).trim_end(), expected, "row {y}");
    }
}

#[test]
fn the_highlight_skips_disabled_items_and_enter_reports_the_index() {
    let picks = state(Vec::new());
    let mut h = harness(picks, state(0));
    open(&mut h);
    h.press(KeyCode::Down);
    h.press(KeyCode::Enter);
    assert_eq!(picks.get(), vec![2]);
    assert!(list(&mut h).is_none());
    assert_eq!(h.document().focused(), Some(trigger(&mut h)));

    open(&mut h);
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    h.press(KeyCode::Down);
    h.press(KeyCode::Char(' '));
    assert_eq!(picks.get(), vec![2, 3]);

    open(&mut h);
    h.press(KeyCode::Down);
    h.press(KeyCode::Up);
    h.press(KeyCode::Enter);
    assert_eq!(picks.get(), vec![2, 3, 0]);
}

#[test]
fn the_highlight_starts_on_the_first_enabled_item() {
    let picks = state(Vec::new());
    let mut h = Harness::new(24, 12, move || {
        install_theme(Theme::dark());
        view! {
            box {
                test_id: "host",
                Menu {
                    label: "File".to_string(),
                    items: vec![MenuItem::new("Gone").disabled(), MenuItem::new("Here")],
                    on_select: move |i| picks.update(|p| p.push(i)),
                },
            }
        }
    });
    open(&mut h);
    h.press(KeyCode::Enter);
    assert_eq!(picks.get(), vec![1]);
}

#[test]
fn clicking_an_item_picks_it_and_a_disabled_one_does_nothing() {
    let picks = state(Vec::new());
    let mut h = harness(picks, state(0));
    open(&mut h);
    h.click_text("Export");
    assert!(picks.get().is_empty());
    assert!(list(&mut h).is_some());
    h.click_text("Quit");
    assert_eq!(picks.get(), vec![3]);
    assert!(list(&mut h).is_none());
}

#[test]
fn escape_and_a_press_outside_close_it_without_a_pick() {
    let (picks, clicks) = (state(Vec::new()), state(0));
    let mut h = harness(picks, clicks);
    open(&mut h);
    h.press(KeyCode::Esc);
    assert!(list(&mut h).is_none());

    open(&mut h);
    h.click_text("beside");
    assert!(list(&mut h).is_none());
    assert!(picks.get().is_empty());
    assert_eq!(clicks.get(), 0);
}

#[test]
fn hints_are_muted_off_the_highlight_and_disabled_items_are_muted() {
    let mut h = harness(state(Vec::new()), state(0));
    open(&mut h);
    let rows = item_rows(&mut h);
    let doc = h.document();
    let color = |row: NodeId, child: usize| {
        let node = doc.get_node(row).unwrap().children[child];
        doc.resolved_style(node).unwrap().color
    };
    let dark = Palette::dark();
    assert_eq!(color(rows[0], 1), literal(&dark.accent_foreground));
    assert_eq!(color(rows[2], 0), literal(&dark.popover_foreground));
    assert_eq!(color(rows[2], 1), literal(&dark.muted_foreground));
    assert_eq!(color(rows[1], 0), literal(&dark.muted_foreground));
}

#[test]
fn a_disabled_menu_stays_closed() {
    let mut h = Harness::new(24, 12, || {
        install_theme(Theme::dark());
        view! {
            box {
                test_id: "host",
                Menu { label: "File".to_string(), items: items(), disabled: true },
            }
        }
    });
    h.click_text("File");
    assert!(list(&mut h).is_none());
}

use std::collections::HashSet;

use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::theme::{Theme, install_theme};

fn nodes() -> Vec<TreeNode> {
    vec![
        TreeNode::new("src", "src").with_children(vec![
            TreeNode::new("main", "main.rs"),
            TreeNode::new("view", "view").with_children(vec![TreeNode::new("mod", "mod.rs")]),
        ]),
        TreeNode::new("cargo", "Cargo.toml"),
    ]
}

struct Fixture {
    h: Harness,
    expanded: Signal<HashSet<String>>,
    selected: Signal<Option<String>>,
    activated: Signal<Vec<String>>,
}

/// The tree with `src` expanded and `src` selected, focused, on a screen it fits.
fn fixture() -> Fixture {
    let expanded = state(HashSet::from(["src".to_string()]));
    let selected = state(Some("src".to_string()));
    let activated = state(Vec::new());
    let mut h = Harness::new(16, 6, move || {
        install_theme(Theme::dark());
        let mut style = Style::new();
        style.width(Length::Cells(16));
        style.height(Length::Cells(6));
        view! {
            box {
                test_id: "host",
                Tree {
                    nodes: nodes(),
                    expanded: expanded,
                    selected: selected,
                    on_activate: move |id| activated.update(|a| a.push(id)),
                    style: style.clone(),
                },
            }
        }
    });
    let tree = tree(&mut h);
    h.focus(tree);
    Fixture {
        h,
        expanded,
        selected,
        activated,
    }
}

fn tree(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    h.document().get_children(host)[0]
}

/// The tree's rows, without the scrollbar column.
fn rows(h: &mut Harness) -> Vec<String> {
    (0..6)
        .map(|y| {
            let row: String = h.row(y).chars().take(15).collect();
            row.trim_end().to_string()
        })
        .filter(|row| !row.is_empty())
        .collect()
}

fn selected(f: &Fixture) -> Option<String> {
    f.selected.get()
}

#[test]
fn expanded_nodes_show_their_children_indented() {
    let mut f = fixture();
    assert_eq!(
        rows(&mut f.h),
        ["▾ src", "    main.rs", "  ▸ view", "  Cargo.toml"]
    );
}

#[test]
fn up_and_down_move_the_selection_over_the_rows_shown() {
    let mut f = fixture();
    f.h.press(KeyCode::Down);
    f.h.press(KeyCode::Down);
    assert_eq!(selected(&f).as_deref(), Some("view"));
    f.h.press(KeyCode::End);
    assert_eq!(selected(&f).as_deref(), Some("cargo"));
    f.h.press(KeyCode::Up);
    assert_eq!(selected(&f).as_deref(), Some("view"));
}

#[test]
fn right_expands_then_enters_and_left_leaves_then_collapses() {
    let mut f = fixture();
    f.h.press(KeyCode::Down);
    f.h.press(KeyCode::Down);
    f.h.press(KeyCode::Right);
    assert!(f.expanded.get().contains("view"));
    assert_eq!(selected(&f).as_deref(), Some("view"));
    assert_eq!(rows(&mut f.h)[3], "      mod.rs");

    f.h.press(KeyCode::Right);
    assert_eq!(selected(&f).as_deref(), Some("mod"));

    f.h.press(KeyCode::Left);
    assert_eq!(selected(&f).as_deref(), Some("view"));
    f.h.press(KeyCode::Left);
    assert!(!f.expanded.get().contains("view"));
    f.h.press(KeyCode::Left);
    assert_eq!(selected(&f).as_deref(), Some("src"));
    f.h.press(KeyCode::Left);
    assert!(f.expanded.get().is_empty());
    assert_eq!(rows(&mut f.h), ["▸ src", "  Cargo.toml"]);
}

#[test]
fn the_selection_stays_on_its_node_as_rows_above_it_close() {
    let mut f = fixture();
    f.h.press(KeyCode::End);
    f.expanded.update(|open| open.clear());
    assert_eq!(rows(&mut f.h), ["▸ src", "  Cargo.toml"]);
    let (x, y) = f.h.find_text("Cargo.toml").unwrap();
    let doc = f.h.document();
    let row = doc
        .get_node(doc.node_at(x, y).unwrap())
        .unwrap()
        .parent
        .unwrap();
    assert!(doc.resolved_style(row).unwrap().background.is_some());

    f.h.press(KeyCode::Up);
    assert_eq!(selected(&f).as_deref(), Some("src"));
    f.h.press(KeyCode::Enter);
    assert_eq!(f.activated.get(), ["src"]);
}

#[test]
fn a_click_on_a_label_selects_without_toggling() {
    let mut f = fixture();
    f.h.click_text("view");
    assert_eq!(selected(&f).as_deref(), Some("view"));
    assert!(!f.expanded.get().contains("view"));
    f.h.click_text("▸");
    assert!(f.expanded.get().contains("view"));
}

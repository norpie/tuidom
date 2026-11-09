use tuidom::NodeId;
use tuidom::style::{FlexDirection, Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{fragment, text, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

/// A full-height column with content above a status bar.
fn harness() -> Harness {
    Harness::new(30, 4, || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.width(Length::Percent(100.0));
        column.height(Length::Percent(100.0));
        let mut grow = Style::new();
        grow.flex_grow(1.0);
        view! {
            box {
                style: column.clone(),
                box { style: grow.clone(), text("content") },
                StatusBar {
                    left: fragment([
                        text("main.rs").into(),
                        view! { KeyHint { key: "q".to_string(), label: "quit".to_string() } },
                    ]),
                    right: view! { text("Ln 4") },
                },
            }
        }
    })
}

fn bar(h: &mut Harness) -> NodeId {
    let (x, y) = h.find_text("Ln 4").unwrap();
    let doc = h.document();
    let mut node = doc.node_at(x, y).unwrap();
    while doc.resolved_style(node).unwrap().background.is_none() {
        node = doc.get_node(node).unwrap().parent.unwrap();
    }
    node
}

#[test]
fn the_bar_spreads_its_sides_across_the_last_row_in_the_muted_color() {
    let mut h = harness();
    assert_eq!(h.row(3), "  main.rs  q quit       Ln 4");
    let fill = h.cell(0, 3).unwrap().bg;
    assert_eq!(h.cell(29, 3).unwrap().bg, fill);
    assert_ne!(h.cell(0, 0).unwrap().bg, fill);
    let bar = bar(&mut h);
    let style = h.document().resolved_style(bar).unwrap();
    assert_eq!(style.background, Some(literal(&Palette::dark().muted)));
}

#[test]
fn plain_text_in_the_bar_takes_the_muted_foreground() {
    let mut h = harness();
    let (x, y) = h.find_text("Ln 4").unwrap();
    let doc = h.document();
    let node = doc.node_at(x, y).unwrap();
    let color = doc.resolved_style(node).unwrap().color;
    assert_eq!(color, literal(&Palette::dark().muted_foreground));
}

#[test]
fn a_key_hint_draws_its_key_bold_in_the_primary() {
    let mut h = harness();
    let (x, y) = h.find_text("q quit").unwrap();
    let doc = h.document();
    let key = doc.node_at(x, y).unwrap();
    let style = doc.resolved_style(key).unwrap();
    assert_eq!(style.color, literal(&Palette::dark().primary));
    assert!(style.bold);
    let label = doc.node_at(x + 2, y).unwrap();
    assert_eq!(
        doc.resolved_style(label).unwrap().color,
        literal(&Palette::dark().muted_foreground)
    );
}

#[test]
fn either_side_can_be_left_out() {
    let mut h = Harness::new(12, 1, || {
        install_theme(Theme::dark());
        let mut full = Style::new();
        full.width(Length::Percent(100.0));
        view! { box { style: full.clone(), StatusBar { right: view! { text("Ln 4") } } } }
    });
    assert_eq!(h.row(0), "      Ln 4");
}

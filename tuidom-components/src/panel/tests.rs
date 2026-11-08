use tuidom::NodeKindView;
use tuidom::style::{AlignItems, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Shape, Theme, install_theme};

fn harness(theme: Theme, title: &'static str) -> Harness {
    Harness::new(16, 5, move || {
        install_theme(theme.clone());
        let mut host = Style::new();
        host.align_items(AlignItems::FlexStart);
        view! {
            box {
                test_id: "host",
                style: host.clone(),
                Panel { title: title.to_string(), text("first"), text("second") },
            }
        }
    })
}

fn surfaced(surface: Surface) -> Theme {
    Theme {
        shape: Shape {
            surface,
            ..Shape::default()
        },
        ..Theme::dark()
    }
}

#[test]
fn a_bordered_panel_draws_its_title_on_the_border() {
    let mut h = harness(Theme::dark(), "Title");
    assert_eq!(h.row(0).trim_end(), "╭─ Title ──╮");
    assert_eq!(h.row(1).trim_end(), "│  first   │");
    assert_eq!(h.row(2).trim_end(), "│  second  │");
    assert_eq!(h.row(3).trim_end(), "╰──────────╯");
}

#[test]
fn without_a_title_the_border_is_unbroken() {
    let mut h = harness(Theme::dark(), "");
    assert_eq!(h.row(0).trim_end(), "╭──────────╮");
}

#[test]
fn unbordered_panels_put_the_title_on_its_own_bold_row() {
    let mut h = harness(surfaced(Surface::Flat), "Title");
    assert_eq!(h.row(0).trim_end(), "  Title");
    assert_eq!(h.row(1).trim_end(), "  first");
    let (x, y) = h.find_text("Title").unwrap();
    assert!(h.cell(x, y).unwrap().bold);

    let mut h = harness(surfaced(Surface::HalfBlock), "Title");
    assert_eq!(h.row(0).trim_end(), "▗▄▄▄▄▄▄▄▄▖");
    assert_eq!(h.row(1).trim_end(), "▐ Title  ▌");
    assert_eq!(h.row(3).trim_end(), "▐ second ▌");
}

#[test]
fn plain_text_inside_takes_the_card_foreground() {
    let mut palette = Palette::dark();
    palette.card_foreground = tuidom::style::Color::red();
    let theme = Theme {
        palette,
        ..Theme::dark()
    };
    let mut h = harness(theme, "");
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document().clone();
    let panel = doc.get_node(host).unwrap().children[0];
    let first = doc
        .get_node(panel)
        .unwrap()
        .children
        .into_iter()
        .find(|&n| matches!(doc.get_node(n).unwrap().kind, NodeKindView::Text { ref content } if content == "first"))
        .unwrap();
    assert_eq!(
        doc.resolved_style(first).unwrap().color,
        tuidom::style::ResolvedColor::red()
    );
    assert_eq!(
        doc.resolved_style(panel).unwrap().background,
        Some(literal(&Palette::dark().card))
    );
}

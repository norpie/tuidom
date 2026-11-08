use tuidom::style::{AlignItems, BorderCharset, EdgeInsets, FlexDirection, Length, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Shape, Theme, install_theme};

fn container(direction: FlexDirection) -> Style {
    let mut style = Style::new();
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style.flex_direction(direction);
    // Not stretching its children, so the divider has to stretch itself.
    style.align_items(AlignItems::FlexStart);
    style
}

#[test]
fn a_horizontal_divider_spans_its_column() {
    let mut h = Harness::new(6, 3, || {
        install_theme(Theme::dark());
        view! {
            box {
                style: container(FlexDirection::Column),
                text("a"),
                Divider {},
                text("b"),
            }
        }
    });
    assert_eq!(h.row(0).trim_end(), "a");
    assert_eq!(h.row(1), "──────");
    assert_eq!(h.row(2).trim_end(), "b");
}

#[test]
fn a_vertical_divider_spans_its_row() {
    let mut h = Harness::new(3, 2, || {
        install_theme(Theme::dark());
        view! {
            box {
                style: container(FlexDirection::Row),
                text("a"),
                Divider { orientation: Orientation::Vertical },
                text("b"),
            }
        }
    });
    assert_eq!(h.row(0), "a│b");
    assert_eq!(h.row(1).trim_end(), " │");
}

#[test]
fn a_divider_uses_the_theme_charset_and_border_color() {
    let mut h = Harness::new(4, 1, || {
        install_theme(Theme {
            shape: Shape {
                border: BorderCharset::thick(),
                ..Shape::default()
            },
            ..Theme::dark()
        });
        view! { box { test_id: "host", style: container(FlexDirection::Column), Divider {} } }
    });
    assert_eq!(h.row(0), "━━━━");
    let host = h.find_attr("test_id", "host").unwrap();
    let line = h.document().get_node(host).unwrap().children[0];
    assert_eq!(
        h.document().resolved_style(line).unwrap().border_color,
        Some(literal(&Palette::dark().border))
    );
}

#[test]
fn the_style_prop_merges_over_its_own() {
    let mut h = Harness::new(6, 1, || {
        install_theme(Theme::dark());
        let mut inset = Style::new();
        inset.margin(EdgeInsets::symmetric(1, 0));
        view! { box { style: container(FlexDirection::Column), Divider { style: inset.clone() } } }
    });
    assert_eq!(h.row(0).trim_end(), " ────");
}

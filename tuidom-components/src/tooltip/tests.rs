use tuidom::style::{Color, EdgeInsets, FlexDirection, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::theme::{Palette, ThemeScope, ThemeScopeProps, install_theme};

/// A tooltip around a focusable box on the middle of three rows, with another focusable box.
fn harness(side: Side) -> Harness {
    Harness::new(16, 3, move || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        view! {
            box {
                style: column.clone(),
                box { test_id: "other", focusable: true, text("other") },
                Tooltip {
                    text: "Saves it".to_string(),
                    side: side,
                    box { test_id: "save", focusable: true, text("save") },
                },
            }
        }
    })
}

#[test]
fn the_tip_shows_above_while_focus_is_inside_and_hides_after() {
    let mut h = harness(Side::Above);
    assert!(h.find_text("Saves it").is_none());

    let save = h.find_attr("test_id", "save").unwrap();
    h.focus(save);
    assert_eq!(h.row(0).trim_end(), " Saves it");
    assert_eq!(h.row(1).trim_end(), "save");

    let other = h.find_attr("test_id", "other").unwrap();
    h.focus(other);
    assert!(h.find_text("Saves it").is_none());
}

#[test]
fn a_tip_below_sits_under_the_content() {
    let mut h = harness(Side::Below);
    let save = h.find_attr("test_id", "save").unwrap();
    h.focus(save);
    assert_eq!(h.row(2).trim_end(), " Saves it");
    assert_eq!(h.document().focused(), Some(save));
}

#[test]
fn the_tip_takes_the_theme_its_content_is_in() {
    let mut palette = Palette::light();
    palette.popover = Color::red();
    let scoped = Theme {
        palette,
        ..Theme::light()
    };
    let mut h = Harness::new(16, 3, move || {
        install_theme(Theme::dark());
        view! {
            box {
                ThemeScope {
                    theme: scoped.clone(),
                    Tooltip {
                        text: "tip".to_string(),
                        side: Side::Below,
                        box { test_id: "save", focusable: true, text("save") },
                    },
                },
            }
        }
    });
    let save = h.find_attr("test_id", "save").unwrap();
    h.focus(save);
    let doc = h.document();
    let tip = *doc.get_node(doc.root()).unwrap().children.last().unwrap();
    assert_eq!(
        doc.resolved_style(tip).unwrap().background,
        Some(ResolvedColor::red())
    );
}

#[test]
fn the_style_prop_merges_over_the_tip() {
    let mut h = Harness::new(16, 3, || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        let mut wide = Style::new();
        wide.padding(EdgeInsets::symmetric(3, 0));
        view! {
            box {
                style: column.clone(),
                text("other"),
                Tooltip {
                    text: "tip".to_string(),
                    side: Side::Above,
                    style: wide.clone(),
                    box { test_id: "save", focusable: true, text("save") },
                },
            }
        }
    });
    let save = h.find_attr("test_id", "save").unwrap();
    h.focus(save);
    assert_eq!(h.row(0).trim_end(), "   tip");
}

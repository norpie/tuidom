use tuidom::NodeId;
use tuidom::style::{Color, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn label_node(h: &mut Harness) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    h.document().get_node(host).unwrap().children[0]
}

#[test]
fn a_label_draws_its_text_in_its_tone() {
    for (tone, color) in [
        (LabelTone::Default, Palette::dark().foreground),
        (LabelTone::Muted, Palette::dark().muted_foreground),
    ] {
        let mut h = Harness::new(12, 1, move || {
            install_theme(Theme::dark());
            view! { box { test_id: "host", Label { text: "hello".to_string(), tone: tone } } }
        });
        assert_eq!(h.row(0).trim_end(), "hello");
        let label = label_node(&mut h);
        assert_eq!(
            h.document().resolved_style(label).unwrap().color,
            literal(&color),
            "{tone:?}"
        );
    }
}

#[test]
fn a_label_is_its_text_node_alone() {
    let mut h = Harness::new(12, 1, || {
        install_theme(Theme::dark());
        view! { box { test_id: "host", Label { text: "hello".to_string() } } }
    });
    let label = label_node(&mut h);
    assert!(h.document().get_node(label).unwrap().children.is_empty());
}

#[test]
fn the_style_prop_merges_over_the_tone() {
    let mut h = Harness::new(12, 1, || {
        install_theme(Theme::dark());
        let mut bold = Style::new();
        bold.bold(true);
        let mut red = Style::new();
        red.color(Color::red());
        view! {
            box {
                test_id: "host",
                Label { text: "a".to_string(), tone: LabelTone::Muted, style: bold.clone() },
                Label { text: "b".to_string(), style: red.clone() },
            }
        }
    });
    let cell = h.cell(0, 0).unwrap();
    assert!(cell.bold);
    let host = h.find_attr("test_id", "host").unwrap();
    let second = h.document().get_node(host).unwrap().children[1];
    assert_eq!(
        h.document().resolved_style(second).unwrap().color,
        ResolvedColor::red()
    );
    let first = h.document().get_node(host).unwrap().children[0];
    assert_eq!(
        h.document().resolved_style(first).unwrap().color,
        literal(&Palette::dark().muted_foreground)
    );
}

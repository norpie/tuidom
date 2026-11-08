use std::time::Duration;

use tuidom_ui::testing::Harness;
use tuidom_ui::view;

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

#[test]
fn it_turns_on_the_engine_clock_beside_its_label() {
    let mut h = Harness::new(12, 1, || {
        install_theme(Theme::dark());
        view! { box { Spinner { label: "Loading".to_string() } } }
    });
    assert_eq!(h.row(0).trim_end(), "⠋ Loading");
    h.advance(Duration::from_millis(80));
    assert_eq!(h.row(0).trim_end(), "⠙ Loading");
    h.advance(Duration::from_millis(80 * 9));
    assert_eq!(h.row(0).trim_end(), "⠋ Loading");
}

#[test]
fn the_glyph_is_primary_and_a_bare_spinner_is_one_cell() {
    let mut h = Harness::new(4, 1, || {
        install_theme(Theme::dark());
        view! { box { Spinner {} } }
    });
    assert_eq!(h.row(0), "⠋");
    let doc = h.document().clone();
    let outer = doc.get_node(doc.root()).unwrap().children[0];
    let spinner = doc.get_node(outer).unwrap().children[0];
    let glyph = doc.get_node(spinner).unwrap().children[0];
    assert_eq!(
        doc.resolved_style(glyph).unwrap().color,
        literal(&Palette::dark().primary)
    );
}

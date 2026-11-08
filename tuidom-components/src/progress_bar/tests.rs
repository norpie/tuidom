use tuidom::NodeId;
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn harness(value: Signal<f64>, tone: Tone) -> Harness {
    Harness::new(10, 1, move || {
        install_theme(Theme::dark());
        let mut full = Style::new();
        full.width(Length::Percent(100.0));
        view! { box { style: full.clone(), ProgressBar { value: value.get(), tone: tone } } }
    })
}

/// The track, its filled segment, and the filled segment's width in cells.
fn parts(h: &mut Harness) -> (NodeId, NodeId, u16) {
    let doc = h.document();
    let outer = doc.get_node(doc.root()).unwrap().children[0];
    let track = doc.get_node(outer).unwrap().children[0];
    let fill = doc.get_node(track).unwrap().children[0];
    let width = doc.get_node(fill).unwrap().layout.unwrap().rect.width;
    (track, fill, width)
}

#[test]
fn the_fill_takes_the_values_share_of_the_track() {
    let value = state(0.3);
    let mut h = harness(value, Tone::Default);
    h.screen();
    assert_eq!(parts(&mut h).2, 3);
    value.set(1.0);
    h.screen();
    assert_eq!(parts(&mut h).2, 10);
    value.set(-2.0);
    h.screen();
    assert_eq!(parts(&mut h).2, 0);
}

#[test]
fn the_fill_takes_the_tones_color_on_a_muted_track() {
    let dark = Palette::dark();
    let cases = [
        (Tone::Default, &dark.primary),
        (Tone::Success, &dark.success),
        (Tone::Warning, &dark.warning),
        (Tone::Destructive, &dark.destructive),
    ];
    for (tone, color) in cases {
        let mut h = harness(state(0.5), tone);
        h.screen();
        let (track, fill, _) = parts(&mut h);
        let doc = h.document();
        assert_eq!(
            doc.resolved_style(fill).unwrap().background,
            Some(literal(color)),
            "{tone:?}"
        );
        assert_eq!(
            doc.resolved_style(track).unwrap().background,
            Some(literal(&dark.muted))
        );
    }
}

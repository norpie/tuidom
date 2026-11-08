use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Color, Length, ResolvedColor, ResolvedStyle, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Theme, install_theme};

fn shaped(surface: Surface, focus: FocusMark) -> Theme {
    Theme {
        shape: Shape {
            surface,
            focus,
            ..Shape::default()
        },
        ..Theme::dark()
    }
}

/// A host that sizes the button to its content rather than stretching it.
fn harness(theme: Theme, button: impl Fn() -> View + Send + Sync + 'static) -> Harness {
    Harness::new(20, 4, move || {
        install_theme(theme);
        let mut host = Style::new();
        host.align_items(AlignItems::FlexStart);
        view! { box { test_id: "host", style: host.clone(), { button() } } }
    })
}

/// The button's own box and its label's text node.
fn nodes(h: &mut Harness) -> (NodeId, NodeId) {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let button = doc.get_node(host).unwrap().children[0];
    let label = doc.get_node(button).unwrap().children[0];
    (button, label)
}

fn resolved(h: &Harness, node: NodeId) -> ResolvedStyle {
    h.document().resolved_style(node).unwrap()
}

#[test]
fn a_bordered_button_frames_its_label() {
    let mut h = harness(
        Theme::dark(),
        || view! { Button { label: "ok".to_string() } },
    );
    assert_eq!(h.row(0).trim_end(), "╭──────╮");
    assert_eq!(h.row(1).trim_end(), "│  ok  │");
    assert_eq!(h.row(2).trim_end(), "╰──────╯");
}

#[test]
fn a_stretched_button_centers_its_label() {
    let mut h = Harness::new(20, 5, || {
        install_theme(Theme::dark());
        view! { box { Button { label: "ok".to_string() } } }
    });
    assert_eq!(h.row(2).trim_end(), "│  ok  │");
    assert_eq!(h.row(4).trim_end(), "╰──────╯");
}

#[test]
fn half_block_and_flat_buttons_take_one_row() {
    let mut h = harness(shaped(Surface::HalfBlock, FocusMark::Ring), || {
        view! { Button { label: "ok".to_string() } }
    });
    assert_eq!(h.row(0).trim_end(), "▐ ok ▌");
    assert_eq!(h.row(1).trim_end(), "");

    let mut h = harness(shaped(Surface::Flat, FocusMark::Ring), || {
        view! { Button { label: "ok".to_string() } }
    });
    assert_eq!(h.row(0).trim_end(), "  ok");
    assert_eq!(h.row(1).trim_end(), "");
}

#[test]
fn compact_density_pads_one_cell() {
    let theme = Theme {
        shape: Shape {
            density: Density::Compact,
            ..Shape::default()
        },
        ..Theme::dark()
    };
    let mut h = harness(theme, || view! { Button { label: "ok".to_string() } });
    assert_eq!(h.row(1).trim_end(), "│ ok │");
}

#[test]
fn each_variant_takes_its_colors() {
    let dark = Palette::dark();
    let cases = [
        (
            ButtonVariant::Primary,
            Some(&dark.primary),
            &dark.primary_foreground,
        ),
        (
            ButtonVariant::Secondary,
            Some(&dark.secondary),
            &dark.secondary_foreground,
        ),
        (
            ButtonVariant::Destructive,
            Some(&dark.destructive),
            &dark.foreground,
        ),
        (ButtonVariant::Outline, None, &dark.foreground),
        (ButtonVariant::Ghost, None, &dark.foreground),
    ];
    for (variant, fill, text) in cases {
        let mut h = harness(Theme::dark(), move || {
            view! { Button { label: "ok".to_string(), variant: variant } }
        });
        let (button, label) = nodes(&mut h);
        assert_eq!(
            resolved(&h, button).background,
            fill.map(literal),
            "{variant:?}"
        );
        assert_eq!(resolved(&h, label).color, literal(text), "{variant:?}");
    }
}

#[test]
fn an_outline_button_is_framed_in_the_input_color() {
    let mut h = harness(Theme::dark(), || {
        view! { Button { label: "ok".to_string(), variant: ButtonVariant::Outline } }
    });
    let (button, _) = nodes(&mut h);
    assert_eq!(
        resolved(&h, button).border_color,
        Some(literal(&Palette::dark().input))
    );
}

#[test]
fn unfilled_buttons_fill_with_the_accent_while_focused() {
    for variant in [ButtonVariant::Outline, ButtonVariant::Ghost] {
        let mut h = harness(Theme::dark(), move || {
            view! { Button { label: "ok".to_string(), variant: variant } }
        });
        let (button, label) = nodes(&mut h);
        h.focus(button);
        assert_eq!(
            resolved(&h, button).background,
            Some(literal(&Palette::dark().accent)),
            "{variant:?}"
        );
        assert_eq!(
            resolved(&h, label).color,
            literal(&Palette::dark().accent_foreground),
            "{variant:?}"
        );
    }
}

fn counting(presses: Signal<u32>, disabled: bool) -> impl Fn() -> View + Send + Sync {
    move || {
        view! {
            Button {
                label: "ok".to_string(),
                disabled: disabled,
                on_press: move |_| presses.update(|n| *n += 1),
            }
        }
    }
}

#[test]
fn a_click_enter_or_space_presses_it() {
    let presses = state(0);
    let mut h = harness(Theme::dark(), counting(presses, false));
    h.click_text("ok");
    assert_eq!(presses.get(), 1);

    let (button, _) = nodes(&mut h);
    h.focus(button);
    h.press(KeyCode::Enter);
    h.press(KeyCode::Char(' '));
    h.press(KeyCode::Char('x'));
    assert_eq!(presses.get(), 3);
}

#[test]
fn a_disabled_button_cannot_be_pressed_or_focused() {
    let presses = state(0);
    let mut h = harness(Theme::dark(), counting(presses, true));
    h.click_text("ok");
    let (button, label) = nodes(&mut h);
    assert!(h.document().focus(button).is_err() || h.document().focused() != Some(button));
    h.press(KeyCode::Enter);
    assert_eq!(presses.get(), 0);

    let dark = Palette::dark();
    assert_eq!(resolved(&h, button).background, Some(literal(&dark.muted)));
    assert_eq!(resolved(&h, label).color, literal(&dark.muted_foreground));
}

#[test]
fn a_ring_recolors_the_border_while_focused() {
    let mut h = harness(
        Theme::dark(),
        || view! { Button { label: "ok".to_string() } },
    );
    let (button, _) = nodes(&mut h);
    h.focus(button);
    assert_eq!(
        resolved(&h, button).border_color,
        Some(literal(&Palette::dark().ring))
    );
}

#[test]
fn a_ring_recolors_half_block_edges_while_focused() {
    let mut h = harness(shaped(Surface::HalfBlock, FocusMark::Ring), || {
        view! { Button { label: "ok".to_string() } }
    });
    let (button, _) = nodes(&mut h);
    assert_eq!(resolved(&h, button).half_block_inner_color, None);
    h.focus(button);
    assert_eq!(
        resolved(&h, button).half_block_inner_color,
        Some(literal(&Palette::dark().ring))
    );
}

#[test]
fn a_flat_button_reverses_instead_of_ringing() {
    let mut h = harness(shaped(Surface::Flat, FocusMark::Ring), || {
        view! { Button { label: "ok".to_string() } }
    });
    let (button, label) = nodes(&mut h);
    h.focus(button);
    let dark = Palette::dark();
    assert_eq!(
        resolved(&h, button).background,
        Some(literal(&dark.primary_foreground))
    );
    assert_eq!(resolved(&h, label).color, literal(&dark.primary));
}

#[test]
fn an_underline_mark_underlines_the_label_while_focused() {
    let mut h = harness(shaped(Surface::Bordered, FocusMark::Underline), || {
        view! { Button { label: "ok".to_string() } }
    });
    let (button, label) = nodes(&mut h);
    assert!(!resolved(&h, label).underline);
    h.focus(button);
    assert!(resolved(&h, label).underline);
    assert_eq!(
        resolved(&h, button).border_color,
        Some(literal(&Palette::dark().primary))
    );
}

#[test]
fn pressing_darkens_the_fill() {
    let mut h = Harness::new(20, 4, || {
        install_theme(Theme::dark());
        let mut darker = Style::new();
        darker.background(Color::var("--primary").darken(0.1));
        view! {
            box {
                box { test_id: "host", Button { label: "ok".to_string() } },
                box { test_id: "probe", style: darker.clone() },
            }
        }
    });
    let (button, _) = nodes(&mut h);
    let probe = h.find_attr("test_id", "probe").unwrap();
    h.document().set_active(button, true).unwrap();
    assert_eq!(
        resolved(&h, button).background,
        resolved(&h, probe).background
    );
}

#[test]
fn the_style_props_merge_over_its_own() {
    let mut h = harness(Theme::dark(), || {
        let mut wide = Style::new();
        wide.width(Length::Cells(12));
        let mut red = Style::new();
        red.background(Color::red());
        view! { Button { label: "ok".to_string(), style: wide.clone(), focus_style: red.clone() } }
    });
    let (button, _) = nodes(&mut h);
    assert_eq!(h.row(1).trim_end(), "│    ok    │");
    h.focus(button);
    assert_eq!(resolved(&h, button).background, Some(ResolvedColor::red()));
}

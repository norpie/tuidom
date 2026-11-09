use std::time::Duration;

use tuidom::NodeId;
use tuidom::style::{Color, ResolvedColor};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, View, component, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Shape, Surface, ThemeScope, ThemeScopeProps, install_theme};

/// Hands the toaster it finds to the test.
#[component]
fn Grab(into: Signal<Option<ToasterHandle>>) -> View {
    let toaster = use_toaster();
    into.update(|slot| *slot = toaster);
    view! { box {} }
}

fn harness(clicks: Signal<u32>) -> (Harness, ToasterHandle) {
    harness_in(Theme::dark(), clicks)
}

/// A toaster around a clickable label, on a 30×10 screen, with its handle.
fn harness_in(theme: Theme, clicks: Signal<u32>) -> (Harness, ToasterHandle) {
    let grabbed = state(None);
    let mut h = Harness::new(30, 10, move || {
        install_theme(theme.clone());
        view! {
            Toaster {
                box {
                    box { on_click: move || clicks.update(|n| *n += 1), text("app") },
                    Grab { into: grabbed },
                }
            }
        }
    });
    h.screen();
    let toaster = grabbed.get().expect("the toaster is in context");
    (h, toaster)
}

fn saved() -> Toast {
    Toast {
        title: "Saved".to_string(),
        description: "All good".to_string(),
        ..Toast::default()
    }
}

/// The open toast cards, oldest first. The stack also holds the `each` block's empty marker.
fn cards(h: &mut Harness) -> Vec<NodeId> {
    h.screen();
    let doc = h.document();
    let stack = *doc.get_node(doc.root()).unwrap().children.last().unwrap();
    doc.get_node(stack)
        .unwrap()
        .children
        .into_iter()
        .filter(|&card| !doc.get_node(card).unwrap().children.is_empty())
        .collect()
}

#[test]
fn toasts_stack_in_the_bottom_right_corner_newest_last() {
    let (mut h, toaster) = harness(state(0));
    toaster.show(saved());
    toaster.show(Toast {
        title: "Second".to_string(),
        ..Toast::default()
    });
    assert_eq!(h.row(3), "                  ▄▄▄▄▄▄▄▄▄▄▄▄");
    assert_eq!(h.row(4), "                    Saved");
    assert_eq!(h.row(5), "                    All good");
    assert_eq!(h.row(6), "                  ▀▀▀▀▀▀▀▀▀▀▀▀");
    assert_eq!(h.row(7), "                    ▄▄▄▄▄▄▄▄▄▄");
    assert_eq!(h.row(8), "                      Second");
    assert_eq!(h.row(9), "                    ▀▀▀▀▀▀▀▀▀▀");
}

#[test]
fn a_toast_holds_then_fades_with_its_text_and_is_removed() {
    let (mut h, toaster) = harness(state(0));
    toaster.show(saved());
    h.advance(HOLD);
    let card = cards(&mut h)[0];
    let (x, y) = h.find_text("Saved").unwrap();
    let solid = h.cell(x, y).unwrap().fg;
    assert_eq!(h.document().resolved_style(card).unwrap().opacity, 1.0);

    h.advance(FADE / 2);
    assert_eq!(h.document().resolved_style(card).unwrap().opacity, 0.5);
    assert_ne!(h.cell(x, y).unwrap().fg, solid);

    h.advance(FADE / 2);
    assert!(cards(&mut h).is_empty());
    assert!(h.find_text("Saved").is_none());
}

#[test]
fn a_click_dismisses_a_toast_and_the_app_stays_clickable() {
    let clicks = state(0);
    let (mut h, toaster) = harness(clicks);
    toaster.show(saved());
    h.click_text("app");
    assert_eq!(clicks.get(), 1);
    assert_eq!(cards(&mut h).len(), 1);

    h.click_text("Saved");
    assert!(cards(&mut h).is_empty());
    assert_eq!(clicks.get(), 1);
}

#[test]
fn dismissing_one_toast_leaves_the_others_on_their_own_clocks() {
    let (mut h, toaster) = harness(state(0));
    toaster.show(saved());
    h.advance(Duration::from_secs(2));
    toaster.show(Toast {
        title: "Second".to_string(),
        ..Toast::default()
    });
    h.advance(HOLD + FADE - Duration::from_secs(2));
    assert!(h.find_text("Saved").is_none());
    assert!(h.find_text("Second").is_some());
    h.advance(Duration::from_secs(2));
    assert!(cards(&mut h).is_empty());
}

#[test]
fn the_tone_colors_the_edge() {
    let dark = Palette::dark();
    for surface in [Surface::Bordered, Surface::HalfBlock] {
        let theme = Theme {
            shape: Shape {
                container: surface,
                ..Shape::default()
            },
            ..Theme::dark()
        };
        let (mut h, toaster) = harness_in(theme, state(0));
        toaster.show(Toast {
            title: "Failed".to_string(),
            tone: Tone::Destructive,
            ..Toast::default()
        });
        let card = cards(&mut h)[0];
        let style = h.document().resolved_style(card).unwrap();
        let edge = match surface {
            Surface::Bordered => style.border_color,
            _ => style.half_block_inner_color,
        };
        assert_eq!(edge, Some(literal(&dark.destructive)), "{surface:?}");
        assert_eq!(
            style.background,
            Some(literal(&dark.popover)),
            "{surface:?}"
        );
    }
}

#[test]
fn toasts_take_the_theme_the_toaster_is_in() {
    let mut palette = Palette::light();
    palette.popover = Color::red();
    let scoped = Theme {
        palette,
        ..Theme::light()
    };
    let grabbed = state(None);
    let mut h = Harness::new(30, 10, move || {
        install_theme(Theme::dark());
        view! {
            box {
                ThemeScope {
                    theme: scoped.clone(),
                    Toaster { Grab { into: grabbed } },
                },
            }
        }
    });
    h.screen();
    grabbed.get().unwrap().show(saved());
    let card = cards(&mut h)[0];
    assert_eq!(
        h.document().resolved_style(card).unwrap().background,
        Some(ResolvedColor::red())
    );
}

#[test]
fn there_is_no_toaster_outside_one() {
    let grabbed = state(None);
    let mut h = Harness::new(4, 1, move || view! { Grab { into: grabbed } });
    h.screen();
    assert!(grabbed.get().is_none());
}

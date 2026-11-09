use tuidom::NodeId;
use tuidom::event::KeyCode;
use tuidom::style::{Color, FlexDirection, ResolvedColor, Style};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::button::{Button, ButtonProps};
use crate::theme::{Palette, ThemeScope, ThemeScopeProps, install_theme};

/// An opener button with a modal holding a close button, on a 24×8 screen.
fn harness(open: Signal<bool>, dismissible: bool, opened: Signal<u32>) -> Harness {
    Harness::new(24, 8, move || {
        install_theme(Theme::dark());
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        view! {
            box {
                style: column.clone(),
                box {
                    test_id: "opener",
                    focusable: true,
                    on_click: move || opened.update(|n| *n += 1),
                    text("opener"),
                },
                Modal {
                    open: open,
                    title: "Confirm".to_string(),
                    dismissible: dismissible,
                    text("Sure?"),
                    Button { label: "Close".to_string(), on_press: move |_| open.set(false) },
                },
            }
        }
    })
}

/// The open modal's backdrop, the root's last child.
fn backdrop(h: &mut Harness) -> Option<NodeId> {
    let doc = h.document();
    let last = *doc.get_node(doc.root()).unwrap().children.last().unwrap();
    let card = doc.get_node(last).unwrap().children.first().copied()?;
    doc.resolved_style(card)
        .unwrap()
        .stacking_context
        .then_some(last)
}

#[test]
fn a_closed_modal_draws_nothing() {
    let mut h = harness(state(false), true, state(0));
    assert!(backdrop(&mut h).is_none());
    assert!(!h.screen().contains("Confirm"));
}

#[test]
fn an_open_modal_centers_its_panel_over_a_dimmed_screen() {
    let mut h = harness(state(true), true, state(0));
    let backdrop = backdrop(&mut h).unwrap();
    assert_eq!(h.row(1).trim_end(), "      ▗▄▄▄▄▄▄▄▄▄▄▄▖");
    assert_eq!(h.row(2).trim_end(), "      ▐ Confirm   ▌");
    assert_eq!(h.row(3).trim_end(), "      ▐ Sure?     ▌");
    assert_eq!(h.row(5).trim_end(), "      ▐ ▐ Close ▌ ▌");
    assert_eq!(h.row(7).trim_end(), "      ▝▀▀▀▀▀▀▀▀▀▀▀▘");
    assert_ne!(
        h.document().resolved_style(backdrop).unwrap().background,
        None
    );
}

#[test]
fn the_backdrop_dims_the_text_behind_it() {
    let open = state(false);
    let mut h = harness(open, true, state(0));
    let lit = h.cell(0, 0).unwrap().fg.unwrap();
    open.set(true);
    let dimmed = h.cell(0, 0).unwrap();
    assert_eq!(dimmed.text, "o");
    assert!(
        dimmed.fg.unwrap().r < lit.r,
        "{:?} under the modal, {lit:?} without",
        dimmed.fg
    );
}

#[test]
fn the_backdrop_is_the_background_at_partial_alpha() {
    let mut h = Harness::new(24, 8, || {
        install_theme(Theme::dark());
        let mut dimmed = Style::new();
        dimmed.background(Color::var("--background").with_alpha(0.6));
        view! {
            box {
                box { test_id: "probe", style: dimmed.clone() },
                Modal { open: state(true), text("inside") },
            }
        }
    });
    let backdrop = backdrop(&mut h).unwrap();
    let probe = h.find_attr("test_id", "probe").unwrap();
    let doc = h.document();
    assert_eq!(
        doc.resolved_style(backdrop).unwrap().background,
        doc.resolved_style(probe).unwrap().background
    );
}

#[test]
fn focus_moves_inside_and_returns_to_the_opener_on_close() {
    let open = state(false);
    let mut h = harness(open, true, state(0));
    let opener = h.find_attr("test_id", "opener").unwrap();
    h.focus(opener);
    open.set(true);
    h.screen();
    assert_ne!(h.document().focused(), Some(opener));
    h.press(KeyCode::Tab);
    assert_ne!(h.document().focused(), Some(opener));

    h.press(KeyCode::Enter);
    assert!(!open.get());
    h.screen();
    assert_eq!(h.document().focused(), Some(opener));
}

#[test]
fn escape_and_a_press_outside_close_a_dismissible_modal() {
    let (open, opened) = (state(true), state(0));
    let mut h = harness(open, true, opened);
    h.press(KeyCode::Esc);
    assert!(!open.get());

    open.set(true);
    h.click_text("Sure?");
    assert!(open.get());
    h.click(0, 7);
    assert!(!open.get());

    // The opener sits under the backdrop; the press that closes the modal does not click it.
    open.set(true);
    h.click_text("opener");
    assert!(!open.get());
    assert_eq!(opened.get(), 0);
}

#[test]
fn a_modal_that_is_not_dismissible_stays_open() {
    let open = state(true);
    let mut h = harness(open, false, state(0));
    h.press(KeyCode::Esc);
    h.click(0, 7);
    assert!(open.get());
    h.click_text("Close");
    assert!(!open.get());
}

#[test]
fn the_modal_takes_the_theme_it_is_declared_in() {
    let mut palette = Palette::light();
    palette.card = Color::red();
    let scoped = Theme {
        palette,
        ..Theme::light()
    };
    let mut h = Harness::new(24, 8, move || {
        install_theme(Theme::dark());
        view! {
            box {
                ThemeScope {
                    theme: scoped.clone(),
                    Modal { open: state(true), text("inside") },
                },
            }
        }
    });
    let backdrop = backdrop(&mut h).unwrap();
    let doc = h.document();
    let card = doc.get_node(backdrop).unwrap().children[0];
    let panel = doc.get_node(card).unwrap().children[0];
    assert_eq!(
        doc.resolved_style(panel).unwrap().background,
        Some(ResolvedColor::red())
    );
}

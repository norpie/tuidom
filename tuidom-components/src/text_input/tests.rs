use tuidom::style::{AlignItems, Length, ResolvedStyle, Style};
use tuidom::{NodeId, NodeKindView};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::test_support::literal;
use crate::theme::{Palette, Shape, Surface, Theme, install_theme};

/// A full-width host that does not stretch the field vertically.
fn harness(theme: Theme, field: impl Fn() -> View + Send + Sync + 'static) -> Harness {
    Harness::new(12, 4, move || {
        install_theme(theme);
        let mut host = Style::new();
        host.width(Length::Percent(100.0));
        host.align_items(AlignItems::FlexStart);
        view! { box { test_id: "host", style: host.clone(), { field() } } }
    })
}

fn surfaced(surface: Surface) -> Theme {
    Theme {
        shape: Shape {
            field: surface,
            ..Shape::default()
        },
        ..Theme::dark()
    }
}

/// The field's frame and its input node.
fn nodes(h: &mut Harness) -> (NodeId, NodeId) {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let frame = doc.get_node(host).unwrap().children[0];
    let line = doc.get_node(frame).unwrap().children[0];
    let input = *doc.get_node(line).unwrap().children.last().unwrap();
    (frame, input)
}

/// The text node showing `content`, which the input laid over it hides from hit testing.
fn text_node(h: &mut Harness, content: &str) -> NodeId {
    let host = h.find_attr("test_id", "host").unwrap();
    let doc = h.document();
    let mut pending = vec![host];
    while let Some(node) = pending.pop() {
        let view = doc.get_node(node).unwrap();
        if matches!(&view.kind, NodeKindView::Text { content: c } if c == content) {
            return node;
        }
        pending.extend(view.children);
    }
    panic!("no text node shows {content:?}");
}

fn resolved(h: &Harness, node: NodeId) -> ResolvedStyle {
    h.document().resolved_style(node).unwrap()
}

fn named(value: Signal<String>) -> impl Fn() -> View + Send + Sync {
    move || view! { TextInput { value: value, placeholder: "Name".to_string() } }
}

#[test]
fn a_bordered_field_fills_its_container_and_shows_the_placeholder_while_empty() {
    let mut h = harness(surfaced(Surface::Bordered), named(state(String::new())));
    assert_eq!(h.row(0), "╭──────────╮");
    assert_eq!(h.row(1), "│ Name     │");
    assert_eq!(h.row(2), "╰──────────╯");

    let placeholder = text_node(&mut h, "Name");
    assert_eq!(
        resolved(&h, placeholder).color,
        literal(&Palette::dark().muted_foreground)
    );
}

#[test]
fn typing_writes_the_callers_signal_and_hides_the_placeholder() {
    let value = state(String::new());
    let mut h = harness(Theme::dark(), named(value));
    let (_, input) = nodes(&mut h);
    h.focus(input);
    h.type_text("Ada");
    assert_eq!(value.get(), "Ada");
    assert_eq!(h.row(0).trim_end(), " Ada");

    value.set("Grace".to_string());
    assert_eq!(h.row(0).trim_end(), " Grace");
    value.set(String::new());
    assert_eq!(h.row(0).trim_end(), " Name");
}

#[test]
fn a_masked_field_hides_its_value() {
    let value = state("pw".to_string());
    let mut h = harness(Theme::dark(), move || {
        view! { TextInput { value: value, mask: Some('•') } }
    });
    assert_eq!(h.row(0).trim_end(), " ••");
}

#[test]
fn a_long_value_scrolls_instead_of_widening_the_field() {
    let value = state(String::new());
    let mut h = harness(Theme::dark(), named(value));
    let (_, input) = nodes(&mut h);
    h.focus(input);
    h.type_text("abcdefghijkl");
    let row = h.row(0);
    assert!(row.starts_with(' '), "{row:?}");
    assert!(row.contains("ijkl") && !row.contains('a'), "{row:?}");
}

#[test]
fn clicking_the_frame_focuses_the_input() {
    let mut h = harness(Theme::dark(), named(state(String::new())));
    let (_, input) = nodes(&mut h);
    h.click(0, 0);
    assert_eq!(h.document().focused(), Some(input));
}

#[test]
fn focus_rings_the_edge() {
    let mut h = harness(surfaced(Surface::Bordered), named(state(String::new())));
    let (frame, input) = nodes(&mut h);
    let dark = Palette::dark();
    assert_eq!(resolved(&h, frame).border_color, Some(literal(&dark.input)));
    h.focus(input);
    assert_eq!(resolved(&h, frame).border_color, Some(literal(&dark.ring)));
}

#[test]
fn a_disabled_field_cannot_be_focused_or_edited() {
    let value = state("x".to_string());
    let mut h = harness(surfaced(Surface::Bordered), move || {
        view! { TextInput { value: value, disabled: true } }
    });
    let (frame, input) = nodes(&mut h);
    h.click(0, 1);
    h.click(2, 1);
    assert_eq!(h.document().focused(), None);
    h.type_text("y");
    assert_eq!(value.get(), "x");

    let dark = Palette::dark();
    assert_eq!(
        resolved(&h, frame).border_color,
        Some(literal(&dark.border))
    );
    assert_eq!(resolved(&h, input).color, literal(&dark.muted_foreground));
}

#[test]
fn unbordered_surfaces_fill_with_the_input_color() {
    for surface in [Surface::HalfBlock, Surface::Flat] {
        let mut h = harness(surfaced(surface), named(state(String::new())));
        let (frame, _) = nodes(&mut h);
        assert_eq!(
            resolved(&h, frame).background,
            Some(literal(&Palette::dark().input)),
            "{surface:?}"
        );
        assert_eq!(h.row(1).trim_end(), "", "{surface:?}");
        assert!(h.row(0).contains("Name"), "{surface:?}");
    }
}

#[test]
fn a_flat_field_tints_its_fill_toward_the_ring_while_focused() {
    let mut h = harness(surfaced(Surface::Flat), named(state(String::new())));
    let (_, input) = nodes(&mut h);
    let unfocused = h.cell(0, 0).unwrap().bg.unwrap();
    h.focus(input);
    let focused = h.cell(0, 0).unwrap().bg.unwrap();
    assert!(
        focused.r > unfocused.r,
        "{focused:?} focused, {unfocused:?} not"
    );
    assert_eq!(
        resolved(&h, input).color,
        literal(&Palette::dark().foreground)
    );
}

#[test]
fn the_style_prop_merges_over_the_frame() {
    let mut h = harness(surfaced(Surface::Bordered), || {
        let mut narrow = Style::new();
        narrow.width(Length::Cells(6));
        view! { TextInput { value: state(String::new()), style: narrow.clone() } }
    });
    assert_eq!(h.row(0).trim_end(), "╭────╮");
}

#[test]
fn a_stretched_field_keeps_its_text_on_the_middle_row() {
    let mut h = Harness::new(12, 5, || {
        install_theme(Theme::dark());
        view! { box { TextInput { value: state("hi".to_string()) } } }
    });
    assert_eq!(h.row(1).trim_end(), "");
    assert_eq!(h.row(2).trim_end(), " hi");
    assert_eq!(h.row(3).trim_end(), "");
}

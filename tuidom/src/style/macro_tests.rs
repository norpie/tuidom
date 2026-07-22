//! Tests for the `style!` macro.
//!
//! The macro lives in `tuidom-macros`, but what it has to produce is a `Style` that is
//! indistinguishable from a hand-built one — so the assertions belong next to `Style`.

use super::*;
use crate::Document;
use crate::headless::{HeadlessRuntime, ScreenRegion};
use crate::style;

#[test]
fn empty_is_a_default_style() {
    assert_eq!(style! {}, Style::new());
}

#[test]
fn expressions_reach_their_setters() {
    let built = style! {
        width: Length::Percent(100.0),
        height: Length::Cells(20),
        padding: EdgeInsets::symmetric(2, 1),
        color: Color::white(),
        opacity: 0.5,
        flex_direction: FlexDirection::Column,
        z_index: 3,
        stacking_context: true,
    };

    let mut expected = Style::new();
    expected.width(Length::Percent(100.0));
    expected.height(Length::Cells(20));
    expected.padding(EdgeInsets::symmetric(2, 1));
    expected.color(Color::white());
    expected.opacity(0.5);
    expected.flex_direction(FlexDirection::Column);
    expected.z_index(3);
    expected.stacking_context(true);

    assert_eq!(built, expected);
}

#[test]
fn inherit_and_unset_are_reachable() {
    let built = style! {
        color: inherit,
        background: unset,
    };

    assert_eq!(built.color, StyleValue::Inherit);
    assert_eq!(built.background, StyleValue::Unset);
}

/// `overflow` is a shorthand over both axes, so its state keywords have to reach both —
/// the engine gives it a setter but no `inherit_overflow`.
#[test]
fn overflow_shorthand_covers_both_axes() {
    let set = style! { overflow: Overflow::Scroll };
    assert_eq!(set.overflow_x, StyleValue::Set(Overflow::Scroll));
    assert_eq!(set.overflow_y, StyleValue::Set(Overflow::Scroll));

    let inherited = style! { overflow: inherit };
    assert_eq!(inherited.overflow_x, StyleValue::Inherit);
    assert_eq!(inherited.overflow_y, StyleValue::Inherit);
}

/// The pattern `style!` is replacing is clone-then-override, so the base has to survive.
#[test]
fn spread_copies_the_base() {
    let mut base = Style::new();
    base.color(Color::white());
    base.background(Color::blue());

    let derived = style! {
        ..base,
        background: Color::red(),
    };

    assert_eq!(derived.color, StyleValue::Set(Color::white()));
    assert_eq!(derived.background, StyleValue::Set(Color::red()));
    assert_eq!(base.background, StyleValue::Set(Color::blue()));
}

#[test]
fn color_variables_and_custom_properties() {
    let built = style! {
        --primary: Color::oklch(0.6, 0.2, 250.0),
        --surface-raised: Color::black(),
        --gray-100: Color::white(),
        "badge-kind": "warning",
    };

    assert_eq!(
        built.get_color_var("--primary"),
        Some(&Color::oklch(0.6, 0.2, 250.0))
    );
    assert_eq!(
        built.get_color_var("--surface-raised"),
        Some(&Color::black())
    );
    assert_eq!(built.get_color_var("--gray-100"), Some(&Color::white()));
    assert_eq!(built.get_custom("badge-kind"), Some("warning"));
}

#[test]
fn length_sugar() {
    let built = style! {
        width: 100%,
        height: 20,
        flex_basis: auto,
    };

    assert_eq!(built.width, StyleValue::Set(Length::Percent(100.0)));
    assert_eq!(built.height, StyleValue::Set(Length::Cells(20)));
    assert_eq!(built.flex_basis, StyleValue::Set(Length::Auto));

    let fractional = style! { width: 33.5% };
    assert_eq!(fractional.width, StyleValue::Set(Length::Percent(33.5)));
}

#[test]
fn edge_shorthand_follows_css_order() {
    assert_eq!(
        style! { padding: 1 }.padding,
        StyleValue::Set(EdgeInsets::all(1))
    );
    assert_eq!(
        style! { padding: 1 2 }.padding,
        StyleValue::Set(EdgeInsets::new(1, 2, 1, 2))
    );
    assert_eq!(
        style! { padding: 1 2 3 }.padding,
        StyleValue::Set(EdgeInsets::new(1, 2, 3, 2))
    );
    assert_eq!(
        style! { margin: 1 2 3 4 }.margin,
        StyleValue::Set(EdgeInsets::new(1, 2, 3, 4))
    );
}

#[test]
fn gap_shorthand() {
    assert_eq!(style! { gap: 2 }.gap, StyleValue::Set(FlexGap::all(2)));
    assert_eq!(style! { gap: 1 0 }.gap, StyleValue::Set(FlexGap::new(1, 0)));
}

#[test]
fn duration_units() {
    let built = style! {
        scrollbar_hide_delay: 300ms,
        scrollbar_fade_duration: 2s,
    };

    assert_eq!(
        built.scrollbar_hide_delay,
        StyleValue::Set(Duration::from_millis(300))
    );
    assert_eq!(
        built.scrollbar_fade_duration,
        StyleValue::Set(Duration::from_secs(2))
    );

    let fractional = style! { scrollbar_fade_duration: 1.5s };
    assert_eq!(
        fractional.scrollbar_fade_duration,
        StyleValue::Set(Duration::from_secs_f64(1.5))
    );
}

/// A bare numeric literal takes the property's own type, so `opacity: 1` is not an
/// integer-into-`f64` error.
#[test]
fn numeric_literals_take_the_property_type() {
    let built = style! {
        opacity: 1,
        flex_grow: 2,
        flex_shrink: 0,
        z_index: 5,
    };

    assert_eq!(built.opacity, StyleValue::Set(1.0));
    assert_eq!(built.flex_grow, StyleValue::Set(2.0));
    assert_eq!(built.flex_shrink, StyleValue::Set(0.0));
    assert_eq!(built.z_index, StyleValue::Set(5));
}

#[test]
fn color_bases() {
    let built = style! {
        color: white,
        background: --primary,
        border_color: oklch(0.6, 0.2, 250),
        selection_bg: current_bg,
        selection_fg: current_fg,
    };

    assert_eq!(built.color, StyleValue::Set(Color::white()));
    assert_eq!(built.background, StyleValue::Set(Color::var("--primary")));
    assert_eq!(
        built.border_color,
        StyleValue::Set(Color::oklch(0.6, 0.2, 250.0))
    );
    assert_eq!(built.selection_bg, StyleValue::Set(Color::CurrentBg));
    assert_eq!(built.selection_fg, StyleValue::Set(Color::CurrentFg));
}

#[test]
fn color_derivation_chains() {
    let built = style! {
        background: --primary.darken(0.1),
        color: current_bg.lighten(0.2).with_alpha(0.5),
        border_color: oklcha(0.6, 0.2, 250, 1).with_hue(120),
    };

    assert_eq!(
        built.background,
        StyleValue::Set(Color::var("--primary").darken(0.1))
    );
    assert_eq!(
        built.color,
        StyleValue::Set(Color::CurrentBg.lighten(0.2).with_alpha(0.5))
    );
    assert_eq!(
        built.border_color,
        StyleValue::Set(Color::oklcha(0.6, 0.2, 250.0, 1.0).with_hue(120.0))
    );
}

/// `mix` takes a color, so the sugar has to nest inside a derivation's arguments.
#[test]
fn colors_nest_in_derivation_arguments() {
    let built = style! {
        background: --primary.mix(--accent, 0.5),
        color: white.mix(black, 0.25),
    };

    assert_eq!(
        built.background,
        StyleValue::Set(Color::var("--primary").mix(Color::var("--accent"), 0.5))
    );
    assert_eq!(
        built.color,
        StyleValue::Set(Color::white().mix(Color::black(), 0.25))
    );
}

#[test]
fn color_variables_take_the_color_grammar() {
    let built = style! {
        --primary: oklch(0.6, 0.2, 250),
        --primary-dim: --primary.darken(0.1),
    };

    assert_eq!(
        built.get_color_var("--primary"),
        Some(&Color::oklch(0.6, 0.2, 250.0))
    );
    assert_eq!(
        built.get_color_var("--primary-dim"),
        Some(&Color::var("--primary").darken(0.1))
    );
}

/// Variant sugar resolves through the property's type, so the macro holds no list of
/// variant names and a new engine variant needs no change to it.
#[test]
fn enum_variants_from_bare_idents() {
    let built = style! {
        display: none,
        flex_direction: column_reverse,
        align_items: flex_start,
        justify_content: space_between,
        flex_wrap: wrap_reverse,
        overflow_y: scroll,
        cursor_shape: underline,
        scrollbar_show: when_focused,
    };

    assert_eq!(built.display, StyleValue::Set(Display::None));
    assert_eq!(
        built.flex_direction,
        StyleValue::Set(FlexDirection::ColumnReverse)
    );
    assert_eq!(built.align_items, StyleValue::Set(AlignItems::FlexStart));
    assert_eq!(
        built.justify_content,
        StyleValue::Set(JustifyContent::SpaceBetween)
    );
    assert_eq!(built.flex_wrap, StyleValue::Set(FlexWrap::WrapReverse));
    assert_eq!(built.overflow_y, StyleValue::Set(Overflow::Scroll));
    assert_eq!(built.cursor_shape, StyleValue::Set(CursorShape::Underline));
    assert_eq!(
        built.scrollbar_show,
        StyleValue::Set(ScrollbarShow::WhenFocused)
    );
}

#[test]
fn border_and_sides() {
    assert_eq!(
        style! { border: rounded }.border,
        StyleValue::Set(Border::new(BorderCharset::rounded()))
    );
    assert_eq!(
        style! { border: none }.border,
        StyleValue::Set(Border::none())
    );
    assert_eq!(
        style! { border: double top bottom }.border,
        StyleValue::Set(
            Border::new(BorderCharset::double()).with_sides(Sides::new(true, false, true, false))
        )
    );
    assert_eq!(
        style! { half_block_edges: all }.half_block_edges,
        StyleValue::Set(Sides::ALL)
    );
    assert_eq!(
        style! { half_block_edges: left right }.half_block_edges,
        StyleValue::Set(Sides::new(false, true, false, true))
    );
}

#[test]
fn scrollbar_charset_and_position() {
    assert_eq!(
        style! { scrollbar_charset: half_block }.scrollbar_charset,
        StyleValue::Set(ScrollbarCharset::half_block())
    );
    assert_eq!(
        style! { position: flow }.position,
        StyleValue::Set(Position::Flow)
    );
    assert_eq!(
        style! { position: absolute(15, -1) }.position,
        StyleValue::Set(Position::Absolute { x: 15, y: -1 })
    );
}

/// A parenthesized expression is the escape hatch from ident sugar.
#[test]
fn parenthesized_expression_is_never_sugar() {
    let column = FlexDirection::Row;
    let built = style! { flex_direction: (column) };
    assert_eq!(built.flex_direction, StyleValue::Set(FlexDirection::Row));
}

/// Paint a fixed scene — a panel with a border and a label, coloured through a variable
/// declared on the root — with whichever three styles it is handed.
fn painted(root: &Style, panel: &Style, label: &Style) -> ScreenRegion {
    let doc = Document::new().unwrap();

    let panel_node = doc.create_box().unwrap();
    let label_node = doc.create_text("Hi").unwrap();
    doc.append_child(panel_node, label_node).unwrap();
    doc.append_child(doc.root(), panel_node).unwrap();

    doc.set_style(doc.root(), root).unwrap();
    doc.set_style(panel_node, panel).unwrap();
    doc.set_style(label_node, label).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 12, 6);
    runtime.render().unwrap();
    runtime.get_screen_region(0, 0, 12, 6)
}

/// The roadmap's "against raw `Document` styling" check: what `style!` builds has to paint
/// the same cells as the builder calls it stands in for. Colour variables are in the scene
/// because they mean nothing until resolution, so a wrong variable name survives a
/// `PartialEq` on `Style` but not this.
#[test]
fn a_macro_built_style_paints_what_the_builder_paints() {
    let mut root = Style::new();
    root.width(Length::Percent(100.0));
    root.height(Length::Percent(100.0));
    root.padding(EdgeInsets::all(1));
    root.align_items(AlignItems::FlexStart);
    root.background(Color::oklch(0.3, 0.05, 250.0));
    root.color_var("--accent", Color::oklch(0.8, 0.15, 60.0));

    let mut panel = Style::new();
    panel.width(Length::Cells(8));
    panel.height(Length::Cells(3));
    panel.border(Border::new(BorderCharset::rounded()));
    panel.border_color(Color::var("--accent"));
    panel.justify_content(JustifyContent::Center);

    let mut label = Style::new();
    label.color(Color::var("--accent").darken(0.2));
    label.bold(true);

    let macro_root = style! {
        --accent: oklch(0.8, 0.15, 60),
        width: 100%,
        height: 100%,
        padding: 1,
        align_items: flex_start,
        background: oklch(0.3, 0.05, 250),
    };
    let macro_panel = style! {
        width: 8,
        height: 3,
        border: rounded,
        border_color: --accent,
        justify_content: center,
    };
    let macro_label = style! {
        color: --accent.darken(0.2),
        bold: true,
    };

    assert_eq!(macro_root, root);
    assert_eq!(macro_panel, panel);
    assert_eq!(macro_label, label);
    assert_eq!(
        painted(&macro_root, &macro_panel, &macro_label),
        painted(&root, &panel, &label)
    );
}

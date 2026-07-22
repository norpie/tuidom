//! Smoke test exercising the full pipeline:
//! Box + Text/Input rendering, keyboard events, focus, focus styles,
//! configurable focus keys, targeted mouse events, bubbling, stop_propagation,
//! wheel events, borders, half-block edges, color variables and derivations,
//! scrolling with overlay scrollbars, terminal text attributes, transitions,
//! and the performance metrics API surfaced through the post-frame event.
//!
//! The terminal is also restored by a panic hook, so a crash cannot strand the
//! user in the alternate screen with raw mode still on.
//!
//! Tab / Shift-Tab      — move focus in DOM order
//! Focus the "focus me" panel — its border recolors, charset and sides untouched
//! Arrows / hjkl        — move focus spatially, or move input cursor
//! Esc                  — blur focused node; press again in the modal to close it
//! Hover buttons/input  — focus node
//! Type in inputs       — edit text / masked password input; the status line tracks the
//!                        value via one bubbled `on_input` listener on the row
//! Space outside input  — toggle first button opacity (fade in/out)
//! Click first button   — toggle button background, stop propagation
//! Click background     — toggle container background
//! Wheel anywhere       — scroll the page (the center area is a scroll container)
//! Wheel over 1st button — adjust its opacity; prevent_default keeps the page still
//! Wheel over the list  — scroll it; its bar tracks the offset
//! Drag a scrollbar     — thumb follows the cursor; pressing the track jumps it
//! Wheel the auto-hide pane — its bar appears while scrolling, then fades away
//! Wheel over the 10k pane — virtualized: only the visible window exists in the DOM
//! Drag over text      — select it (drag in an input selects within the input);
//!                        the two selection columns are separate boundaries, and the
//!                        status line echoes get_selection()
//! Animations section   — a frames-node spinner and an infinite keyframe pulse
//!                        run unpaced (set_animation_fps(None)) as a stress test;
//!                        hover the "hover me" chip — its background fades via a
//!                        transition triggered by the focus pseudo-state
//! b outside input      — ring the terminal bell (what that does is the
//!                        terminal's choice: a sound, a flash, or nothing)
//! Focus another window — the status line tracks it; DOM focus is untouched
//! [ / ] outside input  — scroll the horizontal pane
//! m outside input      — open the modal: focus is trapped inside it and the
//!                        content behind it goes inert (no tab, hover, or clicks)
//! q outside input      — quit

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tuidom::animation::{
    AnimatableProperty, AnimationDirection, Easing, KeyframeAnimation, TransitionConfig,
    TransitionProperty,
};
use tuidom::event::{FocusEventRelation, FocusKeys, KeyCode};
use tuidom::style::{
    AlignItems, Border, BorderCharset, Color, CursorShape, Display, EdgeInsets, FlexDirection,
    FlexGap, JustifyContent, Length, Overflow, Position, ScrollbarCharset, ScrollbarShow, Sides,
    Style,
};
use tuidom::virtualize::Virtualizer;
use tuidom::{Document, NodeId};

fn init_logging() {
    // Best-effort file logging for the smoke test.
    if let Ok(file) = std::fs::File::create("/tmp/tuidom.log") {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .try_init();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let doc = tuidom::Document::new()?;

    // --- Styles -------------------------------------------------------

    let mut container_style = Style::new();
    container_style.width(Length::Percent(100.0));
    container_style.height(Length::Percent(100.0));
    container_style.flex_direction(FlexDirection::Column);
    container_style.background(Color::oklch(0.3, 0.1, 50.0));

    let mut top_bar_style = Style::new();
    top_bar_style.width(Length::Percent(100.0));
    top_bar_style.height(Length::Auto);
    top_bar_style.flex_direction(FlexDirection::Row);
    top_bar_style.justify_content(JustifyContent::FlexEnd);

    // The page viewport: scrolls vertically when the section stack outgrows the
    // terminal. Cross-axis alignment stays at the start — centering an overflowing
    // stack would push its top above the reachable scroll range.
    let mut center_area_style = Style::new();
    center_area_style.width(Length::Percent(100.0));
    center_area_style.flex_grow(1.0);
    center_area_style.justify_content(JustifyContent::Center);
    center_area_style.align_items(AlignItems::FlexStart);
    center_area_style.overflow_y(Overflow::Scroll);

    let mut stack_style = Style::new();
    stack_style.width(Length::Auto);
    stack_style.height(Length::Auto);
    stack_style.flex_direction(FlexDirection::Column);
    stack_style.align_items(AlignItems::Center);
    stack_style.gap(FlexGap::new(1, 0));

    let mut row_style = Style::new();
    row_style.width(Length::Auto);
    row_style.height(Length::Auto);
    row_style.flex_direction(FlexDirection::Row);
    row_style.align_items(AlignItems::Center);
    row_style.gap(FlexGap::new(0, 4));

    let mut field_style = Style::new();
    field_style.width(Length::Auto);
    field_style.height(Length::Auto);
    field_style.flex_direction(FlexDirection::Column);
    field_style.align_items(AlignItems::FlexStart);
    field_style.gap(FlexGap::new(1, 0));

    let mut label_style = Style::new();
    label_style.width(Length::Auto);
    label_style.height(Length::Auto);
    label_style.color(Color::oklch(0.92, 0.04, 260.0));

    let mut section_label_style = label_style.clone();
    section_label_style.color(Color::oklch(0.82, 0.12, 80.0));

    let mut title_style = label_style.clone();
    title_style.color(Color::white());
    title_style.background(Color::oklch(0.22, 0.08, 260.0));

    let mut text_style = Style::new();
    text_style.width(Length::Auto);
    text_style.height(Length::Auto);
    text_style.color(Color::white());
    text_style.background(Color::blue());

    let mut secondary_text_style = text_style.clone();
    secondary_text_style.background(Color::oklch(0.35, 0.12, 250.0));

    let mut focus_style = Style::new();
    focus_style.color(Color::black());
    focus_style.background(Color::yellow());

    // Anchored to the button's own box, so it follows the button as the layout
    // recenters, and overhangs the button's bounds instead of being clipped to them.
    let mut badge_style = Style::new();
    badge_style.position(Position::Absolute { x: 15, y: -1 });
    badge_style.color(Color::black());
    badge_style.background(Color::oklch(0.75, 0.19, 40.0));

    let mut active_style = Style::new();
    active_style.color(Color::white());
    active_style.background(Color::oklch(0.55, 0.2, 25.0));

    let mut disabled_style = Style::new();
    disabled_style.color(Color::oklch(0.55, 0.0, 260.0));
    disabled_style.background(Color::oklch(0.25, 0.0, 260.0));

    // The modal layer is a stacking context, which is what lets it trap focus. Its
    // translucent fill blends with the UI behind it instead of hiding it.
    let mut modal_layer_style = Style::new();
    modal_layer_style.stacking_context(true);
    modal_layer_style.z_index(10);
    modal_layer_style.display(Display::None);
    modal_layer_style.position(Position::Absolute { x: 0, y: 0 });
    modal_layer_style.width(Length::Percent(100.0));
    modal_layer_style.height(Length::Percent(100.0));
    modal_layer_style.background(Color::oklcha(0.15, 0.03, 260.0, 0.6));
    modal_layer_style.justify_content(JustifyContent::Center);
    modal_layer_style.align_items(AlignItems::Center);

    let mut dialog_style = Style::new();
    dialog_style.width(Length::Auto);
    dialog_style.height(Length::Auto);
    dialog_style.flex_direction(FlexDirection::Column);
    dialog_style.align_items(AlignItems::Center);
    dialog_style.gap(FlexGap::new(1, 0));
    dialog_style.padding(EdgeInsets::all(1));
    dialog_style.background(Color::oklch(0.28, 0.06, 280.0));
    dialog_style.border(Border::new(BorderCharset::rounded()));
    dialog_style.border_color(Color::oklch(0.8, 0.1, 280.0));

    let mut input_style = Style::new();
    input_style.width(Length::Cells(24));
    input_style.height(Length::Cells(1));
    input_style.color(Color::white());
    input_style.background(Color::oklch(0.18, 0.04, 260.0));
    input_style.cursor_shape(CursorShape::Bar);

    let mut password_style = input_style.clone();
    password_style.cursor_shape(CursorShape::Block);

    // --- DOM ----------------------------------------------------------

    let container = doc.create_box()?;
    doc.set_style(container, &container_style)?;

    let top_bar = doc.create_box()?;
    doc.set_style(top_bar, &top_bar_style)?;

    let center_area = doc.create_box()?;
    doc.set_style(center_area, &center_area_style)?;

    let stack = doc.create_box()?;
    doc.set_style(stack, &stack_style)?;

    let button_row = doc.create_box()?;
    doc.set_style(button_row, &row_style)?;

    let input_row = doc.create_box()?;
    doc.set_style(input_row, &row_style)?;

    let editable_field = doc.create_box()?;
    doc.set_style(editable_field, &field_style)?;

    let password_field = doc.create_box()?;
    doc.set_style(password_field, &field_style)?;

    let perf_counter = doc.create_text("FPS: --  Frame: --")?;
    let mut perf_style = Style::new();
    perf_style.color(Color::oklch(0.85, 0.16, 145.0));
    perf_style.background(Color::black());
    doc.set_style(perf_counter, &perf_style)?;

    let title = doc.create_text(" tuidom interaction demo ")?;
    doc.set_style(title, &title_style)?;

    let hint =
        doc.create_text("Tab/Shift-Tab or arrows/hjkl move focus; q quits outside inputs")?;
    doc.set_style(hint, &label_style)?;

    let button_label = doc.create_text("Buttons")?;
    doc.set_style(button_label, &section_label_style)?;

    let toggle_button = doc.create_box()?;
    let mut toggle_button_style = Style::new();
    toggle_button_style.width(Length::Auto);
    toggle_button_style.height(Length::Auto);
    doc.set_style(toggle_button, &toggle_button_style)?;

    let text = doc.create_text("  Toggle opacity  ")?;
    doc.set_style(text, &text_style)?;
    doc.set_focusable(text, true)?;
    doc.set_focus_style(text, &focus_style)?;
    doc.set_active_style(text, &active_style)?;

    let badge = doc.create_text(" 3 ")?;
    doc.set_style(badge, &badge_style)?;

    let second = doc.create_text("  Focus target  ")?;
    doc.set_style(second, &secondary_text_style)?;
    doc.set_focusable(second, true)?;
    doc.set_focus_style(second, &focus_style)?;
    doc.set_active_style(second, &active_style)?;

    // Focusable, but disabled — so tab, arrows, and clicks all skip it.
    let disabled = doc.create_text("  Disabled  ")?;
    doc.set_style(disabled, &secondary_text_style)?;
    doc.set_focusable(disabled, true)?;
    doc.set_focus_style(disabled, &focus_style)?;
    doc.set_disabled_style(disabled, &disabled_style)?;
    doc.set_disabled(disabled, true)?;

    let input_label = doc.create_text("Inputs")?;
    doc.set_style(input_label, &section_label_style)?;

    let editable_label = doc.create_text("Editable")?;
    doc.set_style(editable_label, &label_style)?;

    let password_label = doc.create_text("Password")?;
    doc.set_style(password_label, &label_style)?;

    let editable = doc.create_input("edit me")?;
    doc.set_style(editable, &input_style)?;
    doc.set_focus_style(editable, &focus_style)?;

    let password = doc.create_input("secret")?;
    doc.set_style(password, &password_style)?;
    doc.set_input_mask(password, Some('•'))?;
    doc.set_focus_style(password, &focus_style)?;

    doc.append_child(top_bar, perf_counter)?;

    doc.append_child(toggle_button, text)?;
    doc.append_child(toggle_button, badge)?;
    doc.append_child(button_row, toggle_button)?;
    doc.append_child(button_row, second)?;
    doc.append_child(button_row, disabled)?;

    doc.append_child(editable_field, editable_label)?;
    doc.append_child(editable_field, editable)?;
    doc.append_child(password_field, password_label)?;
    doc.append_child(password_field, password)?;
    doc.append_child(input_row, editable_field)?;
    doc.append_child(input_row, password_field)?;

    // --- Borders and text attributes ----------------------------------

    // A bordered panel sizes to its content plus one cell per drawn side, because the border
    // is real layout, not decoration painted over the box.
    let mut panel_style = Style::new();
    panel_style.width(Length::Auto);
    panel_style.height(Length::Auto);
    panel_style.padding(EdgeInsets::symmetric(1, 0));

    let border_label = doc.create_text("Borders")?;
    doc.set_style(border_label, &section_label_style)?;

    let border_row = doc.create_box()?;
    doc.set_style(border_row, &row_style)?;

    for (name, charset) in [
        ("single", BorderCharset::single()),
        ("double", BorderCharset::double()),
        ("rounded", BorderCharset::rounded()),
        ("thick", BorderCharset::thick()),
        ("ascii", BorderCharset::ascii()),
    ] {
        let panel = doc.create_box()?;
        let mut style = panel_style.clone();
        style.border(Border::new(charset));
        style.border_color(Color::oklch(0.72, 0.11, 200.0));
        doc.set_style(panel, &style)?;

        let panel_label = doc.create_text(name)?;
        doc.set_style(panel_label, &label_style)?;
        doc.append_child(panel, panel_label)?;
        doc.append_child(border_row, panel)?;
    }

    // The focus style recolors the border and nothing else — it never restates the charset
    // or the sides, because border_color is its own property.
    let focus_panel = doc.create_box()?;
    let mut focus_panel_style = panel_style.clone();
    focus_panel_style.border(Border::new(BorderCharset::rounded()));
    focus_panel_style.border_color(Color::oklch(0.55, 0.02, 260.0));
    doc.set_style(focus_panel, &focus_panel_style)?;
    doc.set_focusable(focus_panel, true)?;

    let mut focus_panel_focus_style = Style::new();
    focus_panel_focus_style.border_color(Color::oklch(0.85, 0.2, 90.0));
    doc.set_focus_style(focus_panel, &focus_panel_focus_style)?;

    let focus_panel_label = doc.create_text("focus me")?;
    doc.set_style(focus_panel_label, &label_style)?;
    doc.append_child(focus_panel, focus_panel_label)?;
    doc.append_child(border_row, focus_panel)?;

    // Per-side control is presence, not width: a top-only border is a horizontal rule.
    let separator = doc.create_box()?;
    let mut separator_style = Style::new();
    separator_style.width(Length::Percent(100.0));
    separator_style.height(Length::Cells(1));
    separator_style.border(
        Border::new(BorderCharset::single()).with_sides(Sides::new(true, false, false, false)),
    );
    separator_style.border_color(Color::oklch(0.5, 0.02, 260.0));
    doc.set_style(separator, &separator_style)?;

    // A terminal cell is about twice as tall as it is wide, so one cell of vertical padding
    // reads as two cells of horizontal padding. Half-block edges end the fill halfway into its
    // own outermost row, which is what balances the two. Both panels below have the same fill
    // and the same padding — only the second one ends on a half cell.
    let half_block_label = doc.create_text("Half-block edges")?;
    doc.set_style(half_block_label, &section_label_style)?;

    let half_block_row = doc.create_box()?;
    doc.set_style(half_block_row, &row_style)?;

    let mut chip_style = Style::new();
    chip_style.width(Length::Auto);
    chip_style.height(Length::Auto);
    chip_style.padding(EdgeInsets::symmetric(2, 1));
    chip_style.background(Color::oklch(0.45, 0.13, 250.0));

    for (name, edges) in [
        ("squared", Sides::NONE),
        ("half-block", Sides::new(true, false, true, false)),
    ] {
        let chip = doc.create_box()?;
        let mut style = chip_style.clone();
        style.half_block_edges(edges);
        doc.set_style(chip, &style)?;

        let chip_label = doc.create_text(name)?;
        doc.set_style(chip_label, &label_style)?;
        doc.append_child(chip, chip_label)?;
        doc.append_child(half_block_row, chip)?;
    }

    // One variable, four chips. Each derives its fill from `--brand` rather than restating a
    // color, so recoloring the whole row is a one-line change to the variable. The labels name no
    // color at all — each lifts its text off whatever fill its own chip resolved to.
    doc.set_color_var("--brand", Color::oklch(0.55, 0.15, 265.0));

    let derived_label = doc.create_text("Color variables and derivations")?;
    doc.set_style(derived_label, &section_label_style)?;

    let derived_row = doc.create_box()?;
    doc.set_style(derived_row, &row_style)?;

    for (name, fill) in [
        ("--brand", Color::var("--brand")),
        ("darken", Color::var("--brand").darken(0.2)),
        ("desaturated", Color::var("--brand").with_chroma(0.02)),
        (
            "mixed",
            Color::var("--brand").mix(Color::oklch(0.7, 0.16, 25.0), 0.5),
        ),
    ] {
        let chip = doc.create_box()?;
        let mut style = chip_style.clone();
        style.background(fill);
        style.half_block_edges(Sides::new(true, false, true, false));
        doc.set_style(chip, &style)?;

        let chip_label = doc.create_text(name)?;
        let mut text_style = label_style.clone();
        // Lifted far enough off whatever fill this chip ended up with to stay legible.
        text_style.color(Color::CurrentBg.lighten(0.5).with_chroma(0.02));
        doc.set_style(chip_label, &text_style)?;

        doc.append_child(chip, chip_label)?;
        doc.append_child(derived_row, chip)?;
    }

    // --- Scrolling ------------------------------------------------------

    // Two overlay-scrollbar panes: layout is scroll-invariant, so wheeling them repaints
    // without reflowing anything. The bars cost no cells — they overlay the last column
    // and row of each viewport.
    let scroll_label = doc.create_text("Scrolling (wheel over the list)")?;
    doc.set_style(scroll_label, &section_label_style)?;

    let scroll_row = doc.create_box()?;
    doc.set_style(scroll_row, &row_style)?;

    let list = doc.create_box()?;
    let mut list_style = Style::new();
    list_style.flex_direction(FlexDirection::Column);
    list_style.height(Length::Cells(5));
    list_style.overflow_y(Overflow::Scroll);
    list_style.border(Border::new(BorderCharset::single()));
    list_style.border_color(Color::oklch(0.55, 0.02, 260.0));
    list_style.padding(EdgeInsets::new(0, 1, 0, 1));
    doc.set_style(list, &list_style)?;

    for index in 1..=12 {
        let item = doc.create_text(format!("scrollable item {index:02}"))?;
        doc.set_style(item, &label_style)?;
        doc.append_child(list, item)?;
    }

    let scroll_status = doc.create_text("offset 0")?;
    doc.set_style(scroll_status, &label_style)?;
    {
        let doc_for_scroll = doc.clone();
        let status = scroll_status;
        doc.on_scroll(list, move |event| {
            let _ = doc_for_scroll.set_text_content(status, format!("offset {}", event.y));
        })?;
    }
    // The default scroll still runs; stopping propagation only demonstrates that
    // ancestor wheel listeners never see the event.
    doc.on_wheel(list, |event| event.stop_propagation())?;

    let wide = doc.create_box()?;
    let mut wide_style = Style::new();
    wide_style.width(Length::Cells(24));
    wide_style.overflow_x(Overflow::Scroll);
    wide_style.scrollbar_charset(ScrollbarCharset::half_block());
    // The bar overlays the viewport's bottom row, and without this the pane is one row
    // tall — the bar would sit on the text itself. A row of bottom padding gives the bar
    // a row the content never uses, the same way the list's side padding hosts its bar.
    wide_style.padding(EdgeInsets::new(0, 0, 1, 0));
    doc.set_style(wide, &wide_style)?;

    let wide_text =
        doc.create_text("a single long line that scrolls horizontally under a half-block bar")?;
    doc.set_style(wide_text, &label_style)?;
    doc.append_child(wide, wide_text)?;
    doc.on_wheel(wide, |event| event.stop_propagation())?;

    // An auto-hiding pane: the bar exists only around scroll activity — it appears on
    // offset changes (and while grabbed), holds briefly, then fades out by alpha. While
    // hidden the pane repaints nothing and the document is as passive as ever.
    let auto_hide = doc.create_box()?;
    let mut auto_hide_style = Style::new();
    auto_hide_style.flex_direction(FlexDirection::Column);
    auto_hide_style.height(Length::Cells(5));
    auto_hide_style.overflow_y(Overflow::Scroll);
    auto_hide_style.scrollbar_show(ScrollbarShow::WhenScrolling);
    auto_hide_style.scrollbar_hide_delay(Duration::from_millis(800));
    auto_hide_style.scrollbar_fade_duration(Duration::from_millis(300));
    auto_hide_style.border(Border::new(BorderCharset::single()));
    auto_hide_style.border_color(Color::oklch(0.55, 0.02, 260.0));
    auto_hide_style.padding(EdgeInsets::new(0, 1, 0, 1));
    doc.set_style(auto_hide, &auto_hide_style)?;

    for index in 1..=12 {
        let item = doc.create_text(format!("auto-hide item {index:02}"))?;
        doc.set_style(item, &label_style)?;
        doc.append_child(auto_hide, item)?;
    }
    doc.on_wheel(auto_hide, |event| event.stop_propagation())?;

    // A virtualized pane: 10,000 rows, but the DOM only ever holds the visible window
    // plus overscan between two spacers. The spacers keep the container's content size
    // at the true total, so the scrollbar and wheel routing need nothing special.
    let virtual_status = doc.create_text("top row 0")?;
    doc.set_style(virtual_status, &label_style)?;

    let virtual_pane = Arc::new(Mutex::new(VirtualPane::build(&doc, 10_000, &label_style)?));
    let virtual_container = {
        let mut pane = virtual_pane.lock().unwrap();
        // Three content rows: the pane is five cells tall inside a one-cell border.
        pane.apply(0, 3);
        pane.container
    };
    {
        let doc_for_virtual = doc.clone();
        let pane_for_scroll = virtual_pane.clone();
        doc.on_scroll(virtual_container, move |event| {
            if let Ok(mut pane) = pane_for_scroll.lock() {
                pane.apply(event.y, 3);
            }
            let _ =
                doc_for_virtual.set_text_content(virtual_status, format!("top row {}", event.y));
        })?;
    }
    doc.on_wheel(virtual_container, |event| event.stop_propagation())?;

    doc.append_child(scroll_row, list)?;
    doc.append_child(scroll_row, scroll_status)?;
    doc.append_child(scroll_row, wide)?;
    doc.append_child(scroll_row, auto_hide)?;
    doc.append_child(scroll_row, virtual_container)?;
    doc.append_child(scroll_row, virtual_status)?;

    // --- Text selection ------------------------------------------------

    let selection_label = doc.create_text("Selection (drag to select; boundaries don't bleed)")?;
    doc.set_style(selection_label, &section_label_style)?;

    let selection_row = doc.create_box()?;
    doc.set_style(selection_row, &row_style)?;

    // Two columns, each its own selection boundary: a drag started in one cannot
    // bleed into the other. The left one keeps the reverse-video default; the right
    // one sets explicit selection colors.
    let mut boundary_style = Style::new();
    boundary_style.flex_direction(FlexDirection::Column);
    boundary_style.selection_boundary(true);
    boundary_style.padding(EdgeInsets::symmetric(1, 0));
    boundary_style.background(Color::oklch(0.24, 0.05, 260.0));

    let mut styled_boundary_style = boundary_style.clone();
    styled_boundary_style.selection_bg(Color::oklch(0.75, 0.15, 145.0));
    styled_boundary_style.selection_fg(Color::black());

    let left_boundary = doc.create_box()?;
    doc.set_style(left_boundary, &boundary_style)?;
    let right_boundary = doc.create_box()?;
    doc.set_style(right_boundary, &styled_boundary_style)?;

    for line in [
        "reverse video default",
        "drag across these",
        "lines to select",
    ] {
        let text = doc.create_text(line)?;
        doc.set_style(text, &label_style)?;
        doc.append_child(left_boundary, text)?;
    }
    for line in [
        "explicit colors here",
        "inherited from the",
        "boundary via style",
    ] {
        let text = doc.create_text(line)?;
        let mut style = label_style.clone();
        style.inherit_selection_bg();
        style.inherit_selection_fg();
        doc.set_style(text, &style)?;
        doc.append_child(right_boundary, text)?;
    }

    let selection_status = doc.create_text("selection: (none)")?;
    doc.set_style(selection_status, &label_style)?;

    {
        let doc_for_selection = doc.clone();
        doc.on_selection_change(move |event| {
            let summary = match (&event.selection, doc_for_selection.get_selection()) {
                (Some(_), Some(text)) => {
                    format!("selection: {:?}", text)
                }
                _ => "selection: (none)".to_owned(),
            };
            let _ = doc_for_selection.set_text_content(selection_status, summary);
        });
    }

    let input_status = doc.create_text("input: \"edit me\"")?;
    doc.set_style(input_status, &label_style)?;

    // Registered on the row rather than on either input: on_input bubbles, so one
    // listener covers both fields. The masked field reports its real value — masking is
    // a render concern, not a state one.
    {
        let d = doc.clone();
        doc.on_input(input_row, move |event| {
            let which = if event.target() == password {
                "password"
            } else {
                "input"
            };
            let _ = d.set_text_content(input_status, format!("{which}: {:?}", event.value));
        })?;
    }

    let window_status = doc.create_text("window: focused")?;
    doc.set_style(window_status, &label_style)?;

    // Window focus is the OS window, not the DOM: alt-tabbing away leaves the
    // focused node exactly where it was.
    {
        let d = doc.clone();
        doc.on_window_focus(move |_| {
            let _ = d.set_text_content(window_status, "window: focused");
        });
        let d = doc.clone();
        doc.on_window_blur(move |_| {
            let _ = d.set_text_content(window_status, "window: unfocused");
        });
    }

    doc.append_child(selection_row, left_boundary)?;
    doc.append_child(selection_row, right_boundary)?;

    // --- Animations -----------------------------------------------------

    let animation_label = doc.create_text("Animations")?;
    doc.set_style(animation_label, &section_label_style)?;

    let animation_row = doc.create_box()?;
    doc.set_style(animation_row, &row_style)?;

    // A frames node: content-based animation, self-paced at its own interval
    // rather than the animation tick rate.
    let spinner = doc.create_frames(
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        Duration::from_millis(80),
    )?;
    doc.set_style(spinner, &label_style)?;

    let spinner_caption = doc.create_text("spinner")?;
    doc.set_style(spinner_caption, &label_style)?;

    // An infinite alternate keyframe pulse — the reason the FPS counter never
    // reads an idle renderer while this section is on screen.
    let pulse = doc.create_text("  pulse  ")?;
    let mut pulse_style = Style::new();
    pulse_style.color(Color::white());
    pulse_style.background(Color::oklch(0.55, 0.18, 300.0));
    doc.set_style(pulse, &pulse_style)?;
    doc.animate(
        pulse,
        KeyframeAnimation::from_to(
            Duration::from_millis(900),
            [AnimatableProperty::Opacity(1.0)],
            [AnimatableProperty::Opacity(0.35)],
        )
        .easing(Easing::EaseInOut)
        .direction(AnimationDirection::Alternate)
        .infinite(),
    )?;

    // A background transition driven by a pseudo-state change: hovering focuses
    // the chip, and the focus style's background fades in over 300ms (in OKLCH).
    let hover_fade = doc.create_text("  hover me  ")?;
    let mut hover_base = Style::new();
    hover_base.color(Color::white());
    hover_base.background(Color::oklch(0.35, 0.05, 220.0));
    doc.set_style(hover_fade, &hover_base)?;
    doc.set_focusable(hover_fade, true)?;
    let mut hover_focus = Style::new();
    hover_focus.background(Color::oklch(0.6, 0.16, 220.0));
    doc.set_focus_style(hover_fade, &hover_focus)?;
    doc.set_transition(
        hover_fade,
        TransitionConfig::new(
            TransitionProperty::Background,
            Duration::from_millis(300),
            Easing::EaseOut,
        ),
    )?;

    doc.append_child(animation_row, spinner)?;
    doc.append_child(animation_row, spinner_caption)?;
    doc.append_child(animation_row, pulse)?;
    doc.append_child(animation_row, hover_fade)?;

    let attrs_label = doc.create_text("Text attributes")?;
    doc.set_style(attrs_label, &section_label_style)?;

    let attrs_row = doc.create_box()?;
    doc.set_style(attrs_row, &row_style)?;

    for (name, bold, italic, underline) in [
        ("bold", true, false, false),
        ("italic", false, true, false),
        ("underline", false, false, true),
        ("all three", true, true, true),
    ] {
        let sample = doc.create_text(name)?;
        let mut style = label_style.clone();
        style.bold(bold);
        style.italic(italic);
        style.underline(underline);
        doc.set_style(sample, &style)?;
        doc.append_child(attrs_row, sample)?;
    }

    doc.append_child(stack, title)?;
    doc.append_child(stack, hint)?;
    doc.append_child(stack, button_label)?;
    doc.append_child(stack, button_row)?;
    doc.append_child(stack, input_label)?;
    doc.append_child(stack, input_row)?;
    doc.append_child(stack, separator)?;
    doc.append_child(stack, border_label)?;
    doc.append_child(stack, border_row)?;
    doc.append_child(stack, half_block_label)?;
    doc.append_child(stack, half_block_row)?;
    doc.append_child(stack, derived_label)?;
    doc.append_child(stack, derived_row)?;
    doc.append_child(stack, scroll_label)?;
    doc.append_child(stack, scroll_row)?;
    doc.append_child(stack, selection_label)?;
    doc.append_child(stack, selection_row)?;
    doc.append_child(stack, selection_status)?;
    doc.append_child(stack, input_status)?;
    doc.append_child(stack, window_status)?;
    doc.append_child(stack, animation_label)?;
    doc.append_child(stack, animation_row)?;
    doc.append_child(stack, attrs_label)?;
    doc.append_child(stack, attrs_row)?;

    // --- Modal --------------------------------------------------------

    let modal_layer = doc.create_box()?;
    doc.set_style(modal_layer, &modal_layer_style)?;

    let dialog = doc.create_box()?;
    doc.set_style(dialog, &dialog_style)?;

    let modal_title = doc.create_text(" Focus is trapped in here ")?;
    doc.set_style(modal_title, &title_style)?;

    let modal_hint = doc.create_text("Tab cycles these two only. Esc twice closes.")?;
    doc.set_style(modal_hint, &label_style)?;

    let modal_buttons = doc.create_box()?;
    doc.set_style(modal_buttons, &row_style)?;

    let confirm = doc.create_text("  Confirm  ")?;
    doc.set_style(confirm, &text_style)?;
    doc.set_focusable(confirm, true)?;
    doc.set_focus_style(confirm, &focus_style)?;
    doc.set_active_style(confirm, &active_style)?;

    let cancel = doc.create_text("  Cancel  ")?;
    doc.set_style(cancel, &secondary_text_style)?;
    doc.set_focusable(cancel, true)?;
    doc.set_focus_style(cancel, &focus_style)?;
    doc.set_active_style(cancel, &active_style)?;

    doc.append_child(modal_buttons, confirm)?;
    doc.append_child(modal_buttons, cancel)?;
    doc.append_child(dialog, modal_title)?;
    doc.append_child(dialog, modal_hint)?;
    doc.append_child(dialog, modal_buttons)?;
    doc.append_child(modal_layer, dialog)?;

    doc.append_child(center_area, stack)?;
    doc.append_child(container, top_bar)?;
    doc.append_child(container, center_area)?;
    doc.append_child(container, modal_layer)?;
    doc.append_child(doc.root(), container)?;

    let mut focus_keys = FocusKeys::default();
    focus_keys.up.push(KeyCode::Char('k'));
    focus_keys.down.push(KeyCode::Char('j'));
    focus_keys.left.push(KeyCode::Char('h'));
    focus_keys.right.push(KeyCode::Char('l'));
    doc.set_focus_keys(focus_keys);

    // --- Transition config — opacity changes animate over 1000ms -------

    doc.set_transition(
        text,
        TransitionConfig::opacity(Duration::from_millis(1000), Easing::EaseInOut),
    )?;

    // Animations drive frames unpaced on purpose: the FPS counter doubles as a
    // stress readout, so the demo removes the default ~60fps animation tick.
    doc.set_animation_fps(None);

    // --- Shared state -------------------------------------------------

    let opacity_visible = Arc::new(AtomicBool::new(true));
    let text_background_alt = Arc::new(AtomicBool::new(false));
    let container_background_alt = Arc::new(AtomicBool::new(false));
    let text_opacity = Arc::new(Mutex::new(1.0));

    // --- Performance counter -------------------------------------------

    // The counter reflects the frame that just finished — one frame of latency,
    // honestly labeled. Rewriting its text schedules another frame (whose own
    // post-frame event fires in turn), so the rewrite is throttled: without it
    // the counter would keep the renderer permanently active.
    let d = doc.clone();
    let last_perf_update = Mutex::new(None::<Instant>);
    doc.on_post_frame(move |event| {
        let Ok(mut last) = last_perf_update.lock() else {
            return;
        };
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < Duration::from_millis(250)) {
            return;
        }
        *last = Some(now);

        let text = format!(
            "FPS: {:.0}  Frame: {:.3}ms",
            event.fps,
            event.metrics.frame_time.as_secs_f64() * 1000.0
        );
        let _ = d.set_text_content(perf_counter, text);
    });

    // --- Keyboard handler --------------------------------------------

    let d = doc.clone();
    let ov = opacity_visible.clone();
    let opacity_for_key = text_opacity.clone();

    doc.on_key_press(doc.root(), move |key| {
        if matches!(d.focused(), Some(node) if node == editable || node == password) {
            return;
        }

        match key.code {
            KeyCode::Char(' ') => {
                let was_visible = ov.fetch_not(Ordering::Relaxed);
                let target = if !was_visible { 1.0 } else { 0.0 };
                if let Ok(mut opacity) = opacity_for_key.lock() {
                    *opacity = target;
                }
                let _ = d.update_style(text, |s| s.opacity(target));
            }
            KeyCode::Char('m') => {
                let _ = d.update_style(modal_layer, |s| s.display(Display::Flex));
                let _ = d.push_focus_context(modal_layer);
            }
            KeyCode::Char('[') => {
                let _ = d.scroll_by(wide, -4, 0);
            }
            KeyCode::Char(']') => {
                let _ = d.scroll_by(wide, 4, 0);
            }
            KeyCode::Char('b') => d.bell(),
            KeyCode::Char('q') => d.quit(),
            _ => {}
        }
    })?;

    // --- Modal handlers ------------------------------------------------

    let close_modal = {
        let d = doc.clone();
        move || {
            let _ = d.pop_focus_context();
            let _ = d.update_style(modal_layer, |s| s.display(Display::None));
        }
    };

    // Esc first blurs the focused button; the second press reaches the modal itself, which
    // only receives it because keys dispatch from the active focus context, not the root.
    let d = doc.clone();
    let close_on_key = close_modal.clone();
    doc.on_key_press(modal_layer, move |key| {
        if key.code == KeyCode::Esc && d.focused().is_none() {
            close_on_key();
        }
    })?;

    let close_on_confirm = close_modal.clone();
    doc.on_click(confirm, move |event| {
        event.stop_propagation();
        close_on_confirm();
    })?;

    doc.on_click(cancel, move |event| {
        event.stop_propagation();
        close_modal();
    })?;

    // --- Focus handlers ----------------------------------------------

    let d = doc.clone();
    doc.on_focus(container, move |event| {
        if event.relation() == FocusEventRelation::Descendant {
            let _ = d.update_style(container, |s| s.background(Color::oklch(0.38, 0.16, 95.0)));
        }
    })?;

    let d = doc.clone();
    let container_bg = container_background_alt.clone();
    doc.on_blur(container, move |event| {
        if event.relation() == FocusEventRelation::Descendant {
            let color = if container_bg.load(Ordering::Relaxed) {
                Color::oklch(0.25, 0.12, 260.0)
            } else {
                Color::oklch(0.3, 0.1, 50.0)
            };
            let _ = d.update_style(container, |s| s.background(color));
        }
    })?;

    // --- Mouse handlers ----------------------------------------------

    let d = doc.clone();
    let text_bg = text_background_alt.clone();
    doc.on_click(text, move |event| {
        event.stop_propagation();
        let use_alt = text_bg.fetch_not(Ordering::Relaxed);
        let color = if use_alt {
            Color::blue()
        } else {
            Color::oklch(0.65, 0.2, 140.0)
        };
        let _ = d.update_style(text, |s| s.background(color));
    })?;

    let d = doc.clone();
    let container_bg = container_background_alt.clone();
    doc.on_click(container, move |_| {
        let use_alt = container_bg.fetch_not(Ordering::Relaxed);
        let color = if use_alt {
            Color::oklch(0.3, 0.1, 50.0)
        } else {
            Color::oklch(0.25, 0.12, 260.0)
        };
        let _ = d.update_style(container, |s| s.background(color));
    })?;

    // Scoped to the button now that the page itself scrolls: prevent_default keeps
    // the wheel from also scrolling the page while it adjusts opacity.
    let d = doc.clone();
    let opacity_for_wheel = text_opacity.clone();
    doc.on_wheel(toggle_button, move |event| {
        event.prevent_default();
        if let Ok(mut opacity) = opacity_for_wheel.lock() {
            let direction = if event.delta > 0 { 1.0 } else { -1.0 };
            *opacity = (*opacity + direction * 0.1).clamp(0.1, 1.0);
            let target = *opacity;
            let _ = d.update_style(text, |s| s.opacity(target));
        }
    })?;

    // --- Run ----------------------------------------------------------

    doc.run().await?;
    Ok(())
}

/// The spacer pattern behind the demo's virtualized pane: a scroll container holding
/// a leading spacer, the materialized rows, and a trailing spacer, diffed by a
/// [`Virtualizer`] on every scroll. Only the window plus overscan exists in the DOM;
/// the spacers keep the content size at the true total.
struct VirtualPane {
    doc: Document,
    container: NodeId,
    lead: NodeId,
    trail: NodeId,
    virtualizer: Virtualizer,
    rows: BTreeMap<usize, NodeId>,
    row_style: Style,
}

impl VirtualPane {
    fn build(doc: &Document, count: usize, row_style: &Style) -> tuidom::Result<Self> {
        let container = doc.create_box()?;
        let mut style = Style::new();
        style.flex_direction(FlexDirection::Column);
        style.height(Length::Cells(5));
        style.overflow_y(Overflow::Scroll);
        style.border(Border::new(BorderCharset::single()));
        style.border_color(Color::oklch(0.55, 0.02, 260.0));
        style.padding(EdgeInsets::new(0, 1, 0, 1));
        doc.set_style(container, &style)?;

        let lead = doc.create_box()?;
        doc.append_child(container, lead)?;
        let trail = doc.create_box()?;
        doc.append_child(container, trail)?;

        Ok(Self {
            doc: doc.clone(),
            container,
            lead,
            trail,
            virtualizer: Virtualizer::uniform(count, 1, 2),
            rows: BTreeMap::new(),
            row_style: row_style.clone(),
        })
    }

    fn apply(&mut self, offset: u16, viewport: u16) {
        let Some(update) = self.virtualizer.update(offset, viewport) else {
            return;
        };

        for range in update.remove {
            for index in range {
                if let Some(node) = self.rows.remove(&index) {
                    let _ = self.doc.remove_child(self.container, node);
                }
            }
        }
        for range in update.add {
            for index in range {
                let Ok(node) = self.doc.create_text(format!("virtual row {index:04}")) else {
                    continue;
                };
                let _ = self.doc.set_style(node, &self.row_style);
                // Before the next materialized row, or before the trailing spacer.
                let before = self
                    .rows
                    .range(index + 1..)
                    .next()
                    .map(|(_, node)| *node)
                    .unwrap_or(self.trail);
                let _ = self.doc.insert_before(self.container, node, before);
                self.rows.insert(index, node);
            }
        }

        self.set_spacer(self.lead, update.window.lead);
        self.set_spacer(self.trail, update.window.trail);
    }

    fn set_spacer(&self, spacer: NodeId, cells: u64) {
        let mut style = Style::new();
        style.height(Length::Cells(u16::try_from(cells).unwrap_or(u16::MAX)));
        // An empty box has no content floor, so default flex shrink would collapse
        // the spacer to fit the container — and with it the whole scroll range.
        style.flex_shrink(0.0);
        let _ = self.doc.set_style(spacer, &style);
    }
}

//! Smoke test exercising the full pipeline:
//! Box + Text/Input rendering, keyboard events, focus, focus styles,
//! configurable focus keys, targeted mouse events, bubbling, stop_propagation,
//! wheel events, borders, half-block edges, terminal text attributes,
//! transitions, and the performance metrics API.
//!
//! Tab / Shift-Tab      — move focus in DOM order
//! Focus the "focus me" panel — its border recolors, charset and sides untouched
//! Arrows / hjkl        — move focus spatially, or move input cursor
//! Esc                  — blur focused node; press again in the modal to close it
//! Hover buttons/input  — focus node
//! Type in inputs       — edit text / masked password input
//! Space outside input  — toggle first button opacity (fade in/out)
//! Click first button   — toggle button background, stop propagation
//! Click background     — toggle container background
//! Wheel anywhere       — adjust text opacity via container wheel handler
//! m outside input      — open the modal: focus is trapped inside it and the
//!                        content behind it goes inert (no tab, hover, or clicks)
//! q outside input      — quit

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tuidom::animation::{Easing, TransitionConfig};
use tuidom::event::{FocusEventRelation, FocusKeys, KeyCode};
use tuidom::style::{
    AlignItems, Border, BorderCharset, Color, CursorShape, Display, EdgeInsets, FlexDirection,
    FlexGap, JustifyContent, Length, Position, Sides, Style,
};

fn init_logging() {
    // Best-effort file logging for the smoke test.
    if let Ok(file) = std::fs::File::create("/tmp/tuidom.log") {
        let _ = simplelog::WriteLogger::init(
            log::LevelFilter::Trace,
            simplelog::Config::default(),
            file,
        );
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

    let mut center_area_style = Style::new();
    center_area_style.width(Length::Percent(100.0));
    center_area_style.flex_grow(1.0);
    center_area_style.justify_content(JustifyContent::Center);
    center_area_style.align_items(AlignItems::Center);

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
    input_style.width(Length::Pixels(24));
    input_style.height(Length::Pixels(1));
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
    separator_style.height(Length::Pixels(1));
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

    // --- Shared state -------------------------------------------------

    let opacity_visible = Arc::new(AtomicBool::new(true));
    let text_background_alt = Arc::new(AtomicBool::new(false));
    let container_background_alt = Arc::new(AtomicBool::new(false));
    let text_opacity = Arc::new(Mutex::new(1.0));

    update_perf_counter(&doc, perf_counter);

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
                update_perf_counter(&d, perf_counter);
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
        update_perf_counter(&d, perf_counter);
        if event.relation() == FocusEventRelation::Descendant {
            let _ = d.update_style(container, |s| s.background(Color::oklch(0.38, 0.16, 95.0)));
        }
    })?;

    let d = doc.clone();
    let container_bg = container_background_alt.clone();
    doc.on_blur(container, move |event| {
        update_perf_counter(&d, perf_counter);
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
        update_perf_counter(&d, perf_counter);
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
        update_perf_counter(&d, perf_counter);
        let use_alt = container_bg.fetch_not(Ordering::Relaxed);
        let color = if use_alt {
            Color::oklch(0.3, 0.1, 50.0)
        } else {
            Color::oklch(0.25, 0.12, 260.0)
        };
        let _ = d.update_style(container, |s| s.background(color));
    })?;

    let d = doc.clone();
    let opacity_for_wheel = text_opacity.clone();
    doc.on_wheel(container, move |event| {
        update_perf_counter(&d, perf_counter);
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

fn update_perf_counter(doc: &tuidom::Document, node: tuidom::NodeId) {
    let snapshot = doc.performance_snapshot();
    let Some(frame) = snapshot.latest else {
        return;
    };

    let text = format!(
        "FPS: {:.0}  Frame: {:.3}ms",
        snapshot.fps,
        frame.frame_time.as_secs_f64() * 1000.0
    );
    let _ = doc.set_text_content(node, text);
}

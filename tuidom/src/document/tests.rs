use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::TuidomError;
use crate::animation::{Easing, TransitionConfig};
use crate::event::{
    EventPhase, FocusEventRelation, FocusKeys, KeyCode, KeyEvent, KeyModifiers, MouseButton,
    MouseEvent, ResizeEvent, WheelEvent,
};
use crate::headless::{HeadlessRuntime, ScreenCell, ScreenColor};
use crate::node::{LayoutRect, NodeKindView, NodeLayout};
use crate::performance::PerformanceDetail;
use crate::style::{
    AlignContent, AlignItems, AlignSelf, Border, BorderCharset, Color, CursorShape, Display,
    EdgeInsets, FlexDirection, FlexGap, FlexWrap, Length, Overflow, Position, ResolvedColor,
    ScrollbarShow, Sides, Style,
};
use crate::virtualize::Virtualizer;

#[test]
fn create_nodes() {
    let doc = Document::new().unwrap();
    let box_id = doc.create_box().unwrap();
    let text_id = doc.create_text("hello").unwrap();

    let box_view = doc.get_node(box_id).unwrap();
    let text_view = doc.get_node(text_id).unwrap();

    assert!(matches!(box_view.kind, NodeKindView::Box));
    assert!(matches!(text_view.kind, NodeKindView::Text { .. }));

    assert!(doc.get_node(NodeId::new(999)).is_none());
}

#[test]
fn text_content_api_sets_text_and_rejects_other_nodes() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hello").unwrap();
    let box_id = doc.create_box().unwrap();

    doc.set_text_content(text, "updated").unwrap();
    let view = doc.get_node(text).unwrap();
    assert!(matches!(view.kind, NodeKindView::Text { content } if content == "updated"));

    assert_eq!(
        doc.set_text_content(box_id, "nope").unwrap_err(),
        TuidomError::NodeNotText { id: box_id }
    );
}

#[tokio::test]
async fn set_text_content_with_unchanged_content_does_not_notify_render() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hello").unwrap();

    // A real change notifies; awaiting it also drains the stored permit.
    let notified = doc.inner.notify.notified();
    doc.set_text_content(text, "updated").unwrap();
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .unwrap();

    let notified = doc.inner.notify.notified();
    doc.set_text_content(text, "updated").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), notified)
            .await
            .is_err()
    );

    let view = doc.get_node(text).unwrap();
    assert!(matches!(view.kind, NodeKindView::Text { content } if content == "updated"));
}

#[test]
fn performance_snapshot_exposes_recorded_metrics_and_detail() {
    let doc = Document::new().unwrap();
    assert_eq!(doc.performance_snapshot().detail, PerformanceDetail::Basic);
    assert!(doc.performance_snapshot().latest.is_none());

    doc.set_performance_detail(PerformanceDetail::Detailed);
    doc.record_frame_metrics(
        Duration::from_millis(4),
        Duration::from_millis(1),
        crate::performance::RenderMetrics {
            diff_dirty_cells: 2,
            cells_flushed: 3,
            ..Default::default()
        },
    );

    let snapshot = doc.performance_snapshot();
    let latest = snapshot.latest.unwrap();
    assert_eq!(snapshot.detail, PerformanceDetail::Detailed);
    assert_eq!(snapshot.frame_count, 1);
    assert_eq!(latest.frame_time, Duration::from_millis(4));
    assert_eq!(latest.layout_time, Duration::from_millis(1));
    assert_eq!(latest.render.diff_dirty_cells, 2);
    assert_eq!(latest.render.cells_flushed, 3);
}

#[test]
fn attribute_api_sets_gets_removes_and_exposes_snapshot() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    doc.set_attr(node, String::from("role"), String::from("button"))
        .unwrap();
    assert_eq!(
        doc.get_attr(node, "role").unwrap(),
        Some("button".to_owned())
    );
    assert_eq!(
        doc.get_node(node).unwrap().attrs.get("role"),
        Some(&"button".to_owned())
    );

    doc.set_attr(node, "role", "tab").unwrap();
    assert_eq!(doc.get_attr(node, "role").unwrap(), Some("tab".to_owned()));

    doc.remove_attr(node, "role").unwrap();
    assert_eq!(doc.get_attr(node, "role").unwrap(), None);
    assert!(!doc.get_node(node).unwrap().attrs.contains_key("role"));
}

#[test]
fn attribute_api_rejects_missing_nodes_and_empty_keys() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    let missing = NodeId::new(999);

    assert_eq!(
        doc.set_attr(missing, "role", "button").unwrap_err(),
        TuidomError::NodeNotFound { id: missing }
    );
    assert_eq!(
        doc.get_attr(missing, "role").unwrap_err(),
        TuidomError::NodeNotFound { id: missing }
    );
    assert_eq!(
        doc.remove_attr(missing, "role").unwrap_err(),
        TuidomError::NodeNotFound { id: missing }
    );

    assert_eq!(
        doc.set_attr(node, "", "button").unwrap_err(),
        TuidomError::InvalidAttributeKey
    );
    assert_eq!(
        doc.get_attr(node, "").unwrap_err(),
        TuidomError::InvalidAttributeKey
    );
    assert_eq!(
        doc.remove_attr(node, "").unwrap_err(),
        TuidomError::InvalidAttributeKey
    );
}

#[tokio::test]
async fn attribute_mutations_notify_render() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let notified = doc.inner.notify.notified();
    doc.set_attr(node, "role", "button").unwrap();
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .unwrap();

    let notified = doc.inner.notify.notified();
    doc.remove_attr(node, "role").unwrap();
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .unwrap();
}

#[test]
fn style_custom_properties_are_raw_inline_metadata() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let mut style = Style::new();
    style.set_custom("--kind", String::from("panel"));
    assert_eq!(style.get_custom("--kind"), Some("panel"));

    let cloned = style.clone();
    assert_eq!(cloned.get_custom("--kind"), Some("panel"));

    doc.set_style(node, &style).unwrap();
    assert_eq!(
        doc.inner
            .nodes
            .get(&node)
            .unwrap()
            .style
            .get_custom("--kind"),
        Some("panel")
    );

    doc.update_style(node, |style| {
        style.set_custom("--kind", "dialog");
        style.remove_custom("--missing");
    })
    .unwrap();
    assert_eq!(
        doc.inner
            .nodes
            .get(&node)
            .unwrap()
            .style
            .get_custom("--kind"),
        Some("dialog")
    );

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(resolved.width, Length::Auto);
}

#[test]
fn position_resolves_from_set_inherit_and_default() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), parent).unwrap();
    doc.append_child(parent, child).unwrap();

    assert_eq!(doc.resolved_style(child).unwrap().position, Position::Flow);

    doc.update_style(parent, |style| {
        style.position(Position::Absolute { x: 3, y: -1 })
    })
    .unwrap();
    assert_eq!(
        doc.resolved_style(parent).unwrap().position,
        Position::Absolute { x: 3, y: -1 }
    );
    assert_eq!(doc.resolved_style(child).unwrap().position, Position::Flow);

    doc.update_style(child, |style| style.inherit_position())
        .unwrap();
    assert_eq!(
        doc.resolved_style(child).unwrap().position,
        Position::Absolute { x: 3, y: -1 }
    );

    doc.update_style(child, |style| style.unset_position())
        .unwrap();
    assert_eq!(doc.resolved_style(child).unwrap().position, Position::Flow);
}

#[test]
fn create_input_node_is_focusable_by_default() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("hello").unwrap();

    assert!(doc.is_focusable(input).unwrap());
    assert!(matches!(
        doc.get_node(input).unwrap().kind,
        NodeKindView::Input {
            value,
            cursor: 5,
            selection: None,
            multiline: false,
            mask: None,
            scroll_x: 0,
            scroll_y: 0,
        } if value == "hello"
    ));
}

#[test]
fn padding_affects_rendered_child_position() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("A").unwrap();
    doc.append_child(doc.root(), text).unwrap();

    let mut style = Style::new();
    style.padding(EdgeInsets::new(1, 0, 0, 2));
    style.align_items(AlignItems::FlexStart);
    doc.set_style(doc.root(), &style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 5, 3);
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(0, 0).unwrap().text, " ");
    assert_eq!(runtime.get_cell(2, 1).unwrap().text, "A");
}

/// An absolute node overflowing its parent must paint and hit-test at its offset
/// position, which exercises layout, paint, and `node_at` against real layout rects.
#[test]
fn absolute_node_paints_and_hit_tests_outside_its_parent() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    let mut parent_style = Style::new();
    parent_style.width(Length::Cells(2));
    parent_style.height(Length::Cells(1));
    doc.set_style(parent, &parent_style).unwrap();
    doc.append_child(doc.root(), parent).unwrap();

    // Anchored below-right of a 2x1 parent that sits at the origin.
    let badge = doc.create_text("X").unwrap();
    let mut badge_style = Style::new();
    badge_style.position(Position::Absolute { x: 3, y: 2 });
    doc.set_style(badge, &badge_style).unwrap();
    doc.append_child(parent, badge).unwrap();

    let mut root_style = Style::new();
    root_style.align_items(AlignItems::FlexStart);
    doc.set_style(doc.root(), &root_style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc.clone(), 6, 4);
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(3, 2).unwrap().text, "X");
    assert_eq!(doc.node_at(3, 2), Some(badge));
    assert_eq!(doc.get_node(badge).unwrap().layout.unwrap().rect.x, 3);
}

/// Absolute positioning must not let a descendant's `z_index` escape its parent
/// subtree: paint order stays subtree-atomic regardless of positioning mode.
#[test]
fn absolute_descendant_z_index_stays_inside_parent_subtree() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let absolute_child = doc.create_box().unwrap();
    let sibling = doc.create_box().unwrap();

    set_z_index(&doc, parent, 0);
    set_z_index(&doc, absolute_child, 999);
    set_z_index(&doc, sibling, 1);
    doc.update_style(absolute_child, |style| {
        style.position(Position::Absolute { x: 0, y: 0 })
    })
    .unwrap();

    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, absolute_child).unwrap();
    doc.append_child(root, sibling).unwrap();
    set_one_cell_layouts(&doc, &[root, parent, absolute_child, sibling]);

    assert_eq!(doc.node_at(0, 0), Some(sibling));
}

/// The pressed node is the focus target of the hit, so pressing a child marks the
/// focusable ancestor active rather than the child itself.
#[test]
fn mouse_down_activates_focus_target_and_mouse_up_clears_it() {
    let doc = Document::new().unwrap();
    let button = doc.create_box().unwrap();
    let label = doc.create_text("X").unwrap();
    doc.append_child(doc.root(), button).unwrap();
    doc.append_child(button, label).unwrap();
    doc.set_focusable(button, true).unwrap();

    let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 2);
    runtime.render().unwrap();

    assert_eq!(doc.active(), None);

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    assert_eq!(doc.active(), Some(button));

    runtime.simulate_mouse_up(0, 0, MouseButton::Left);
    assert_eq!(doc.active(), None);
}

/// Releasing away from the pressed node must not leave it stuck active.
#[test]
fn mouse_up_outside_pressed_node_still_clears_active() {
    let doc = Document::new().unwrap();
    let button = doc.create_box().unwrap();
    let mut button_style = Style::new();
    button_style.width(Length::Cells(1));
    button_style.height(Length::Cells(1));
    doc.set_style(button, &button_style).unwrap();
    doc.append_child(doc.root(), button).unwrap();
    doc.set_focusable(button, true).unwrap();

    let mut runtime = HeadlessRuntime::new(doc.clone(), 6, 3);
    runtime.render().unwrap();

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    assert_eq!(doc.active(), Some(button));

    runtime.simulate_mouse_up(5, 2, MouseButton::Left);
    assert_eq!(doc.active(), None);
}

#[test]
fn active_style_merges_over_focus_style_while_pressed() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.color(Color::white());
    base.background(Color::blue());
    doc.set_style(node, &base).unwrap();

    let mut focus_style = Style::new();
    focus_style.background(Color::yellow());
    doc.set_focus_style(node, &focus_style).unwrap();

    let mut active_style = Style::new();
    active_style.background(Color::red());
    doc.set_active_style(node, &active_style).unwrap();

    doc.focus(node).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::yellow())
    );

    // Active merges on top of focus, and only overrides what it sets.
    doc.set_active(node, true).unwrap();
    let pressed = doc.resolved_style(node).unwrap();
    assert_eq!(pressed.background, Some(ResolvedColor::red()));
    assert_eq!(pressed.color, ResolvedColor::white());

    doc.set_active(node, false).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::yellow())
    );
}

#[test]
fn removing_the_active_node_clears_active_state() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();

    doc.set_active(node, true).unwrap();
    assert_eq!(doc.active(), Some(node));

    doc.remove_child(doc.root(), node).unwrap();
    assert_eq!(doc.active(), None);
}

#[test]
fn disabled_is_inherited_by_the_whole_subtree() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, child).unwrap();

    doc.set_disabled(panel, true).unwrap();

    assert!(doc.is_disabled(panel).unwrap());
    assert!(doc.is_effectively_disabled(child).unwrap());
    // The child is disabled through its ancestor, not in its own right.
    assert!(!doc.is_disabled(child).unwrap());

    doc.set_disabled(panel, false).unwrap();
    assert!(!doc.is_effectively_disabled(child).unwrap());
}

#[test]
fn disabled_nodes_cannot_be_focused_and_are_skipped_by_tab() {
    let doc = Document::new().unwrap();
    let first = doc.create_box().unwrap();
    let panel = doc.create_box().unwrap();
    let inside_panel = doc.create_box().unwrap();
    let last = doc.create_box().unwrap();

    doc.append_child(doc.root(), first).unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, inside_panel).unwrap();
    doc.append_child(doc.root(), last).unwrap();
    for node in [first, inside_panel, last] {
        doc.set_focusable(node, true).unwrap();
    }

    doc.set_disabled(panel, true).unwrap();

    // Tab skips the whole disabled subtree rather than just the disabled node.
    doc.focus(first).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(last));

    assert!(doc.focus(inside_panel).is_err());
}

#[test]
fn disabling_a_node_blurs_it_and_clears_active() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let button = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, button).unwrap();
    doc.set_focusable(button, true).unwrap();

    doc.focus(button).unwrap();
    doc.set_active(button, true).unwrap();
    assert_eq!(doc.focused(), Some(button));
    assert_eq!(doc.active(), Some(button));

    // Disabling the ancestor must release focus and the pressed state held by the child.
    doc.set_disabled(panel, true).unwrap();
    assert_eq!(doc.focused(), None);
    assert_eq!(doc.active(), None);

    // An effectively disabled node cannot be made active again.
    doc.set_active(button, true).unwrap();
    assert_eq!(doc.active(), None);
}

#[test]
fn disabled_nodes_swallow_targeted_events() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let button = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, button).unwrap();

    // Empty boxes measure zero, so give them size for the hit test to land on the button.
    let mut sized = Style::new();
    sized.width(Length::Cells(2));
    sized.height(Length::Cells(1));
    doc.set_style(panel, &sized).unwrap();
    doc.set_style(button, &sized).unwrap();

    let panel_clicks = Arc::new(AtomicUsize::new(0));
    let button_clicks = Arc::new(AtomicUsize::new(0));
    {
        let panel_clicks = panel_clicks.clone();
        doc.on_click(panel, move |_| {
            panel_clicks.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let button_clicks = button_clicks.clone();
        doc.on_click(button, move |_| {
            button_clicks.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 2);
    runtime.render().unwrap();

    runtime.simulate_click(0, 0);
    assert_eq!(button_clicks.load(Ordering::SeqCst), 1);
    assert_eq!(panel_clicks.load(Ordering::SeqCst), 1);

    // The disabled button drops the event rather than bubbling it to the enabled panel.
    doc.set_disabled(button, true).unwrap();
    runtime.simulate_click(0, 0);
    assert_eq!(button_clicks.load(Ordering::SeqCst), 1);
    assert_eq!(panel_clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn disabled_style_merges_over_focus_and_active_styles() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.color(Color::white());
    base.background(Color::blue());
    doc.set_style(node, &base).unwrap();

    let mut focus_style = Style::new();
    focus_style.background(Color::yellow());
    doc.set_focus_style(node, &focus_style).unwrap();

    let mut disabled_style = Style::new();
    disabled_style.color(Color::black());
    doc.set_disabled_style(node, &disabled_style).unwrap();

    doc.focus(node).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::white()
    );

    // Disabling blurs the node, so the focus style drops and the disabled style applies.
    doc.set_disabled(node, true).unwrap();
    let disabled = doc.resolved_style(node).unwrap();
    assert_eq!(disabled.color, ResolvedColor::black());
    assert_eq!(disabled.background, Some(ResolvedColor::blue()));
}

#[test]
fn descendant_disabled_style_applies_through_a_disabled_ancestor() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, child).unwrap();

    let mut base = Style::new();
    base.color(Color::white());
    doc.set_style(child, &base).unwrap();

    let mut disabled_style = Style::new();
    disabled_style.color(Color::black());
    doc.set_disabled_style(child, &disabled_style).unwrap();

    assert_eq!(
        doc.resolved_style(child).unwrap().color,
        ResolvedColor::white()
    );

    // The child defines its own disabled style, so disabling the panel restyles it.
    doc.set_disabled(panel, true).unwrap();
    assert_eq!(
        doc.resolved_style(child).unwrap().color,
        ResolvedColor::black()
    );
}

#[test]
fn input_state_apis_read_write_and_normalize_offsets() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("a\u{301}b").unwrap();

    assert_eq!(doc.input_value(input).unwrap(), "a\u{301}b");
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}b".len());

    doc.set_input_cursor(input, 1).unwrap();
    assert_eq!(doc.input_cursor(input).unwrap(), 0);

    let reversed_selection = std::ops::Range { start: 4, end: 1 };
    doc.set_input_selection(input, reversed_selection).unwrap();
    assert_eq!(doc.input_selection(input).unwrap(), Some(0..4));

    doc.clear_input_selection(input).unwrap();
    assert_eq!(doc.input_selection(input).unwrap(), None);

    doc.set_input_multiline(input, true).unwrap();
    assert!(doc.input_multiline(input).unwrap());

    doc.set_input_mask(input, Some('*')).unwrap();
    assert_eq!(doc.input_mask(input).unwrap(), Some('*'));

    doc.set_input_value(input, "xy").unwrap();
    assert_eq!(doc.input_value(input).unwrap(), "xy");
    assert_eq!(doc.input_cursor(input).unwrap(), 0);
}

#[test]
fn input_state_apis_reject_non_input_nodes() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hello").unwrap();

    assert_eq!(
        doc.input_value(text).unwrap_err(),
        TuidomError::NodeNotInput { id: text }
    );
    assert_eq!(
        doc.set_input_value(text, "new").unwrap_err(),
        TuidomError::NodeNotInput { id: text }
    );
}

#[test]
fn cursor_style_fields_resolve_and_focus_style_overrides_them() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("hello").unwrap();

    let mut base = Style::new();
    base.cursor_shape(CursorShape::Bar);
    doc.set_style(input, &base).unwrap();

    let resolved = doc.resolved_style(input).unwrap();
    assert_eq!(resolved.cursor_shape, CursorShape::Bar);

    let mut focus = Style::new();
    focus.cursor_shape(CursorShape::Underline);
    doc.set_focus_style(input, &focus).unwrap();
    doc.focus(input).unwrap();

    let focused = doc.resolved_style(input).unwrap();
    assert_eq!(focused.cursor_shape, CursorShape::Underline);
}

#[test]
fn border_resolves_and_defaults_to_none_with_a_foreground_following_color() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();

    let unset = doc.resolved_style(node).unwrap();
    assert_eq!(unset.border, Border::none());
    assert!(!unset.border.sides.any());
    // Unset border color means "follow the node's foreground", not "no color".
    assert_eq!(unset.border_color, None);

    let mut style = Style::new();
    style.border(Border::new(BorderCharset::double()));
    doc.set_style(node, &style).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(resolved.border.charset, BorderCharset::double());
    assert_eq!(resolved.border.sides, Sides::ALL);
    assert_eq!(resolved.border_color, None);
}

#[test]
fn focus_style_recolors_a_border_without_respecifying_it() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.border(
        Border::new(BorderCharset::rounded()).with_sides(Sides::new(true, false, true, false)),
    );
    doc.set_style(node, &base).unwrap();

    // The whole point of border_color being its own property: a focus style changes the
    // color alone, and the charset and sides survive the merge untouched.
    let mut focus = Style::new();
    focus.border_color(Color::red());
    doc.set_focus_style(node, &focus).unwrap();
    doc.focus(node).unwrap();

    let focused = doc.resolved_style(node).unwrap();
    assert_eq!(focused.border_color, Some(ResolvedColor::red()));
    assert_eq!(focused.border.charset, BorderCharset::rounded());
    assert_eq!(focused.border.sides, Sides::new(true, false, true, false));
}

#[test]
fn border_none_in_a_pseudo_style_removes_a_base_border() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();

    let mut base = Style::new();
    base.border(Border::new(BorderCharset::single()));
    doc.set_style(node, &base).unwrap();

    let mut disabled = Style::new();
    disabled.border(Border::none());
    doc.set_disabled_style(node, &disabled).unwrap();
    doc.set_disabled(node, true).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert!(!resolved.border.sides.any());
}

#[test]
fn half_block_edges_resolve_with_both_colors_defaulting_to_follow_the_node() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();

    let unset = doc.resolved_style(node).unwrap();
    assert_eq!(unset.half_block_edges, Sides::NONE);

    let mut style = Style::new();
    style.half_block_edges(Sides::new(true, false, true, false));
    doc.set_style(node, &style).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(
        resolved.half_block_edges,
        Sides::new(true, false, true, false)
    );
    // Unset inner means "follow the node's background"; unset outer means "keep what is
    // already painted there". Neither means "no color".
    assert_eq!(resolved.half_block_inner_color, None);
    assert_eq!(resolved.half_block_outer_color, None);
}

#[test]
fn focus_style_recolors_a_half_block_edge_without_respecifying_the_sides() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.half_block_edges(Sides::new(true, false, true, false));
    doc.set_style(node, &base).unwrap();

    let mut focus = Style::new();
    focus.half_block_inner_color(Color::red());
    doc.set_focus_style(node, &focus).unwrap();
    doc.focus(node).unwrap();

    let focused = doc.resolved_style(node).unwrap();
    assert_eq!(focused.half_block_inner_color, Some(ResolvedColor::red()));
    assert_eq!(
        focused.half_block_edges,
        Sides::new(true, false, true, false)
    );
}

fn bordered_box(doc: &Document, border: Border, width: u16, height: u16) -> NodeId {
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(width));
    style.height(Length::Cells(height));
    style.border(border);
    doc.set_style(node, &style).unwrap();
    node
}

fn row_text(runtime: &HeadlessRuntime, y: i32, width: i32) -> String {
    (0..width)
        .map(|x| runtime.get_cell(x, y).unwrap().text)
        .collect()
}

#[test]
fn border_presets_render_their_own_characters() {
    for (charset, expected) in [
        (BorderCharset::single(), ["┌──┐", "│  │", "└──┘"]),
        (BorderCharset::double(), ["╔══╗", "║  ║", "╚══╝"]),
        (BorderCharset::rounded(), ["╭──╮", "│  │", "╰──╯"]),
        (BorderCharset::thick(), ["┏━━┓", "┃  ┃", "┗━━┛"]),
        (BorderCharset::ascii(), ["+--+", "|  |", "+--+"]),
    ] {
        let doc = Document::new().unwrap();
        bordered_box(&doc, Border::new(charset), 4, 3);

        let mut runtime = HeadlessRuntime::new(doc, 4, 3);
        runtime.render().unwrap();

        for (y, line) in expected.iter().enumerate() {
            assert_eq!(row_text(&runtime, y as i32, 4), *line);
        }
    }
}

#[test]
fn a_top_only_border_draws_a_clean_rule_without_stray_corners() {
    let doc = Document::new().unwrap();
    bordered_box(
        &doc,
        Border::new(BorderCharset::single()).with_sides(Sides::new(true, false, false, false)),
        4,
        3,
    );

    let mut runtime = HeadlessRuntime::new(doc, 4, 3);
    runtime.render().unwrap();

    // The top side runs through both corner cells, because no vertical side meets it there.
    assert_eq!(row_text(&runtime, 0, 4), "────");
    assert_eq!(row_text(&runtime, 1, 4), "    ");
    assert_eq!(row_text(&runtime, 2, 4), "    ");
}

#[test]
fn adjacent_drawn_sides_still_meet_in_a_corner() {
    let doc = Document::new().unwrap();
    bordered_box(
        &doc,
        Border::new(BorderCharset::single()).with_sides(Sides::new(true, false, false, true)),
        4,
        3,
    );

    let mut runtime = HeadlessRuntime::new(doc, 4, 3);
    runtime.render().unwrap();

    assert_eq!(row_text(&runtime, 0, 4), "┌───");
    assert_eq!(row_text(&runtime, 1, 4), "│   ");
    assert_eq!(row_text(&runtime, 2, 4), "│   ");
}

#[test]
fn border_color_follows_the_foreground_until_it_is_set() {
    let doc = Document::new().unwrap();
    let node = bordered_box(&doc, Border::new(BorderCharset::single()), 4, 3);
    doc.update_style(node, |style| style.color(Color::red()))
        .unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 4, 3);
    runtime.render().unwrap();
    let red = ScreenColor::from_rgb(255, 0, 0);
    assert_eq!(runtime.get_cell(0, 0).unwrap().fg, Some(red));

    runtime
        .document()
        .update_style(node, |style| style.border_color(Color::blue()))
        .unwrap();
    runtime.render().unwrap();

    // The border takes its own color; the node's foreground is unchanged.
    assert_eq!(
        runtime.get_cell(0, 0).unwrap().fg,
        Some(ScreenColor::from_rgb(0, 0, 255))
    );
    assert_eq!(
        runtime.document().resolved_style(node).unwrap().color,
        ResolvedColor::red()
    );
}

#[test]
fn a_bordered_node_clips_at_the_screen_edge() {
    let doc = Document::new().unwrap();
    let node = bordered_box(&doc, Border::new(BorderCharset::single()), 4, 3);
    doc.update_style(node, |style| {
        style.position(Position::Absolute { x: -1, y: -1 })
    })
    .unwrap();

    // The top and left sides fall offscreen. What remains on the grid is the right side and
    // the bottom side, meeting in the bottom-right corner.
    let mut runtime = HeadlessRuntime::new(doc, 3, 2);
    runtime.render().unwrap();

    assert_eq!(row_text(&runtime, 0, 3), "  │");
    assert_eq!(row_text(&runtime, 1, 3), "──┘");
}

#[test]
fn a_translucent_overlay_blends_over_a_border() {
    let doc = Document::new().unwrap();
    let node = bordered_box(&doc, Border::new(BorderCharset::single()), 4, 3);
    doc.update_style(node, |style| style.color(Color::white()))
        .unwrap();

    let overlay = doc.create_box().unwrap();
    doc.append_child(doc.root(), overlay).unwrap();
    let mut overlay_style = Style::new();
    overlay_style.position(Position::Absolute { x: 0, y: 0 });
    overlay_style.width(Length::Percent(100.0));
    overlay_style.height(Length::Percent(100.0));
    overlay_style.background(Color::oklcha(0.0, 0.0, 0.0, 0.5));
    doc.set_style(overlay, &overlay_style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 4, 3);
    runtime.render().unwrap();

    // A border glyph behaves exactly like a text glyph under a translucent fill: the glyph
    // and its foreground survive, and the overlay blends into the cell's background.
    let corner = runtime.get_cell(0, 0).unwrap();
    assert_eq!(corner.text, "┌");
    assert_eq!(corner.fg, Some(ScreenColor::from_rgb(255, 255, 255)));
    assert!(
        corner.bg.is_some(),
        "the translucent overlay should have blended into the border cell's background"
    );
}

#[test]
fn text_attributes_reach_the_cell() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hi").unwrap();
    doc.append_child(doc.root(), text).unwrap();
    doc.update_style(text, |style| {
        style.bold(true);
        style.underline(true);
    })
    .unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 4, 1);
    runtime.render().unwrap();

    let cell = runtime.get_cell(0, 0).unwrap();
    assert_eq!(cell.text, "h");
    assert!(cell.bold);
    assert!(cell.underline);
    assert!(!cell.italic);

    // An empty cell beside the text carries no attributes of its own.
    assert!(!runtime.get_cell(3, 0).unwrap().bold);
}

#[test]
fn a_focus_style_merges_text_attributes() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("hi").unwrap();
    doc.append_child(doc.root(), input).unwrap();

    let mut focus = Style::new();
    focus.bold(true);
    doc.set_focus_style(input, &focus).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 4, 1);
    runtime.render().unwrap();
    assert!(!runtime.get_cell(0, 0).unwrap().bold);

    runtime.document().focus(input).unwrap();
    runtime.render().unwrap();
    assert!(runtime.get_cell(0, 0).unwrap().bold);
}

#[test]
fn attributes_survive_a_translucent_fill_and_are_cleared_by_an_opaque_one() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hi").unwrap();
    doc.append_child(doc.root(), text).unwrap();
    doc.update_style(text, |style| style.italic(true)).unwrap();

    let overlay = doc.create_box().unwrap();
    doc.append_child(doc.root(), overlay).unwrap();
    let mut overlay_style = Style::new();
    overlay_style.position(Position::Absolute { x: 0, y: 0 });
    overlay_style.width(Length::Percent(100.0));
    overlay_style.height(Length::Percent(100.0));
    overlay_style.background(Color::oklcha(0.0, 0.0, 0.0, 0.5));
    doc.set_style(overlay, &overlay_style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 4, 1);
    runtime.render().unwrap();

    // The translucent fill keeps the glyph, so it keeps the glyph's attributes.
    let cell = runtime.get_cell(0, 0).unwrap();
    assert_eq!(cell.text, "h");
    assert!(cell.italic);

    // An opaque fill replaces the glyph, and the italics go with it.
    runtime
        .document()
        .update_style(overlay, |style| style.background(Color::blue()))
        .unwrap();
    runtime.render().unwrap();

    let cell = runtime.get_cell(0, 0).unwrap();
    assert_eq!(cell.text, " ");
    assert!(!cell.italic);
}

#[test]
fn input_layout_measures_displayed_single_line_multiline_and_masked_content() {
    let doc = Document::new().unwrap();
    doc.update_style(doc.root(), |style| {
        style.align_items(AlignItems::FlexStart);
    })
    .unwrap();
    let input = doc.create_input("ab\ncdef").unwrap();
    doc.append_child(doc.root(), input).unwrap();

    doc.compute_layout(20, 5).unwrap();
    let single_line = doc.get_node(input).unwrap().layout.unwrap().rect;
    assert_eq!(single_line.width, 7);
    assert_eq!(single_line.height, 1);

    doc.set_input_multiline(input, true).unwrap();
    doc.compute_layout(20, 5).unwrap();
    let multiline = doc.get_node(input).unwrap().layout.unwrap().rect;
    assert_eq!(multiline.width, 4);
    assert_eq!(multiline.height, 2);

    doc.set_input_value(input, "abcd").unwrap();
    doc.set_input_mask(input, Some('界')).unwrap();
    doc.compute_layout(20, 5).unwrap();
    let masked = doc.get_node(input).unwrap().layout.unwrap().rect;
    assert_eq!(masked.width, 8);
    assert_eq!(masked.height, 1);
}

#[test]
fn input_rendering_uses_display_content_without_changing_stored_value() {
    let doc = Document::new().unwrap();
    doc.update_style(doc.root(), |style| {
        style.align_items(AlignItems::FlexStart);
    })
    .unwrap();
    let input = doc.create_input("ab\ncd").unwrap();
    doc.append_child(doc.root(), input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 10, 4);
    runtime.render().unwrap();
    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "a");
    assert_eq!(runtime.get_cell(1, 0).unwrap().text, "b");
    assert_eq!(runtime.get_cell(2, 0).unwrap().text, " ");
    assert_eq!(runtime.get_cell(3, 0).unwrap().text, "c");

    runtime.document().set_input_multiline(input, true).unwrap();
    runtime.render().unwrap();
    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "a");
    assert_eq!(runtime.get_cell(1, 0).unwrap().text, "b");
    assert_eq!(runtime.get_cell(0, 1).unwrap().text, "c");

    runtime.document().set_input_mask(input, Some('*')).unwrap();
    runtime.render().unwrap();
    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "*");
    assert_eq!(runtime.get_cell(1, 0).unwrap().text, "*");
    assert_eq!(runtime.get_cell(0, 1).unwrap().text, "*");
    assert_eq!(runtime.document().input_value(input).unwrap(), "ab\ncd");
}

#[test]
fn single_line_input_scroll_keeps_cursor_visible() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abcdef").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(3));
    doc.set_style(input, &style).unwrap();
    doc.focus(input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 8, 2);
    runtime.render().unwrap();
    runtime.document().set_input_cursor(input, 6).unwrap();
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "e");
    assert_eq!(runtime.get_cell(1, 0).unwrap().text, "f");
    let cursor = runtime.cursor().unwrap();
    assert_eq!((cursor.x, cursor.y), (2, 0));
    assert!(cursor.visible);

    runtime.document().set_input_cursor(input, 0).unwrap();
    runtime.render().unwrap();
    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "a");
    assert_eq!(runtime.get_cell(1, 0).unwrap().text, "b");
    assert_eq!(runtime.get_cell(2, 0).unwrap().text, "c");
}

#[test]
fn multiline_input_scroll_keeps_cursor_visible() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("a\nb\nc\nd").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.set_input_multiline(input, true).unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(2));
    style.height(Length::Cells(2));
    doc.set_style(input, &style).unwrap();
    doc.focus(input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 8, 4);
    runtime.render().unwrap();
    runtime
        .document()
        .set_input_cursor(input, "a\nb\nc\nd".len())
        .unwrap();
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "c");
    assert_eq!(runtime.get_cell(0, 1).unwrap().text, "d");
    let cursor = runtime.cursor().unwrap();
    assert_eq!((cursor.x, cursor.y), (1, 1));
    assert!(cursor.visible);
}

#[test]
fn padded_text_node_paints_inside_its_padding_box() {
    let doc = Document::new().unwrap();
    let text = doc.create_text("hi").unwrap();
    doc.append_child(doc.root(), text).unwrap();

    let mut style = Style::new();
    style.padding(EdgeInsets::all(1));
    style.background(Color::black());
    doc.set_style(text, &style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 6, 4);
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(1, 1).unwrap().text, "h");
    assert_eq!(runtime.get_cell(2, 1).unwrap().text, "i");
    assert_eq!(runtime.get_cell(0, 0).unwrap().text, " ");

    // Padding is space inside the node, so the background still fills the whole rect.
    let background = Some(ScreenColor::from_rgb(0, 0, 0));
    assert_eq!(runtime.get_cell(0, 0).unwrap().bg, background);
    assert_eq!(runtime.get_cell(3, 3).unwrap().bg, background);
    assert_eq!(runtime.get_cell(4, 0).unwrap().bg, None);
}

#[test]
fn padded_input_places_cursor_and_scrolls_against_its_content_rect() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abcdef").unwrap();
    doc.append_child(doc.root(), input).unwrap();

    // Border-box width 5 minus one cell of padding per side leaves a 3-cell content rect,
    // whose origin is (1, 1).
    let mut style = Style::new();
    style.width(Length::Cells(5));
    style.height(Length::Cells(3));
    style.padding(EdgeInsets::all(1));
    doc.set_style(input, &style).unwrap();
    doc.focus(input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 8, 4);
    runtime.render().unwrap();
    runtime.document().set_input_cursor(input, 6).unwrap();
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(1, 1).unwrap().text, "e");
    assert_eq!(runtime.get_cell(2, 1).unwrap().text, "f");
    let cursor = runtime.cursor().unwrap();
    assert_eq!((cursor.x, cursor.y), (3, 1));
    assert!(cursor.visible);

    runtime.document().set_input_cursor(input, 0).unwrap();
    runtime.render().unwrap();
    assert_eq!(runtime.get_cell(1, 1).unwrap().text, "a");
    assert_eq!(runtime.get_cell(3, 1).unwrap().text, "c");
    let cursor = runtime.cursor().unwrap();
    assert_eq!((cursor.x, cursor.y), (1, 1));
}

#[test]
fn focused_block_cursor_inverts_cell_and_exposes_metadata() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("A").unwrap();
    let other = doc.create_input("B").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.append_child(doc.root(), other).unwrap();

    let mut style = Style::new();
    style.width(Length::Cells(3));
    style.color(Color::black());
    doc.set_style(input, &style).unwrap();
    doc.set_style(other, &style).unwrap();
    doc.focus(input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 10, 2);
    runtime.render().unwrap();

    let cell = runtime.get_cell(1, 0).unwrap();
    assert_eq!(cell.text, " ");
    assert_eq!(cell.fg, None);
    assert_eq!(cell.bg, Some(ScreenColor::from_rgb(0, 0, 0)));

    let cursor = runtime.cursor().unwrap();
    assert_eq!((cursor.x, cursor.y), (1, 0));
    assert_eq!(cursor.shape, CursorShape::Block);
    assert_eq!(cursor.color, ScreenColor::from_rgb(0, 0, 0));
    assert!(cursor.visible);
}

#[test]
fn input_cursor_shapes_render_distinct_metadata_without_replacing_text() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("A").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.set_input_cursor(input, 0).unwrap();
    doc.focus(input).unwrap();

    let style = Style::new();
    doc.set_style(input, &style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 5, 2);

    for shape in [CursorShape::Block, CursorShape::Bar, CursorShape::Underline] {
        runtime
            .document()
            .update_style(input, |s| s.cursor_shape(shape))
            .unwrap();
        runtime.render().unwrap();

        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "A");
        let cursor = runtime.cursor().unwrap();
        let cell = runtime.get_cell(0, 0).unwrap();
        if shape == CursorShape::Block {
            assert_eq!(cell.bg, Some(cursor.color));
        } else {
            assert_eq!(cell.bg, None);
        }
        assert_eq!(cursor.shape, shape);
        assert_eq!((cursor.x, cursor.y), (0, 0));
        assert!(cursor.visible);
    }
}

#[test]
fn cursor_metadata_over_wide_grapheme_points_at_head_cell() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("界").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.set_input_cursor(input, 0).unwrap();
    doc.focus(input).unwrap();

    let mut style = Style::new();
    style.cursor_shape(CursorShape::Block);
    doc.set_style(input, &style).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 5, 2);
    runtime.render().unwrap();

    assert_eq!(runtime.get_cell(0, 0).unwrap().text, "界");
    assert_eq!(runtime.get_cell(0, 0).unwrap().width, 2);
    assert!(runtime.get_cell(1, 0).unwrap().is_wide_continuation);

    let cursor = runtime.cursor().unwrap();
    assert_eq!(runtime.get_cell(0, 0).unwrap().bg, Some(cursor.color));
    assert_eq!(runtime.get_cell(1, 0).unwrap().bg, None);
    assert_eq!((cursor.x, cursor.y), (0, 0));
    assert_eq!(cursor.shape, CursorShape::Block);
    assert!(cursor.visible);
}

#[test]
fn cursor_metadata_is_hidden_when_cursor_is_outside_screen() {
    let doc = Document::new().unwrap();
    let spacer = doc.create_text("xxxxx").unwrap();
    let input = doc.create_input("A").unwrap();
    doc.append_child(doc.root(), spacer).unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.focus(input).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 5, 2);
    runtime.render().unwrap();

    let cursor = runtime.cursor().unwrap();
    assert!(cursor.x >= i32::from(runtime.width()));
    assert_eq!(cursor.y, 0);
    assert!(!cursor.visible);
}

#[test]
fn input_default_action_inserts_text_and_replaces_selection() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("ac").unwrap();
    doc.focus(input).unwrap();

    doc.set_input_cursor(input, 1).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('b')));
    assert_eq!(doc.input_value(input).unwrap(), "abc");
    assert_eq!(doc.input_cursor(input).unwrap(), 2);

    doc.set_input_selection(input, 1..3).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('z')));
    assert_eq!(doc.input_value(input).unwrap(), "az");
    assert_eq!(doc.input_cursor(input).unwrap(), 2);
    assert_eq!(doc.input_selection(input).unwrap(), None);
}

#[test]
fn input_default_action_deletes_by_grapheme() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("a\u{301}b").unwrap();
    doc.focus(input).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Backspace));
    assert_eq!(doc.input_value(input).unwrap(), "a\u{301}");
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}".len());

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Backspace));
    assert_eq!(doc.input_value(input).unwrap(), "");
    assert_eq!(doc.input_cursor(input).unwrap(), 0);

    doc.set_input_value(input, "a\u{301}b").unwrap();
    doc.set_input_cursor(input, 0).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Delete));
    assert_eq!(doc.input_value(input).unwrap(), "b");
    assert_eq!(doc.input_cursor(input).unwrap(), 0);
}

#[test]
fn input_default_action_moves_cursor_by_grapheme_and_line() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("a\u{301}b\ncd").unwrap();
    doc.focus(input).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Left));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}b\nc".len());
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Left));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}b\n".len());
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Home));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}b\n".len());
    doc.dispatch_key_press(KeyEvent::new(KeyCode::End));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}b\ncd".len());

    doc.set_input_cursor(input, "a\u{301}b".len()).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Left));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}".len());
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Left));
    assert_eq!(doc.input_cursor(input).unwrap(), 0);
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));
    assert_eq!(doc.input_cursor(input).unwrap(), "a\u{301}".len());
}

/// The goal column is the whole reason vertical motion carries state: without it the
/// short middle line would clamp the column and the second Down could not recover it,
/// so a run of Downs would walk leftward through the content.
#[test]
fn vertical_motion_holds_its_column_across_a_shorter_line() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abcdef\nxy\nghijkl").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 6).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Down));
    assert_eq!(doc.input_cursor(input).unwrap(), "abcdef\nxy".len());

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Down));
    assert_eq!(doc.input_cursor(input).unwrap(), "abcdef\nxy\nghijkl".len());

    // Moving horizontally ends the run, so the next Up starts from where the cursor
    // actually is rather than from a column it no longer occupies.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Home));
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Up));
    assert_eq!(doc.input_cursor(input).unwrap(), "abcdef\n".len());
}

/// Up on the first line is handled but unmoved: a key does not chain out to focus
/// navigation the way a wheel chains to an ancestor scroller.
#[test]
fn vertical_motion_in_a_multiline_input_does_not_escape_to_focus() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let input = doc.create_input("ab\ncd").unwrap();
    let sibling = doc.create_box().unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.append_child(root, input).unwrap();
    doc.append_child(root, sibling).unwrap();
    doc.set_focusable(sibling, true).unwrap();
    set_layout(
        &doc,
        root,
        LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 8,
        },
    );
    set_layout(
        &doc,
        input,
        LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        },
    );
    set_layout(
        &doc,
        sibling,
        LayoutRect {
            x: 0,
            y: 4,
            width: 4,
            height: 1,
        },
    );
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 1).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Up));

    assert_eq!(doc.focused(), Some(input));
    assert_eq!(doc.input_cursor(input).unwrap(), 1);
}

/// A single-line input has no line to move to, so Up and Down are not its keys — they
/// reach focus navigation instead of being swallowed for nothing.
#[test]
fn vertical_motion_in_a_single_line_input_moves_focus() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let input = doc.create_input("abc").unwrap();
    let below = doc.create_box().unwrap();
    doc.append_child(root, input).unwrap();
    doc.append_child(root, below).unwrap();
    doc.set_focusable(below, true).unwrap();
    set_layout(
        &doc,
        root,
        LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
    );
    set_layout(
        &doc,
        input,
        LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        },
    );
    set_layout(
        &doc,
        below,
        LayoutRect {
            x: 0,
            y: 2,
            width: 4,
            height: 1,
        },
    );
    doc.focus(input).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Down));

    assert_eq!(doc.focused(), Some(below));
}

/// Page motion sizes itself on the input's laid-out height, keeping one row of overlap,
/// and stops at the end rather than paging whatever is behind it.
#[test]
fn page_motion_moves_by_the_visible_height_and_stops_at_the_end() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("l0\nl1\nl2\nl3\nl4\nl5").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.focus(input).unwrap();
    // Four visible rows means a page of three: one row is kept to read against.
    set_layout(
        &doc,
        input,
        LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
    );
    doc.set_input_cursor(input, 0).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));
    assert_eq!(doc.input_cursor(input).unwrap(), "l0\nl1\nl2\n".len());

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));
    assert_eq!(
        doc.input_cursor(input).unwrap(),
        "l0\nl1\nl2\nl3\nl4\n".len()
    );

    // Already on the last line: handled, unmoved.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));
    assert_eq!(
        doc.input_cursor(input).unwrap(),
        "l0\nl1\nl2\nl3\nl4\n".len()
    );

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageUp));
    assert_eq!(doc.input_cursor(input).unwrap(), "l0\nl1\n".len());
}

/// Control turns the line-wise pair into value-wise ends, and the arrows into word
/// motion. Plain Home and End keep their line scope in between.
#[test]
fn control_chords_move_by_word_and_to_the_value_ends() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("one two\nthree four").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 0).unwrap();

    let ctrl = KeyModifiers::CONTROL;
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::End, ctrl));
    assert_eq!(
        doc.input_cursor(input).unwrap(),
        "one two\nthree four".len()
    );

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Home, ctrl));
    assert_eq!(doc.input_cursor(input).unwrap(), 0);

    // Whitespace between words is skipped rather than counted as a word of its own.
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Right, ctrl));
    assert_eq!(doc.input_cursor(input).unwrap(), "one ".len());
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Right, ctrl));
    assert_eq!(doc.input_cursor(input).unwrap(), "one two\n".len());

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Left, ctrl));
    assert_eq!(doc.input_cursor(input).unwrap(), "one ".len());
}

/// Shifting back past the starting point has to keep growing from the same anchor rather
/// than starting over, which is why the anchor outlives the collapsed range in between.
#[test]
fn shift_extension_grows_from_a_fixed_anchor_in_both_directions() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abcdef").unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 3).unwrap();
    let shift = KeyModifiers::SHIFT;

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Right, shift));
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Right, shift));
    assert_eq!(doc.input_selection(input).unwrap(), Some(3..5));
    assert_eq!(doc.input_cursor(input).unwrap(), 5);

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Left, shift));
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Left, shift));
    assert_eq!(doc.input_selection(input).unwrap(), None);
    assert_eq!(doc.input_cursor(input).unwrap(), 3);

    // Back through the anchor: the highlight returns on the other side of it.
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Left, shift));
    assert_eq!(doc.input_selection(input).unwrap(), Some(2..3));

    // An unshifted motion drops the anchor, so the next extension starts where the
    // cursor now is rather than reviving the old one.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));
    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Right, shift));
    assert_eq!(doc.input_selection(input).unwrap(), Some(3..4));
}

/// The case that rules out deriving the anchor from which end the cursor sits on: a drag
/// extends its high end past the glyph under it, so after a leftward drag the cursor
/// matches the range's start while the real anchor is neither end.
#[test]
fn shift_extension_after_a_drag_grows_from_the_drags_own_anchor() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abcdef").unwrap();
    doc.focus(input).unwrap();

    doc.drive_input_drag(input, 5, 2).unwrap();
    assert_eq!(doc.input_selection(input).unwrap(), Some(2..6));
    assert_eq!(doc.input_cursor(input).unwrap(), 2);

    // Anchored at 5, so moving the cursor to 3 leaves 3..5. Reading the anchor off the
    // range instead would give 6, and this would be 3..6.
    doc.dispatch_key_press(KeyEvent::with_modifiers(
        KeyCode::Right,
        KeyModifiers::SHIFT,
    ));
    assert_eq!(doc.input_selection(input).unwrap(), Some(3..5));
}

/// Vertical and line-wise motions extend on the same rule as the horizontal ones, since
/// shift is read once against the motion rather than per binding.
#[test]
fn shift_extension_covers_vertical_and_line_motions() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("abc\ndef").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 1).unwrap();
    let shift = KeyModifiers::SHIFT;

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Down, shift));
    assert_eq!(doc.input_selection(input).unwrap(), Some(1.."abc\nd".len()));

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Home, shift));
    assert_eq!(doc.input_selection(input).unwrap(), Some(1.."abc\n".len()));

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::End, shift));
    assert_eq!(
        doc.input_selection(input).unwrap(),
        Some(1.."abc\ndef".len())
    );
}

/// Ctrl+A arrives as a plain letter with control held, so it has to be caught above the
/// insert arm — the same collision that made the chord guard necessary in the first place.
#[test]
fn control_a_selects_all_and_the_next_keystroke_replaces_it() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "hello");

    doc.dispatch_key_press(KeyEvent::with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(doc.input_selection(input).unwrap(), Some(0..5));
    assert_eq!(doc.input_value(input).unwrap(), "hello");
    assert!(seen.lock().unwrap().is_empty());

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));
    assert_eq!(doc.input_value(input).unwrap(), "x");
    assert_eq!(doc.input_cursor(input).unwrap(), 1);
    assert_eq!(doc.input_selection(input).unwrap(), None);
}

/// Ctrl+Up is not a binding, so it must not act like a plain Up — an unmatched chord
/// falls through instead of being quietly widened to its unmodified form.
#[test]
fn an_unbound_control_chord_does_not_act_as_its_plain_key() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("ab\ncd").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, "ab\nc".len()).unwrap();

    doc.dispatch_key_press(KeyEvent::with_modifiers(KeyCode::Up, KeyModifiers::CONTROL));

    assert_eq!(doc.input_cursor(input).unwrap(), "ab\nc".len());
}

#[test]
fn input_default_action_handles_enter_by_multiline_flag() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("ab").unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 1).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Enter));
    assert_eq!(doc.input_value(input).unwrap(), "ab");

    doc.set_input_multiline(input, true).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Enter));
    assert_eq!(doc.input_value(input).unwrap(), "a\nb");
}

#[test]
fn prevent_default_skips_input_default_action() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("").unwrap();
    doc.focus(input).unwrap();
    doc.on_key_press(input, |event| event.prevent_default())
        .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));

    assert_eq!(doc.input_value(input).unwrap(), "");
}

#[test]
fn input_default_action_takes_precedence_over_focus_navigation() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("a").unwrap();
    let right = doc.create_box().unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.append_child(doc.root(), right).unwrap();
    doc.set_focusable(right, true).unwrap();
    set_layout(
        &doc,
        input,
        LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    set_layout(
        &doc,
        right,
        LayoutRect {
            x: 2,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    doc.focus(input).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));

    assert_eq!(doc.focused(), Some(input));
}

#[test]
fn node_ids_are_scoped_to_their_document() {
    let first = Document::new().unwrap();
    let second = Document::new().unwrap();
    let first_root = first.root();
    let second_root = second.root();

    assert_ne!(first_root, second_root);
    assert!(second.get_node(first_root).is_none());

    let mut style = Style::new();
    style.width(Length::Cells(1));
    assert_eq!(
        second.set_style(first_root, &style),
        Err(TuidomError::NodeNotFound { id: first_root })
    );
    assert!(second.get_node(second_root).is_some());
}

#[test]
fn creating_dom_nodes_creates_persistent_layout_nodes() {
    let doc = Document::new().unwrap();
    let root = doc.create_box().unwrap();
    let text = doc.create_text("hello").unwrap();

    assert_eq!(doc.layout_node_count(), 3);
    assert_eq!(doc.layout_mapping_snapshot().len(), 3);
    assert!(
        doc.layout_mapping_snapshot()
            .iter()
            .any(|(id, _)| *id == root)
    );
    assert!(
        doc.layout_mapping_snapshot()
            .iter()
            .any(|(id, _)| *id == text)
    );
}

#[test]
fn repeated_layout_uses_same_taffy_nodes() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_text("hello").unwrap();
    doc.append_child(root, child).unwrap();

    let before = doc.layout_mapping_snapshot();
    doc.compute_layout(20, 5).unwrap();
    doc.compute_layout(20, 5).unwrap();
    let after = doc.layout_mapping_snapshot();

    assert_eq!(before, after);
}

#[test]
fn failed_layout_preserves_previous_snapshot() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();

    let mut child_style = Style::new();
    child_style.width(Length::Cells(7));
    child_style.height(Length::Cells(1));
    doc.set_style(child, &child_style).unwrap();
    doc.append_child(root, child).unwrap();
    doc.compute_layout(20, 5).unwrap();

    let before = doc.get_node(child).unwrap().layout.unwrap().rect;

    doc.remove_layout_mapping_for_test(child);

    assert_eq!(
        doc.compute_layout(20, 5),
        Err(TuidomError::LayoutMappingMissing { id: child })
    );
    let after = doc.get_node(child).unwrap().layout.unwrap().rect;
    assert_eq!(after.x, before.x);
    assert_eq!(after.y, before.y);
    assert_eq!(after.width, before.width);
    assert_eq!(after.height, before.height);
}

#[test]
fn reparenting_syncs_taffy_child_order() {
    let doc = Document::new().unwrap();
    let first_parent = doc.create_box().unwrap();
    let second_parent = doc.create_box().unwrap();
    let first = doc.create_text("first").unwrap();
    let second = doc.create_text("second").unwrap();
    let third = doc.create_text("third").unwrap();

    doc.append_child(first_parent, first).unwrap();
    doc.append_child(first_parent, second).unwrap();
    doc.insert_before(first_parent, third, second).unwrap();
    assert_eq!(
        doc.layout_children(first_parent),
        vec![first, third, second]
    );

    doc.move_child(second_parent, third, first).unwrap();
    assert_eq!(doc.layout_children(first_parent), vec![first, second]);
    assert_eq!(doc.layout_children(second_parent), vec![third]);
}

#[test]
fn inherited_style_change_updates_layout_without_recreating_taffy_nodes() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();

    let mut root_style = Style::new();
    root_style.width(Length::Cells(10));
    root_style.height(Length::Cells(1));
    doc.set_style(root, &root_style).unwrap();

    let mut child_style = Style::new();
    child_style.inherit_width();
    child_style.height(Length::Cells(1));
    doc.set_style(child, &child_style).unwrap();

    doc.append_child(root, child).unwrap();
    let before = doc.layout_mapping_snapshot();

    doc.compute_layout(100, 10).unwrap();
    assert_eq!(doc.get_node(child).unwrap().layout.unwrap().rect.width, 10);

    doc.update_style(root, |style| style.width(Length::Cells(20)))
        .unwrap();
    doc.compute_layout(100, 10).unwrap();

    assert_eq!(doc.layout_mapping_snapshot(), before);
    assert_eq!(doc.get_node(child).unwrap().layout.unwrap().rect.width, 20);
}

#[test]
fn removing_subtree_removes_layout_nodes() {
    let doc = Document::new().unwrap();
    let root = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    let grandchild = doc.create_text("deep").unwrap();

    doc.append_child(root, child).unwrap();
    doc.append_child(child, grandchild).unwrap();
    assert_eq!(doc.layout_node_count(), 4);

    doc.remove_child(root, child).unwrap();

    assert_eq!(doc.layout_node_count(), 2);
    assert_eq!(doc.layout_children(root), Vec::<NodeId>::new());
}

fn key_event() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('x'))
}

fn set_layout(doc: &Document, node: NodeId, layout: LayoutRect) {
    lock::rw_write(&doc.inner.layout_snapshot).insert(
        node,
        NodeLayout {
            rect: layout,
            ..NodeLayout::default()
        },
    );
}

fn one_cell() -> LayoutRect {
    LayoutRect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    }
}

fn set_one_cell_layouts(doc: &Document, nodes: &[NodeId]) {
    for node in nodes {
        set_layout(doc, *node, one_cell());
    }
}

fn set_z_index(doc: &Document, node: NodeId, z_index: i32) {
    doc.update_style(node, |style| style.z_index(z_index))
        .unwrap();
}

fn targeted_listener_count(doc: &Document) -> usize {
    lock::mutex(&doc.inner.targeted_listeners)
        .values()
        .map(Vec::len)
        .sum()
}

#[test]
fn node_at_returns_none_before_layout_is_available() {
    let doc = Document::new().unwrap();
    assert_eq!(doc.node_at(0, 0), None);
}

#[test]
fn node_at_uses_layout_bounds() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    set_layout(&doc, root, one_cell());

    assert_eq!(doc.node_at(0, 0), Some(root));
    assert_eq!(doc.node_at(1, 0), None);
    assert_eq!(doc.node_at(0, 1), None);
    assert_eq!(doc.node_at(-1, 0), None);
}

#[test]
fn node_at_uses_dom_order_for_equal_z_index() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let first = doc.create_box().unwrap();
    let second = doc.create_box().unwrap();

    doc.append_child(root, first).unwrap();
    doc.append_child(root, second).unwrap();
    set_one_cell_layouts(&doc, &[root, first, second]);

    assert_eq!(doc.node_at(0, 0), Some(second));
}

#[test]
fn node_at_uses_z_index_over_dom_order() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let high = doc.create_box().unwrap();
    let low = doc.create_box().unwrap();

    set_z_index(&doc, high, 10);
    set_z_index(&doc, low, 0);
    doc.append_child(root, high).unwrap();
    doc.append_child(root, low).unwrap();
    set_one_cell_layouts(&doc, &[root, high, low]);

    assert_eq!(doc.node_at(0, 0), Some(high));
}

#[test]
fn node_at_skips_display_none_and_opacity_zero_nodes() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let visible = doc.create_box().unwrap();
    let hidden = doc.create_box().unwrap();
    let transparent = doc.create_box().unwrap();

    doc.update_style(hidden, |style| style.display(Display::None))
        .unwrap();
    doc.update_style(transparent, |style| style.opacity(0.0))
        .unwrap();
    doc.append_child(root, visible).unwrap();
    doc.append_child(root, hidden).unwrap();
    doc.append_child(root, transparent).unwrap();
    set_one_cell_layouts(&doc, &[root, visible, hidden, transparent]);

    assert_eq!(doc.node_at(0, 0), Some(visible));
}

#[test]
fn node_at_keeps_descendant_z_index_inside_parent_subtree() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    let sibling = doc.create_box().unwrap();

    set_z_index(&doc, parent, 0);
    set_z_index(&doc, child, 999);
    set_z_index(&doc, sibling, 1);
    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();
    doc.append_child(root, sibling).unwrap();
    set_one_cell_layouts(&doc, &[root, parent, child, sibling]);

    assert_eq!(doc.node_at(0, 0), Some(sibling));
}

// ---------------------------------------------------------------------------
// Scrolling
// ---------------------------------------------------------------------------

/// A 10×4 screen with one `overflow_y: Scroll` column of eight one-row texts,
/// so the container's scrollable range is 4.
fn scrolling_column() -> (Document, NodeId, Vec<NodeId>) {
    let doc = Document::new().unwrap();

    let container = doc.create_box().unwrap();
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Scroll);
    doc.set_style(container, &style).unwrap();
    doc.append_child(doc.root(), container).unwrap();

    let mut lines = Vec::new();
    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(container, text).unwrap();
        lines.push(text);
    }

    doc.compute_layout(10, 4).unwrap();
    (doc, container, lines)
}

#[test]
fn scroll_to_clamps_to_the_scrollable_range() {
    let (doc, container, _) = scrolling_column();

    doc.scroll_to(container, 5, 99).unwrap();
    // The horizontal axis is not scrollable, so it clamps to zero; the vertical
    // axis clamps to content minus viewport.
    let offset = doc.scroll_offset(container);
    assert_eq!((offset.x, offset.y), (0, 4));

    doc.scroll_by(container, 0, -1).unwrap();
    assert_eq!(doc.scroll_offset(container).y, 3);

    doc.scroll_by(container, 0, -100).unwrap();
    assert_eq!(doc.scroll_offset(container).y, 0);
}

/// An outer scroller, an inner one with nothing to scroll, and a focusable leaf inside
/// the inner one — the shape that makes keyboard chaining observable.
fn nested_scrolling_columns() -> (Document, NodeId, NodeId, NodeId) {
    let doc = Document::new().unwrap();
    let mut column = Style::new();
    column.flex_direction(FlexDirection::Column);
    column.overflow_y(Overflow::Scroll);

    let outer = doc.create_box().unwrap();
    doc.set_style(outer, &column).unwrap();
    doc.append_child(doc.root(), outer).unwrap();

    // Scrollable in principle with nothing to scroll: what routing must skip past. The
    // explicit height matters — a Scroll axis drops the content-size floor, so left to
    // size itself this box would collapse to zero and *gain* a scroll range.
    let inner = doc.create_box().unwrap();
    let mut inner_style = column.clone();
    inner_style.height(Length::Cells(1));
    inner_style.flex_shrink(0.0);
    doc.set_style(inner, &inner_style).unwrap();
    doc.append_child(outer, inner).unwrap();

    let leaf = doc.create_text("leaf").unwrap();
    doc.append_child(inner, leaf).unwrap();
    doc.set_focusable(leaf, true).unwrap();

    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(outer, text).unwrap();
    }

    doc.compute_layout(10, 4).unwrap();
    (doc, outer, inner, leaf)
}

/// Page keys route the same way a wheel does: rootward to the nearest container that can
/// still move, skipping one that is scrollable but has nowhere to go.
#[test]
fn page_keys_scroll_the_nearest_container_that_can_move() {
    let (doc, outer, inner, leaf) = nested_scrolling_columns();
    doc.focus(leaf).unwrap();

    // Four visible rows means a page of three: one row is kept to read against.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));
    assert_eq!(doc.scroll_offset(outer).y, 3);
    assert_eq!(doc.scroll_offset(inner).y, 0);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageUp));
    assert_eq!(doc.scroll_offset(outer).y, 0);
}

/// Home and End take the whole range, clamped by `scroll_to` rather than by reading a
/// maximum here — so the extreme cannot disagree with the layout that defines it.
#[test]
fn home_and_end_reach_the_ends_of_the_scroll_range() {
    let (doc, container, lines) = scrolling_column();
    doc.set_focusable(lines[0], true).unwrap();
    doc.focus(lines[0]).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::End));
    assert_eq!(doc.scroll_offset(container).y, 4);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Home));
    assert_eq!(doc.scroll_offset(container).y, 0);
}

/// The precedence that keeps the two halves of this feature from colliding: an input
/// consumes its own page and line keys, so the container behind it never sees them.
#[test]
fn a_focused_input_takes_page_keys_before_the_container_behind_it() {
    let doc = Document::new().unwrap();
    let mut column = Style::new();
    column.flex_direction(FlexDirection::Column);
    column.overflow_y(Overflow::Scroll);

    let container = doc.create_box().unwrap();
    doc.set_style(container, &column).unwrap();
    doc.append_child(doc.root(), container).unwrap();

    let input = doc.create_input("l0\nl1\nl2\nl3\nl4\nl5").unwrap();
    doc.set_input_multiline(input, true).unwrap();
    doc.append_child(container, input).unwrap();
    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(container, text).unwrap();
    }

    doc.compute_layout(10, 4).unwrap();
    doc.focus(input).unwrap();
    doc.set_input_cursor(input, 0).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));

    assert!(doc.input_cursor(input).unwrap() > 0);
    assert_eq!(doc.scroll_offset(container).y, 0);
}

/// Keyboard scrolling is a default action like any other, so a listener can refuse it.
#[test]
fn prevent_default_on_a_key_press_suppresses_the_scroll() {
    let (doc, outer, _, leaf) = nested_scrolling_columns();
    doc.focus(leaf).unwrap();
    doc.on_key_press(leaf, |event| event.prevent_default())
        .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::PageDown));

    assert_eq!(doc.scroll_offset(outer).y, 0);
}

/// A pane with nothing focusable inside it never receives focus by hovering, so without
/// the pointer fallback its page keys would do nothing at all.
#[test]
fn page_keys_fall_back_to_the_pane_under_the_pointer() {
    let doc = Document::new().unwrap();
    let mut column = Style::new();
    column.flex_direction(FlexDirection::Column);
    column.overflow_y(Overflow::Scroll);

    let pane = doc.create_box().unwrap();
    doc.set_style(pane, &column).unwrap();
    doc.append_child(doc.root(), pane).unwrap();
    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(pane, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();

    // An unmoved pointer is no pointer: the fallback must not invent a position.
    runtime.simulate_key(KeyCode::PageDown);
    assert_eq!(doc.scroll_offset(pane).y, 0);

    runtime.simulate_mouse_move(2, 2);
    assert_eq!(doc.focused(), None, "nothing in the pane is focusable");

    runtime.simulate_key(KeyCode::PageDown);
    assert_eq!(doc.scroll_offset(pane).y, 3);
}

/// The pointer is a fallback, not a priority. A parked mouse must not outrank a
/// deliberate Tab, since a keyboard user has no reason to think about where it sits.
#[test]
fn focus_outranks_the_pointer_when_the_two_disagree() {
    let doc = Document::new().unwrap();
    let mut row = Style::new();
    row.flex_direction(FlexDirection::Row);
    doc.set_style(doc.root(), &row).unwrap();

    let mut column = Style::new();
    column.flex_direction(FlexDirection::Column);
    column.overflow_y(Overflow::Scroll);
    column.width(Length::Cells(5));

    let mut panes = Vec::new();
    for pane_index in 0..2 {
        let pane = doc.create_box().unwrap();
        doc.set_style(pane, &column).unwrap();
        doc.append_child(doc.root(), pane).unwrap();
        for line in 0..8 {
            let text = doc.create_text(format!("p{pane_index}l{line}")).unwrap();
            doc.append_child(pane, text).unwrap();
            // Only the left pane holds anything focusable.
            if pane_index == 0 && line == 0 {
                doc.set_focusable(text, true).unwrap();
                doc.focus(text).unwrap();
            }
        }
        panes.push(pane);
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();

    // Pointer parked over the right pane, focus deliberately in the left one.
    runtime.simulate_mouse_move(7, 2);
    runtime.simulate_key(KeyCode::PageDown);

    assert_eq!(doc.scroll_offset(panes[0]).y, 3);
    assert_eq!(doc.scroll_offset(panes[1]).y, 0);

    // Proves the assertion above was not vacuous: the same pointer position does reach
    // the right pane, and that pane can scroll — it was outranked, not unreachable.
    doc.blur();
    runtime.simulate_key(KeyCode::PageDown);
    assert_eq!(doc.scroll_offset(panes[1]).y, 3);
}

/// Arrows stay spatial focus navigation. A document where an arrow sometimes moves focus
/// and sometimes scrolls has no rule a user can learn, so the defaults bind none.
#[test]
fn arrow_keys_are_not_bound_to_scrolling_by_default() {
    let (doc, outer, _, leaf) = nested_scrolling_columns();
    doc.focus(leaf).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Down));

    assert_eq!(doc.scroll_offset(outer).y, 0);
    assert!(doc.scroll_keys().down.is_empty());
}

#[test]
fn only_scroll_overflow_is_scrollable() {
    let (doc, container, _) = scrolling_column();

    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Clip);
    doc.set_style(container, &style).unwrap();
    doc.compute_layout(10, 4).unwrap();

    doc.scroll_to(container, 0, 2).unwrap();
    assert_eq!(doc.scroll_offset(container).y, 0);
}

#[test]
fn node_view_exposes_scrollport_and_max_scroll() {
    let (doc, container, lines) = scrolling_column();

    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!((layout.max_scroll_x, layout.max_scroll_y), (0, 4));
    // Borderless, so the scrollport is the whole rect.
    assert_eq!(layout.scrollport.height, layout.rect.height);
    assert_eq!(layout.scrollport.width, layout.rect.width);

    // A non-scroll node overflowed by nothing reports no scroll range either way.
    let line = doc.get_node(lines[0]).unwrap().layout.unwrap();
    assert_eq!((line.max_scroll_x, line.max_scroll_y), (0, 0));

    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Scroll);
    style.border(Border::new(BorderCharset::single()));
    doc.set_style(container, &style).unwrap();
    doc.compute_layout(10, 4).unwrap();

    // The scrollport is the padding box: the border frames it on every side.
    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!(layout.scrollport.x, layout.rect.x + 1);
    assert_eq!(layout.scrollport.y, layout.rect.y + 1);
    assert_eq!(layout.scrollport.width, layout.rect.width - 2);
    assert_eq!(layout.scrollport.height, layout.rect.height - 2);
}

#[test]
fn node_view_max_scroll_is_zero_without_scroll_overflow() {
    let (doc, container, _) = scrolling_column();

    // Same overflowing content, but a Clip axis offers no scroll range.
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Clip);
    doc.set_style(container, &style).unwrap();
    doc.compute_layout(10, 4).unwrap();

    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!((layout.max_scroll_x, layout.max_scroll_y), (0, 0));
}

#[test]
fn scrolling_a_removed_node_errors() {
    let (doc, container, _) = scrolling_column();

    doc.remove_child(doc.root(), container).unwrap();
    assert!(doc.scroll_to(container, 0, 1).is_err());
}

#[test]
fn node_at_sees_scrolled_content_at_its_translated_position() {
    let (doc, container, lines) = scrolling_column();

    assert_eq!(doc.node_at(0, 0), Some(lines[0]));

    doc.scroll_to(container, 0, 2).unwrap();
    assert_eq!(doc.node_at(0, 0), Some(lines[2]));
    assert_eq!(doc.node_at(0, 3), Some(lines[5]));

    // The scrolled-away line is culled: no coordinate hits it.
    for y in 0..4 {
        assert_ne!(doc.node_at(0, y), Some(lines[0]));
    }
}

#[test]
fn relayout_reclamps_scroll_offsets_when_content_shrinks() {
    let (doc, container, lines) = scrolling_column();

    doc.scroll_to(container, 0, 4).unwrap();
    assert_eq!(doc.scroll_offset(container).y, 4);

    for line in &lines[2..] {
        doc.remove_child(container, *line).unwrap();
    }
    doc.compute_layout(10, 4).unwrap();

    // Two rows of content in a four-row viewport leaves nothing to scroll.
    assert_eq!(doc.scroll_offset(container).y, 0);
}

#[test]
fn relayout_reclamp_fires_on_scroll_with_the_clamped_offset() {
    let (doc, container, lines) = scrolling_column();
    doc.scroll_to(container, 0, 4).unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_handler = events.clone();
    doc.on_scroll(container, move |event| {
        events_for_handler
            .lock()
            .unwrap()
            .push((event.target(), event.x, event.y));
    })
    .unwrap();

    // Six rows of content in a four-row viewport: the offset clamps from 4 to 2.
    for line in &lines[6..] {
        doc.remove_child(container, *line).unwrap();
    }
    doc.compute_layout(10, 4).unwrap();
    assert_eq!(*events.lock().unwrap(), vec![(container, 0, 2)]);

    // A relayout that leaves the offset where it is reports nothing.
    doc.compute_layout(10, 4).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
}

#[test]
fn wheel_scrolls_the_nearest_scrollable_ancestor() {
    let (doc, container, _) = scrolling_column();
    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();

    // Wheel down over a text deep inside the container.
    runtime.simulate_scroll(0, 0, -1);
    assert_eq!(doc.scroll_offset(container).y, 1);

    // Wheel up moves back toward the start.
    runtime.simulate_scroll(0, 0, 1);
    assert_eq!(doc.scroll_offset(container).y, 0);

    // At the start, a wheel up has nowhere to go and nothing else to scroll.
    runtime.simulate_scroll(0, 0, 1);
    assert_eq!(doc.scroll_offset(container).y, 0);
}

#[test]
fn wheel_chains_to_the_outer_scroller_at_the_end() {
    let doc = Document::new().unwrap();

    let outer = doc.create_box().unwrap();
    let mut outer_style = Style::new();
    outer_style.flex_direction(FlexDirection::Column);
    outer_style.overflow_y(Overflow::Scroll);
    doc.set_style(outer, &outer_style).unwrap();
    doc.append_child(doc.root(), outer).unwrap();

    let inner = doc.create_box().unwrap();
    let mut inner_style = Style::new();
    inner_style.flex_direction(FlexDirection::Column);
    inner_style.overflow_y(Overflow::Scroll);
    inner_style.height(Length::Cells(2));
    inner_style.flex_shrink(0.0);
    doc.set_style(inner, &inner_style).unwrap();
    doc.append_child(outer, inner).unwrap();
    for i in 0..3 {
        let text = doc.create_text(format!("b{i}")).unwrap();
        doc.append_child(inner, text).unwrap();
    }

    for i in 0..4 {
        let text = doc.create_text(format!("a{i}")).unwrap();
        doc.append_child(outer, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();

    // The inner scroller has one row to give; the wheel moves it first.
    runtime.simulate_scroll(0, 0, -1);
    assert_eq!(doc.scroll_offset(inner).y, 1);
    assert_eq!(doc.scroll_offset(outer).y, 0);

    // At its end, the next wheel chains to the outer scroller.
    runtime.simulate_scroll(0, 0, -1);
    assert_eq!(doc.scroll_offset(inner).y, 1);
    assert_eq!(doc.scroll_offset(outer).y, 1);
}

#[test]
fn horizontal_wheel_scrolls_the_horizontal_axis() {
    let doc = Document::new().unwrap();

    let container = doc.create_box().unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(5));
    style.overflow_x(Overflow::Scroll);
    doc.set_style(container, &style).unwrap();
    doc.append_child(doc.root(), container).unwrap();
    for content in ["abcde", "fghij"] {
        let text = doc.create_text(content).unwrap();
        doc.append_child(container, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 1);
    runtime.render().unwrap();

    runtime.simulate_horizontal_scroll(0, 0, -2);
    let offset = doc.scroll_offset(container);
    assert_eq!((offset.x, offset.y), (2, 0));
}

#[test]
fn on_scroll_fires_only_when_the_offset_changes() {
    let (doc, container, _) = scrolling_column();

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_handler = events.clone();
    doc.on_scroll(container, move |event| {
        events_for_handler
            .lock()
            .unwrap()
            .push((event.target(), event.x, event.y));
    })
    .unwrap();

    doc.scroll_to(container, 0, 2).unwrap();
    doc.scroll_to(container, 0, 2).unwrap();
    doc.scroll_to(container, 0, 99).unwrap();
    doc.scroll_to(container, 0, 4).unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![(container, 0, 2), (container, 0, 4)]
    );
}

#[test]
fn prevent_default_suppresses_the_wheel_scroll() {
    let (doc, container, lines) = scrolling_column();

    doc.on_wheel(lines[0], |event| event.prevent_default())
        .unwrap();

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();
    runtime.simulate_scroll(0, 0, -1);

    assert_eq!(doc.scroll_offset(container).y, 0);
}

/// Downstream wiring of the spacer pattern: a scroll container holding a leading
/// spacer, the materialized window, and a trailing spacer, kept in sync with a
/// [`Virtualizer`]. This is the shape the `virtualize` module is designed around,
/// so it doubles as the end-to-end proof the engine primitives compose.
struct VirtualList {
    doc: Document,
    container: NodeId,
    lead: NodeId,
    trail: NodeId,
    virtualizer: Virtualizer,
    items: std::collections::BTreeMap<usize, NodeId>,
}

impl VirtualList {
    fn new(doc: &Document, count: usize) -> Self {
        let container = doc.create_box().unwrap();
        let mut style = Style::new();
        style.flex_direction(FlexDirection::Column);
        style.overflow_y(Overflow::Scroll);
        style.scrollbar_show(ScrollbarShow::Never);
        doc.set_style(container, &style).unwrap();
        doc.append_child(doc.root(), container).unwrap();

        let lead = doc.create_box().unwrap();
        doc.append_child(container, lead).unwrap();
        let trail = doc.create_box().unwrap();
        doc.append_child(container, trail).unwrap();

        Self {
            doc: doc.clone(),
            container,
            lead,
            trail,
            virtualizer: Virtualizer::uniform(count, 1, 2),
            items: std::collections::BTreeMap::new(),
        }
    }

    fn apply(&mut self, offset: u16, viewport: u16) {
        let Some(update) = self.virtualizer.update(offset, viewport) else {
            return;
        };

        for range in update.remove {
            for index in range {
                if let Some(node) = self.items.remove(&index) {
                    self.doc.remove_child(self.container, node).unwrap();
                }
            }
        }
        for range in update.add {
            for index in range {
                let node = self.doc.create_text(format!("item{index}")).unwrap();
                // Before the next materialized item, or before the trailing spacer.
                let before = self
                    .items
                    .range(index + 1..)
                    .next()
                    .map(|(_, node)| *node)
                    .unwrap_or(self.trail);
                self.doc
                    .insert_before(self.container, node, before)
                    .unwrap();
                self.items.insert(index, node);
            }
        }

        self.set_spacer(self.lead, update.window.lead);
        self.set_spacer(self.trail, update.window.trail);
    }

    fn set_spacer(&self, spacer: NodeId, cells: u64) {
        let mut style = Style::new();
        style.height(Length::Cells(u16::try_from(cells).unwrap_or(u16::MAX)));
        // An empty Box has no content floor, so default flex shrink would collapse
        // the spacer to fit the container — and with it the whole scroll range.
        style.flex_shrink(0.0);
        self.doc.set_style(spacer, &style).unwrap();
    }
}

#[test]
fn a_virtualized_list_stays_correct_and_bounded_over_large_scrolls() {
    let doc = Document::new().unwrap();
    let list = Arc::new(Mutex::new(VirtualList::new(&doc, 10_000)));
    let container = list.lock().unwrap().container;

    let list_for_handler = list.clone();
    doc.on_scroll(container, move |event| {
        list_for_handler.lock().unwrap().apply(event.y, 4);
    })
    .unwrap();

    // Initial fill; every later window change flows from on_scroll.
    list.lock().unwrap().apply(0, 4);
    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();

    let row = |runtime: &HeadlessRuntime, y: i32| -> String {
        (0..10)
            .map(|x| runtime.get_cell(x, y).unwrap().text)
            .collect::<String>()
            .trim_end()
            .to_owned()
    };
    assert_eq!(row(&runtime, 0), "item0");
    assert_eq!(row(&runtime, 3), "item3");

    // Spacers make the content its true total: 10k rows minus the 4-row viewport.
    let layout = doc.get_node(container).unwrap().layout.unwrap();
    assert_eq!(layout.max_scroll_y, 9_996);

    // A deep jump: the handler rebuilds the window before the next frame.
    doc.scroll_to(container, 0, 5_000).unwrap();
    runtime.render().unwrap();
    assert_eq!(row(&runtime, 0), "item5000");
    assert_eq!(row(&runtime, 3), "item5003");

    // The DOM holds the window plus overscan, never the collection: the four visible
    // items, two overscan on each side, both spacers, the container, and the root.
    assert_eq!(list.lock().unwrap().items.len(), 8);
    assert_eq!(doc.layout_node_count(), 12);

    // Clamped at the very end, the last items materialize flush with the bottom.
    doc.scroll_to(container, 0, u16::MAX).unwrap();
    runtime.render().unwrap();
    assert_eq!(doc.scroll_offset(container).y, 9_996);
    assert_eq!(row(&runtime, 0), "item9996");
    assert_eq!(row(&runtime, 3), "item9999");
    assert!(doc.layout_node_count() <= 13);
}

#[test]
fn max_fps_defaults_to_uncapped_and_can_be_configured() {
    let doc = Document::new().unwrap();

    assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());

    doc.set_max_fps(Some(120.0));
    assert_eq!(
        *lock::rw_read(&doc.inner.max_frame_interval),
        Some(Duration::try_from_secs_f64(1.0 / 120.0).unwrap())
    );

    doc.set_max_fps(None);
    assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());
}

#[test]
fn invalid_max_fps_values_disable_the_cap() {
    let doc = Document::new().unwrap();

    for fps in [
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::MIN_POSITIVE),
    ] {
        doc.set_max_fps(fps);
        assert!(lock::rw_read(&doc.inner.max_frame_interval).is_none());
    }
}

#[test]
fn animation_fps_defaults_paced_and_none_unrestricts() {
    let doc = Document::new().unwrap();

    assert!(lock::rw_read(&doc.inner.animation_frame_interval).is_some());

    doc.set_animation_fps(Some(30.0));
    assert_eq!(
        *lock::rw_read(&doc.inner.animation_frame_interval),
        Some(Duration::try_from_secs_f64(1.0 / 30.0).unwrap())
    );

    doc.set_animation_fps(None);
    assert!(lock::rw_read(&doc.inner.animation_frame_interval).is_none());

    doc.set_animation_fps(Some(f64::NAN));
    assert!(lock::rw_read(&doc.inner.animation_frame_interval).is_none());
}

#[test]
fn listener_handle_removes_registered_listener() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_handler = calls.clone();

    let handle = doc
        .on_key_press(root, move |_| {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

    doc.dispatch_key_press(key_event());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    assert!(doc.remove_listener(handle));
    assert!(!doc.remove_listener(handle));

    doc.dispatch_key_press(key_event());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn listener_handles_are_scoped_to_their_document() {
    let first = Document::new().unwrap();
    let second = Document::new().unwrap();
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));

    let first_calls_for_handler = first_calls.clone();
    let first_handle = first
        .on_key_press(first.root(), move |_| {
            first_calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

    let second_calls_for_handler = second_calls.clone();
    second
        .on_key_press(second.root(), move |_| {
            second_calls_for_handler.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();

    assert!(!second.remove_listener(first_handle));

    first.dispatch_key_press(key_event());
    second.dispatch_key_press(key_event());
    assert_eq!(first_calls.load(Ordering::Relaxed), 1);
    assert_eq!(second_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn listener_can_register_listener_during_dispatch() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let calls = Arc::new(AtomicUsize::new(0));
    let doc_for_handler = doc.clone();
    let calls_for_handler = calls.clone();

    doc.on_key_press(root, move |_| {
        calls_for_handler.fetch_add(1, Ordering::Relaxed);
        let calls_for_new_handler = calls_for_handler.clone();
        doc_for_handler
            .on_key_press(root, move |_| {
                calls_for_new_handler.fetch_add(10, Ordering::Relaxed);
            })
            .unwrap();
    })
    .unwrap();

    doc.dispatch_key_press(key_event());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    doc.dispatch_key_press(key_event());
    assert_eq!(calls.load(Ordering::Relaxed), 12);
}

#[test]
fn listener_panic_is_caught_and_later_listeners_still_run() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_handler = calls.clone();

    doc.on_key_press(root, |_| panic!("listener boom")).unwrap();
    doc.on_key_press(root, move |_| {
        calls_for_handler.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    doc.dispatch_key_press(key_event());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn key_dispatch_targets_root_until_focus_exists() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();
    doc.append_child(root, child).unwrap();

    let root_calls = Arc::new(AtomicUsize::new(0));
    let child_calls = Arc::new(AtomicUsize::new(0));

    let root_calls_for_handler = root_calls.clone();
    doc.on_key_press(root, move |_| {
        root_calls_for_handler.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    let child_calls_for_handler = child_calls.clone();
    doc.on_key_press(child, move |_| {
        child_calls_for_handler.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    doc.dispatch_key_press(key_event());

    assert_eq!(root_calls.load(Ordering::Relaxed), 1);
    assert_eq!(child_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn key_dispatch_targets_focused_node_when_present() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();
    doc.append_child(root, child).unwrap();
    doc.set_focusable(child, true).unwrap();
    doc.focus(child).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));

    let root_calls = calls.clone();
    doc.on_key_press(root, move |event| {
        root_calls.lock().unwrap().push((
            "root",
            event.target(),
            event.current_target(),
            event.phase(),
        ));
    })
    .unwrap();

    let child_calls = calls.clone();
    doc.on_key_press(child, move |event| {
        child_calls.lock().unwrap().push((
            "child",
            event.target(),
            event.current_target(),
            event.phase(),
        ));
    })
    .unwrap();

    doc.dispatch_key_press(key_event());

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("child", child, child, EventPhase::Target),
            ("root", child, root, EventPhase::Bubble),
        ]
    );
}

#[test]
fn focusable_state_and_manual_focus_api_work() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    assert_eq!(doc.focused(), None);
    assert!(!doc.is_focusable(node).unwrap());
    assert_eq!(
        doc.focus(node),
        Err(TuidomError::NodeNotFocusable { id: node })
    );

    doc.set_focusable(node, true).unwrap();
    assert!(doc.is_focusable(node).unwrap());
    doc.focus(node).unwrap();
    assert_eq!(doc.focused(), Some(node));

    doc.blur();
    assert_eq!(doc.focused(), None);
}

#[test]
fn focus_keys_default_to_standard_navigation_keys() {
    let doc = Document::new().unwrap();

    assert_eq!(
        doc.focus_keys(),
        FocusKeys {
            next: vec![(KeyCode::Tab, KeyModifiers::empty())],
            previous: vec![(KeyCode::BackTab, KeyModifiers::empty())],
            up: vec![(KeyCode::Up, KeyModifiers::empty())],
            down: vec![(KeyCode::Down, KeyModifiers::empty())],
            left: vec![(KeyCode::Left, KeyModifiers::empty())],
            right: vec![(KeyCode::Right, KeyModifiers::empty())],
            blur: vec![(KeyCode::Esc, KeyModifiers::empty())],
        }
    );
}

#[test]
fn focus_keys_are_configurable() {
    let doc = Document::new().unwrap();
    let plain = KeyModifiers::empty();
    let keys = FocusKeys {
        next: vec![(KeyCode::Char('n'), plain)],
        previous: vec![(KeyCode::Char('p'), plain)],
        up: vec![(KeyCode::Char('k'), plain)],
        down: vec![(KeyCode::Char('j'), plain)],
        left: vec![(KeyCode::Char('h'), plain)],
        right: vec![(KeyCode::Char('l'), plain)],
        blur: vec![(KeyCode::Char('q'), plain)],
    };

    doc.set_focus_keys(keys.clone());

    assert_eq!(doc.focus_keys(), keys);
}

#[test]
fn escape_dispatches_normally_when_no_node_is_focused() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_handler = calls.clone();
    doc.on_key_press(root, move |event| {
        if event.code == KeyCode::Esc {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
        }
    })
    .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Esc));

    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(doc.focused(), None);
}

#[test]
fn focus_style_merges_into_focused_node_style() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.width(Length::Cells(1));
    base.height(Length::Cells(1));
    base.color(Color::blue());
    doc.set_style(node, &base).unwrap();

    let mut focus = Style::new();
    focus.color(Color::red());
    focus.background(Color::green());
    doc.set_focus_style(node, &focus).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::blue()
    );
    assert_eq!(doc.resolved_style(node).unwrap().background, None);

    doc.focus(node).unwrap();
    let focused = doc.resolved_style(node).unwrap();
    assert_eq!(focused.width, Length::Cells(1));
    assert_eq!(focused.height, Length::Cells(1));
    assert_eq!(focused.color, ResolvedColor::red());
    assert_eq!(focused.background, Some(ResolvedColor::green()));

    doc.clear_focus_style(node).unwrap();
    let cleared = doc.resolved_style(node).unwrap();
    assert_eq!(cleared.color, ResolvedColor::blue());
    assert_eq!(cleared.background, None);
}

#[test]
fn focus_style_affects_rendered_output() {
    let doc = Document::new().unwrap();
    let node = doc.create_text("A").unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut focus = Style::new();
    focus.color(Color::red());
    doc.set_focus_style(node, &focus).unwrap();
    doc.focus(node).unwrap();

    let mut runtime = HeadlessRuntime::new(doc, 5, 2);
    runtime.render().unwrap();

    let fg = runtime.get_cell(0, 0).unwrap().fg.unwrap();
    assert!(fg.r > fg.g);
    assert!(fg.r > fg.b);
}

#[test]
fn focus_style_layout_effect_refreshes_on_focus_change() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut base = Style::new();
    base.width(Length::Cells(1));
    base.height(Length::Cells(1));
    doc.set_style(node, &base).unwrap();

    let mut focus = Style::new();
    focus.width(Length::Cells(4));
    doc.set_focus_style(node, &focus).unwrap();

    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().rect.width, 1);

    doc.focus(node).unwrap();
    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().rect.width, 4);

    doc.blur();
    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().rect.width, 1);
}

#[test]
fn focus_default_action_blurs_on_configured_key() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();
    doc.focus(node).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Esc));

    assert_eq!(doc.focused(), None);
}

#[test]
fn prevent_default_skips_focus_default_action() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();
    doc.focus(node).unwrap();

    doc.on_key_press(node, |event| {
        if event.code == KeyCode::Esc {
            event.prevent_default();
        }
    })
    .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Esc));

    assert_eq!(doc.focused(), Some(node));
}

#[test]
fn configured_focus_keys_drive_default_actions() {
    let doc = Document::new().unwrap();
    let first = doc.create_box().unwrap();
    let second = doc.create_box().unwrap();
    doc.append_child(doc.root(), first).unwrap();
    doc.append_child(doc.root(), second).unwrap();
    doc.set_focusable(first, true).unwrap();
    doc.set_focusable(second, true).unwrap();

    let keys = FocusKeys {
        next: vec![(KeyCode::Char('n'), KeyModifiers::empty())],
        previous: vec![(KeyCode::Char('p'), KeyModifiers::empty())],
        up: Vec::new(),
        down: Vec::new(),
        left: Vec::new(),
        right: Vec::new(),
        blur: vec![(KeyCode::Char('q'), KeyModifiers::empty())],
    };
    doc.set_focus_keys(keys);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('n')));
    assert_eq!(doc.focused(), Some(first));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('n')));
    assert_eq!(doc.focused(), Some(second));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('p')));
    assert_eq!(doc.focused(), Some(first));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('q')));
    assert_eq!(doc.focused(), None);
}

#[test]
fn tab_navigation_uses_dom_order_without_wrapping() {
    let doc = Document::new().unwrap();
    let first = doc.create_box().unwrap();
    let parent = doc.create_box().unwrap();
    let nested = doc.create_box().unwrap();
    doc.append_child(doc.root(), first).unwrap();
    doc.append_child(doc.root(), parent).unwrap();
    doc.append_child(parent, nested).unwrap();
    for node in [first, parent, nested] {
        doc.set_focusable(node, true).unwrap();
    }

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(first));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(parent));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(nested));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(nested));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::BackTab));
    assert_eq!(doc.focused(), Some(parent));
}

#[test]
fn backtab_from_none_focuses_last_focusable_node() {
    let doc = Document::new().unwrap();
    let first = doc.create_box().unwrap();
    let second = doc.create_box().unwrap();
    doc.append_child(doc.root(), first).unwrap();
    doc.append_child(doc.root(), second).unwrap();
    doc.set_focusable(first, true).unwrap();
    doc.set_focusable(second, true).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::BackTab));

    assert_eq!(doc.focused(), Some(second));
}

#[test]
fn spatial_navigation_chooses_nearest_focusable_node() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let current = doc.create_box().unwrap();
    let near_right = doc.create_box().unwrap();
    let far_right = doc.create_box().unwrap();
    doc.append_child(root, current).unwrap();
    doc.append_child(root, near_right).unwrap();
    doc.append_child(root, far_right).unwrap();
    for node in [current, near_right, far_right] {
        doc.set_focusable(node, true).unwrap();
    }
    set_layout(
        &doc,
        root,
        LayoutRect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        },
    );
    set_layout(
        &doc,
        current,
        LayoutRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        },
    );
    set_layout(
        &doc,
        near_right,
        LayoutRect {
            x: 4,
            y: 1,
            width: 2,
            height: 2,
        },
    );
    set_layout(
        &doc,
        far_right,
        LayoutRect {
            x: 10,
            y: 1,
            width: 2,
            height: 2,
        },
    );
    doc.focus(current).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));

    assert_eq!(doc.focused(), Some(near_right));
}

/// Spatial navigation reads published layout rects, so an absolute node must be
/// reachable at the position its offset puts it, not at its DOM-order flow slot.
#[test]
fn spatial_navigation_reaches_absolute_node_at_its_offset_position() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let current = doc.create_box().unwrap();
    let absolute = doc.create_box().unwrap();

    let mut root_style = Style::new();
    root_style.align_items(AlignItems::FlexStart);
    doc.set_style(root, &root_style).unwrap();

    let mut current_style = Style::new();
    current_style.width(Length::Cells(2));
    current_style.height(Length::Cells(2));
    doc.set_style(current, &current_style).unwrap();

    let mut absolute_style = Style::new();
    absolute_style.width(Length::Cells(2));
    absolute_style.height(Length::Cells(2));
    absolute_style.position(Position::Absolute { x: 10, y: 0 });
    doc.set_style(absolute, &absolute_style).unwrap();

    doc.append_child(root, current).unwrap();
    doc.append_child(root, absolute).unwrap();
    for node in [current, absolute] {
        doc.set_focusable(node, true).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 20, 10);
    runtime.render().unwrap();

    doc.focus(current).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));

    assert_eq!(doc.focused(), Some(absolute));
}

#[test]
fn spatial_navigation_uses_topmost_tiebreaker_and_does_not_wrap() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let current = doc.create_box().unwrap();
    let low = doc.create_box().unwrap();
    let high = doc.create_box().unwrap();
    doc.append_child(root, current).unwrap();
    doc.append_child(root, low).unwrap();
    doc.append_child(root, high).unwrap();
    for node in [current, low, high] {
        doc.set_focusable(node, true).unwrap();
    }
    set_z_index(&doc, low, 1);
    set_z_index(&doc, high, 2);
    set_layout(
        &doc,
        root,
        LayoutRect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        },
    );
    set_layout(
        &doc,
        current,
        LayoutRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        },
    );
    for node in [low, high] {
        set_layout(
            &doc,
            node,
            LayoutRect {
                x: 4,
                y: 1,
                width: 2,
                height: 2,
            },
        );
    }
    doc.focus(current).unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));
    assert_eq!(doc.focused(), Some(high));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Right));
    assert_eq!(doc.focused(), Some(high));
}

#[test]
fn focus_and_blur_events_bubble_with_relation() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();
    doc.set_focusable(child, true).unwrap();

    let focus_calls = Arc::new(Mutex::new(Vec::new()));
    for node in [root, parent, child] {
        let calls = focus_calls.clone();
        doc.on_focus(node, move |event| {
            calls.lock().unwrap().push((
                event.target(),
                event.current_target(),
                event.phase(),
                event.relation(),
            ));
        })
        .unwrap();
    }

    let blur_calls = Arc::new(Mutex::new(Vec::new()));
    for node in [root, parent, child] {
        let calls = blur_calls.clone();
        doc.on_blur(node, move |event| {
            calls.lock().unwrap().push((
                event.target(),
                event.current_target(),
                event.phase(),
                event.relation(),
            ));
        })
        .unwrap();
    }

    doc.focus(child).unwrap();
    doc.blur();

    let expected = vec![
        (
            child,
            child,
            EventPhase::Target,
            FocusEventRelation::SelfNode,
        ),
        (
            child,
            parent,
            EventPhase::Bubble,
            FocusEventRelation::Descendant,
        ),
        (
            child,
            root,
            EventPhase::Bubble,
            FocusEventRelation::Descendant,
        ),
    ];
    assert_eq!(*focus_calls.lock().unwrap(), expected);
    assert_eq!(*blur_calls.lock().unwrap(), expected);
}

#[test]
fn stop_propagation_prevents_focus_event_from_reaching_ancestors() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();
    doc.set_focusable(child, true).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));
    let child_calls = calls.clone();
    doc.on_focus(child, move |event| {
        child_calls.lock().unwrap().push("child");
        event.stop_propagation();
    })
    .unwrap();

    let parent_calls = calls.clone();
    doc.on_focus(parent, move |_| {
        parent_calls.lock().unwrap().push("parent");
    })
    .unwrap();

    let root_calls = calls.clone();
    doc.on_focus(root, move |_| {
        root_calls.lock().unwrap().push("root");
    })
    .unwrap();

    doc.focus(child).unwrap();

    assert_eq!(*calls.lock().unwrap(), vec!["child"]);
}

#[test]
fn focus_state_is_cleared_when_focused_node_is_removed() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();
    doc.append_child(root, child).unwrap();
    doc.set_focusable(child, true).unwrap();
    doc.focus(child).unwrap();

    doc.remove_child(root, child).unwrap();

    assert_eq!(doc.focused(), None);
    assert_eq!(
        doc.is_focusable(child),
        Err(TuidomError::NodeNotFound { id: child })
    );
}

#[test]
fn registering_listener_on_missing_node_returns_error() {
    let doc = Document::new().unwrap();
    let result = doc.on_key_press(NodeId::new(999), |_| {});
    assert!(matches!(result, Err(TuidomError::NodeNotFound { .. })));
}

#[test]
fn targeted_event_bubbles_from_target_to_root() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));
    for node in [root, parent, child] {
        let calls_for_handler = calls.clone();
        doc.on_key_press(node, move |event| {
            calls_for_handler
                .lock()
                .unwrap()
                .push((event.current_target(), event.phase()));
        })
        .unwrap();
    }

    let mut event = key_event();
    doc.dispatch_key_press_to(child, &mut event);

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            (child, EventPhase::Target),
            (parent, EventPhase::Bubble),
            (root, EventPhase::Bubble),
        ]
    );
    assert_eq!(event.target(), child);
}

#[test]
fn stop_propagation_prevents_ancestor_dispatch_after_current_node() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(root, parent).unwrap();
    doc.append_child(parent, child).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));

    let calls_for_child = calls.clone();
    doc.on_key_press(child, move |_| {
        calls_for_child.lock().unwrap().push("child");
    })
    .unwrap();

    let calls_for_parent_first = calls.clone();
    doc.on_key_press(parent, move |event| {
        calls_for_parent_first.lock().unwrap().push("parent-first");
        event.stop_propagation();
    })
    .unwrap();

    let calls_for_parent_second = calls.clone();
    doc.on_key_press(parent, move |_| {
        calls_for_parent_second
            .lock()
            .unwrap()
            .push("parent-second");
    })
    .unwrap();

    let calls_for_root = calls.clone();
    doc.on_key_press(root, move |_| {
        calls_for_root.lock().unwrap().push("root");
    })
    .unwrap();

    let mut event = key_event();
    doc.dispatch_key_press_to(child, &mut event);

    assert_eq!(
        *calls.lock().unwrap(),
        vec!["child", "parent-first", "parent-second"]
    );
    assert!(event.propagation_stopped());
}

#[test]
fn key_prevent_default_does_not_stop_propagation() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();
    doc.append_child(root, child).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));

    let child_calls = calls.clone();
    doc.on_key_press(child, move |event| {
        child_calls.lock().unwrap().push("child");
        event.prevent_default();
    })
    .unwrap();

    let root_calls = calls.clone();
    doc.on_key_press(root, move |_| {
        root_calls.lock().unwrap().push("root");
    })
    .unwrap();

    let mut event = key_event();
    doc.dispatch_key_press_to(child, &mut event);

    assert_eq!(*calls.lock().unwrap(), vec!["child", "root"]);
    assert!(event.default_prevented());
    assert!(!event.propagation_stopped());
}

#[test]
fn resize_listener_is_document_level() {
    let doc = Document::new().unwrap();
    let seen = Arc::new(Mutex::new(None));
    let seen_for_handler = seen.clone();

    doc.on_resize(move |event| {
        *seen_for_handler.lock().unwrap() = Some((event.width, event.height));
    });

    doc.dispatch_resize(ResizeEvent {
        width: 80,
        height: 24,
    });

    assert_eq!(*seen.lock().unwrap(), Some((80, 24)));
}

#[test]
fn mouse_button_listeners_dispatch_by_event_type() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let calls = Arc::new(Mutex::new(Vec::new()));

    let calls_for_down = calls.clone();
    doc.on_mouse_down(root, move |event| {
        calls_for_down
            .lock()
            .unwrap()
            .push(("down", event.x, event.y, event.button));
    })
    .unwrap();

    let calls_for_up = calls.clone();
    doc.on_mouse_up(root, move |event| {
        calls_for_up
            .lock()
            .unwrap()
            .push(("up", event.x, event.y, event.button));
    })
    .unwrap();

    let calls_for_click = calls.clone();
    doc.on_click(root, move |event| {
        calls_for_click
            .lock()
            .unwrap()
            .push(("click", event.x, event.y, event.button));
    })
    .unwrap();

    let mut down = MouseEvent::new(3, 4, MouseButton::Left);
    doc.dispatch_mouse_down_to(root, &mut down);
    let mut up = MouseEvent::new(3, 4, MouseButton::Left);
    doc.dispatch_mouse_up_to(root, &mut up);
    let mut click = MouseEvent::new(3, 4, MouseButton::Left);
    doc.dispatch_click_to(root, &mut click);

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("down", 3, 4, MouseButton::Left),
            ("up", 3, 4, MouseButton::Left),
            ("click", 3, 4, MouseButton::Left),
        ]
    );
}

#[test]
fn wheel_listener_dispatches_wheel_event() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let seen = Arc::new(Mutex::new(None));
    let seen_for_handler = seen.clone();

    doc.on_wheel(root, move |event| {
        *seen_for_handler.lock().unwrap() = Some((
            event.x,
            event.y,
            event.delta,
            event.target(),
            event.current_target(),
            event.phase(),
        ));
    })
    .unwrap();

    let mut event = WheelEvent::new(5, 6, -1);
    doc.dispatch_wheel_to(root, &mut event);

    assert_eq!(
        *seen.lock().unwrap(),
        Some((5, 6, -1, root, root, EventPhase::Target))
    );
}

#[test]
fn tree_ops() {
    let doc = Document::new().unwrap();

    let root = doc.create_box().unwrap();
    let child1 = doc.create_text("one").unwrap();
    let child2 = doc.create_text("two").unwrap();
    let child3 = doc.create_text("three").unwrap();

    // append
    doc.append_child(root, child1).unwrap();
    doc.append_child(root, child2).unwrap();
    assert_eq!(doc.get_children(root), vec![child1, child2]);

    // insert_before
    doc.insert_before(root, child3, child2).unwrap();
    assert_eq!(doc.get_children(root), vec![child1, child3, child2]);

    // move_child
    let other = doc.create_box().unwrap();
    doc.move_child(other, child3, child2).unwrap(); // inserts at end since child2 isn't in other
    assert_eq!(doc.get_children(root), vec![child1, child2]);
    assert_eq!(doc.get_children(other), vec![child3]);

    assert_eq!(doc.get_parent(child3), Some(other));
}

#[test]
fn append_child_reparents_without_stale_reference() {
    let doc = Document::new().unwrap();
    let first_parent = doc.create_box().unwrap();
    let second_parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();

    doc.append_child(first_parent, child).unwrap();
    doc.append_child(second_parent, child).unwrap();

    assert!(doc.get_children(first_parent).is_empty());
    assert_eq!(doc.get_children(second_parent), vec![child]);
    assert_eq!(doc.get_parent(child), Some(second_parent));
}

#[test]
fn append_child_does_not_duplicate_existing_child() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();

    doc.append_child(parent, child).unwrap();
    doc.append_child(parent, child).unwrap();

    assert_eq!(doc.get_children(parent), vec![child]);
    assert_eq!(doc.get_parent(child), Some(parent));
}

#[test]
fn insert_before_reorders_existing_child_without_duplicate() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let first = doc.create_text("first").unwrap();
    let second = doc.create_text("second").unwrap();
    let third = doc.create_text("third").unwrap();

    doc.append_child(parent, first).unwrap();
    doc.append_child(parent, second).unwrap();
    doc.append_child(parent, third).unwrap();
    doc.insert_before(parent, third, first).unwrap();

    assert_eq!(doc.get_children(parent), vec![third, first, second]);
    assert_eq!(doc.get_parent(third), Some(parent));
}

#[test]
fn cycle_attempt_returns_error_and_does_not_mutate() {
    let doc = Document::new().unwrap();
    let ancestor = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();

    doc.append_child(ancestor, child).unwrap();

    let err = doc.append_child(child, ancestor).unwrap_err();
    assert_eq!(
        err,
        TuidomError::TreeCycle {
            parent: child,
            child: ancestor,
        }
    );
    assert_eq!(doc.get_children(ancestor), vec![child]);
    assert!(doc.get_children(child).is_empty());
    assert_eq!(doc.get_parent(ancestor), None);
    assert_eq!(doc.get_parent(child), Some(ancestor));
}

#[test]
fn invalid_node_error_does_not_partially_mutate_tree() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();
    let missing = NodeId::new(999);

    assert_eq!(
        doc.append_child(parent, missing),
        Err(TuidomError::NodeNotFound { id: missing })
    );
    assert!(doc.get_children(parent).is_empty());

    assert_eq!(
        doc.append_child(missing, child),
        Err(TuidomError::NodeNotFound { id: missing })
    );
    assert_eq!(doc.get_parent(child), None);
}

#[test]
fn move_child_invalid_parent_does_not_detach_child() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();
    let missing = NodeId::new(999);

    doc.append_child(parent, child).unwrap();

    assert_eq!(
        doc.move_child(missing, child, child),
        Err(TuidomError::NodeNotFound { id: missing })
    );
    assert_eq!(doc.get_children(parent), vec![child]);
    assert_eq!(doc.get_parent(child), Some(parent));
}

#[test]
fn remove_child_noops_when_child_belongs_to_another_parent() {
    let doc = Document::new().unwrap();
    let unrelated_parent = doc.create_box().unwrap();
    let actual_parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();

    doc.append_child(actual_parent, child).unwrap();
    doc.remove_child(unrelated_parent, child).unwrap();

    assert!(doc.get_children(unrelated_parent).is_empty());
    assert_eq!(doc.get_children(actual_parent), vec![child]);
    assert!(doc.get_node(child).is_some());
    assert_eq!(doc.get_parent(child), Some(actual_parent));
}

#[test]
fn remove_child_missing_node_returns_error_without_mutation() {
    let doc = Document::new().unwrap();
    let parent = doc.create_box().unwrap();
    let child = doc.create_text("child").unwrap();
    let missing = NodeId::new(999);

    doc.append_child(parent, child).unwrap();

    assert_eq!(
        doc.remove_child(parent, missing),
        Err(TuidomError::NodeNotFound { id: missing })
    );
    assert_eq!(doc.get_children(parent), vec![child]);
    assert!(doc.get_node(child).is_some());

    assert_eq!(
        doc.remove_child(missing, child),
        Err(TuidomError::NodeNotFound { id: missing })
    );
    assert_eq!(doc.get_children(parent), vec![child]);
    assert_eq!(doc.get_parent(child), Some(parent));
}

#[test]
fn remove_subtree() {
    let doc = Document::new().unwrap();

    let root = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    let grandchild = doc.create_text("deep").unwrap();

    doc.append_child(root, child).unwrap();
    doc.append_child(child, grandchild).unwrap();

    doc.remove_child(root, child).unwrap();

    // grandchild is also gone
    assert!(doc.get_node(child).is_none());
    assert!(doc.get_node(grandchild).is_none());
    assert!(doc.get_children(root).is_empty());
}

#[test]
fn remove_subtree_removes_attached_listeners() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();
    let grandchild = doc.create_text("deep").unwrap();

    doc.append_child(root, child).unwrap();
    doc.append_child(child, grandchild).unwrap();
    doc.on_key_press(child, |_| {}).unwrap();
    doc.on_key_press(grandchild, |_| {}).unwrap();
    assert_eq!(targeted_listener_count(&doc), 2);

    doc.remove_child(root, child).unwrap();

    assert_eq!(targeted_listener_count(&doc), 0);
}

#[test]
fn remove_subtree_removes_active_animations() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let child = doc.create_box().unwrap();

    doc.append_child(root, child).unwrap();
    doc.set_transition(
        child,
        TransitionConfig::opacity(Duration::from_secs(60), Easing::Linear),
    )
    .unwrap();
    doc.update_style(child, |style| style.opacity(0.0)).unwrap();
    assert!(lock::mutex(&doc.inner.animation).has_active());

    doc.remove_child(root, child).unwrap();

    assert!(!lock::mutex(&doc.inner.animation).has_active());
}

#[test]
fn cannot_remove_document_root() {
    let doc = Document::new().unwrap();
    let root = doc.root();
    let parent = doc.create_box().unwrap();

    assert_eq!(
        doc.remove_child(parent, root),
        Err(TuidomError::CannotRemoveRoot { id: root })
    );
    assert!(doc.get_node(root).is_some());
    assert_eq!(doc.get_parent(root), None);
}

#[test]
fn is_descendant_of() {
    let doc = Document::new().unwrap();

    let a = doc.create_box().unwrap();
    let b = doc.create_box().unwrap();
    let c = doc.create_text("deep").unwrap();

    doc.append_child(a, b).unwrap();
    doc.append_child(b, c).unwrap();

    assert!(doc.is_descendant_of(c, a));
    assert!(doc.is_descendant_of(c, b));
    assert!(doc.is_descendant_of(b, a));
    assert!(!doc.is_descendant_of(a, c));
    assert!(!doc.is_descendant_of(a, a)); // not its own descendant
}

#[test]
fn move_child_preserves_children() {
    let doc = Document::new().unwrap();

    let a = doc.create_box().unwrap();
    let b = doc.create_box().unwrap();
    let child = doc.create_box().unwrap();
    let grandchild = doc.create_text("deep").unwrap();

    doc.append_child(a, child).unwrap();
    doc.append_child(child, grandchild).unwrap();

    // Move child (with grandchild) from a to b
    doc.move_child(b, child, b).unwrap(); // before_sibling doesn't exist → append

    assert_eq!(doc.get_parent(child), Some(b));
    assert_eq!(doc.get_parent(grandchild), Some(child));
    assert!(doc.get_children(a).is_empty());
    assert_eq!(doc.get_children(b), vec![child]);
}

#[test]
fn document_has_permanent_root() {
    let doc = Document::new().unwrap();
    let root = doc.root();

    assert!(doc.get_node(root).is_some());
    assert_eq!(doc.get_parent(root), None);
    assert_eq!(doc.layout_node_count(), 1);

    let parent = doc.create_box().unwrap();
    assert_eq!(
        doc.append_child(parent, root),
        Err(TuidomError::CannotReparentRoot { id: root })
    );
    assert_eq!(doc.get_parent(root), None);
}

#[test]
fn document_root_defaults_to_full_viewport_size() {
    let doc = Document::new().unwrap();
    let normal_node = doc.create_box().unwrap();

    let root_style = doc.resolved_style(doc.root()).unwrap();
    let normal_style = doc.resolved_style(normal_node).unwrap();

    assert_eq!(root_style.width, Length::Percent(100.0));
    assert_eq!(root_style.height, Length::Percent(100.0));
    assert_eq!(normal_style.width, Length::Auto);
    assert_eq!(normal_style.height, Length::Auto);
}

// -- Style resolution tests ---------------------------------------

#[test]
fn set_style_gets_resolved() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let mut style = Style::new();
    style.width(Length::Cells(42));
    style.padding(EdgeInsets::symmetric(2, 1));
    style.margin(EdgeInsets::new(1, 2, 3, 4));
    style.flex_direction(FlexDirection::Column);
    style.flex_basis(Length::Cells(3));
    style.flex_grow(1.0);
    style.flex_shrink(0.5);
    style.flex_wrap(FlexWrap::Wrap);
    style.gap(FlexGap::new(1, 2));
    style.align_self(AlignSelf::Center);
    style.align_content(AlignContent::Center);
    doc.set_style(node, &style).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(resolved.width, Length::Cells(42));
    assert_eq!(resolved.padding, EdgeInsets::symmetric(2, 1));
    assert_eq!(resolved.margin, EdgeInsets::new(1, 2, 3, 4));
    assert_eq!(resolved.flex_direction, FlexDirection::Column);
    assert_eq!(resolved.flex_basis, Length::Cells(3));
    assert_eq!(resolved.flex_grow, 1.0);
    assert_eq!(resolved.flex_shrink, 0.5);
    assert_eq!(resolved.flex_wrap, FlexWrap::Wrap);
    assert_eq!(resolved.gap, FlexGap::new(1, 2));
    assert_eq!(resolved.align_self, Some(AlignSelf::Center));
    assert_eq!(resolved.align_content, AlignContent::Center);
    assert_eq!(resolved.opacity, 1.0); // Inherit → default
    assert_eq!(resolved.color, ResolvedColor::white()); // Inherit → default
}

#[test]
fn set_style_missing_node_returns_error() {
    let doc = Document::new().unwrap();
    let missing = NodeId::new(999);

    assert_eq!(
        doc.set_style(missing, &Style::new()),
        Err(TuidomError::NodeNotFound { id: missing })
    );
}

#[test]
fn set_transition_missing_node_returns_error() {
    let doc = Document::new().unwrap();
    let missing = NodeId::new(999);
    let config = TransitionConfig::opacity(Duration::from_millis(100), Easing::Linear);

    assert_eq!(
        doc.set_transition(missing, config),
        Err(TuidomError::NodeNotFound { id: missing })
    );
}

#[test]
fn update_style_invalidates_cache() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let mut style = Style::new();
    style.width(Length::Cells(10));
    doc.set_style(node, &style).unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Cells(10));

    doc.update_style(node, |s| {
        s.width(Length::Cells(20));
    })
    .unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Cells(20));
}

#[test]
fn panicking_update_style_does_not_partially_mutate_style() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let mut style = Style::new();
    style.width(Length::Cells(10));
    doc.set_style(node, &style).unwrap();
    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Cells(10));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = doc.update_style(node, |style| {
            style.width(Length::Cells(20));
            panic!("boom");
        });
    }));

    assert!(result.is_err());
    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Cells(10));
}

#[test]
fn update_style_missing_node_returns_error() {
    let doc = Document::new().unwrap();
    let missing = NodeId::new(999);

    assert_eq!(
        doc.update_style(missing, |s| s.opacity(0.5)),
        Err(TuidomError::NodeNotFound { id: missing })
    );
}

#[test]
fn resolved_style_missing_node_returns_error() {
    let doc = Document::new().unwrap();
    let missing = NodeId::new(999);

    assert!(matches!(
        doc.resolved_style(missing),
        Err(TuidomError::NodeNotFound { id }) if id == missing
    ));
}

#[test]
fn unset_properties_use_defaults_not_parent_values() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    let mut parent_style = Style::new();
    parent_style.color(Color::red());
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_text("hi").unwrap();
    doc.append_child(parent, child).unwrap();

    let child_resolved = doc.resolved_style(child).unwrap();
    assert_eq!(child_resolved.color, ResolvedColor::white());
    assert_eq!(child_resolved.width, Length::Auto);
    assert_eq!(child_resolved.padding, EdgeInsets::ZERO);
    assert_eq!(child_resolved.margin, EdgeInsets::ZERO);
    assert_eq!(child_resolved.flex_direction, FlexDirection::Row);
    assert_eq!(child_resolved.flex_basis, Length::Auto);
    assert_eq!(child_resolved.flex_grow, 0.0);
    assert_eq!(child_resolved.flex_shrink, 1.0);
    assert_eq!(child_resolved.flex_wrap, FlexWrap::NoWrap);
    assert_eq!(child_resolved.gap, FlexGap::ZERO);
    assert_eq!(child_resolved.align_self, None);
    assert_eq!(child_resolved.align_content, AlignContent::Stretch);
}

#[test]
fn explicitly_inherits_from_parent() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    let mut parent_style = Style::new();
    parent_style.color(Color::red());
    parent_style.padding(EdgeInsets::all(2));
    parent_style.margin(EdgeInsets::new(1, 2, 3, 4));
    parent_style.flex_direction(FlexDirection::Column);
    parent_style.flex_basis(Length::Cells(3));
    parent_style.flex_grow(1.0);
    parent_style.flex_shrink(0.5);
    parent_style.flex_wrap(FlexWrap::Wrap);
    parent_style.gap(FlexGap::new(1, 2));
    parent_style.align_self(AlignSelf::Center);
    parent_style.align_content(AlignContent::Center);
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_text("hi").unwrap();
    let mut child_style = Style::new();
    child_style.inherit_color();
    child_style.inherit_padding();
    child_style.inherit_margin();
    child_style.inherit_flex_direction();
    child_style.inherit_flex_basis();
    child_style.inherit_flex_grow();
    child_style.inherit_flex_shrink();
    child_style.inherit_flex_wrap();
    child_style.inherit_gap();
    child_style.inherit_align_self();
    child_style.inherit_align_content();
    doc.set_style(child, &child_style).unwrap();
    doc.append_child(parent, child).unwrap();

    let child_resolved = doc.resolved_style(child).unwrap();
    assert_eq!(child_resolved.color, ResolvedColor::red());
    assert_eq!(child_resolved.padding, EdgeInsets::all(2));
    assert_eq!(child_resolved.margin, EdgeInsets::new(1, 2, 3, 4));
    assert_eq!(child_resolved.flex_direction, FlexDirection::Column);
    assert_eq!(child_resolved.flex_basis, Length::Cells(3));
    assert_eq!(child_resolved.flex_grow, 1.0);
    assert_eq!(child_resolved.flex_shrink, 0.5);
    assert_eq!(child_resolved.flex_wrap, FlexWrap::Wrap);
    assert_eq!(child_resolved.gap, FlexGap::new(1, 2));
    assert_eq!(child_resolved.align_self, Some(AlignSelf::Center));
    assert_eq!(child_resolved.align_content, AlignContent::Center);
    assert_eq!(child_resolved.width, Length::Auto);
}

#[test]
fn override_breaks_inheritance() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    let mut parent_style = Style::new();
    parent_style.color(Color::red());
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_text("hi").unwrap();
    let mut child_style = Style::new();
    child_style.color(Color::blue()); // Explicit override
    doc.set_style(child, &child_style).unwrap();
    doc.append_child(parent, child).unwrap();

    let child_resolved = doc.resolved_style(child).unwrap();
    assert_eq!(child_resolved.color, ResolvedColor::blue()); // Override wins
}

#[test]
fn move_child_triggers_re_resolve() {
    let doc = Document::new().unwrap();

    let parent_red = doc.create_box().unwrap();
    let mut red_style = Style::new();
    red_style.color(Color::red());
    doc.set_style(parent_red, &red_style).unwrap();

    let parent_blue = doc.create_box().unwrap();
    let mut blue_style = Style::new();
    blue_style.color(Color::blue());
    doc.set_style(parent_blue, &blue_style).unwrap();

    let child = doc.create_text("movable").unwrap();
    let mut child_style = Style::new();
    child_style.inherit_color();
    doc.set_style(child, &child_style).unwrap();
    doc.append_child(parent_red, child).unwrap();

    assert_eq!(
        doc.resolved_style(child).unwrap().color,
        ResolvedColor::red()
    );

    // Move to blue parent
    doc.move_child(parent_blue, child, child).unwrap();
    assert_eq!(
        doc.resolved_style(child).unwrap().color,
        ResolvedColor::blue()
    );
}

fn stacking_context_box(doc: &Document) -> NodeId {
    let node = doc.create_box().unwrap();
    let mut style = Style::new();
    style.stacking_context(true);
    doc.set_style(node, &style).unwrap();
    node
}

fn focusable_child(doc: &Document, parent: NodeId) -> NodeId {
    let node = doc.create_box().unwrap();
    doc.append_child(parent, node).unwrap();
    doc.set_focusable(node, true).unwrap();
    node
}

#[test]
fn focus_context_defaults_to_the_document_root() {
    let doc = Document::new().unwrap();
    assert_eq!(doc.active_focus_context(), doc.root());
    assert_eq!(doc.focus_context_depth(), 1);
}

#[test]
fn push_focus_context_requires_a_stacking_context() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();

    assert_eq!(
        doc.push_focus_context(panel),
        Err(TuidomError::NotAStackingContext { id: panel })
    );
    assert_eq!(doc.active_focus_context(), doc.root());
}

#[test]
fn push_focus_context_rejects_an_already_open_context() {
    let doc = Document::new().unwrap();
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();

    doc.push_focus_context(modal).unwrap();
    assert_eq!(
        doc.push_focus_context(modal),
        Err(TuidomError::FocusContextAlreadyOpen { id: modal })
    );
    assert_eq!(doc.focus_context_depth(), 2);
}

#[test]
fn push_focus_context_focuses_the_first_focusable_inside_it() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    let confirm = focusable_child(&doc, modal);
    let cancel = focusable_child(&doc, modal);

    doc.focus(background).unwrap();
    doc.push_focus_context(modal).unwrap();

    assert_eq!(doc.active_focus_context(), modal);
    assert_eq!(doc.focused(), Some(confirm));
    assert_ne!(doc.focused(), Some(cancel));
}

#[test]
fn push_focus_context_dispatches_blur_then_focus_across_the_boundary() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    let confirm = focusable_child(&doc, modal);

    doc.focus(background).unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));
    let blur_calls = calls.clone();
    doc.on_blur(background, move |_| {
        blur_calls.lock().unwrap().push("blur background");
    })
    .unwrap();
    let focus_calls = calls.clone();
    doc.on_focus(confirm, move |_| {
        focus_calls.lock().unwrap().push("focus confirm");
    })
    .unwrap();

    doc.push_focus_context(modal).unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        vec!["blur background", "focus confirm"]
    );
}

#[test]
fn pop_focus_context_restores_the_interrupted_focus() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    focusable_child(&doc, modal);

    doc.focus(background).unwrap();
    doc.push_focus_context(modal).unwrap();

    assert_eq!(doc.pop_focus_context(), Ok(modal));
    assert_eq!(doc.active_focus_context(), doc.root());
    assert_eq!(doc.focused(), Some(background));
}

#[test]
fn pop_focus_context_on_the_root_context_errors() {
    let doc = Document::new().unwrap();
    assert_eq!(
        doc.pop_focus_context(),
        Err(TuidomError::CannotPopRootFocusContext)
    );
}

#[test]
fn nested_focus_contexts_unwind_in_order() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());

    let outer = stacking_context_box(&doc);
    doc.append_child(doc.root(), outer).unwrap();
    let outer_button = focusable_child(&doc, outer);

    // A nested modal is a sibling of the first, not a descendant.
    let inner = stacking_context_box(&doc);
    doc.append_child(doc.root(), inner).unwrap();
    let inner_button = focusable_child(&doc, inner);

    doc.focus(background).unwrap();
    doc.push_focus_context(outer).unwrap();
    assert_eq!(doc.focused(), Some(outer_button));

    doc.push_focus_context(inner).unwrap();
    assert_eq!(doc.focused(), Some(inner_button));
    assert_eq!(doc.focus_context_depth(), 3);

    doc.pop_focus_context().unwrap();
    assert_eq!(doc.active_focus_context(), outer);
    assert_eq!(doc.focused(), Some(outer_button));

    doc.pop_focus_context().unwrap();
    assert_eq!(doc.active_focus_context(), doc.root());
    assert_eq!(doc.focused(), Some(background));
}

#[test]
fn pop_focus_context_clears_focus_when_the_remembered_node_is_gone() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    focusable_child(&doc, modal);

    doc.focus(background).unwrap();
    doc.push_focus_context(modal).unwrap();
    doc.remove_child(doc.root(), background).unwrap();

    doc.pop_focus_context().unwrap();

    assert_eq!(doc.focused(), None);
}

#[test]
fn pop_focus_context_clears_focus_when_the_remembered_node_is_disabled() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    focusable_child(&doc, modal);

    doc.focus(background).unwrap();
    doc.push_focus_context(modal).unwrap();
    doc.set_disabled(background, true).unwrap();

    doc.pop_focus_context().unwrap();

    assert_eq!(doc.focused(), None);
}

#[test]
fn removing_an_open_focus_context_node_closes_it() {
    let doc = Document::new().unwrap();
    let background = focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    focusable_child(&doc, modal);

    doc.focus(background).unwrap();
    doc.push_focus_context(modal).unwrap();

    doc.remove_child(doc.root(), modal).unwrap();

    assert_eq!(doc.focus_context_depth(), 1);
    assert_eq!(doc.active_focus_context(), doc.root());
    assert_eq!(doc.focused(), Some(background));
}

#[test]
fn tab_order_is_scoped_to_the_active_focus_context() {
    let doc = Document::new().unwrap();
    // Background focusables sit on both sides of the modal in DOM order, so an unscoped
    // walk would escape the context in one direction or the other.
    focusable_child(&doc, doc.root());
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    let confirm = focusable_child(&doc, modal);
    let cancel = focusable_child(&doc, modal);
    focusable_child(&doc, doc.root());

    doc.push_focus_context(modal).unwrap();
    assert_eq!(doc.focused(), Some(confirm));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(cancel));

    // Tab does not wrap, and stops at the context edge instead of falling through.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(cancel));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::BackTab));
    assert_eq!(doc.focused(), Some(confirm));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::BackTab));
    assert_eq!(doc.focused(), Some(confirm));
}

/// A background button at (0,0) and a modal at (2,0) holding one button, side by side so a
/// click can target either without overlap.
fn modal_scene(doc: &Document) -> (NodeId, NodeId, NodeId) {
    let mut background_style = Style::new();
    background_style.position(Position::Absolute { x: 0, y: 0 });
    background_style.width(Length::Cells(2));
    background_style.height(Length::Cells(1));

    let background = doc.create_box().unwrap();
    doc.append_child(doc.root(), background).unwrap();
    doc.set_style(background, &background_style).unwrap();
    doc.set_focusable(background, true).unwrap();

    let mut modal_style = Style::new();
    modal_style.stacking_context(true);
    modal_style.position(Position::Absolute { x: 2, y: 0 });
    modal_style.width(Length::Cells(2));
    modal_style.height(Length::Cells(1));

    let modal = doc.create_box().unwrap();
    doc.append_child(doc.root(), modal).unwrap();
    doc.set_style(modal, &modal_style).unwrap();

    let mut confirm_style = Style::new();
    confirm_style.width(Length::Cells(2));
    confirm_style.height(Length::Cells(1));

    let confirm = doc.create_box().unwrap();
    doc.append_child(modal, confirm).unwrap();
    doc.set_style(confirm, &confirm_style).unwrap();
    doc.set_focusable(confirm, true).unwrap();

    (background, modal, confirm)
}

#[test]
fn is_inert_reports_nodes_outside_the_active_focus_context() {
    let doc = Document::new().unwrap();
    let (background, modal, confirm) = modal_scene(&doc);

    // Nothing is inert while the root context is active.
    assert_eq!(doc.is_inert(background), Ok(false));
    assert_eq!(doc.is_inert(confirm), Ok(false));

    doc.push_focus_context(modal).unwrap();

    assert_eq!(doc.is_inert(background), Ok(true));
    assert_eq!(doc.is_inert(doc.root()), Ok(true));
    assert_eq!(doc.is_inert(modal), Ok(false));
    assert_eq!(doc.is_inert(confirm), Ok(false));

    doc.pop_focus_context().unwrap();
    assert_eq!(doc.is_inert(background), Ok(false));
}

#[test]
fn inert_nodes_cannot_be_focused() {
    let doc = Document::new().unwrap();
    let (background, modal, confirm) = modal_scene(&doc);

    doc.push_focus_context(modal).unwrap();

    assert_eq!(
        doc.focus(background),
        Err(TuidomError::NodeNotFocusable { id: background })
    );
    assert_eq!(doc.focused(), Some(confirm));
}

#[test]
fn inert_nodes_swallow_input_events() {
    let doc = Document::new().unwrap();
    let (background, modal, confirm) = modal_scene(&doc);

    let background_clicks = Arc::new(AtomicUsize::new(0));
    let confirm_clicks = Arc::new(AtomicUsize::new(0));
    {
        let background_clicks = background_clicks.clone();
        doc.on_click(background, move |_| {
            background_clicks.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let confirm_clicks = confirm_clicks.clone();
        doc.on_click(confirm, move |_| {
            confirm_clicks.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 2);
    runtime.render().unwrap();

    runtime.simulate_click(0, 0);
    assert_eq!(background_clicks.load(Ordering::SeqCst), 1);

    doc.push_focus_context(modal).unwrap();

    // The background is inert now: its click is dropped, not bubbled to the root.
    runtime.simulate_click(0, 0);
    assert_eq!(background_clicks.load(Ordering::SeqCst), 1);

    // The modal still works.
    runtime.simulate_click(2, 0);
    assert_eq!(confirm_clicks.load(Ordering::SeqCst), 1);
}

#[test]
fn events_inside_a_focus_context_still_bubble_to_the_root() {
    let doc = Document::new().unwrap();
    let (_, modal, confirm) = modal_scene(&doc);

    let root_clicks = Arc::new(AtomicUsize::new(0));
    {
        let root_clicks = root_clicks.clone();
        doc.on_click(doc.root(), move |_| {
            root_clicks.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 2);
    runtime.render().unwrap();

    doc.push_focus_context(modal).unwrap();
    runtime.simulate_click(2, 0);

    // The root is inert, but bubbling from a live target is not interaction *with* the root.
    assert_eq!(root_clicks.load(Ordering::SeqCst), 1);
    assert_eq!(doc.focused(), Some(confirm));
}

#[test]
fn inert_nodes_are_not_focused_by_hover_and_cannot_be_pressed() {
    let doc = Document::new().unwrap();
    let (background, modal, confirm) = modal_scene(&doc);

    let mut runtime = HeadlessRuntime::new(doc.clone(), 4, 2);
    runtime.render().unwrap();

    doc.push_focus_context(modal).unwrap();

    runtime.simulate_mouse_move(0, 0);
    assert_eq!(doc.focused(), Some(confirm));

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    assert_eq!(doc.active(), None);
    runtime.simulate_mouse_up(0, 0, MouseButton::Left);

    assert_eq!(doc.set_active(background, true), Ok(()));
    assert_eq!(doc.active(), None);
}

#[test]
fn spatial_navigation_skips_inert_candidates_instead_of_dead_ending_on_them() {
    let doc = Document::new().unwrap();

    fn absolute_button(doc: &Document, parent: NodeId, x: i32, width: u16) -> NodeId {
        let mut style = Style::new();
        style.position(Position::Absolute { x, y: 0 });
        style.width(Length::Cells(width));
        style.height(Length::Cells(1));

        let node = doc.create_box().unwrap();
        doc.append_child(parent, node).unwrap();
        doc.set_style(node, &style).unwrap();
        doc.set_focusable(node, true).unwrap();
        node
    }

    let mut modal_style = Style::new();
    modal_style.stacking_context(true);
    modal_style.position(Position::Absolute { x: 0, y: 0 });
    modal_style.width(Length::Cells(8));
    modal_style.height(Length::Cells(1));

    let modal = doc.create_box().unwrap();
    doc.append_child(doc.root(), modal).unwrap();
    doc.set_style(modal, &modal_style).unwrap();

    let far = absolute_button(&doc, modal, 0, 2);
    let current = absolute_button(&doc, modal, 6, 2);

    // A background button sits *between* the two modal buttons on screen, so it is the
    // nearest candidate to the left even though it is outside the context.
    let between = absolute_button(&doc, doc.root(), 3, 2);

    let mut runtime = HeadlessRuntime::new(doc.clone(), 8, 2);
    runtime.render().unwrap();

    doc.push_focus_context(modal).unwrap();
    doc.focus(current).unwrap();

    // Skipping the inert candidate lets the search reach the valid one behind it; without
    // the skip, the nearest match would be `between` and the arrow key would do nothing.
    runtime.simulate_key(KeyCode::Left);
    assert_eq!(doc.focused(), Some(far));
    assert_ne!(doc.focused(), Some(between));
}

#[test]
fn inert_nodes_do_not_merge_the_disabled_style() {
    let doc = Document::new().unwrap();
    let (background, modal, _) = modal_scene(&doc);

    let mut disabled_style = Style::new();
    disabled_style.color(Color::red());
    doc.set_disabled_style(background, &disabled_style).unwrap();

    let before = doc.resolved_style(background).unwrap().color;
    doc.push_focus_context(modal).unwrap();

    // Inertness blocks interaction without restyling: content behind a modal keeps its
    // own appearance rather than looking disabled.
    assert!(doc.is_inert(background).unwrap());
    assert_eq!(doc.resolved_style(background).unwrap().color, before);
    assert_ne!(
        doc.resolved_style(background).unwrap().color,
        ResolvedColor::red()
    );
}

#[test]
fn key_dispatch_targets_the_active_focus_context_when_nothing_is_focused() {
    let doc = Document::new().unwrap();
    let (_, modal, _) = modal_scene(&doc);

    let modal_keys = Arc::new(AtomicUsize::new(0));
    {
        let modal_keys = modal_keys.clone();
        doc.on_key_press(modal, move |_| {
            modal_keys.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    }

    doc.push_focus_context(modal).unwrap();
    doc.blur();
    assert_eq!(doc.focused(), None);

    // Dispatching from the root instead would start bubbling outside the modal, so the
    // modal's own handler would never run.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));
    assert_eq!(modal_keys.load(Ordering::SeqCst), 1);
}

#[test]
fn escape_blurs_first_then_reaches_the_focus_context_handler() {
    let doc = Document::new().unwrap();
    let (_, modal, confirm) = modal_scene(&doc);

    let closes = Arc::new(AtomicUsize::new(0));
    {
        let closes = closes.clone();
        let doc_for_handler = doc.clone();
        doc.on_key_press(modal, move |event| {
            // The documented modal idiom: Escape closes only once focus is already cleared.
            if event.code == KeyCode::Esc && doc_for_handler.focused().is_none() {
                closes.fetch_add(1, Ordering::SeqCst);
            }
        })
        .unwrap();
    }

    doc.push_focus_context(modal).unwrap();
    assert_eq!(doc.focused(), Some(confirm));

    // First press blurs the focused node inside the context.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Esc));
    assert_eq!(doc.focused(), None);
    assert_eq!(closes.load(Ordering::SeqCst), 0);

    // Second press reaches the modal itself.
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Esc));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[test]
fn tab_order_skips_hidden_subtrees() {
    let doc = Document::new().unwrap();
    let visible = focusable_child(&doc, doc.root());

    let hidden = doc.create_box().unwrap();
    doc.append_child(doc.root(), hidden).unwrap();
    let mut hidden_style = Style::new();
    hidden_style.display(Display::None);
    doc.set_style(hidden, &hidden_style).unwrap();

    // Focusable, but inside a hidden subtree — and hidden itself is not focusable, so this
    // also covers pruning rather than merely skipping the hidden node.
    let offscreen = focusable_child(&doc, hidden);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(visible));

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(visible));
    assert_ne!(doc.focused(), Some(offscreen));

    // Showing the subtree puts it back in the tab order.
    doc.update_style(hidden, |style| style.display(Display::Flex))
        .unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Tab));
    assert_eq!(doc.focused(), Some(offscreen));
}

// ---------------------------------------------------------------------------
// Color variables
// ---------------------------------------------------------------------------

#[test]
fn a_document_color_variable_resolves_on_any_node() {
    let doc = Document::new().unwrap();
    doc.set_color_var("--primary", Color::red());

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.background(Color::var("--primary"));
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::red())
    );
}

#[test]
fn a_color_variable_is_in_scope_for_descendants() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    doc.append_child(doc.root(), parent).unwrap();
    let mut parent_style = Style::new();
    parent_style.color_var("--accent", Color::blue());
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_box().unwrap();
    doc.append_child(parent, child).unwrap();
    let mut child_style = Style::new();
    child_style.background(Color::var("--accent"));
    doc.set_style(child, &child_style).unwrap();

    assert_eq!(
        doc.resolved_style(child).unwrap().background,
        Some(ResolvedColor::blue())
    );
}

#[test]
fn a_node_variable_shadows_the_document_variable() {
    let doc = Document::new().unwrap();
    doc.set_color_var("--accent", Color::red());

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color_var("--accent", Color::blue());
    style.background(Color::var("--accent"));
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::blue())
    );
}

#[test]
fn a_variable_declaration_can_derive_from_the_name_it_shadows() {
    // The declaration resolves against the parent's scope, so shadowing a name with a derivation
    // of itself terminates instead of looping.
    let doc = Document::new().unwrap();
    doc.set_color_var("--accent", Color::oklch(0.5, 0.1, 180.0));

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color_var("--accent", Color::var("--accent").darken(0.25));
    style.background(Color::var("--accent"));
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::oklch(0.25, 0.1, 180.0))
    );
}

#[test]
fn an_undefined_variable_falls_back_to_the_property_default() {
    let doc = Document::new().unwrap();

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color(Color::var("--nope"));
    style.background(Color::var("--nope").darken(0.1));
    doc.set_style(node, &style).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(resolved.color, ResolvedColor::white());
    assert_eq!(resolved.background, None);
}

#[test]
fn a_broken_variable_declaration_leaves_the_name_undefined_rather_than_inherited() {
    // Falling through to the ancestor's value would paint the ancestor's color and hide the
    // broken declaration entirely.
    let doc = Document::new().unwrap();
    doc.set_color_var("--accent", Color::red());

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color_var("--accent", Color::var("--typo"));
    style.background(Color::var("--accent"));
    doc.set_style(node, &style).unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().background, None);
}

#[test]
fn changing_a_document_variable_re_resolves_the_tree() {
    let doc = Document::new().unwrap();
    doc.set_color_var("--primary", Color::red());

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.background(Color::var("--primary"));
    doc.set_style(node, &style).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::red())
    );

    doc.set_color_var("--primary", Color::blue());
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::blue())
    );

    doc.remove_color_var("--primary");
    assert_eq!(doc.resolved_style(node).unwrap().background, None);
}

#[test]
fn changing_a_node_variable_re_resolves_its_subtree() {
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    doc.append_child(doc.root(), parent).unwrap();
    let mut parent_style = Style::new();
    parent_style.color_var("--accent", Color::red());
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_box().unwrap();
    doc.append_child(parent, child).unwrap();
    let mut child_style = Style::new();
    child_style.background(Color::var("--accent"));
    doc.set_style(child, &child_style).unwrap();
    assert_eq!(
        doc.resolved_style(child).unwrap().background,
        Some(ResolvedColor::red())
    );

    doc.update_style(parent, |s| s.color_var("--accent", Color::blue()))
        .unwrap();
    assert_eq!(
        doc.resolved_style(child).unwrap().background,
        Some(ResolvedColor::blue())
    );
}

#[test]
fn a_pseudo_state_style_can_reference_a_variable() {
    let doc = Document::new().unwrap();
    doc.set_color_var("--accent", Color::blue());

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();

    let mut focus_style = Style::new();
    focus_style.background(Color::var("--accent"));
    doc.set_focus_style(node, &focus_style).unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().background, None);

    doc.focus(node).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::blue())
    );
}

// ---------------------------------------------------------------------------
// CurrentBg / CurrentFg
// ---------------------------------------------------------------------------

#[test]
fn current_fg_resolves_to_the_nodes_own_color() {
    let doc = Document::new().unwrap();

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color(Color::red());
    style.border_color(Color::CurrentFg);
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().border_color,
        Some(ResolvedColor::red())
    );
}

#[test]
fn current_bg_in_color_resolves_to_the_nodes_own_background() {
    // A foreground derived from the background it sits on is the point of the feature, so `color`
    // sees this node's background rather than the parent's.
    let doc = Document::new().unwrap();

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.background(Color::oklch(0.5, 0.1, 180.0));
    style.color(Color::CurrentBg.lighten(0.25));
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::oklch(0.75, 0.1, 180.0)
    );
}

#[test]
fn current_bg_in_background_resolves_to_the_parents_background() {
    // Self-reference is circular, so in `background` itself the reference means the background the
    // node would otherwise have sat on.
    let doc = Document::new().unwrap();

    let parent = doc.create_box().unwrap();
    doc.append_child(doc.root(), parent).unwrap();
    let mut parent_style = Style::new();
    parent_style.background(Color::oklch(0.5, 0.1, 180.0));
    doc.set_style(parent, &parent_style).unwrap();

    let child = doc.create_box().unwrap();
    doc.append_child(parent, child).unwrap();
    let mut child_style = Style::new();
    child_style.background(Color::CurrentBg.lighten(0.25));
    doc.set_style(child, &child_style).unwrap();

    assert_eq!(
        doc.resolved_style(child).unwrap().background,
        Some(ResolvedColor::oklch(0.75, 0.1, 180.0))
    );
}

#[test]
fn current_bg_sees_through_transparent_ancestors() {
    let doc = Document::new().unwrap();

    let painted = doc.create_box().unwrap();
    doc.append_child(doc.root(), painted).unwrap();
    let mut painted_style = Style::new();
    painted_style.background(Color::oklch(0.5, 0.1, 180.0));
    doc.set_style(painted, &painted_style).unwrap();

    // No background of its own — it shows the painted ancestor.
    let transparent = doc.create_box().unwrap();
    doc.append_child(painted, transparent).unwrap();

    let child = doc.create_box().unwrap();
    doc.append_child(transparent, child).unwrap();
    let mut child_style = Style::new();
    child_style.color(Color::CurrentBg.lighten(0.25));
    doc.set_style(child, &child_style).unwrap();

    assert_eq!(
        doc.resolved_style(child).unwrap().color,
        ResolvedColor::oklch(0.75, 0.1, 180.0)
    );
}

#[test]
fn current_bg_falls_back_to_the_declared_terminal_background() {
    let doc = Document::new().unwrap();
    doc.set_terminal_background(Color::oklch(0.25, 0.05, 250.0));

    // Nothing in this node's ancestry paints a background.
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color(Color::CurrentBg);
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::oklch(0.25, 0.05, 250.0)
    );
}

#[test]
fn changing_the_terminal_background_re_resolves_the_tree() {
    let doc = Document::new().unwrap();

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.color(Color::CurrentBg);
    doc.set_style(node, &style).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::black()
    );

    doc.set_terminal_background(Color::red());
    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::red()
    );
}

#[test]
fn a_color_variable_can_derive_from_the_terminal_background() {
    let doc = Document::new().unwrap();
    doc.set_terminal_background(Color::oklch(0.25, 0.05, 250.0));
    doc.set_color_var("--surface", Color::CurrentBg.lighten(0.25));

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    let mut style = Style::new();
    style.background(Color::var("--surface"));
    doc.set_style(node, &style).unwrap();

    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(ResolvedColor::oklch(0.5, 0.05, 250.0))
    );
}

#[test]
fn a_pseudo_style_background_is_what_its_other_colors_derive_from() {
    // A focus style that changes the background must have its own `CurrentBg` colors resolve
    // against that new background, not the one it replaced.
    let doc = Document::new().unwrap();

    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();
    let mut base = Style::new();
    base.background(Color::black());
    doc.set_style(node, &base).unwrap();

    let mut focus_style = Style::new();
    focus_style.background(Color::oklch(0.5, 0.1, 180.0));
    focus_style.color(Color::CurrentBg.lighten(0.25));
    doc.set_focus_style(node, &focus_style).unwrap();

    doc.focus(node).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().color,
        ResolvedColor::oklch(0.75, 0.1, 180.0)
    );
}

#[test]
fn effective_background_is_reported_on_the_resolved_style() {
    let doc = Document::new().unwrap();
    doc.set_terminal_background(Color::blue());

    let painted = doc.create_box().unwrap();
    doc.append_child(doc.root(), painted).unwrap();
    let mut painted_style = Style::new();
    painted_style.background(Color::red());
    doc.set_style(painted, &painted_style).unwrap();

    let child = doc.create_box().unwrap();
    doc.append_child(painted, child).unwrap();

    let bare = doc.create_box().unwrap();
    doc.append_child(doc.root(), bare).unwrap();

    assert_eq!(
        doc.resolved_style(painted).unwrap().effective_background,
        ResolvedColor::red()
    );
    assert_eq!(
        doc.resolved_style(child).unwrap().effective_background,
        ResolvedColor::red()
    );
    assert_eq!(
        doc.resolved_style(bare).unwrap().effective_background,
        ResolvedColor::blue()
    );
}

// -- Scrollbar drag -----------------------------------------------------------

/// A 10×4 screen with a 5-cell-wide scroll column holding 8 rows of content: the
/// vertical bar sits in column 4 with a 2-cell thumb over a 4-cell strip, so thumb
/// starts 0/1/2 map to offsets 0/2/4.
fn scrollbar_drag_setup() -> (Document, HeadlessRuntime, NodeId) {
    let doc = Document::new().unwrap();
    let container = doc.create_box().unwrap();
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Scroll);
    doc.set_style(container, &style).unwrap();
    doc.append_child(doc.root(), container).unwrap();
    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(container, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();
    (doc, runtime, container)
}

#[test]
fn scrollbar_thumb_drag_scrolls_the_container() {
    let (doc, mut runtime, container) = scrollbar_drag_setup();

    // Grabbing the thumb in place does not perturb the offset.
    runtime.simulate_mouse_down(4, 0, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 0);

    runtime.simulate_mouse_drag_move(4, 1, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 2);

    // Dragging past the strip end clamps to the maximum offset.
    runtime.simulate_mouse_drag_move(4, 3, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 4);

    runtime.simulate_mouse_drag_move(4, 0, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 0);

    runtime.simulate_mouse_up(4, 0, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 0);
}

#[test]
fn scrollbar_track_press_jumps_the_thumb_and_keeps_dragging() {
    let (doc, mut runtime, container) = scrollbar_drag_setup();

    // A press on the track's far end jumps the thumb under the cursor.
    runtime.simulate_mouse_down(4, 3, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 4);

    // The same press continues as a drag, grabbed where the thumb landed.
    runtime.simulate_mouse_drag_move(4, 1, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 0);
}

#[test]
fn scrollbar_press_starts_no_selection_and_fires_no_click() {
    let (doc, mut runtime, container) = scrollbar_drag_setup();
    let clicks = Arc::new(AtomicUsize::new(0));
    let clicks_for_handler = clicks.clone();
    doc.on_click(container, move |_| {
        clicks_for_handler.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    runtime.simulate_mouse_down(4, 0, MouseButton::Left);
    // Dragging across the text content selects nothing — the drag belongs to the bar.
    runtime.simulate_mouse_drag_move(1, 2, MouseButton::Left);
    assert_eq!(doc.get_selection(), None);
    runtime.simulate_mouse_up(4, 0, MouseButton::Left);

    // Down and up on the same strip cell still produces no click.
    assert_eq!(clicks.load(Ordering::Relaxed), 0);
}

#[test]
fn scrollbar_drag_does_not_move_focus() {
    let (doc, mut runtime, _container) = scrollbar_drag_setup();
    let other = doc.create_box().unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(2));
    style.height(Length::Cells(2));
    doc.set_style(other, &style).unwrap();
    doc.append_child(doc.root(), other).unwrap();
    doc.set_focusable(other, true).unwrap();
    runtime.render().unwrap();

    runtime.simulate_mouse_down(4, 0, MouseButton::Left);
    // The drag crosses a focusable node; hover-to-focus must not fire mid-drag.
    runtime.simulate_mouse_drag_move(6, 1, MouseButton::Left);
    assert_ne!(doc.focused(), Some(other));
}

#[test]
fn prevent_default_keeps_a_scrollbar_press_ordinary() {
    let (doc, mut runtime, container) = scrollbar_drag_setup();
    doc.on_mouse_down(container, |event| event.prevent_default())
        .unwrap();
    let clicks = Arc::new(AtomicUsize::new(0));
    let clicks_for_handler = clicks.clone();
    doc.on_click(container, move |_| {
        clicks_for_handler.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    runtime.simulate_mouse_down(4, 3, MouseButton::Left);
    runtime.simulate_mouse_drag_move(4, 1, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).y, 0);

    // With the grab suppressed the press behaves like any container press,
    // click candidacy included.
    runtime.simulate_mouse_up(4, 3, MouseButton::Left);
    assert_eq!(clicks.load(Ordering::Relaxed), 1);
}

#[test]
fn horizontal_scrollbar_drag_scrolls_x() {
    let doc = Document::new().unwrap();
    let container = doc.create_box().unwrap();
    let mut style = Style::new();
    style.width(Length::Cells(5));
    style.overflow_x(Overflow::Scroll);
    doc.set_style(container, &style).unwrap();
    doc.append_child(doc.root(), container).unwrap();
    for content in ["abcde", "fghij"] {
        let text = doc.create_text(content).unwrap();
        doc.append_child(container, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 2);
    runtime.render().unwrap();

    // A 3-cell thumb on a 5-cell strip along the bottom row.
    runtime.simulate_mouse_down(0, 1, MouseButton::Left);
    runtime.simulate_mouse_drag_move(4, 1, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).x, 5);

    runtime.simulate_mouse_drag_move(0, 1, MouseButton::Left);
    assert_eq!(doc.scroll_offset(container).x, 0);
}

// -- WhenScrolling scrollbars -------------------------------------------------

/// The scrollbar-drag fixture with `WhenScrolling` bars and fast timings: fully
/// visible for 100ms after activity, fading over the next 100ms.
fn when_scrolling_setup() -> (Document, HeadlessRuntime, NodeId) {
    let doc = Document::new().unwrap();
    let container = doc.create_box().unwrap();
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Scroll);
    style.scrollbar_show(ScrollbarShow::WhenScrolling);
    style.scrollbar_hide_delay(Duration::from_millis(100));
    style.scrollbar_fade_duration(Duration::from_millis(100));
    doc.set_style(container, &style).unwrap();
    doc.append_child(doc.root(), container).unwrap();
    for i in 0..8 {
        let text = doc.create_text(format!("line{i}")).unwrap();
        doc.append_child(container, text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();
    (doc, runtime, container)
}

/// The bar column's top cell: `░`/`█` while the bar shows, content otherwise.
fn bar_top_cell(runtime: &HeadlessRuntime) -> ScreenCell {
    runtime.get_cell(4, 0).unwrap()
}

#[test]
fn when_scrolling_bar_appears_on_scroll_and_fades_away() {
    let (doc, mut runtime, container) = when_scrolling_setup();

    // Never scrolled: no bar, the content shows through.
    assert_eq!(bar_top_cell(&runtime).text, "0");

    doc.scroll_to(container, 0, 1).unwrap();
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");
    let opaque_fg = bar_top_cell(&runtime).fg;

    // Still opaque just before the hide delay elapses.
    runtime.advance_time(Duration::from_millis(90));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");
    assert_eq!(bar_top_cell(&runtime).fg, opaque_fg);

    // Mid-fade the glyph is still there but its color has faded toward the background.
    runtime.advance_time(Duration::from_millis(60));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");
    assert_ne!(bar_top_cell(&runtime).fg, opaque_fg);

    // Past delay + fade the bar is gone and the scrolled content shows again.
    runtime.advance_time(Duration::from_millis(60));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "1");
}

#[test]
fn renewed_scrolling_restarts_the_hide_countdown() {
    let (doc, mut runtime, container) = when_scrolling_setup();

    doc.scroll_to(container, 0, 1).unwrap();
    runtime.advance_time(Duration::from_millis(150));
    doc.scroll_to(container, 0, 2).unwrap();

    // The second scroll landed mid-fade; the bar is opaque and held again.
    runtime.advance_time(Duration::from_millis(90));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");
}

#[test]
fn grabbed_when_scrolling_bar_stays_visible() {
    let (doc, mut runtime, container) = when_scrolling_setup();

    doc.scroll_to(container, 0, 1).unwrap();
    runtime.render().unwrap();

    // Offset 1 puts the 2-cell thumb on rows 1-2; grab it there.
    runtime.simulate_mouse_down(4, 1, MouseButton::Left);
    runtime.advance_time(Duration::from_secs(5));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");

    // Release restarts the countdown rather than hiding instantly.
    runtime.simulate_mouse_up(4, 1, MouseButton::Left);
    runtime.advance_time(Duration::from_millis(90));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "░");

    runtime.advance_time(Duration::from_millis(200));
    runtime.render().unwrap();
    assert_eq!(bar_top_cell(&runtime).text, "1");
}

#[test]
fn scrollbar_fade_schedule_tracks_phases_and_prunes() {
    let (doc, mut runtime, container) = when_scrolling_setup();

    // Nothing scrolled yet: nothing scheduled, nothing recorded.
    assert!(!doc.scrollbar_fade_schedule(doc.now()).is_active());

    // Fully visible: one deadline wake at fade start, no smooth ticking.
    doc.scroll_to(container, 0, 1).unwrap();
    let schedule = doc.scrollbar_fade_schedule(doc.now());
    assert!(!schedule.fading);
    assert_eq!(
        schedule.next_deadline,
        Some(doc.now() + Duration::from_millis(100))
    );

    // Mid-fade: smooth ticking, no further deadline.
    runtime.advance_time(Duration::from_millis(150));
    let schedule = doc.scrollbar_fade_schedule(doc.now());
    assert!(schedule.fading);
    assert_eq!(schedule.next_deadline, None);

    // Fully faded: inactive, and the activity entry is pruned.
    runtime.advance_time(Duration::from_millis(100));
    assert!(!doc.scrollbar_fade_schedule(doc.now()).is_active());
    assert!(lock::mutex(&doc.inner.scroll_activity).is_empty());
}

#[test]
fn always_shown_scrollbars_schedule_nothing() {
    let (doc, mut runtime, container) = scrollbar_drag_setup();

    doc.scroll_to(container, 0, 2).unwrap();
    runtime.render().unwrap();

    // An `Always` bar records no activity and asks for no frames.
    assert!(lock::mutex(&doc.inner.scroll_activity).is_empty());
    assert!(!doc.scrollbar_fade_schedule(doc.now()).is_active());
}

#[test]
fn grabbed_bar_schedules_nothing_until_release() {
    let (doc, mut runtime, container) = when_scrolling_setup();

    doc.scroll_to(container, 0, 1).unwrap();
    runtime.render().unwrap();
    runtime.simulate_mouse_down(4, 1, MouseButton::Left);

    // Pinned visible while held: no deadline, no ticking, however stale the activity.
    runtime.advance_time(Duration::from_secs(5));
    assert!(!doc.scrollbar_fade_schedule(doc.now()).is_active());

    // Release restarts the countdown, so scheduling resumes.
    runtime.simulate_mouse_up(4, 1, MouseButton::Left);
    let schedule = doc.scrollbar_fade_schedule(doc.now());
    assert!(schedule.next_deadline.is_some());
}

/// The panic hook must not tear the terminal down for a handler panic that is
/// caught and survived, so every downstream callback runs guarded. Observing the
/// guard from inside a handler is what proves dispatch actually sets it.
#[test]
fn handlers_run_guarded_against_the_panic_hook() {
    let doc = Document::new().unwrap();
    let guarded = Arc::new(Mutex::new(None));

    let observed = guarded.clone();
    doc.on_resize(move |_| {
        *observed.lock().unwrap() = Some(crate::panic::catching_panic());
    });

    assert!(
        !crate::panic::catching_panic(),
        "the dispatching thread starts unguarded"
    );
    doc.dispatch_resize(ResizeEvent {
        width: 80,
        height: 24,
    });

    assert_eq!(*guarded.lock().unwrap(), Some(true));
    assert!(
        !crate::panic::catching_panic(),
        "the guard must not outlive the dispatch"
    );
}

/// A targeted, bubbling handler gets the same guard as the document-level ones.
#[test]
fn targeted_handlers_run_guarded_and_survive_a_panic() {
    let doc = Document::new().unwrap();
    let guarded = Arc::new(Mutex::new(None));
    let reached_parent = Arc::new(AtomicUsize::new(0));

    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), child).unwrap();

    doc.on_click(child, |_| panic!("handler bug")).unwrap();

    let observed = guarded.clone();
    let parent_hits = reached_parent.clone();
    doc.on_click(doc.root(), move |_| {
        *observed.lock().unwrap() = Some(crate::panic::catching_panic());
        parent_hits.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    let mut click = MouseEvent::new(0, 0, MouseButton::Left);
    doc.dispatch_click_to(child, &mut click);

    assert_eq!(*guarded.lock().unwrap(), Some(true));
    assert_eq!(
        reached_parent.load(Ordering::Relaxed),
        1,
        "a panicking child handler must not stop the event bubbling"
    );
    assert!(!crate::panic::catching_panic());
}

/// A bell rides the next flush, so it is only ever heard if one is scheduled.
/// Nothing on screen has to change for that to be true.
#[tokio::test]
async fn bell_schedules_a_frame() {
    let doc = Document::new().unwrap();

    let notified = doc.inner.notify.notified();
    doc.bell();
    tokio::time::timeout(Duration::from_millis(100), notified)
        .await
        .expect("bell must wake the render task");
}

#[test]
fn a_pending_bell_is_claimed_once() {
    let doc = Document::new().unwrap();

    assert!(!doc.take_pending_bell(), "no bell has been rung yet");
    doc.bell();
    assert!(doc.take_pending_bell());
    assert!(
        !doc.take_pending_bell(),
        "the frame that claimed the bell consumed it"
    );
}

#[test]
fn window_focus_and_blur_reach_document_listeners() {
    let doc = Document::new().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));

    let focus_log = log.clone();
    doc.on_window_focus(move |_| focus_log.lock().unwrap().push("focus"));
    let blur_log = log.clone();
    doc.on_window_blur(move |_| blur_log.lock().unwrap().push("blur"));

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 3);
    runtime.simulate_window_blur();
    runtime.simulate_window_focus();
    runtime.simulate_window_blur();

    assert_eq!(&*log.lock().unwrap(), &["blur", "focus", "blur"]);
}

/// Window focus is the OS window, not the DOM. Alt-tabbing away and back must
/// return the user to the node they left — including through a modal's trapped
/// focus context, which is where losing it would be most destructive.
#[test]
fn window_focus_changes_leave_dom_focus_untouched() {
    let doc = Document::new().unwrap();

    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), modal).unwrap();
    let inner = focusable_child(&doc, modal);

    doc.push_focus_context(modal).unwrap();
    doc.focus(inner).unwrap();

    let focused_before = doc.focused();
    let depth_before = lock::mutex(&doc.inner.focus_contexts).depth();
    assert_eq!(focused_before, Some(inner));

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 3);
    runtime.simulate_window_blur();
    assert_eq!(
        doc.focused(),
        focused_before,
        "blur must not clear DOM focus"
    );

    runtime.simulate_window_focus();
    assert_eq!(doc.focused(), focused_before);
    assert_eq!(
        lock::mutex(&doc.inner.focus_contexts).depth(),
        depth_before,
        "the focus stack must survive a window focus cycle"
    );
}

#[test]
fn window_focus_listeners_can_be_removed() {
    let doc = Document::new().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));

    let hits = calls.clone();
    let handle = doc.on_window_focus(move |_| {
        hits.fetch_add(1, Ordering::Relaxed);
    });

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 3);
    runtime.simulate_window_focus();
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    assert!(doc.remove_listener(handle));
    runtime.simulate_window_focus();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

/// Focused input in the tree, plus a sink recording every `on_input` value.
fn input_with_recorder(doc: &Document, content: &str) -> (NodeId, Arc<Mutex<Vec<String>>>) {
    let input = doc.create_input(content).unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.focus(input).unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    doc.on_input(input, move |event| {
        sink.lock().unwrap().push(event.value.clone());
    })
    .unwrap();

    (input, seen)
}

#[test]
fn typing_fires_input_with_the_new_value() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "");

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('h')));
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('i')));

    // The value carried is the value *after* the edit, which is the whole point: an
    // on_key_press handler reading input_value() sees the state before it.
    assert_eq!(&*seen.lock().unwrap(), &["h".to_string(), "hi".to_string()]);
    assert_eq!(doc.input_value(input).unwrap(), "hi");
}

#[test]
fn a_keystroke_that_edits_nothing_fires_no_input_event() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "ab");

    // Backspace at the very start and delete at the very end both no-op.
    doc.set_input_cursor(input, 0).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Backspace));
    doc.set_input_cursor(input, 2).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Delete));

    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(doc.input_value(input).unwrap(), "ab");
}

#[test]
fn cursor_movement_fires_no_input_event() {
    let doc = Document::new().unwrap();
    let (_, seen) = input_with_recorder(&doc, "abc");

    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Up,
        KeyCode::Down,
    ] {
        doc.dispatch_key_press(KeyEvent::new(code));
    }

    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn set_input_value_fires_no_input_event() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "");

    // Programmatic writes stay silent, so a downstream two-way binding pushing a value
    // into the input cannot loop back through its own listener.
    doc.set_input_value(input, "written").unwrap();

    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(doc.input_value(input).unwrap(), "written");
}

#[test]
fn enter_fires_only_in_a_multiline_input() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "a");

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Enter));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(doc.input_value(input).unwrap(), "a");

    doc.set_input_multiline(input, true).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Enter));
    assert_eq!(&*seen.lock().unwrap(), &["a\n".to_string()]);
}

#[test]
fn backspace_over_a_selection_fires_once() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "abcd");

    doc.set_input_selection(input, 1..3).unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Backspace));

    assert_eq!(&*seen.lock().unwrap(), &["ad".to_string()]);
}

#[test]
fn input_events_bubble_to_an_ancestor() {
    let doc = Document::new().unwrap();
    let form = doc.create_box().unwrap();
    doc.append_child(doc.root(), form).unwrap();
    let input = doc.create_input("").unwrap();
    doc.append_child(form, input).unwrap();
    doc.focus(input).unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    doc.on_input(form, move |event| {
        sink.lock()
            .unwrap()
            .push((event.target(), event.current_target(), event.phase()));
    })
    .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));

    // A form-shaped container observes the field without registering on it.
    assert_eq!(&*seen.lock().unwrap(), &[(input, form, EventPhase::Bubble)]);
}

#[test]
fn stop_propagation_keeps_an_input_event_off_the_ancestor() {
    let doc = Document::new().unwrap();
    let form = doc.create_box().unwrap();
    doc.append_child(doc.root(), form).unwrap();
    let input = doc.create_input("").unwrap();
    doc.append_child(form, input).unwrap();
    doc.focus(input).unwrap();

    doc.on_input(input, |event| event.stop_propagation())
        .unwrap();
    let reached = Arc::new(AtomicUsize::new(0));
    let counter = reached.clone();
    doc.on_input(form, move |_| {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));

    assert_eq!(reached.load(Ordering::Relaxed), 0);
}

#[test]
fn preventing_the_key_default_suppresses_the_edit_and_the_event() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "");

    // The only place to intervene: the event itself reports a change already made, so it
    // carries no prevent_default of its own.
    doc.on_key_press(input, |event| event.prevent_default())
        .unwrap();
    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));

    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(doc.input_value(input).unwrap(), "");
}

#[test]
fn disabling_a_focused_input_blurs_it_so_later_keys_fire_nothing() {
    let doc = Document::new().unwrap();
    let (input, seen) = input_with_recorder(&doc, "");

    doc.set_disabled(input, true).unwrap();
    assert_eq!(doc.focused(), None);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));

    // This is why on_input needs no disabled/inert exemption: the default action only
    // runs on the focused node, and a disabled node cannot hold focus.
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(doc.input_value(input).unwrap(), "");
}

// ---------------------------------------------------------------------------
// Hiding a focused node
// ---------------------------------------------------------------------------

#[test]
fn hiding_an_ancestor_blurs_the_focused_node_and_stops_its_keys() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let input = doc.create_input("").unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, input).unwrap();
    doc.focus(input).unwrap();

    doc.update_style(panel, |style| style.display(Display::None))
        .unwrap();

    // The input's own display is still Flex — hiding prunes the subtree at the ancestor,
    // which is why this cannot be answered by reading one style.
    assert_eq!(doc.focused(), None);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));
    assert_eq!(doc.input_value(input).unwrap(), "");
}

#[test]
fn hiding_the_focused_node_itself_blurs_it() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();
    doc.focus(node).unwrap();

    let keys = Arc::new(AtomicUsize::new(0));
    let sink = keys.clone();
    doc.on_key_press(node, move |_| {
        sink.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();

    doc.update_style(node, |style| style.display(Display::None))
        .unwrap();
    assert_eq!(doc.focused(), None);

    doc.dispatch_key_press(KeyEvent::new(KeyCode::Char('x')));
    assert_eq!(keys.load(Ordering::SeqCst), 0);
}

#[test]
fn hiding_the_focused_node_dispatches_blur() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.set_focusable(node, true).unwrap();
    doc.focus(node).unwrap();

    let blurred = Arc::new(Mutex::new(Vec::new()));
    let sink = blurred.clone();
    doc.on_blur(node, move |event| {
        sink.lock().unwrap().push(event.target());
    })
    .unwrap();

    doc.update_style(node, |style| style.display(Display::None))
        .unwrap();

    // Focus does not just vanish: downstream that tracks it by event stays in sync.
    assert_eq!(&*blurred.lock().unwrap(), &[node]);
}

#[test]
fn hiding_an_unrelated_subtree_leaves_focus_alone() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();
    let other = doc.create_box().unwrap();
    doc.append_child(doc.root(), node).unwrap();
    doc.append_child(doc.root(), other).unwrap();
    doc.set_focusable(node, true).unwrap();
    doc.focus(node).unwrap();

    doc.update_style(other, |style| style.display(Display::None))
        .unwrap();

    assert_eq!(doc.focused(), Some(node));
}

#[test]
fn a_node_under_a_hidden_ancestor_cannot_be_focused() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let node = doc.create_box().unwrap();
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(panel, node).unwrap();
    doc.set_focusable(node, true).unwrap();

    doc.update_style(panel, |style| style.display(Display::None))
        .unwrap();

    assert_eq!(
        doc.focus(node),
        Err(TuidomError::NodeNotFocusable { id: node })
    );
    assert_eq!(doc.focused(), None);
}

#[test]
fn popping_a_context_does_not_restore_focus_to_a_node_hidden_meanwhile() {
    let doc = Document::new().unwrap();
    let panel = doc.create_box().unwrap();
    let outer = focusable_child(&doc, panel);
    let modal = stacking_context_box(&doc);
    doc.append_child(doc.root(), panel).unwrap();
    doc.append_child(doc.root(), modal).unwrap();
    focusable_child(&doc, modal);

    doc.focus(outer).unwrap();
    doc.push_focus_context(modal).unwrap();

    // Hiding what the outer context remembers, while it is not the active context.
    doc.update_style(panel, |style| style.display(Display::None))
        .unwrap();

    doc.pop_focus_context().unwrap();
    assert_eq!(doc.focused(), None);
}

// -- Keyboard selection extension ---------------------------------------------

/// Two stacked Text nodes on a 10×4 screen, so extension has both a within-node step
/// and a node boundary to cross.
fn selection_extension_setup() -> (Document, HeadlessRuntime) {
    let doc = Document::new().unwrap();
    let mut column = Style::new();
    column.flex_direction(FlexDirection::Column);
    doc.set_style(doc.root(), &column).unwrap();

    for content in ["abcd", "efgh"] {
        let text = doc.create_text(content).unwrap();
        doc.append_child(doc.root(), text).unwrap();
    }

    let mut runtime = HeadlessRuntime::new(doc.clone(), 10, 4);
    runtime.render().unwrap();
    (doc, runtime)
}

/// Extend-only: with nothing selected there is no anchor to grow from, so the key is
/// declined outright rather than inventing a starting point.
#[test]
fn shift_arrows_do_nothing_without_an_existing_selection() {
    let (doc, mut runtime) = selection_extension_setup();
    assert_eq!(doc.get_selection(), None);

    runtime.simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT);
    runtime.simulate_key_with_modifiers(KeyCode::Down, KeyModifiers::SHIFT);

    assert_eq!(doc.get_selection(), None);
}

/// Horizontal extension steps by a grapheme and keeps going into the next Text node —
/// a selection is a range in the document, not in one node.
#[test]
fn shift_right_extends_by_a_grapheme_and_crosses_into_the_next_node() {
    let (doc, mut runtime) = selection_extension_setup();

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
    runtime.simulate_mouse_up(1, 0, MouseButton::Left);
    assert_eq!(doc.get_selection().as_deref(), Some("ab"));

    runtime.simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("abc"));

    runtime.simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT);
    runtime.simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("abcd"));

    // Off the end of the first node: the next step lands in the second one.
    runtime.simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("abcd\ne"));

    // And shrinks back the same way, since the anchor never moved.
    runtime.simulate_key_with_modifiers(KeyCode::Left, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("abcd"));
}

/// Vertical extension re-snaps through the same screen-cell mapping a drag uses, so it
/// lands on the column it started from.
#[test]
fn shift_down_extends_to_the_row_below() {
    let (doc, mut runtime) = selection_extension_setup();

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
    runtime.simulate_mouse_up(1, 0, MouseButton::Left);
    assert_eq!(doc.get_selection().as_deref(), Some("ab"));

    runtime.simulate_key_with_modifiers(KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("abcd\nef"));

    runtime.simulate_key_with_modifiers(KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(doc.get_selection().as_deref(), Some("ab"));
}

/// A plain arrow is still focus navigation, and a control chord is unbound here rather
/// than widened to its plain form.
#[test]
fn only_shifted_arrows_extend_a_selection() {
    let (doc, mut runtime) = selection_extension_setup();

    runtime.simulate_mouse_down(0, 0, MouseButton::Left);
    runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
    runtime.simulate_mouse_up(1, 0, MouseButton::Left);

    runtime.simulate_key(KeyCode::Right);
    assert_eq!(doc.get_selection().as_deref(), Some("ab"));

    runtime
        .simulate_key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT | KeyModifiers::CONTROL);
    assert_eq!(doc.get_selection().as_deref(), Some("ab"));
}

/// `Debug` must not read the arena.
///
/// It is reachable from inside a held `nodes` guard — a `tracing::error!("{doc:?}")` in a
/// listener does exactly that — so an impl printing node count would take shard locks
/// under that guard and deadlock instead of returning. Formatting under a real guard is
/// the only way to assert it does not, and the failure mode is a hang, so the format runs
/// on another thread against a timeout.
#[test]
fn debug_does_not_touch_the_arena() {
    let doc = Document::new().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), child).unwrap();

    let guard = doc.inner.nodes.get_mut(&child).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let probe = doc.clone();
    std::thread::spawn(move || {
        let _ = tx.send(format!("{probe:?}"));
    });

    let rendered = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Debug deadlocked against a held arena guard");
    drop(guard);

    assert!(rendered.starts_with("Document {"), "got {rendered}");
    assert!(rendered.contains("nodes_allocated"), "got {rendered}");
}

/// The count is allocations, not live nodes, and the field name has to keep saying so —
/// `NodeId` indices come from a monotonic counter and are never reused, so removing a
/// subtree cannot decrement it.
#[test]
fn debug_reports_allocations_not_live_nodes() {
    let doc = Document::new().unwrap();
    let child = doc.create_box().unwrap();
    doc.append_child(doc.root(), child).unwrap();
    let before = format!("{doc:?}");

    doc.remove_child(doc.root(), child).unwrap();

    assert!(doc.get_node(child).is_none());
    assert_eq!(format!("{doc:?}"), before);
}

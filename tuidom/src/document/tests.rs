use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::TuidomError;
use crate::animation::{Easing, TransitionConfig};
use crate::event::{
    EventPhase, FocusEventRelation, FocusKeys, KeyCode, KeyEvent, MouseButton, MouseEvent,
    ResizeEvent, WheelEvent,
};
use crate::headless::{HeadlessRuntime, ScreenColor};
use crate::node::{LayoutRect, NodeKindView};
use crate::performance::PerformanceDetail;
use crate::style::{
    AlignContent, AlignItems, AlignSelf, Color, CursorShape, Display, EdgeInsets, FlexDirection,
    FlexGap, FlexWrap, Length, Position, Style,
};

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
    parent_style.width(Length::Pixels(2));
    parent_style.height(Length::Pixels(1));
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
    assert_eq!(doc.get_node(badge).unwrap().layout.unwrap().x, 3);
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
    button_style.width(Length::Pixels(1));
    button_style.height(Length::Pixels(1));
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
        Some(Color::yellow())
    );

    // Active merges on top of focus, and only overrides what it sets.
    doc.set_active(node, true).unwrap();
    let pressed = doc.resolved_style(node).unwrap();
    assert_eq!(pressed.background, Some(Color::red()));
    assert_eq!(pressed.color, Color::white());

    doc.set_active(node, false).unwrap();
    assert_eq!(
        doc.resolved_style(node).unwrap().background,
        Some(Color::yellow())
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
fn input_layout_measures_displayed_single_line_multiline_and_masked_content() {
    let doc = Document::new().unwrap();
    doc.update_style(doc.root(), |style| {
        style.align_items(AlignItems::FlexStart);
    })
    .unwrap();
    let input = doc.create_input("ab\ncdef").unwrap();
    doc.append_child(doc.root(), input).unwrap();

    doc.compute_layout(20, 5).unwrap();
    let single_line = doc.get_node(input).unwrap().layout.unwrap();
    assert_eq!(single_line.width, 7);
    assert_eq!(single_line.height, 1);

    doc.set_input_multiline(input, true).unwrap();
    doc.compute_layout(20, 5).unwrap();
    let multiline = doc.get_node(input).unwrap().layout.unwrap();
    assert_eq!(multiline.width, 4);
    assert_eq!(multiline.height, 2);

    doc.set_input_value(input, "abcd").unwrap();
    doc.set_input_mask(input, Some('界')).unwrap();
    doc.compute_layout(20, 5).unwrap();
    let masked = doc.get_node(input).unwrap().layout.unwrap();
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
    style.width(Length::Pixels(3));
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
    style.width(Length::Pixels(2));
    style.height(Length::Pixels(2));
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
fn focused_block_cursor_inverts_cell_and_exposes_metadata() {
    let doc = Document::new().unwrap();
    let input = doc.create_input("A").unwrap();
    let other = doc.create_input("B").unwrap();
    doc.append_child(doc.root(), input).unwrap();
    doc.append_child(doc.root(), other).unwrap();

    let mut style = Style::new();
    style.width(Length::Pixels(3));
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
    style.width(Length::Pixels(1));
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
    child_style.width(Length::Pixels(7));
    child_style.height(Length::Pixels(1));
    doc.set_style(child, &child_style).unwrap();
    doc.append_child(root, child).unwrap();
    doc.compute_layout(20, 5).unwrap();

    let before = doc.get_node(child).unwrap().layout.unwrap();

    doc.remove_layout_mapping_for_test(child);

    assert_eq!(
        doc.compute_layout(20, 5),
        Err(TuidomError::LayoutMappingMissing { id: child })
    );
    let after = doc.get_node(child).unwrap().layout.unwrap();
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
    root_style.width(Length::Pixels(10));
    root_style.height(Length::Pixels(1));
    doc.set_style(root, &root_style).unwrap();

    let mut child_style = Style::new();
    child_style.inherit_width();
    child_style.height(Length::Pixels(1));
    doc.set_style(child, &child_style).unwrap();

    doc.append_child(root, child).unwrap();
    let before = doc.layout_mapping_snapshot();

    doc.compute_layout(100, 10).unwrap();
    assert_eq!(doc.get_node(child).unwrap().layout.unwrap().width, 10);

    doc.update_style(root, |style| style.width(Length::Pixels(20)))
        .unwrap();
    doc.compute_layout(100, 10).unwrap();

    assert_eq!(doc.layout_mapping_snapshot(), before);
    assert_eq!(doc.get_node(child).unwrap().layout.unwrap().width, 20);
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
    lock::rw_write(&doc.inner.layout_rects).insert(node, layout);
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
            next: vec![KeyCode::Tab],
            previous: vec![KeyCode::BackTab],
            up: vec![KeyCode::Up],
            down: vec![KeyCode::Down],
            left: vec![KeyCode::Left],
            right: vec![KeyCode::Right],
            blur: vec![KeyCode::Esc],
        }
    );
}

#[test]
fn focus_keys_are_configurable() {
    let doc = Document::new().unwrap();
    let keys = FocusKeys {
        next: vec![KeyCode::Char('n')],
        previous: vec![KeyCode::Char('p')],
        up: vec![KeyCode::Char('k')],
        down: vec![KeyCode::Char('j')],
        left: vec![KeyCode::Char('h')],
        right: vec![KeyCode::Char('l')],
        blur: vec![KeyCode::Char('q')],
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
    base.width(Length::Pixels(1));
    base.height(Length::Pixels(1));
    base.color(Color::blue());
    doc.set_style(node, &base).unwrap();

    let mut focus = Style::new();
    focus.color(Color::red());
    focus.background(Color::green());
    doc.set_focus_style(node, &focus).unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().color, Color::blue());
    assert_eq!(doc.resolved_style(node).unwrap().background, None);

    doc.focus(node).unwrap();
    let focused = doc.resolved_style(node).unwrap();
    assert_eq!(focused.width, Length::Pixels(1));
    assert_eq!(focused.height, Length::Pixels(1));
    assert_eq!(focused.color, Color::red());
    assert_eq!(focused.background, Some(Color::green()));

    doc.clear_focus_style(node).unwrap();
    let cleared = doc.resolved_style(node).unwrap();
    assert_eq!(cleared.color, Color::blue());
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
    base.width(Length::Pixels(1));
    base.height(Length::Pixels(1));
    doc.set_style(node, &base).unwrap();

    let mut focus = Style::new();
    focus.width(Length::Pixels(4));
    doc.set_focus_style(node, &focus).unwrap();

    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().width, 1);

    doc.focus(node).unwrap();
    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().width, 4);

    doc.blur();
    doc.compute_layout(10, 3).unwrap();
    assert_eq!(doc.get_node(node).unwrap().layout.unwrap().width, 1);
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
        next: vec![KeyCode::Char('n')],
        previous: vec![KeyCode::Char('p')],
        up: Vec::new(),
        down: Vec::new(),
        left: Vec::new(),
        right: Vec::new(),
        blur: vec![KeyCode::Char('q')],
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
    current_style.width(Length::Pixels(2));
    current_style.height(Length::Pixels(2));
    doc.set_style(current, &current_style).unwrap();

    let mut absolute_style = Style::new();
    absolute_style.width(Length::Pixels(2));
    absolute_style.height(Length::Pixels(2));
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
    style.width(Length::Pixels(42));
    style.padding(EdgeInsets::symmetric(2, 1));
    style.margin(EdgeInsets::new(1, 2, 3, 4));
    style.flex_direction(FlexDirection::Column);
    style.flex_basis(Length::Pixels(3));
    style.flex_grow(1.0);
    style.flex_shrink(0.5);
    style.flex_wrap(FlexWrap::Wrap);
    style.gap(FlexGap::new(1, 2));
    style.align_self(AlignSelf::Center);
    style.align_content(AlignContent::Center);
    doc.set_style(node, &style).unwrap();

    let resolved = doc.resolved_style(node).unwrap();
    assert_eq!(resolved.width, Length::Pixels(42));
    assert_eq!(resolved.padding, EdgeInsets::symmetric(2, 1));
    assert_eq!(resolved.margin, EdgeInsets::new(1, 2, 3, 4));
    assert_eq!(resolved.flex_direction, FlexDirection::Column);
    assert_eq!(resolved.flex_basis, Length::Pixels(3));
    assert_eq!(resolved.flex_grow, 1.0);
    assert_eq!(resolved.flex_shrink, 0.5);
    assert_eq!(resolved.flex_wrap, FlexWrap::Wrap);
    assert_eq!(resolved.gap, FlexGap::new(1, 2));
    assert_eq!(resolved.align_self, Some(AlignSelf::Center));
    assert_eq!(resolved.align_content, AlignContent::Center);
    assert_eq!(resolved.opacity, 1.0); // Inherit → default
    assert_eq!(resolved.color, Color::white()); // Inherit → default
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
    style.width(Length::Pixels(10));
    doc.set_style(node, &style).unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));

    doc.update_style(node, |s| {
        s.width(Length::Pixels(20));
    })
    .unwrap();

    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(20));
}

#[test]
fn panicking_update_style_does_not_partially_mutate_style() {
    let doc = Document::new().unwrap();
    let node = doc.create_box().unwrap();

    let mut style = Style::new();
    style.width(Length::Pixels(10));
    doc.set_style(node, &style).unwrap();
    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = doc.update_style(node, |style| {
            style.width(Length::Pixels(20));
            panic!("boom");
        });
    }));

    assert!(result.is_err());
    assert_eq!(doc.resolved_style(node).unwrap().width, Length::Pixels(10));
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
    assert_eq!(child_resolved.color, Color::white());
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
    parent_style.flex_basis(Length::Pixels(3));
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
    assert_eq!(child_resolved.color, Color::red());
    assert_eq!(child_resolved.padding, EdgeInsets::all(2));
    assert_eq!(child_resolved.margin, EdgeInsets::new(1, 2, 3, 4));
    assert_eq!(child_resolved.flex_direction, FlexDirection::Column);
    assert_eq!(child_resolved.flex_basis, Length::Pixels(3));
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
    assert_eq!(child_resolved.color, Color::blue()); // Override wins
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

    assert_eq!(doc.resolved_style(child).unwrap().color, Color::red());

    // Move to blue parent
    doc.move_child(parent_blue, child, child).unwrap();
    assert_eq!(doc.resolved_style(child).unwrap().color, Color::blue());
}

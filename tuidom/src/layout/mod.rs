//! Taffy-based flexbox layout.
//!
//! Builds a parallel taffy tree from the DOM, computes layout, and stores
//! resulting positions/sizes back on each node.

use std::collections::HashMap;

use taffy::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::id::NodeId;
use crate::node::LayoutRect;
use crate::style::resolution::ResolvedStyle;
use crate::style::{AlignItems, Display, JustifyContent, Length};

// ---------------------------------------------------------------------------
// Measure context — stored per-leaf node for text measurement
// ---------------------------------------------------------------------------

/// Context attached to taffy leaf nodes for text measurement.
#[derive(Clone)]
enum MeasureContext {
    /// Non-text leaf (no measurement needed).
    None,
    /// Text node content for width calculation.
    Text { content: String },
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a taffy tree from the DOM and compute layout for all nodes.
///
/// Stores the resulting [`LayoutRect`] on each node in the document.
/// Nodes with `display: None` are skipped and receive no layout.
pub fn compute_layout(doc: &Document, screen_width: u16, screen_height: u16) {
    let root = match doc.root() {
        Some(r) => r,
        None => return,
    };

    clear_layouts(doc, root);

    let mut taffy_tree = TaffyTree::<MeasureContext>::new();
    let mut mapping = HashMap::new();

    let taffy_root = build_node(doc, &mut taffy_tree, root, &mut mapping);

    if let Some(taffy_root) = taffy_root {
        let available = Size {
            width: AvailableSpace::Definite(screen_width as f32),
            height: AvailableSpace::Definite(screen_height as f32),
        };

        if let Err(err) = taffy_tree.compute_layout_with_measure(taffy_root, available, measure_fn)
        {
            log::error!("taffy compute_layout failed: {err:?}");
            return;
        }

        // Store absolute screen coordinates back on DOM nodes. Taffy stores
        // locations relative to each node's parent, while the renderer paints
        // from screen-space coordinates.
        store_layouts(doc, &taffy_tree, root, &mapping, 0.0, 0.0);
    }
}

fn clear_layouts(doc: &Document, node_id: NodeId) {
    if let Some(mut data) = doc.inner.nodes.get_mut(&node_id) {
        data.layout = None;
    }

    for child in doc.get_children(node_id) {
        clear_layouts(doc, child);
    }
}

fn store_layouts(
    doc: &Document,
    taffy_tree: &TaffyTree<MeasureContext>,
    node_id: NodeId,
    mapping: &HashMap<NodeId, taffy::NodeId>,
    parent_x: f32,
    parent_y: f32,
) {
    let Some(&taffy_id) = mapping.get(&node_id) else {
        return;
    };

    let Ok(layout) = taffy_tree.layout(taffy_id) else {
        return;
    };

    let absolute_x = parent_x + layout.location.x;
    let absolute_y = parent_y + layout.location.y;
    let rect = LayoutRect {
        x: round_to_u16(absolute_x),
        y: round_to_u16(absolute_y),
        width: round_to_u16(layout.size.width),
        height: round_to_u16(layout.size.height),
    };

    if let Some(mut data) = doc.inner.nodes.get_mut(&node_id) {
        data.layout = Some(rect);
    }

    for child in doc.get_children(node_id) {
        store_layouts(doc, taffy_tree, child, mapping, absolute_x, absolute_y);
    }
}

fn round_to_u16(value: f32) -> u16 {
    value.round().clamp(0.0, u16::MAX as f32) as u16
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Recursively build a taffy node for a DOM node.
fn build_node(
    doc: &Document,
    taffy: &mut TaffyTree<MeasureContext>,
    node_id: NodeId,
    mapping: &mut HashMap<NodeId, taffy::NodeId>,
) -> Option<taffy::NodeId> {
    let Ok(resolved) = doc.resolved_style(node_id) else {
        return None;
    };

    // Skip hidden nodes
    if resolved.display == Display::None {
        return None;
    }

    let children_ids = doc.get_children(node_id);

    let taffy_node = if children_ids.is_empty() {
        build_leaf(taffy, doc, node_id)
    } else {
        build_container(taffy, doc, node_id, &children_ids, mapping)
    };

    if let Some(tn) = taffy_node {
        mapping.insert(node_id, tn);
    }

    taffy_node
}

/// Build a leaf node (no children).
fn build_leaf(
    taffy: &mut TaffyTree<MeasureContext>,
    doc: &Document,
    node_id: NodeId,
) -> Option<taffy::NodeId> {
    let node_view = doc.get_node(node_id)?;
    let resolved = doc.resolved_style(node_id).ok()?;
    let style = to_taffy_leaf_style(&resolved);

    let context = match &node_view.kind {
        crate::node::NodeKindView::Text { content } => MeasureContext::Text {
            content: content.clone(),
        },
        _ => MeasureContext::None,
    };

    match taffy.new_leaf_with_context(style, context) {
        Ok(node) => Some(node),
        Err(err) => {
            log::error!("taffy new_leaf_with_context failed for {node_id:?}: {err:?}");
            None
        }
    }
}

/// Build a container node with children.
fn build_container(
    taffy: &mut TaffyTree<MeasureContext>,
    doc: &Document,
    _node_id: NodeId,
    children_ids: &[NodeId],
    mapping: &mut HashMap<NodeId, taffy::NodeId>,
) -> Option<taffy::NodeId> {
    let resolved = doc.resolved_style(_node_id).ok()?;
    let style = to_taffy_container_style(&resolved);

    let child_taffy_nodes: Vec<taffy::NodeId> = children_ids
        .iter()
        .filter_map(|&child| build_node(doc, taffy, child, mapping))
        .collect();

    match taffy.new_with_children(style, &child_taffy_nodes) {
        Ok(node) => Some(node),
        Err(err) => {
            log::error!("taffy new_with_children failed for {_node_id:?}: {err:?}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Measure function
// ---------------------------------------------------------------------------

/// Measure function passed to taffy for computing text node sizes.
fn measure_fn(
    _known_dimensions: Size<Option<f32>>,
    _available_space: Size<AvailableSpace>,
    _node_id: taffy::NodeId,
    context: Option<&mut MeasureContext>,
    _style: &Style,
) -> Size<f32> {
    match context {
        Some(MeasureContext::Text { content }) => {
            let width = content
                .lines()
                .map(|line| UnicodeWidthStr::width(line) as f32)
                .fold(0.0_f32, f32::max);
            let height = content.lines().count() as f32;
            Size { width, height }
        }
        _ => Size::ZERO,
    }
}

// ---------------------------------------------------------------------------
// Style translation
// ---------------------------------------------------------------------------

/// Convert a resolved style to a taffy style for a leaf node (no children).
fn to_taffy_leaf_style(resolved: &ResolvedStyle) -> Style {
    Style {
        size: Size {
            width: to_dimension(resolved.width),
            height: to_dimension(resolved.height),
        },
        ..Default::default()
    }
}

/// Convert a resolved style to a taffy style for a container node (has children).
fn to_taffy_container_style(resolved: &ResolvedStyle) -> Style {
    Style {
        display: match resolved.display {
            Display::Flex => taffy::style::Display::Flex,
            Display::None => taffy::style::Display::None,
        },
        size: Size {
            width: to_dimension(resolved.width),
            height: to_dimension(resolved.height),
        },
        align_items: Some(to_align_items(resolved.align_items)),
        justify_content: Some(to_justify_content(resolved.justify_content)),
        ..Default::default()
    }
}

fn to_dimension(length: Length) -> Dimension {
    match length {
        Length::Pixels(n) => Dimension::length(n as f32),
        Length::Percent(p) => Dimension::percent(p as f32 / 100.0),
        Length::Auto => Dimension::auto(),
    }
}

fn to_align_items(a: AlignItems) -> taffy::style::AlignItems {
    match a {
        AlignItems::FlexStart => taffy::style::AlignItems::FLEX_START,
        AlignItems::FlexEnd => taffy::style::AlignItems::FLEX_END,
        AlignItems::Center => taffy::style::AlignItems::CENTER,
        AlignItems::Stretch => taffy::style::AlignItems::STRETCH,
    }
}

fn to_justify_content(j: JustifyContent) -> taffy::style::JustifyContent {
    match j {
        JustifyContent::FlexStart => taffy::style::JustifyContent::FLEX_START,
        JustifyContent::FlexEnd => taffy::style::JustifyContent::FLEX_END,
        JustifyContent::Center => taffy::style::JustifyContent::CENTER,
        JustifyContent::SpaceBetween => taffy::style::JustifyContent::SPACE_BETWEEN,
        JustifyContent::SpaceAround => taffy::style::JustifyContent::SPACE_AROUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style as DomStyle;

    fn fixed_centered_style(width: u16, height: u16) -> DomStyle {
        let mut style = DomStyle::new();
        style.width(Length::Pixels(width));
        style.height(Length::Pixels(height));
        style.justify_content(JustifyContent::Center);
        style.align_items(AlignItems::Center);
        style
    }

    #[test]
    fn nested_layout_positions_are_stored_as_absolute_screen_coordinates() {
        let doc = Document::new();

        let root = doc.create_box();
        let parent = doc.create_box();
        let child = doc.create_box();

        doc.set_style(root, &fixed_centered_style(100, 40)).unwrap();
        doc.set_style(parent, &fixed_centered_style(50, 20))
            .unwrap();

        let mut child_style = DomStyle::new();
        child_style.width(Length::Pixels(10));
        child_style.height(Length::Pixels(5));
        doc.set_style(child, &child_style).unwrap();

        doc.append_child(root, parent).unwrap();
        doc.append_child(parent, child).unwrap();
        doc.set_root(root);

        compute_layout(&doc, 100, 40);

        let root_layout = doc.get_node(root).unwrap().layout.unwrap();
        let parent_layout = doc.get_node(parent).unwrap().layout.unwrap();
        let child_layout = doc.get_node(child).unwrap().layout.unwrap();

        assert_eq!((root_layout.x, root_layout.y), (0, 0));
        assert_eq!((parent_layout.x, parent_layout.y), (25, 10));
        assert_eq!((child_layout.x, child_layout.y), (45, 18));
    }
}

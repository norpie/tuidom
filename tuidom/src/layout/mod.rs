//! Taffy-based flexbox layout.
//!
//! Builds a parallel taffy tree from the DOM, computes layout, and stores
//! resulting positions/sizes back on each node.

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

    let mut taffy_tree = TaffyTree::<MeasureContext>::new();
    let mut mapping = Vec::new();

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

        // Store results back on DOM nodes
        for (node_id, taffy_id) in &mapping {
            if let Ok(layout) = taffy_tree.layout(*taffy_id) {
                let rect = LayoutRect {
                    x: layout.location.x.round() as u16,
                    y: layout.location.y.round() as u16,
                    width: layout.size.width.round() as u16,
                    height: layout.size.height.round() as u16,
                };
                if let Some(mut data) = doc.inner.nodes.get_mut(node_id) {
                    data.layout = Some(rect);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Recursively build a taffy node for a DOM node.
fn build_node(
    doc: &Document,
    taffy: &mut TaffyTree<MeasureContext>,
    node_id: NodeId,
    mapping: &mut Vec<(NodeId, taffy::NodeId)>,
) -> Option<taffy::NodeId> {
    let resolved = doc.resolved_style(node_id);

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
        mapping.push((node_id, tn));
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
    let resolved = doc.resolved_style(node_id);
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
    mapping: &mut Vec<(NodeId, taffy::NodeId)>,
) -> Option<taffy::NodeId> {
    let resolved = doc.resolved_style(_node_id);
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

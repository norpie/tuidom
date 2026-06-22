//! Taffy-based flexbox layout.
//!
//! Maintains a persistent 1:1 mapping from DOM nodes to taffy nodes, computes
//! layout through taffy, and stores absolute screen-space layout rectangles back
//! onto DOM nodes.

use std::collections::{HashMap, HashSet};

use taffy::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::id::NodeId;
use crate::lock;
use crate::node::{LayoutRect, NodeKind};
use crate::style::resolution::ResolvedStyle;
use crate::style::{AlignItems, Display, JustifyContent, Length};

// ---------------------------------------------------------------------------
// Persistent layout engine
// ---------------------------------------------------------------------------

/// Document-owned persistent layout engine.
pub(crate) struct LayoutEngine {
    taffy: TaffyTree<MeasureContext>,
    mapping: HashMap<NodeId, taffy::prelude::NodeId>,
    reverse_mapping: HashMap<taffy::prelude::NodeId, NodeId>,
    last_laid_out: HashSet<NodeId>,
}

// Taffy stores compact length values in tagged raw pointers, which prevents
// automatic `Send` derivation. This is a known upstream issue; see:
// - https://github.com/DioxusLabs/taffy/issues/823
// - https://github.com/DioxusLabs/taffy/pull/855
// The layout engine is only accessed behind `DocumentInner::layout`'s mutex,
// and tuidom only constructs taffy styles from plain numeric/default values,
// so moving the engine between threads is safe.
unsafe impl Send for LayoutEngine {}

impl LayoutEngine {
    /// Create an empty layout engine.
    pub fn new() -> Self {
        Self {
            taffy: TaffyTree::new(),
            mapping: HashMap::new(),
            reverse_mapping: HashMap::new(),
            last_laid_out: HashSet::new(),
        }
    }

    /// Insert the persistent taffy node for a newly allocated DOM node.
    pub fn insert_node(&mut self, node_id: NodeId, kind: &NodeKind, resolved: &ResolvedStyle) {
        if self.mapping.contains_key(&node_id) {
            self.update_node(node_id, kind, resolved);
            return;
        }

        let style = to_taffy_style(resolved);
        let created = match kind {
            NodeKind::Text { content } => self.taffy.new_leaf_with_context(
                style,
                MeasureContext::Text {
                    content: content.clone(),
                },
            ),
            NodeKind::Box => self.taffy.new_leaf(style),
        };

        match created {
            Ok(taffy_id) => {
                self.mapping.insert(node_id, taffy_id);
                self.reverse_mapping.insert(taffy_id, node_id);
            }
            Err(err) => log::error!("taffy node creation failed for {node_id:?}: {err:?}"),
        }
    }

    /// Remove a DOM node's persistent taffy node.
    pub fn remove_node(&mut self, node_id: NodeId) {
        let Some(taffy_id) = self.mapping.remove(&node_id) else {
            return;
        };
        self.reverse_mapping.remove(&taffy_id);
        self.last_laid_out.remove(&node_id);

        if let Err(err) = self.taffy.remove(taffy_id) {
            log::error!("taffy node removal failed for {node_id:?}: {err:?}");
        }
    }

    /// Update style and measurement context for an existing node.
    pub fn update_node(&mut self, node_id: NodeId, kind: &NodeKind, resolved: &ResolvedStyle) {
        self.set_style(node_id, resolved);
        self.set_measure_context(node_id, kind);
    }

    /// Update a node's taffy style.
    pub fn set_style(&mut self, node_id: NodeId, resolved: &ResolvedStyle) {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return;
        };

        if let Err(err) = self.taffy.set_style(taffy_id, to_taffy_style(resolved)) {
            log::error!("taffy style update failed for {node_id:?}: {err:?}");
        }
    }

    /// Update a node's taffy measurement context from DOM data.
    pub fn set_measure_context(&mut self, node_id: NodeId, kind: &NodeKind) {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return;
        };

        let context = match kind {
            NodeKind::Text { content } => Some(MeasureContext::Text {
                content: content.clone(),
            }),
            NodeKind::Box => None,
        };

        if let Err(err) = self.taffy.set_node_context(taffy_id, context) {
            log::error!("taffy context update failed for {node_id:?}: {err:?}");
        }
    }

    /// Replace a parent's taffy child list with the DOM child order.
    pub fn sync_children(&mut self, parent: NodeId, children: &[NodeId]) {
        let Some(&parent_taffy) = self.mapping.get(&parent) else {
            return;
        };

        let taffy_children: Vec<_> = children
            .iter()
            .filter_map(|child| {
                let mapped = self.mapping.get(child).copied();
                if mapped.is_none() {
                    log::error!("missing taffy mapping for child {child:?} of parent {parent:?}");
                }
                mapped
            })
            .collect();

        if let Err(err) = self.taffy.set_children(parent_taffy, &taffy_children) {
            log::error!("taffy child sync failed for {parent:?}: {err:?}");
        }
    }

    fn compute(
        &mut self,
        root: NodeId,
        visible_children: &HashMap<NodeId, Vec<NodeId>>,
        screen_width: u16,
        screen_height: u16,
    ) -> Vec<(NodeId, LayoutRect)> {
        let Some(&taffy_root) = self.mapping.get(&root) else {
            return Vec::new();
        };

        let available = Size {
            width: AvailableSpace::Definite(screen_width as f32),
            height: AvailableSpace::Definite(screen_height as f32),
        };

        if let Err(err) = self
            .taffy
            .compute_layout_with_measure(taffy_root, available, measure_fn)
        {
            log::error!("taffy compute_layout failed: {err:?}");
            return Vec::new();
        }

        let mut layouts = Vec::new();
        self.collect_absolute_layouts(root, visible_children, 0.0, 0.0, &mut layouts);
        self.last_laid_out = layouts.iter().map(|(id, _)| *id).collect();
        layouts
    }

    fn collect_absolute_layouts(
        &self,
        node_id: NodeId,
        visible_children: &HashMap<NodeId, Vec<NodeId>>,
        parent_x: f32,
        parent_y: f32,
        out: &mut Vec<(NodeId, LayoutRect)>,
    ) {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return;
        };
        let Ok(layout) = self.taffy.layout(taffy_id) else {
            return;
        };

        let absolute_x = parent_x + layout.location.x;
        let absolute_y = parent_y + layout.location.y;
        out.push((
            node_id,
            LayoutRect {
                x: round_to_u16(absolute_x),
                y: round_to_u16(absolute_y),
                width: round_to_u16(layout.size.width),
                height: round_to_u16(layout.size.height),
            },
        ));

        if let Some(children) = visible_children.get(&node_id) {
            for child in children {
                self.collect_absolute_layouts(
                    *child,
                    visible_children,
                    absolute_x,
                    absolute_y,
                    out,
                );
            }
        }
    }

    fn stale_layouts(&self, visible: &HashSet<NodeId>) -> Vec<NodeId> {
        self.last_laid_out
            .difference(visible)
            .copied()
            .collect::<Vec<_>>()
    }

    #[cfg(test)]
    pub fn mapped_node_count(&self) -> usize {
        self.mapping.len()
    }

    #[cfg(test)]
    pub fn mapping_snapshot(&self) -> Vec<(NodeId, taffy::prelude::NodeId)> {
        let mut entries = self
            .mapping
            .iter()
            .map(|(dom, taffy)| (*dom, *taffy))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(dom, _)| dom.index);
        entries
    }

    #[cfg(test)]
    pub fn dom_children(&self, parent: NodeId) -> Vec<NodeId> {
        let Some(&parent_taffy) = self.mapping.get(&parent) else {
            return Vec::new();
        };
        let Ok(children) = self.taffy.children(parent_taffy) else {
            return Vec::new();
        };
        children
            .into_iter()
            .filter_map(|child| self.reverse_mapping.get(&child).copied())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Measure context
// ---------------------------------------------------------------------------

/// Context attached to taffy leaf nodes for text measurement.
#[derive(Clone, Debug)]
enum MeasureContext {
    /// Text node content for width calculation.
    Text { content: String },
}

// ---------------------------------------------------------------------------
// Public layout entry point
// ---------------------------------------------------------------------------

/// Compute layout for the current document root using persistent taffy state.
pub fn compute_layout(doc: &Document, screen_width: u16, screen_height: u16) {
    let Some(root) = doc.root() else {
        clear_all_previous_layouts(doc);
        return;
    };

    let mut visible = HashSet::new();
    let mut visible_children = HashMap::new();
    collect_visible_tree(doc, root, &mut visible, &mut visible_children);

    let (stale, layouts) = {
        let mut engine = lock::mutex(&doc.inner.layout);
        let stale = engine.stale_layouts(&visible);
        let layouts = if visible.contains(&root) {
            engine.compute(root, &visible_children, screen_width, screen_height)
        } else {
            Vec::new()
        };
        (stale, layouts)
    };

    for id in stale {
        if let Some(mut data) = doc.inner.nodes.get_mut(&id) {
            data.layout = None;
        }
    }

    for id in visible {
        if let Some(mut data) = doc.inner.nodes.get_mut(&id) {
            data.layout = None;
        }
    }

    for (id, rect) in layouts {
        if let Some(mut data) = doc.inner.nodes.get_mut(&id) {
            data.layout = Some(rect);
        }
    }
}

fn clear_all_previous_layouts(doc: &Document) {
    let stale = {
        let engine = lock::mutex(&doc.inner.layout);
        engine.last_laid_out.iter().copied().collect::<Vec<_>>()
    };

    for id in stale {
        if let Some(mut data) = doc.inner.nodes.get_mut(&id) {
            data.layout = None;
        }
    }
}

fn collect_visible_tree(
    doc: &Document,
    node_id: NodeId,
    visible: &mut HashSet<NodeId>,
    visible_children: &mut HashMap<NodeId, Vec<NodeId>>,
) {
    let Ok(resolved) = doc.resolved_style(node_id) else {
        return;
    };
    if resolved.display == Display::None {
        return;
    }

    visible.insert(node_id);

    let children = doc
        .get_children(node_id)
        .into_iter()
        .filter(|child| {
            doc.resolved_style(*child)
                .is_ok_and(|resolved| resolved.display != Display::None)
        })
        .collect::<Vec<_>>();

    visible_children.insert(node_id, children.clone());

    for child in children {
        collect_visible_tree(doc, child, visible, visible_children);
    }
}

// ---------------------------------------------------------------------------
// Measure function
// ---------------------------------------------------------------------------

/// Measure function passed to taffy for computing text node sizes.
fn measure_fn(
    _known_dimensions: Size<Option<f32>>,
    _available_space: Size<AvailableSpace>,
    _node_id: taffy::prelude::NodeId,
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
        None => Size::ZERO,
    }
}

// ---------------------------------------------------------------------------
// Style translation
// ---------------------------------------------------------------------------

fn to_taffy_style(resolved: &ResolvedStyle) -> Style {
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

fn round_to_u16(value: f32) -> u16 {
    value.round().clamp(0.0, u16::MAX as f32) as u16
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

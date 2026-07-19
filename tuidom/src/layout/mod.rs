//! Taffy-based flexbox layout.
//!
//! Maintains a persistent 1:1 mapping from DOM nodes to taffy nodes, computes
//! layout through taffy, and stores absolute screen-space layout rectangles back
//! onto DOM nodes.

use std::collections::{HashMap, HashSet};

use taffy::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::event::ScrollEvent;
use crate::id::NodeId;
use crate::lock;
use crate::node::{LayoutRect, NodeKind, NodeLayout, ScrollOffset};
use crate::style::resolution::ResolvedStyle;
// `Position` shadows `taffy::prelude::Position`; taffy's own positioning types are
// referenced through `taffy::style::` below.
use crate::style::{
    AlignContent, AlignItems, Border, Display, EdgeInsets, FlexDirection, FlexGap, FlexWrap,
    JustifyContent, Length, Overflow, Position,
};

// ---------------------------------------------------------------------------
// Persistent layout engine
// ---------------------------------------------------------------------------

/// Document-owned persistent layout engine.
pub(crate) struct LayoutEngine {
    taffy: TaffyTree<MeasureContext>,
    mapping: HashMap<NodeId, taffy::prelude::NodeId>,
    reverse_mapping: HashMap<taffy::prelude::NodeId, NodeId>,
    /// Whether a layout input has changed since the last successful compute.
    ///
    /// Every layout input reaches taffy through one of this type's mutating
    /// methods, so marking there covers every DOM path by construction: a path
    /// that failed to mark would be one that failed to tell taffy, and would
    /// already be broken. Marking is deliberately unconditional rather than
    /// compared against the previous value — a spurious mark costs one
    /// redundant pass, a missed one renders a stale frame.
    dirty: bool,
    /// Screen size at the last compute. It arrives as a `compute` argument
    /// rather than a mutation, so it is the one input `dirty` cannot see.
    last_dimensions: Option<(u16, u16)>,
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
        let mut taffy = TaffyTree::new();
        taffy.enable_rounding();

        Self {
            taffy,
            mapping: HashMap::new(),
            reverse_mapping: HashMap::new(),
            // Nothing has been computed yet, so the first pass must run.
            dirty: true,
            last_dimensions: None,
        }
    }

    /// Whether a layout pass would produce anything the snapshot does not
    /// already hold.
    ///
    /// A clean engine at unchanged dimensions recomputes a bit-identical
    /// snapshot, so the caller can keep the one it has.
    pub fn needs_compute(&self, screen_width: u16, screen_height: u16) -> bool {
        self.dirty || self.last_dimensions != Some((screen_width, screen_height))
    }

    /// Insert the persistent taffy node for a newly allocated DOM node.
    pub fn insert_node(
        &mut self,
        node_id: NodeId,
        kind: &NodeKind,
        resolved: &ResolvedStyle,
    ) -> Result<()> {
        if self.mapping.contains_key(&node_id) {
            return self.update_node(node_id, kind, resolved);
        }

        self.dirty = true;
        let style = to_taffy_style(resolved);
        let taffy_id = match measure_context_for(kind) {
            Some(context) => self.taffy.new_leaf_with_context(style, context)?,
            None => self.taffy.new_leaf(style)?,
        };

        self.mapping.insert(node_id, taffy_id);
        self.reverse_mapping.insert(taffy_id, node_id);
        Ok(())
    }

    /// Remove a DOM node's persistent taffy node.
    pub fn remove_node(&mut self, node_id: NodeId) -> Result<()> {
        let Some(taffy_id) = self.mapping.remove(&node_id) else {
            return Ok(());
        };
        self.dirty = true;
        self.reverse_mapping.remove(&taffy_id);
        self.taffy.remove(taffy_id)?;
        Ok(())
    }

    /// Update style and measurement context for an existing node.
    pub fn update_node(
        &mut self,
        node_id: NodeId,
        kind: &NodeKind,
        resolved: &ResolvedStyle,
    ) -> Result<()> {
        self.set_style(node_id, resolved)?;
        self.set_measure_context(node_id, kind)
    }

    /// Update a node's taffy style.
    pub fn set_style(&mut self, node_id: NodeId, resolved: &ResolvedStyle) -> Result<()> {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return Err(TuidomError::LayoutMappingMissing { id: node_id });
        };

        self.dirty = true;
        self.taffy.set_style(taffy_id, to_taffy_style(resolved))?;
        Ok(())
    }

    /// Update a node's taffy measurement context from DOM data.
    pub fn set_measure_context(&mut self, node_id: NodeId, kind: &NodeKind) -> Result<()> {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return Err(TuidomError::LayoutMappingMissing { id: node_id });
        };

        self.dirty = true;
        self.taffy
            .set_node_context(taffy_id, measure_context_for(kind))?;
        Ok(())
    }

    /// Replace a parent's taffy child list with the DOM child order.
    pub fn sync_children(&mut self, parent: NodeId, children: &[NodeId]) -> Result<()> {
        let Some(&parent_taffy) = self.mapping.get(&parent) else {
            return Err(TuidomError::LayoutMappingMissing { id: parent });
        };

        let taffy_children = children
            .iter()
            .map(|child| {
                self.mapping
                    .get(child)
                    .copied()
                    .ok_or(TuidomError::LayoutMappingMissing { id: *child })
            })
            .collect::<Result<Vec<_>>>()?;

        self.dirty = true;
        self.taffy.set_children(parent_taffy, &taffy_children)?;
        Ok(())
    }

    fn compute(
        &mut self,
        root: NodeId,
        visible_children: &HashMap<NodeId, Vec<NodeId>>,
        screen_width: u16,
        screen_height: u16,
    ) -> Result<Vec<(NodeId, NodeLayout)>> {
        let Some(&taffy_root) = self.mapping.get(&root) else {
            return Err(TuidomError::LayoutMappingMissing { id: root });
        };

        let available = Size {
            width: AvailableSpace::Definite(screen_width as f32),
            height: AvailableSpace::Definite(screen_height as f32),
        };

        self.taffy
            .compute_layout_with_measure(taffy_root, available, measure_fn)?;

        let mut layouts = Vec::new();
        self.collect_absolute_layouts(root, visible_children, 0.0, 0.0, &mut layouts)?;

        // Cleared only here: an error above leaves the engine dirty, so the next
        // frame retries rather than trusting a snapshot that was never produced.
        self.dirty = false;
        self.last_dimensions = Some((screen_width, screen_height));
        Ok(layouts)
    }

    fn collect_absolute_layouts(
        &self,
        node_id: NodeId,
        visible_children: &HashMap<NodeId, Vec<NodeId>>,
        parent_x: f32,
        parent_y: f32,
        out: &mut Vec<(NodeId, NodeLayout)>,
    ) -> Result<()> {
        let Some(&taffy_id) = self.mapping.get(&node_id) else {
            return Err(TuidomError::LayoutMappingMissing { id: node_id });
        };
        let layout = self.taffy.layout(taffy_id)?;

        let absolute_x = parent_x + layout.location.x;
        let absolute_y = parent_y + layout.location.y;
        out.push((
            node_id,
            NodeLayout {
                rect: LayoutRect {
                    x: rounded_taffy_position_to_i32(absolute_x),
                    y: rounded_taffy_position_to_i32(absolute_y),
                    width: rounded_taffy_size_to_u16(layout.size.width),
                    height: rounded_taffy_size_to_u16(layout.size.height),
                },
                // How far content extends beyond the box, from the same taffy pass as the
                // rect. Published unconditionally — whether an axis is *scrollable* is the
                // overflow style's call, made where offsets are clamped.
                max_scroll_x: rounded_taffy_size_to_u16(layout.scroll_width()),
                max_scroll_y: rounded_taffy_size_to_u16(layout.scroll_height()),
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
                )?;
            }
        }

        Ok(())
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
    /// Frame-cycling content: measured on the largest frame, so cycling never
    /// reflows the layout around it.
    Frames { frames: Vec<String> },
}

/// The measurement context a node kind carries into taffy, if any.
fn measure_context_for(kind: &NodeKind) -> Option<MeasureContext> {
    match kind {
        NodeKind::Text { content } => Some(MeasureContext::Text {
            content: content.clone(),
        }),
        NodeKind::Input { state } => Some(MeasureContext::Text {
            content: state.display_content(),
        }),
        NodeKind::Frames { frames, .. } => Some(MeasureContext::Frames {
            frames: frames.clone(),
        }),
        NodeKind::Box => None,
    }
}

// ---------------------------------------------------------------------------
// Public layout entry point
// ---------------------------------------------------------------------------

/// Compute layout for the permanent document root using persistent taffy state.
pub fn compute_layout(doc: &Document, screen_width: u16, screen_height: u16) -> Result<()> {
    // A clean engine at unchanged dimensions would rebuild a bit-identical
    // snapshot, so the whole pass is skipped — including the visible-tree walk,
    // which resolves every node, and the snapshot rebuild, which clears and
    // refills the map. Both are unconditional otherwise, and at a few thousand
    // nodes they are the single largest cost in a frame that changes nothing.
    //
    // The scroll re-clamp below is skipped with them, and safely: `scroll_to`
    // clamps against the snapshot on every call, so the re-clamp exists only to
    // catch a relayout shrinking content out from under a stored offset. No
    // relayout, nothing to catch.
    if !lock::mutex(&doc.inner.layout).needs_compute(screen_width, screen_height) {
        return Ok(());
    }

    let reclamped = {
        let _tree_guard = lock::rw_read(&doc.inner.tree_mutation);
        let root = doc.root();

        let mut visible = HashSet::new();
        let mut visible_children = HashMap::new();
        collect_visible_tree(doc, root, &mut visible, &mut visible_children)?;

        let mut engine = lock::mutex(&doc.inner.layout);
        let layouts = if visible.contains(&root) {
            engine.compute(root, &visible_children, screen_width, screen_height)?
        } else {
            Vec::new()
        };

        let mut layout_snapshot = lock::rw_write(&doc.inner.layout_snapshot);
        layout_snapshot.clear();
        layout_snapshot.extend(layouts);

        // A relayout can shrink content, so stored offsets are re-clamped here — otherwise a
        // stale offset would keep a container scrolled past content that no longer exists.
        // A node absent from the snapshot (hidden subtree) keeps its offset untouched.
        let mut scroll_offsets = lock::mutex(&doc.inner.scroll_offsets);
        let mut reclamped = Vec::new();
        for (id, offset) in scroll_offsets.iter_mut() {
            if let Some(layout) = layout_snapshot.get(id) {
                let clamped = ScrollOffset {
                    x: offset.x.min(layout.max_scroll_x),
                    y: offset.y.min(layout.max_scroll_y),
                };
                if clamped != *offset {
                    *offset = clamped;
                    reclamped.push((*id, clamped));
                }
            }
        }
        reclamped
    };

    // The re-clamp is a scroll the engine made on the container's behalf, so it reports
    // like one. Dispatched with every lock released: a listener may mutate the document.
    for (id, offset) in reclamped {
        let mut event = ScrollEvent::new(offset.x, offset.y);
        doc.dispatch_scroll_to(id, &mut event);
    }
    Ok(())
}

fn collect_visible_tree(
    doc: &Document,
    node_id: NodeId,
    visible: &mut HashSet<NodeId>,
    visible_children: &mut HashMap<NodeId, Vec<NodeId>>,
) -> Result<()> {
    let resolved = doc.resolved_style_unlocked(node_id)?;
    if resolved.display == Display::None {
        return Ok(());
    }

    visible.insert(node_id);

    let mut children = Vec::new();
    for child in doc.get_children_unlocked(node_id) {
        let resolved = doc.resolved_style_unlocked(child)?;
        if resolved.display != Display::None {
            children.push(child);
        }
    }

    visible_children.insert(node_id, children.clone());

    for child in children {
        collect_visible_tree(doc, child, visible, visible_children)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Measure function
// ---------------------------------------------------------------------------

/// Measure function passed to taffy for computing text node sizes.
fn measure_fn(
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    _node_id: taffy::prelude::NodeId,
    context: Option<&mut MeasureContext>,
    _style: &Style,
) -> Size<f32> {
    match context {
        Some(MeasureContext::Text { content }) => measure_natural(
            natural_text_size(content),
            known_dimensions,
            available_space,
        ),
        // The largest frame sizes the node, so a spinner's narrow frames never
        // reflow the content around it as they cycle.
        Some(MeasureContext::Frames { frames }) => {
            let natural = frames.iter().map(|frame| natural_text_size(frame)).fold(
                Size::ZERO,
                |acc, size| Size {
                    width: acc.width.max(size.width),
                    height: acc.height.max(size.height),
                },
            );
            measure_natural(natural, known_dimensions, available_space)
        }
        None => Size::ZERO,
    }
}

fn measure_natural(
    natural: Size<f32>,
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
) -> Size<f32> {
    Size {
        width: resolve_measured_axis(known_dimensions.width, available_space.width, natural.width),
        height: resolve_measured_axis(
            known_dimensions.height,
            available_space.height,
            natural.height,
        ),
    }
}

fn natural_text_size(content: &str) -> Size<f32> {
    let width = content
        .lines()
        .map(|line| UnicodeWidthStr::width(line) as f32)
        .fold(0.0_f32, f32::max);
    let height = content.lines().count() as f32;
    Size { width, height }
}

fn resolve_measured_axis(known: Option<f32>, available: AvailableSpace, natural: f32) -> f32 {
    if let Some(known) = known {
        return known;
    }

    match available {
        AvailableSpace::Definite(limit) => natural.min(limit),
        AvailableSpace::MinContent | AvailableSpace::MaxContent => natural,
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
        margin: to_taffy_margin(resolved.margin),
        padding: to_taffy_padding(resolved.padding),
        border: to_taffy_border(resolved.border),
        flex_direction: to_taffy_flex_direction(resolved.flex_direction),
        flex_wrap: to_taffy_flex_wrap(resolved.flex_wrap),
        flex_basis: to_dimension(resolved.flex_basis),
        flex_grow: resolved.flex_grow,
        flex_shrink: resolved.flex_shrink,
        gap: to_taffy_gap(resolved.gap),
        align_self: resolved.align_self.map(to_align_items),
        align_items: Some(to_align_items(resolved.align_items)),
        align_content: Some(to_align_content(resolved.align_content)),
        justify_content: Some(to_justify_content(resolved.justify_content)),
        position: to_taffy_position(resolved.position),
        inset: to_taffy_inset(resolved.position),
        overflow: taffy::geometry::Point {
            x: to_taffy_overflow(resolved.overflow_x),
            y: to_taffy_overflow(resolved.overflow_y),
        },
        ..Default::default()
    }
}

/// Sizing is the one thing scroll containers need from taffy: `Visible` keeps a node at
/// least large enough to contain its children, while `Scroll`/`Hidden` drop that floor to
/// zero so the container takes its styled/flex size and the content overflows it. The
/// scroll offset itself never reaches taffy — it is applied at paint time.
fn to_taffy_overflow(overflow: Overflow) -> taffy::style::Overflow {
    match overflow {
        Overflow::Visible => taffy::style::Overflow::Visible,
        Overflow::Scroll => taffy::style::Overflow::Scroll,
        Overflow::Clip => taffy::style::Overflow::Hidden,
    }
}

fn to_taffy_position(position: Position) -> taffy::style::Position {
    match position {
        Position::Flow => taffy::style::Position::Relative,
        Position::Absolute { .. } => taffy::style::Position::Absolute,
    }
}

/// Absolute offsets are anchored from the parent's top-left, so only `left`/`top`
/// are set. Leaving `right`/`bottom` auto keeps the node sized by its own style
/// rather than stretched between opposing insets.
fn to_taffy_inset(position: Position) -> Rect<LengthPercentageAuto> {
    match position {
        Position::Flow => Rect::auto(),
        Position::Absolute { x, y } => Rect {
            left: LengthPercentageAuto::length(x as f32),
            top: LengthPercentageAuto::length(y as f32),
            right: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        },
    }
}

fn to_dimension(length: Length) -> Dimension {
    match length {
        Length::Pixels(n) => Dimension::length(n as f32),
        Length::Percent(p) => Dimension::percent(p as f32 / 100.0),
        Length::Auto => Dimension::auto(),
    }
}

fn to_taffy_margin(insets: EdgeInsets) -> Rect<LengthPercentageAuto> {
    Rect {
        left: LengthPercentageAuto::length(insets.left as f32),
        right: LengthPercentageAuto::length(insets.right as f32),
        top: LengthPercentageAuto::length(insets.top as f32),
        bottom: LengthPercentageAuto::length(insets.bottom as f32),
    }
}

fn to_taffy_padding(insets: EdgeInsets) -> Rect<LengthPercentage> {
    Rect {
        left: LengthPercentage::length(insets.left as f32),
        right: LengthPercentage::length(insets.right as f32),
        top: LengthPercentage::length(insets.top as f32),
        bottom: LengthPercentage::length(insets.bottom as f32),
    }
}

/// A border occupies real cells, so layout has to know about it: taffy shrinks the content
/// box by one cell per drawn side, which is what keeps children off the frame.
fn to_taffy_border(border: Border) -> Rect<LengthPercentage> {
    let insets = border.insets();
    Rect {
        left: LengthPercentage::length(insets.left as f32),
        right: LengthPercentage::length(insets.right as f32),
        top: LengthPercentage::length(insets.top as f32),
        bottom: LengthPercentage::length(insets.bottom as f32),
    }
}

fn to_taffy_flex_direction(direction: FlexDirection) -> taffy::style::FlexDirection {
    match direction {
        FlexDirection::Row => taffy::style::FlexDirection::Row,
        FlexDirection::Column => taffy::style::FlexDirection::Column,
        FlexDirection::RowReverse => taffy::style::FlexDirection::RowReverse,
        FlexDirection::ColumnReverse => taffy::style::FlexDirection::ColumnReverse,
    }
}

fn to_taffy_gap(gap: FlexGap) -> Size<LengthPercentage> {
    Size {
        width: LengthPercentage::length(gap.column as f32),
        height: LengthPercentage::length(gap.row as f32),
    }
}

fn to_taffy_flex_wrap(wrap: FlexWrap) -> taffy::style::FlexWrap {
    match wrap {
        FlexWrap::NoWrap => taffy::style::FlexWrap::NoWrap,
        FlexWrap::Wrap => taffy::style::FlexWrap::Wrap,
        FlexWrap::WrapReverse => taffy::style::FlexWrap::WrapReverse,
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

fn to_align_content(a: AlignContent) -> taffy::style::AlignContent {
    match a {
        AlignContent::FlexStart => taffy::style::AlignContent::FLEX_START,
        AlignContent::FlexEnd => taffy::style::AlignContent::FLEX_END,
        AlignContent::Center => taffy::style::AlignContent::CENTER,
        AlignContent::Stretch => taffy::style::AlignContent::STRETCH,
        AlignContent::SpaceBetween => taffy::style::AlignContent::SPACE_BETWEEN,
        AlignContent::SpaceAround => taffy::style::AlignContent::SPACE_AROUND,
    }
}

fn rounded_taffy_position_to_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }

    debug_assert!(
        (value - value.round()).abs() <= 0.001,
        "expected taffy final layout value to already be rounded, got {value}"
    );

    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn rounded_taffy_size_to_u16(value: f32) -> u16 {
    if !value.is_finite() {
        return 0;
    }

    debug_assert!(
        (value - value.round()).abs() <= 0.001,
        "expected taffy final layout value to already be rounded, got {value}"
    );

    value.round().clamp(0.0, u16::MAX as f32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{AlignSelf, BorderCharset, Sides, Style as DomStyle};

    fn fixed_centered_style(width: u16, height: u16) -> DomStyle {
        let mut style = DomStyle::new();
        style.width(Length::Pixels(width));
        style.height(Length::Pixels(height));
        style.justify_content(JustifyContent::Center);
        style.align_items(AlignItems::Center);
        style
    }

    /// A 10×4 screen holding one container with two 8×4 children, so the container's
    /// content is 16 cells wide laid out as a row and 8 cells tall laid out as a column.
    fn overflowing_container(
        root_direction: FlexDirection,
        container_style: DomStyle,
    ) -> (Document, NodeId) {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let mut root_style = DomStyle::new();
        root_style.flex_direction(root_direction);
        doc.set_style(root, &root_style).unwrap();

        let container = doc.create_box().unwrap();
        doc.set_style(container, &container_style).unwrap();
        doc.append_child(root, container).unwrap();

        for _ in 0..2 {
            let child = doc.create_box().unwrap();
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(8));
            child_style.height(Length::Pixels(4));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(container, child).unwrap();
        }

        compute_layout(&doc, 10, 4).unwrap();
        (doc, container)
    }

    #[test]
    fn visible_overflow_keeps_a_container_at_least_content_size() {
        let mut style = DomStyle::new();
        style.flex_direction(FlexDirection::Column);
        let (doc, container) = overflowing_container(FlexDirection::Column, style);

        let layout = doc.get_node(container).unwrap().layout.unwrap().rect;
        assert_eq!(layout.height, 8);
    }

    #[test]
    fn scroll_overflow_lets_a_container_shrink_below_its_content() {
        let mut style = DomStyle::new();
        style.flex_direction(FlexDirection::Column);
        style.overflow_y(Overflow::Scroll);
        let (doc, container) = overflowing_container(FlexDirection::Column, style);

        let layout = doc.get_node(container).unwrap().layout.unwrap().rect;
        assert_eq!(layout.height, 4);
    }

    #[test]
    fn clip_overflow_lets_a_container_shrink_below_its_content() {
        let mut style = DomStyle::new();
        style.flex_direction(FlexDirection::Column);
        style.overflow_y(Overflow::Clip);
        let (doc, container) = overflowing_container(FlexDirection::Column, style);

        let layout = doc.get_node(container).unwrap().layout.unwrap().rect;
        assert_eq!(layout.height, 4);
    }

    #[test]
    fn horizontal_scroll_overflow_shrinks_width() {
        let mut style = DomStyle::new();
        style.overflow_x(Overflow::Scroll);
        let (doc, container) = overflowing_container(FlexDirection::Row, style);

        let layout = doc.get_node(container).unwrap().layout.unwrap().rect;
        assert_eq!(layout.width, 10);
    }

    #[test]
    fn overflow_axes_are_independent() {
        // Scrollable vertically only: the horizontal axis keeps its content-size floor.
        let mut style = DomStyle::new();
        style.overflow_y(Overflow::Scroll);
        let (doc, container) = overflowing_container(FlexDirection::Row, style);

        let layout = doc.get_node(container).unwrap().layout.unwrap().rect;
        assert_eq!(layout.width, 16);
    }

    #[test]
    fn taffy_position_conversion_preserves_negative_values() {
        assert_eq!(rounded_taffy_position_to_i32(-3.0), -3);
        assert_eq!(rounded_taffy_size_to_u16(-3.0), 0);
    }

    #[test]
    fn text_measurement_uses_known_dimensions() {
        let measured = measure_natural(
            natural_text_size("hello"),
            Size {
                width: Some(2.0),
                height: Some(3.0),
            },
            Size {
                width: AvailableSpace::Definite(10.0),
                height: AvailableSpace::Definite(10.0),
            },
        );

        assert_eq!(measured.width, 2.0);
        assert_eq!(measured.height, 3.0);
    }

    #[test]
    fn text_measurement_clips_to_definite_available_space() {
        let measured = measure_natural(
            natural_text_size("hello world\nwide line"),
            Size {
                width: None,
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(5.0),
                height: AvailableSpace::Definite(1.0),
            },
        );

        assert_eq!(measured.width, 5.0);
        assert_eq!(measured.height, 1.0);
    }

    #[test]
    fn fractional_sibling_widths_stay_adjacent_after_taffy_rounding() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();
        let third = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second, third] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Percent(100.0 / 3.0));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 10, 1).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        let third_layout = doc.get_node(third).unwrap().layout.unwrap().rect;

        assert_eq!(first_layout.x, 0);
        assert_eq!(
            second_layout.x,
            first_layout.x + i32::from(first_layout.width)
        );
        assert_eq!(
            third_layout.x,
            second_layout.x + i32::from(second_layout.width)
        );
        assert_eq!(third_layout.x + i32::from(third_layout.width), 10);
    }

    #[test]
    fn padding_offsets_children_inside_content_box() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let child = doc.create_text("A").unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(4));
        root_style.padding(EdgeInsets::new(1, 2, 0, 3));
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();
        doc.append_child(root, child).unwrap();

        compute_layout(&doc, 10, 4).unwrap();

        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!((child_layout.x, child_layout.y), (3, 1));
    }

    #[test]
    fn margin_offsets_siblings_in_flex_layout() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        let mut first_style = DomStyle::new();
        first_style.width(Length::Pixels(2));
        first_style.height(Length::Pixels(1));
        first_style.margin(EdgeInsets::new(0, 1, 0, 0));
        doc.set_style(first, &first_style).unwrap();

        let mut second_style = DomStyle::new();
        second_style.width(Length::Pixels(2));
        second_style.height(Length::Pixels(1));
        doc.set_style(second, &second_style).unwrap();

        doc.append_child(root, first).unwrap();
        doc.append_child(root, second).unwrap();

        compute_layout(&doc, 10, 1).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.x, 0);
        assert_eq!(second_layout.x, 3);
    }

    #[test]
    fn flex_direction_column_stacks_children_vertically() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(4));
        root_style.flex_direction(FlexDirection::Column);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 10, 4).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!((first_layout.x, first_layout.y), (0, 0));
        assert_eq!((second_layout.x, second_layout.y), (0, 1));
    }

    #[test]
    fn row_flex_gap_spaces_children_horizontally() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        root_style.gap(FlexGap::new(0, 2));
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 10, 1).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.x, 0);
        assert_eq!(second_layout.x, 4);
    }

    #[test]
    fn column_flex_gap_spaces_children_vertically() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(2));
        root_style.height(Length::Pixels(5));
        root_style.flex_direction(FlexDirection::Column);
        root_style.gap(FlexGap::new(2, 0));
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 2, 5).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.y, 0);
        assert_eq!(second_layout.y, 3);
    }

    #[test]
    fn flex_grow_distributes_extra_space() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(12));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        for (child, grow) in [(first, 1.0), (second, 2.0)] {
            let mut child_style = DomStyle::new();
            child_style.height(Length::Pixels(1));
            child_style.flex_basis(Length::Pixels(0));
            child_style.flex_grow(grow);
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 12, 1).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.width, 4);
        assert_eq!(second_layout.width, 8);
        assert_eq!(second_layout.x, 4);
    }

    #[test]
    fn flex_shrink_distributes_overflow() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(8));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        for (child, shrink) in [(first, 1.0), (second, 3.0)] {
            let mut child_style = DomStyle::new();
            child_style.height(Length::Pixels(1));
            child_style.flex_basis(Length::Pixels(6));
            child_style.flex_shrink(shrink);
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 8, 1).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.width, 5);
        assert_eq!(second_layout.width, 3);
        assert_eq!(second_layout.x, 5);
    }

    /// Absolute insets resolve against the parent's padding box, so the parent's own
    /// padding does not push an absolute child inward. A border does, because the padding
    /// box starts inside it — see
    /// `absolute_child_of_a_bordered_parent_lands_inside_the_frame`. This parent has no
    /// border, so its padding box origin coincides with its box origin.
    #[test]
    fn absolute_offset_resolves_against_parent_origin_ignoring_parent_padding() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let parent = doc.create_box().unwrap();
        let flush = doc.create_box().unwrap();
        let offset = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(20));
        root_style.height(Length::Pixels(10));
        root_style.padding(EdgeInsets::all(4));
        doc.set_style(root, &root_style).unwrap();

        let mut parent_style = DomStyle::new();
        parent_style.width(Length::Pixels(10));
        parent_style.height(Length::Pixels(5));
        parent_style.padding(EdgeInsets::new(3, 0, 0, 2));
        doc.set_style(parent, &parent_style).unwrap();
        doc.append_child(root, parent).unwrap();

        for (child, position) in [
            (flush, Position::Absolute { x: 0, y: 0 }),
            (offset, Position::Absolute { x: 3, y: 1 }),
        ] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(1));
            child_style.height(Length::Pixels(1));
            child_style.position(position);
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(parent, child).unwrap();
        }

        compute_layout(&doc, 20, 10).unwrap();

        // The parent sits at the root's padding origin, and published rects are screen-absolute.
        let parent_layout = doc.get_node(parent).unwrap().layout.unwrap().rect;
        assert_eq!((parent_layout.x, parent_layout.y), (4, 4));

        let flush_layout = doc.get_node(flush).unwrap().layout.unwrap().rect;
        assert_eq!((flush_layout.x, flush_layout.y), (4, 4));

        let offset_layout = doc.get_node(offset).unwrap().layout.unwrap().rect;
        assert_eq!((offset_layout.x, offset_layout.y), (7, 5));
    }

    #[test]
    fn border_insets_children_by_one_cell_per_drawn_side() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        let mut parent_style = DomStyle::new();
        parent_style.width(Length::Pixels(10));
        parent_style.height(Length::Pixels(6));
        parent_style.border(Border::new(BorderCharset::single()));
        doc.set_style(parent, &parent_style).unwrap();
        doc.append_child(doc.root(), parent).unwrap();

        let mut child_style = DomStyle::new();
        child_style.flex_grow(1.0);
        child_style.align_self(AlignSelf::Stretch);
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent, child).unwrap();

        compute_layout(&doc, 20, 10).unwrap();

        // The parent keeps its full border-box rect; the frame comes out of the child.
        let parent_layout = doc.get_node(parent).unwrap().layout.unwrap().rect;
        assert_eq!(
            (
                parent_layout.x,
                parent_layout.y,
                parent_layout.width,
                parent_layout.height
            ),
            (0, 0, 10, 6)
        );

        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!(
            (
                child_layout.x,
                child_layout.y,
                child_layout.width,
                child_layout.height
            ),
            (1, 1, 8, 4)
        );
    }

    #[test]
    fn a_border_on_one_side_only_insets_that_side() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        let mut parent_style = DomStyle::new();
        parent_style.width(Length::Pixels(10));
        parent_style.height(Length::Pixels(6));
        parent_style.border(
            Border::new(BorderCharset::single()).with_sides(Sides::new(false, false, false, true)),
        );
        doc.set_style(parent, &parent_style).unwrap();
        doc.append_child(doc.root(), parent).unwrap();

        let mut child_style = DomStyle::new();
        child_style.flex_grow(1.0);
        child_style.align_self(AlignSelf::Stretch);
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent, child).unwrap();

        compute_layout(&doc, 20, 10).unwrap();

        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!(
            (
                child_layout.x,
                child_layout.y,
                child_layout.width,
                child_layout.height
            ),
            (1, 0, 9, 6)
        );
    }

    /// The counterpart to `absolute_offset_resolves_against_parent_origin_ignoring_parent_padding`:
    /// absolute insets resolve against the padding box, which a border — unlike padding —
    /// moves. `Absolute { x: 0, y: 0 }` therefore lands just inside the frame instead of on
    /// top of it, which is what stops an absolute child from painting over its parent's border.
    #[test]
    fn absolute_child_of_a_bordered_parent_lands_inside_the_frame() {
        let doc = Document::new().unwrap();

        let parent = doc.create_box().unwrap();
        let flush = doc.create_box().unwrap();

        let mut parent_style = DomStyle::new();
        parent_style.width(Length::Pixels(10));
        parent_style.height(Length::Pixels(6));
        parent_style.padding(EdgeInsets::all(2));
        parent_style.border(Border::new(BorderCharset::single()));
        doc.set_style(parent, &parent_style).unwrap();
        doc.append_child(doc.root(), parent).unwrap();

        let mut flush_style = DomStyle::new();
        flush_style.width(Length::Pixels(1));
        flush_style.height(Length::Pixels(1));
        flush_style.position(Position::Absolute { x: 0, y: 0 });
        doc.set_style(flush, &flush_style).unwrap();
        doc.append_child(parent, flush).unwrap();

        compute_layout(&doc, 20, 10).unwrap();

        // Inset by the border, but not by the parent's padding.
        let flush_layout = doc.get_node(flush).unwrap().layout.unwrap().rect;
        assert_eq!((flush_layout.x, flush_layout.y), (1, 1));
    }

    #[test]
    fn absolute_node_is_removed_from_sibling_flow() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let absolute = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(2));
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, absolute, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            if child == absolute {
                child_style.position(Position::Absolute { x: 7, y: 1 });
            }
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 10, 2).unwrap();

        // `second` takes the slot the absolute node would have occupied in flow.
        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!((first_layout.x, first_layout.y), (0, 0));
        assert_eq!((second_layout.x, second_layout.y), (2, 0));

        let absolute_layout = doc.get_node(absolute).unwrap().layout.unwrap().rect;
        assert_eq!((absolute_layout.x, absolute_layout.y), (7, 1));
    }

    #[test]
    fn children_of_absolute_node_lay_out_relative_to_it() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let absolute = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(20));
        root_style.height(Length::Pixels(10));
        doc.set_style(root, &root_style).unwrap();

        let mut absolute_style = DomStyle::new();
        absolute_style.width(Length::Pixels(8));
        absolute_style.height(Length::Pixels(4));
        absolute_style.padding(EdgeInsets::all(1));
        absolute_style.position(Position::Absolute { x: 5, y: 2 });
        doc.set_style(absolute, &absolute_style).unwrap();
        doc.append_child(root, absolute).unwrap();

        let mut child_style = DomStyle::new();
        child_style.width(Length::Pixels(2));
        child_style.height(Length::Pixels(1));
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(absolute, child).unwrap();

        compute_layout(&doc, 20, 10).unwrap();

        // Flow child of an absolute node still honors that node's padding.
        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!((child_layout.x, child_layout.y), (6, 3));
    }

    #[test]
    fn absolute_node_may_overflow_parent_bounds_with_negative_offsets() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(20));
        root_style.height(Length::Pixels(10));
        root_style.padding(EdgeInsets::all(2));
        doc.set_style(root, &root_style).unwrap();

        let mut parent_style = DomStyle::new();
        parent_style.width(Length::Pixels(4));
        parent_style.height(Length::Pixels(2));
        doc.set_style(parent, &parent_style).unwrap();
        doc.append_child(root, parent).unwrap();

        let mut child_style = DomStyle::new();
        child_style.width(Length::Pixels(1));
        child_style.height(Length::Pixels(1));
        child_style.position(Position::Absolute { x: -3, y: -3 });
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(parent, child).unwrap();

        compute_layout(&doc, 20, 10).unwrap();

        // Escapes the parent box entirely; paint clips to the screen, not to the parent.
        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!((child_layout.x, child_layout.y), (-1, -1));
    }

    #[test]
    fn absolute_node_honors_display_none() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let absolute = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(5));
        doc.set_style(root, &root_style).unwrap();

        let mut absolute_style = DomStyle::new();
        absolute_style.width(Length::Pixels(2));
        absolute_style.height(Length::Pixels(1));
        absolute_style.position(Position::Absolute { x: 3, y: 3 });
        absolute_style.display(Display::None);
        doc.set_style(absolute, &absolute_style).unwrap();
        doc.append_child(root, absolute).unwrap();

        compute_layout(&doc, 10, 5).unwrap();

        assert!(doc.get_node(absolute).unwrap().layout.is_none());
    }

    #[test]
    fn row_reverse_lays_children_out_from_the_end_of_the_main_axis() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        root_style.flex_direction(FlexDirection::RowReverse);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 10, 1).unwrap();

        // First child packs against the right edge; DOM order runs right to left.
        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.x, 8);
        assert_eq!(second_layout.x, 6);
    }

    #[test]
    fn column_reverse_lays_children_out_from_the_bottom() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(4));
        root_style.height(Length::Pixels(6));
        root_style.flex_direction(FlexDirection::ColumnReverse);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(2));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 4, 6).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        assert_eq!(first_layout.y, 4);
        assert_eq!(second_layout.y, 2);
    }

    #[test]
    fn wrap_reverse_stacks_lines_in_reverse_cross_axis_order() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();
        let third = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(2));
        root_style.flex_wrap(FlexWrap::WrapReverse);
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second, third] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 5, 2).unwrap();

        // Same wrapping as `Wrap`, but the first line sits at the bottom.
        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        let third_layout = doc.get_node(third).unwrap().layout.unwrap().rect;
        assert_eq!((first_layout.x, first_layout.y), (0, 1));
        assert_eq!((second_layout.x, second_layout.y), (2, 1));
        assert_eq!((third_layout.x, third_layout.y), (0, 0));
    }

    #[test]
    fn flex_basis_sets_initial_main_axis_size() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let child = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        let mut child_style = DomStyle::new();
        child_style.height(Length::Pixels(1));
        child_style.flex_basis(Length::Pixels(4));
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(root, child).unwrap();

        compute_layout(&doc, 10, 1).unwrap();

        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!(child_layout.width, 4);
    }

    #[test]
    fn flex_wrap_moves_overflowing_children_to_next_line() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();
        let third = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(2));
        root_style.flex_wrap(FlexWrap::Wrap);
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second, third] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 5, 2).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        let third_layout = doc.get_node(third).unwrap().layout.unwrap().rect;
        assert_eq!((first_layout.x, first_layout.y), (0, 0));
        assert_eq!((second_layout.x, second_layout.y), (2, 0));
        assert_eq!((third_layout.x, third_layout.y), (0, 1));
    }

    #[test]
    fn align_content_places_wrapped_lines_in_cross_axis() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();
        let third = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(6));
        root_style.flex_wrap(FlexWrap::Wrap);
        root_style.align_content(AlignContent::Center);
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();

        for child in [first, second, third] {
            let mut child_style = DomStyle::new();
            child_style.width(Length::Pixels(2));
            child_style.height(Length::Pixels(1));
            doc.set_style(child, &child_style).unwrap();
            doc.append_child(root, child).unwrap();
        }

        compute_layout(&doc, 5, 6).unwrap();

        let first_layout = doc.get_node(first).unwrap().layout.unwrap().rect;
        let second_layout = doc.get_node(second).unwrap().layout.unwrap().rect;
        let third_layout = doc.get_node(third).unwrap().layout.unwrap().rect;
        assert_eq!((first_layout.x, first_layout.y), (0, 2));
        assert_eq!((second_layout.x, second_layout.y), (2, 2));
        assert_eq!((third_layout.x, third_layout.y), (0, 3));
    }

    #[test]
    fn align_self_overrides_parent_cross_axis_alignment() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let child = doc.create_box().unwrap();

        let mut root_style = DomStyle::new();
        root_style.width(Length::Pixels(10));
        root_style.height(Length::Pixels(5));
        root_style.align_items(AlignItems::FlexStart);
        doc.set_style(root, &root_style).unwrap();

        let mut child_style = DomStyle::new();
        child_style.width(Length::Pixels(2));
        child_style.height(Length::Pixels(1));
        child_style.align_self(AlignSelf::Center);
        doc.set_style(child, &child_style).unwrap();
        doc.append_child(root, child).unwrap();

        compute_layout(&doc, 10, 5).unwrap();

        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;
        assert_eq!((child_layout.x, child_layout.y), (0, 2));
    }

    #[test]
    fn nested_layout_positions_are_stored_as_absolute_screen_coordinates() {
        let doc = Document::new().unwrap();

        let root = doc.root();
        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

        doc.set_style(root, &fixed_centered_style(100, 40)).unwrap();
        doc.set_style(parent, &fixed_centered_style(50, 20))
            .unwrap();

        let mut child_style = DomStyle::new();
        child_style.width(Length::Pixels(10));
        child_style.height(Length::Pixels(5));
        doc.set_style(child, &child_style).unwrap();

        doc.append_child(root, parent).unwrap();
        doc.append_child(parent, child).unwrap();

        compute_layout(&doc, 100, 40).unwrap();

        let root_layout = doc.get_node(root).unwrap().layout.unwrap().rect;
        let parent_layout = doc.get_node(parent).unwrap().layout.unwrap().rect;
        let child_layout = doc.get_node(child).unwrap().layout.unwrap().rect;

        assert_eq!((root_layout.x, root_layout.y), (0, 0));
        assert_eq!((parent_layout.x, parent_layout.y), (25, 10));
        assert_eq!((child_layout.x, child_layout.y), (45, 18));
    }
}

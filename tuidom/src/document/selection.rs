//! Document-level text selection: model, runtime state, and screen-cell mapping.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::document::Document;
use crate::document::input::clamp_to_grapheme_boundary;
use crate::event::{KeyCode, KeyModifiers, SelectionChangeEvent};
use crate::id::NodeId;
use crate::lock;
use crate::node::{NodeKind, NodeKindView};
use crate::paint_order::{PaintEntry, paint_order};

/// A position in a document selection: a Text node plus a byte offset into its content.
///
/// Offsets always lie on grapheme boundaries. A point is content-addressed rather than
/// screen-addressed, so scrolling never moves or invalidates it — rendering re-maps it
/// through the current layout each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectionPoint {
    /// The Text node the point lies in.
    pub node: NodeId,
    /// Byte offset into the node's content, on a grapheme boundary.
    pub offset: usize,
}

/// The document's raw selection state: the drag geometry as the user made it.
///
/// `anchor` and `focus` are kept unordered so a drag can extend in either direction;
/// consumers see the pair normalized to document order with the end extended to cover
/// the glyph under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionState {
    /// The boundary container the selection is confined to.
    pub boundary: NodeId,
    /// Where the drag started.
    pub anchor: SelectionPoint,
    /// Where the drag currently ends.
    pub focus: SelectionPoint,
}

/// An armed selection drag: the boundary and anchor resolved at mouse down, waiting for
/// drag movement to produce a visible selection.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingSelection {
    /// A drag over document text, confined to a boundary subtree.
    Document {
        /// The boundary the drag is confined to.
        boundary: NodeId,
        /// The snapped press position.
        anchor: SelectionPoint,
    },
    /// A drag inside an Input. The Input is an implicit boundary: the drag drives the
    /// input's own selection, and document selection never crosses its edge.
    Input {
        /// The Input node being dragged in.
        node: NodeId,
        /// The value byte offset the press landed on.
        anchor: usize,
    },
}

/// What a drag starting on a node is bounded by.
enum DragBoundary {
    /// A boundary subtree for document selection.
    Document(NodeId),
    /// An Input, selecting within its own value.
    Input(NodeId),
}

impl Document {
    /// The current selection as a document-ordered `(start, end)` pair.
    ///
    /// `start` is the earlier point in DOM order regardless of drag direction, and
    /// `end` is extended to the end of the grapheme under it, so both endpoint cells
    /// of a drag are included — the way terminals select. Returns `None` when nothing
    /// is selected.
    pub fn selection(&self) -> Option<(SelectionPoint, SelectionPoint)> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.selection_unlocked()
    }

    /// [`selection`](Self::selection) for callers already holding the tree guard.
    ///
    /// Ordering two points walks both their root paths, so the guard belongs around the
    /// whole comparison rather than around each step of it — which is what taking it per
    /// `get_parent` amounted to before.
    pub(crate) fn selection_unlocked(&self) -> Option<(SelectionPoint, SelectionPoint)> {
        let state = (*lock::mutex(&self.inner.selection))?;
        self.ordered_selection(&state)
    }

    /// Clear the document selection, if any.
    pub fn clear_selection(&self) {
        self.set_selection_state(None);
    }

    /// The selected text in reading order, or `None` when nothing is selected.
    ///
    /// Reading order is document order: each selected Text node contributes its
    /// selected slice, and a newline separates two consecutive slices whose glyphs
    /// do not share a screen row. Slices on one row concatenate directly, so text
    /// split across sibling nodes on a line copies as one line.
    pub fn get_selection(&self) -> Option<String> {
        let ranges = self.selection_ranges();
        if ranges.is_empty() {
            return None;
        }

        let mut out = String::new();
        let mut previous_end_row: Option<i32> = None;
        for (node, range) in ranges {
            let Some((slice, start_line, end_line)) = self.selected_slice(node, &range) else {
                continue;
            };
            let base_row = self.content_base_row(node);
            let start_row = base_row.map(|base| base + start_line);
            let end_row = base_row.map(|base| base + end_line);

            // Without layout for either side, the row test is unanswerable; a newline
            // is the reading-order-safe separator.
            match (previous_end_row, start_row) {
                (Some(previous), Some(start)) if previous == start => {}
                (None, _) => {}
                _ => out.push('\n'),
            }
            out.push_str(&slice);
            previous_end_row = end_row;
        }
        Some(out)
    }

    /// A selected slice plus the line indices (within the node's content) of its
    /// first and last character.
    fn selected_slice(&self, node: NodeId, range: &Range<usize>) -> Option<(String, i32, i32)> {
        let data = self.inner.nodes.get(&node)?;
        let NodeKind::Text { content } = &data.kind else {
            return None;
        };
        let slice = content.get(range.clone())?.to_owned();
        let start_line = content[..range.start].matches('\n').count() as i32;
        let end_line = content[..range.end].matches('\n').count() as i32;
        Some((slice, start_line, end_line))
    }

    /// The screen row of a node's first content line, per the layout snapshot.
    fn content_base_row(&self, node: NodeId) -> Option<i32> {
        let rect = lock::rw_read(&self.inner.layout_snapshot)
            .get(&node)
            .map(|layout| layout.rect)?;
        let resolved = self.resolved_style(node).ok()?;
        Some(rect.content_rect(&resolved).y)
    }

    /// Arm a selection drag from a left mouse down.
    ///
    /// Resolves the boundary from the hit node and snaps the press position to the
    /// nearest character within it. A press inside an Input arms an input drag
    /// instead and positions the input cursor — click-to-position — since the press
    /// is the collapsed start of that drag. Returns `None` when the boundary contains
    /// no selectable text, in which case the drag selects nothing.
    pub(crate) fn begin_selection_drag(
        &self,
        x: i32,
        y: i32,
        hit: NodeId,
    ) -> Option<PendingSelection> {
        match self.drag_boundary_of(hit) {
            DragBoundary::Input(node) => {
                let anchor = self.input_offset_at(node, x, y)?;
                if let Err(err) = self.set_input_cursor(node, anchor) {
                    tracing::error!("input click positioning failed: {err}");
                }
                Some(PendingSelection::Input { node, anchor })
            }
            DragBoundary::Document(boundary) => {
                let anchor = self.selection_point_at(x, y, boundary)?;
                Some(PendingSelection::Document { boundary, anchor })
            }
        }
    }

    /// Extend an armed selection drag to the current pointer position.
    pub(crate) fn update_selection_drag(&self, pending: &PendingSelection, x: i32, y: i32) {
        match pending {
            PendingSelection::Document { boundary, anchor } => {
                let Some(focus) = self.selection_point_at(x, y, *boundary) else {
                    return;
                };
                self.set_selection_state(Some(SelectionState {
                    boundary: *boundary,
                    anchor: *anchor,
                    focus,
                }));
            }
            PendingSelection::Input { node, anchor } => {
                let Some(focus) = self.input_offset_at(*node, x, y) else {
                    return;
                };
                if let Err(err) = self.drive_input_drag(*node, *anchor, focus) {
                    tracing::error!("input drag selection failed: {err}");
                }
            }
        }
    }

    pub(crate) fn set_selection_state(&self, state: Option<SelectionState>) {
        let changed = {
            let mut selection = lock::mutex(&self.inner.selection);
            let changed = *selection != state;
            *selection = state;
            changed
        };
        if changed {
            self.inner.notify.notify_one();
            let selection = state.and_then(|state| self.ordered_selection(&state));
            self.dispatch_selection_change(SelectionChangeEvent { selection });
        }
    }

    /// Clear the selection if any part of it no longer exists in the tree.
    ///
    /// Called after a tree mutation has released the tree lock — like focus settling —
    /// so the change event dispatches with no internal lock held and handlers may
    /// touch the tree freely.
    pub(crate) fn settle_selection(&self) {
        let dead = lock::mutex(&self.inner.selection).is_some_and(|state| {
            !self.inner.nodes.contains_key(&state.boundary)
                || !self.inner.nodes.contains_key(&state.anchor.node)
                || !self.inner.nodes.contains_key(&state.focus.node)
        });
        if dead {
            self.set_selection_state(None);
        }
    }

    /// Re-clamp selection offsets into `node` after its text content changed.
    ///
    /// Offsets snap to grapheme boundaries of the new content. A selection whose
    /// visible range collapses to nothing is cleared rather than kept as an invisible
    /// pair of equal points.
    pub(crate) fn clamp_selection_to_text(&self, node: NodeId) {
        let Some(mut state) = *lock::mutex(&self.inner.selection) else {
            return;
        };
        if state.anchor.node != node && state.focus.node != node {
            return;
        }

        let Some(content) = self.text_content_of(node) else {
            self.set_selection_state(None);
            return;
        };
        if state.anchor.node == node {
            state.anchor.offset = clamp_to_grapheme_boundary(&content, state.anchor.offset);
        }
        if state.focus.node == node {
            state.focus.offset = clamp_to_grapheme_boundary(&content, state.focus.offset);
        }

        let collapsed = self
            .ordered_selection(&state)
            .is_none_or(|(start, end)| start == end);
        self.set_selection_state((!collapsed).then_some(state));
    }

    fn text_content_of(&self, node: NodeId) -> Option<String> {
        let data = self.inner.nodes.get(&node)?;
        match &data.kind {
            NodeKind::Text { content } => Some(content.clone()),
            _ => None,
        }
    }

    /// The boundary a drag starting on `hit` is confined to: the nearest
    /// ancestor-or-self that is an Input or is marked `selection_boundary`, or the root.
    fn drag_boundary_of(&self, hit: NodeId) -> DragBoundary {
        let mut current = Some(hit);
        while let Some(node) = current {
            if self
                .inner
                .nodes
                .get(&node)
                .is_some_and(|data| matches!(data.kind, NodeKind::Input { .. }))
            {
                return DragBoundary::Input(node);
            }
            if self
                .resolved_style(node)
                .is_ok_and(|style| style.selection_boundary)
            {
                return DragBoundary::Document(node);
            }
            current = self.get_parent(node);
        }
        DragBoundary::Document(self.root())
    }

    /// Extend an existing document selection from the keyboard.
    ///
    /// Extend-only by design: with nothing selected there is no anchor to grow from, and
    /// seeding one would need a document caret — a collapsed selection is unrepresentable
    /// here, since [`Document::selection`] runs every pair through the same end-extension
    /// that makes a drag's last cell inclusive.
    ///
    /// Returns whether the key was claimed, so an unclaimed one still reaches focus
    /// navigation and scrolling.
    pub(crate) fn apply_selection_default_action(
        &self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> bool {
        // Shift is what distinguishes extension from navigation. Control and alt chords
        // are unbound rather than widened to their plain form.
        if !modifiers.contains(KeyModifiers::SHIFT)
            || modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        // Copied out, so no selection lock is held while the change below dispatches to
        // handlers that may read or clear the selection themselves.
        let Some(state) = *lock::mutex(&self.inner.selection) else {
            return false;
        };

        let focus = match code {
            KeyCode::Left => self.horizontal_focus(&state, false),
            KeyCode::Right => self.horizontal_focus(&state, true),
            KeyCode::Up => self.vertical_focus(&state, -1),
            KeyCode::Down => self.vertical_focus(&state, 1),
            KeyCode::PageUp => self.vertical_focus(&state, -self.selection_page_rows()),
            KeyCode::PageDown => self.vertical_focus(&state, self.selection_page_rows()),
            _ => return false,
        };

        // Claimed but with nowhere to go: the selection is at the edge of its boundary.
        // Still handled, so the key does not fall through to moving focus instead.
        if let Some(focus) = focus {
            self.set_selection_state(Some(SelectionState { focus, ..state }));
        }
        true
    }

    /// The focus point one grapheme along, crossing Text nodes at their edges.
    fn horizontal_focus(&self, state: &SelectionState, forward: bool) -> Option<SelectionPoint> {
        let content = self.text_content_of(state.focus.node)?;
        let offset = clamp_to_grapheme_boundary(&content, state.focus.offset);

        // `None` here means the offset is already at the node's edge, which is what sends
        // the walk into the neighbouring node below.
        let within = if forward {
            content[offset..]
                .graphemes(true)
                .next()
                .map(|grapheme| offset + grapheme.len())
        } else {
            content[..offset]
                .grapheme_indices(true)
                .next_back()
                .map(|(index, _)| index)
        };
        if let Some(offset) = within {
            return Some(SelectionPoint {
                node: state.focus.node,
                offset,
            });
        }

        // At the node's edge, so the next position lives in the neighbouring Text node —
        // a selection is a range in the document, not in one node.
        let nodes = self.selectable_text_nodes(state.boundary);
        let index = nodes.iter().position(|node| *node == state.focus.node)?;
        let neighbour = if forward {
            *nodes.get(index + 1)?
        } else {
            *nodes.get(index.checked_sub(1)?)?
        };
        let neighbour_content = self.text_content_of(neighbour)?;
        Some(SelectionPoint {
            node: neighbour,
            offset: if forward { 0 } else { neighbour_content.len() },
        })
    }

    /// The focus point `rows` screen rows away, re-snapped through the same mapping a
    /// drag uses.
    ///
    /// Screen-based, so it reaches only text that is currently painted: a row beyond the
    /// last visible one snaps back to the nearest painted line rather than scrolling to
    /// reveal more.
    fn vertical_focus(&self, state: &SelectionState, rows: i32) -> Option<SelectionPoint> {
        let (x, y) = self.selection_point_cell(state.focus)?;
        self.selection_point_at(x, y.saturating_add(rows), state.boundary)
    }

    /// How many rows one page of selection extension covers.
    ///
    /// The document's own height, less a row of overlap — a selection spans the screen
    /// rather than any one container, so no container's scrollport is the right measure.
    fn selection_page_rows(&self) -> i32 {
        self.get_node(self.root())
            .and_then(|view| view.layout)
            .map(|layout| i32::from(layout.rect.height.saturating_sub(1)).max(1))
            .unwrap_or(1)
    }

    /// The screen cell a selection point sits on, per the current layout.
    ///
    /// The inverse of [`Self::selection_point_at`], and the reason vertical extension can
    /// reuse that snapping instead of needing its own geometry.
    fn selection_point_cell(&self, point: SelectionPoint) -> Option<(i32, i32)> {
        let entries = paint_order(self);
        let entry = entries
            .iter()
            .find(|entry| entry.id == point.node && entry.scrollbar.is_none())?;
        let NodeKindView::Text { content } = &entry.kind else {
            return None;
        };

        let rect = entry.layout.content_rect(&entry.resolved);
        let prefix = content.get(..point.offset.min(content.len()))?;
        let line = prefix.matches('\n').count() as i32;
        let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
        let column: i32 = prefix[line_start..]
            .graphemes(true)
            .map(|grapheme| unicode_width::UnicodeWidthStr::width(grapheme).min(2) as i32)
            .sum();
        Some((rect.x + column, rect.y + line))
    }

    /// Text nodes in `boundary`'s subtree in document order, skipping the ones a drag
    /// would also skip: empty content, and anything inert or disabled.
    fn selectable_text_nodes(&self, boundary: NodeId) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        let mut stack = vec![boundary];
        while let Some(node) = stack.pop() {
            if self
                .text_content_of(node)
                .is_some_and(|content| !content.is_empty())
                && !self.blocks_interaction(node)
            {
                nodes.push(node);
            }
            let children = self.get_children(node);
            stack.extend(children.iter().rev());
        }
        nodes
    }

    /// Map a screen cell to the nearest character position within `boundary`.
    ///
    /// Direct glyph hits win outright. Anything else snaps: nearest painted line by row
    /// distance, then nearest column on it, with cells past a line's last glyph mapping
    /// to the line's end offset. Topmost paint order wins ties, so the position the
    /// user can actually see is the one selected.
    fn selection_point_at(&self, x: i32, y: i32, boundary: NodeId) -> Option<SelectionPoint> {
        let subtree = self.subtree_of(boundary);
        let mut best: Option<(u32, u32, SelectionPoint)> = None;

        for entry in paint_order(self).iter().rev() {
            if entry.scrollbar.is_some() {
                continue;
            }
            let NodeKindView::Text { content } = &entry.kind else {
                continue;
            };
            if content.is_empty() || !subtree.contains(&entry.id) {
                continue;
            }
            // Inert and disabled subtrees swallow interaction, selection included.
            if self.blocks_interaction(entry.id) {
                continue;
            }
            let Some((distance, offset)) = text_candidate(entry, content, x, y) else {
                continue;
            };
            let point = SelectionPoint {
                node: entry.id,
                offset,
            };
            if best.is_none_or(|(dy, dx, _)| distance < (dy, dx)) {
                best = Some((distance.0, distance.1, point));
            }
        }

        best.map(|(_, _, point)| point)
    }

    /// Map a screen cell to a byte offset in an Input's value.
    ///
    /// Works in display space — masked glyphs, single-line newline flattening, and the
    /// input's own scroll offsets — then converts back to a value offset through the
    /// 1:1 grapheme correspondence between value and display content. Positions
    /// outside the input clamp to its nearest visible line and column.
    fn input_offset_at(&self, node: NodeId, x: i32, y: i32) -> Option<usize> {
        let (value, multiline, mask, scroll_x, scroll_y) = {
            let data = self.inner.nodes.get(&node)?;
            let NodeKind::Input { state } = &data.kind else {
                return None;
            };
            (
                state.content.clone(),
                state.multiline,
                state.mask,
                state.scroll_x,
                state.scroll_y,
            )
        };

        let rect = lock::rw_read(&self.inner.layout_snapshot)
            .get(&node)
            .map(|layout| layout.rect)?
            .content_rect(&self.resolved_style(node).ok()?);

        let display = crate::node::input_display_content(&value, multiline, mask);
        let lines: Vec<&str> = display.split('\n').collect();
        let line_index =
            ((y - rect.y).max(0) as usize + scroll_y as usize).min(lines.len().saturating_sub(1));
        let line = lines[line_index];
        let line_start: usize = lines[..line_index].iter().map(|line| line.len() + 1).sum();

        let target_col = i32::from(scroll_x) + (x - rect.x).max(0);
        let mut col = 0i32;
        let mut offset_in_line = line.len();
        for (offset, grapheme) in line.grapheme_indices(true) {
            let width = unicode_width::UnicodeWidthStr::width(grapheme).min(2) as i32;
            if width == 0 {
                continue;
            }
            if target_col < col + width {
                offset_in_line = offset;
                break;
            }
            col += width;
        }

        Some(display_to_value_offset(
            &value,
            &display,
            line_start + offset_in_line,
        ))
    }

    /// All nodes in `boundary`'s subtree, boundary included.
    fn subtree_of(&self, boundary: NodeId) -> HashSet<NodeId> {
        let mut subtree = HashSet::new();
        let mut stack = vec![boundary];
        while let Some(node) = stack.pop() {
            if subtree.insert(node) {
                stack.extend(self.get_children(node));
            }
        }
        subtree
    }

    /// The selected byte range of every Text node in the selection, in document order.
    ///
    /// A preorder walk of the boundary subtree from the start point to the end point:
    /// the endpoints contribute partial ranges, everything between contributes its full
    /// content — including text the pointer never crossed, since a selection is a range
    /// in the document, not a screen region. Empty when nothing is selected.
    pub(crate) fn selection_ranges(&self) -> Vec<(NodeId, Range<usize>)> {
        let Some(state) = *lock::mutex(&self.inner.selection) else {
            return Vec::new();
        };
        let Some((start, end)) = self.ordered_selection(&state) else {
            return Vec::new();
        };

        let mut ranges = Vec::new();
        let mut in_range = false;
        let mut stack = vec![state.boundary];
        while let Some(node) = stack.pop() {
            if node == start.node {
                in_range = true;
            }
            if in_range && let Some(len) = self.text_content_len(node) {
                let from = if node == start.node { start.offset } else { 0 };
                let to = if node == end.node {
                    end.offset.min(len)
                } else {
                    len
                };
                if from < to {
                    ranges.push((node, from..to));
                }
            }
            if node == end.node {
                break;
            }
            let children = self.get_children(node);
            stack.extend(children.iter().rev());
        }
        ranges
    }

    fn text_content_len(&self, node: NodeId) -> Option<usize> {
        let data = self.inner.nodes.get(&node)?;
        match &data.kind {
            NodeKind::Text { content } => Some(content.len()),
            _ => None,
        }
    }

    /// Normalize raw drag geometry to a document-ordered, end-extended pair.
    fn ordered_selection(
        &self,
        state: &SelectionState,
    ) -> Option<(SelectionPoint, SelectionPoint)> {
        let (start, end) = match self.compare_points(state.anchor, state.focus)? {
            Ordering::Greater => (state.focus, state.anchor),
            _ => (state.anchor, state.focus),
        };
        Some((start, self.extend_to_grapheme_end(end)))
    }

    /// Compare two selection points in document order.
    ///
    /// Returns `None` when either node no longer exists in the tree.
    fn compare_points(&self, a: SelectionPoint, b: SelectionPoint) -> Option<Ordering> {
        if a.node == b.node {
            if !self.inner.nodes.contains_key(&a.node) {
                return None;
            }
            return Some(a.offset.cmp(&b.offset));
        }

        let path_a = self.path_from_root(a.node)?;
        let path_b = self.path_from_root(b.node)?;

        // Walk the two root paths to the first divergence; sibling order there decides.
        // Two Text nodes cannot be ancestor and descendant, so a divergence always exists.
        for (step_a, step_b) in path_a.iter().zip(path_b.iter()) {
            if step_a == step_b {
                continue;
            }
            let parent = self.get_parent_unlocked(*step_a)?;
            let children = self.get_children_unlocked(parent);
            let index_a = children.iter().position(|child| child == step_a)?;
            let index_b = children.iter().position(|child| child == step_b)?;
            return Some(index_a.cmp(&index_b));
        }
        None
    }

    fn path_from_root(&self, node: NodeId) -> Option<Vec<NodeId>> {
        if !self.inner.nodes.contains_key(&node) {
            return None;
        }
        let mut path = vec![node];
        let mut current = node;
        while let Some(parent) = self.get_parent_unlocked(current) {
            path.push(parent);
            current = parent;
        }
        path.reverse();
        Some(path)
    }

    /// Extend a document-order end point past the glyph under it, so the endpoint cell
    /// is included in the range. A point at content end or on a line break is left
    /// unchanged — it already means "up to here".
    fn extend_to_grapheme_end(&self, end: SelectionPoint) -> SelectionPoint {
        let Some(node) = self.inner.nodes.get(&end.node) else {
            return end;
        };
        let NodeKind::Text { content } = &node.kind else {
            return end;
        };
        let Some(grapheme) = content[end.offset.min(content.len())..]
            .graphemes(true)
            .next()
        else {
            return end;
        };
        if grapheme == "\n" {
            return end;
        }
        SelectionPoint {
            node: end.node,
            offset: end.offset + grapheme.len(),
        }
    }
}

/// Convert an Input display-content byte offset to a value byte offset.
///
/// Display content is grapheme-for-grapheme with the value — masking replaces each
/// grapheme with one mask character, single-line mode replaces newlines with spaces —
/// so the offset converts through the grapheme index.
pub(crate) fn display_to_value_offset(value: &str, display: &str, offset: usize) -> usize {
    let index = display
        .get(..offset.min(display.len()))
        .map_or(0, |prefix| prefix.graphemes(true).count());
    value
        .grapheme_indices(true)
        .nth(index)
        .map_or(value.len(), |(offset, _)| offset)
}

/// Convert an Input value byte offset to a display-content byte offset.
pub(crate) fn value_to_display_offset(value: &str, display: &str, offset: usize) -> usize {
    let index = value
        .get(..offset.min(value.len()))
        .map_or(0, |prefix| prefix.graphemes(true).count());
    display
        .grapheme_indices(true)
        .nth(index)
        .map_or(display.len(), |(offset, _)| offset)
}

/// The best character position this text node offers for a pointer cell, with its
/// `(row, column)` cell distance. A direct glyph hit has distance `(0, 0)`.
fn text_candidate(
    entry: &PaintEntry,
    content: &str,
    x: i32,
    y: i32,
) -> Option<((u32, u32), usize)> {
    let rect = entry.layout.content_rect(&entry.resolved);
    if rect.width == 0 || rect.height == 0 {
        return None;
    }

    let mut best: Option<((u32, u32), usize)> = None;
    let mut line_start = 0usize;
    // Mirror painting: one line per row, truncated to the content rect's height.
    for (index, line) in content.split('\n').take(rect.height as usize).enumerate() {
        let row = rect.y + index as i32;
        let dy = y.abs_diff(row);
        let (dx, offset_in_line) = line_candidate(line, rect.x, x);
        let candidate = ((dy, dx), line_start + offset_in_line);
        if best.is_none_or(|(distance, _)| candidate.0 < distance) {
            best = Some(candidate);
        }
        line_start += line.len() + 1;
    }
    best
}

/// The best position on one painted line: the glyph under `x`, or the nearer of the
/// line's two ends. Only a glyph directly under the pointer scores a zero column
/// distance, so real hits always beat snapped ones.
fn line_candidate(line: &str, start_x: i32, x: i32) -> (u32, usize) {
    if x < start_x {
        return (start_x.abs_diff(x), 0);
    }

    let mut col = start_x;
    for (offset, grapheme) in line.grapheme_indices(true) {
        let width = unicode_width::UnicodeWidthStr::width(grapheme).min(2) as i32;
        if width == 0 {
            continue;
        }
        if x < col + width {
            return (0, offset);
        }
        col += width;
    }

    // Past the last glyph: snap to the line's end offset, one column of distance per
    // cell beyond the first empty one.
    (x.abs_diff(col) + 1, line.len())
}

//! Document-level text selection: model, runtime state, and screen-cell mapping.

use std::cmp::Ordering;
use std::collections::HashSet;

use unicode_segmentation::UnicodeSegmentation;

use crate::document::Document;
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
pub(crate) struct PendingSelection {
    boundary: NodeId,
    anchor: SelectionPoint,
}

impl Document {
    /// The current selection as a document-ordered `(start, end)` pair.
    ///
    /// `start` is the earlier point in DOM order regardless of drag direction, and
    /// `end` is extended to the end of the grapheme under it, so both endpoint cells
    /// of a drag are included — the way terminals select. Returns `None` when nothing
    /// is selected.
    pub fn selection(&self) -> Option<(SelectionPoint, SelectionPoint)> {
        let state = (*lock::mutex(&self.inner.selection))?;
        self.ordered_selection(&state)
    }

    /// Clear the document selection, if any.
    pub fn clear_selection(&self) {
        self.set_selection_state(None);
    }

    /// Arm a selection drag from a left mouse down.
    ///
    /// Resolves the boundary from the hit node and snaps the press position to the
    /// nearest character within it. Returns `None` when the boundary contains no
    /// selectable text, in which case the drag selects nothing.
    pub(crate) fn begin_selection_drag(
        &self,
        x: i32,
        y: i32,
        hit: NodeId,
    ) -> Option<PendingSelection> {
        let boundary = self.selection_boundary_of(hit);
        let anchor = self.selection_point_at(x, y, boundary)?;
        Some(PendingSelection { boundary, anchor })
    }

    /// Extend an armed selection drag to the current pointer position.
    pub(crate) fn update_selection_drag(&self, pending: &PendingSelection, x: i32, y: i32) {
        let Some(focus) = self.selection_point_at(x, y, pending.boundary) else {
            return;
        };
        self.set_selection_state(Some(SelectionState {
            boundary: pending.boundary,
            anchor: pending.anchor,
            focus,
        }));
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
        }
    }

    /// The boundary a drag starting on `hit` is confined to: the nearest
    /// ancestor-or-self marked `selection_boundary`, or the root.
    fn selection_boundary_of(&self, hit: NodeId) -> NodeId {
        let mut current = Some(hit);
        while let Some(node) = current {
            if self
                .resolved_style(node)
                .is_ok_and(|style| style.selection_boundary)
            {
                return node;
            }
            current = self.get_parent(node);
        }
        self.root()
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
            let parent = self.get_parent(*step_a)?;
            let children = self.get_children(parent);
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
        while let Some(parent) = self.get_parent(current) {
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

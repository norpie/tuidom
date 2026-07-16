use std::collections::HashSet;

use crate::document::Document;
use crate::id::NodeId;
use crate::lock;
use crate::node::{LayoutRect, NodeKindView};
use crate::render::grid::ClipRect;
use crate::style::resolution::ResolvedStyle;
use crate::style::{Display, Overflow, ScrollbarShow};

#[derive(Debug, Clone)]
pub(crate) struct PaintEntry {
    pub id: NodeId,
    pub kind: NodeKindView,
    /// The node's screen rectangle, translated by every scrollable ancestor's offset.
    pub layout: LayoutRect,
    pub resolved: ResolvedStyle,
    /// The intersection of every scrolling/clipping ancestor's viewport. Painting and
    /// hit-testing outside it are dropped.
    pub clip: ClipRect,
    /// When set, this entry is a scrollbar strip rather than the node itself: `layout`
    /// is the strip and painting draws only the bar. It is pushed after the container's
    /// subtree, so the bar overlays the content it scrolls but stays under anything that
    /// paints over the container later. Hit-testing a strip resolves to the container.
    pub scrollbar: Option<ScrollbarPaint>,
}

/// Geometry of one scrollbar strip, computed against the container's viewport.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollbarPaint {
    /// Whether this is the vertical bar (right column) or the horizontal one (bottom row).
    pub vertical: bool,
    /// Cells from the strip start to the thumb.
    pub thumb_start: u16,
    /// Thumb length in cells, at least one.
    pub thumb_len: u16,
}

pub(crate) fn paint_order(doc: &Document) -> Vec<PaintEntry> {
    let mut entries = Vec::new();
    let focus_path = focused_path(doc);
    collect_ordered_entries(
        doc,
        doc.root(),
        (0, 0),
        ClipRect::UNBOUNDED,
        &focus_path,
        &mut entries,
    );
    entries
}

/// The focused node and its ancestors, for `ScrollbarShow::WhenFocused`.
fn focused_path(doc: &Document) -> HashSet<NodeId> {
    let mut path = HashSet::new();
    let Some(focused) = doc.focused() else {
        return path;
    };
    path.insert(focused);
    let mut node = focused;
    while let Some(parent) = doc.get_node(node).and_then(|view| view.parent) {
        path.insert(parent);
        node = parent;
    }
    path
}

fn collect_ordered_entries(
    doc: &Document,
    node_id: NodeId,
    translation: (i32, i32),
    clip: ClipRect,
    focus_path: &HashSet<NodeId>,
    entries: &mut Vec<PaintEntry>,
) {
    let Some(mut entry) = collect_entry(doc, node_id) else {
        return;
    };
    entry.layout.x -= translation.0;
    entry.layout.y -= translation.1;
    entry.clip = clip;

    // Children paint in this node's context: a Scroll axis translates them by the scroll
    // offset, and a Scroll or Clip axis bounds them to this node's padding box. Both are
    // computed in translated space, since the node itself may sit inside another scroller.
    let mut child_translation = translation;
    let mut child_clip = clip;
    let viewport = entry.layout.padding_box(&entry.resolved);
    if entry.resolved.overflow_x != Overflow::Visible {
        child_clip = child_clip.bound_x(
            i64::from(viewport.x),
            i64::from(viewport.x) + i64::from(viewport.width),
        );
    }
    if entry.resolved.overflow_y != Overflow::Visible {
        child_clip = child_clip.bound_y(
            i64::from(viewport.y),
            i64::from(viewport.y) + i64::from(viewport.height),
        );
    }
    if entry.resolved.overflow_x == Overflow::Scroll
        || entry.resolved.overflow_y == Overflow::Scroll
    {
        let offset = doc.scroll_offset(node_id);
        if entry.resolved.overflow_x == Overflow::Scroll {
            child_translation.0 += i32::from(offset.x);
        }
        if entry.resolved.overflow_y == Overflow::Scroll {
            child_translation.1 += i32::from(offset.y);
        }
    }

    // Culling: a node whose rect lies entirely outside its clip paints nothing, so it is
    // dropped from the entries — still in the DOM, never painted and never hit. Its
    // children are visited as long as something of them could show: a non-clipping node's
    // child may overflow into view, but once the child clip is empty nothing in the
    // subtree can paint a cell.
    let culled = clip.excludes(
        entry.layout.x,
        entry.layout.y,
        entry.layout.width,
        entry.layout.height,
    );
    let scrollbars = if culled {
        Vec::new()
    } else {
        scrollbar_entries(doc, &entry, viewport, focus_path)
    };
    if !culled {
        entries.push(entry);
    }

    if !child_clip.is_empty() {
        let mut children = doc
            .get_children(node_id)
            .into_iter()
            .enumerate()
            .filter_map(|(sequence, child)| {
                let resolved = doc.resolved_style(child).ok()?;
                Some((resolved.z_index, sequence, child))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(z_index, sequence, _)| (*z_index, *sequence));

        for (_, _, child) in children {
            collect_ordered_entries(
                doc,
                child,
                child_translation,
                child_clip,
                focus_path,
                entries,
            );
        }
    }

    entries.extend(scrollbars);
}

/// The scrollbar strips a scroll container shows, if any.
fn scrollbar_entries(
    doc: &Document,
    entry: &PaintEntry,
    viewport: LayoutRect,
    focus_path: &HashSet<NodeId>,
) -> Vec<PaintEntry> {
    let scrolls_x = entry.resolved.overflow_x == Overflow::Scroll;
    let scrolls_y = entry.resolved.overflow_y == Overflow::Scroll;
    if !scrolls_x && !scrolls_y {
        return Vec::new();
    }
    let shown = match entry.resolved.scrollbar_show {
        ScrollbarShow::Always => true,
        ScrollbarShow::WhenFocused => focus_path.contains(&entry.id),
        ScrollbarShow::Never => false,
    };
    if !shown || viewport.width == 0 || viewport.height == 0 {
        return Vec::new();
    }

    let (max_x, max_y) = {
        let snapshot = lock::rw_read(&doc.inner.layout_snapshot);
        match snapshot.get(&entry.id) {
            Some(layout) => (layout.max_scroll_x, layout.max_scroll_y),
            None => (0, 0),
        }
    };
    let offset = doc.scroll_offset(entry.id);
    let vertical_bar = scrolls_y && max_y > 0;
    let horizontal_bar = scrolls_x && max_x > 0;

    let mut strips = Vec::new();
    if vertical_bar {
        let span = viewport.height;
        let (thumb_start, thumb_len) = thumb_geometry(span, viewport.height, max_y, offset.y);
        strips.push(strip_entry(
            entry,
            LayoutRect {
                x: viewport.x + i32::from(viewport.width) - 1,
                y: viewport.y,
                width: 1,
                height: span,
            },
            ScrollbarPaint {
                vertical: true,
                thumb_start,
                thumb_len,
            },
        ));
    }
    if horizontal_bar {
        // The vertical bar keeps the shared corner cell, so the horizontal one stops short.
        let span = viewport.width - u16::from(vertical_bar);
        let (thumb_start, thumb_len) = thumb_geometry(span, viewport.width, max_x, offset.x);
        strips.push(strip_entry(
            entry,
            LayoutRect {
                x: viewport.x,
                y: viewport.y + i32::from(viewport.height) - 1,
                width: span,
                height: 1,
            },
            ScrollbarPaint {
                vertical: false,
                thumb_start,
                thumb_len,
            },
        ));
    }
    strips
}

fn strip_entry(entry: &PaintEntry, layout: LayoutRect, bar: ScrollbarPaint) -> PaintEntry {
    PaintEntry {
        id: entry.id,
        kind: NodeKindView::Box,
        layout,
        resolved: entry.resolved.clone(),
        clip: entry.clip,
        scrollbar: Some(bar),
    }
}

/// Thumb position and length along a scrollbar strip.
///
/// The thumb's length shows how much of the content the viewport covers; its position
/// shows where the scroll offset sits in its range. Both ends are exact: offset zero
/// puts the thumb at the strip start, and the maximum offset puts its far end at the
/// strip end.
fn thumb_geometry(span: u16, viewport: u16, max_scroll: u16, offset: u16) -> (u16, u16) {
    let content = u32::from(viewport) + u32::from(max_scroll);
    if span == 0 || content == 0 {
        return (0, span);
    }

    let len = rounded_div(u32::from(span) * u32::from(viewport), content).clamp(1, u32::from(span))
        as u16;
    let range = u32::from(span - len);
    let start = if max_scroll == 0 {
        0
    } else {
        rounded_div(u32::from(offset) * range, u32::from(max_scroll)) as u16
    };
    (start, len)
}

fn rounded_div(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}

fn collect_entry(doc: &Document, node_id: NodeId) -> Option<PaintEntry> {
    let view = doc.get_node(node_id)?;
    let resolved = doc.resolved_style(node_id).ok()?;
    if resolved.display == Display::None || resolved.opacity <= 0.0 {
        return None;
    }
    let layout = view.layout?;

    Some(PaintEntry {
        id: node_id,
        kind: view.kind,
        layout,
        resolved,
        clip: ClipRect::UNBOUNDED,
        scrollbar: None,
    })
}

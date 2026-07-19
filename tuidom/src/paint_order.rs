use std::collections::HashSet;
use std::sync::Arc;

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
    /// Shared with the style cache rather than copied: the walk resolves every node
    /// and again every child, and `ResolvedStyle` is large enough that copying it
    /// per entry was a measurable share of the frame.
    pub resolved: Arc<ResolvedStyle>,
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
    /// The bar's own visibility (0–1), multiplied into the container's opacity when
    /// painting. Below 1 only while a `WhenScrolling` bar is fading out.
    pub alpha: f64,
}

pub(crate) fn paint_order(doc: &Document) -> Vec<PaintEntry> {
    let mut entries = Vec::new();
    let focus_path = focused_path(doc);
    collect_ordered_entries(
        doc,
        doc.root(),
        None,
        (0, 0),
        ClipRect::UNBOUNDED,
        &focus_path,
        &mut entries,
    );
    entries
}

/// The topmost paint entry at a screen coordinate, scrollbar strips included.
///
/// This is the paint-order ground truth behind hit-testing: whatever entry would
/// have painted the cell last is the one the coordinate resolves to.
pub(crate) fn entry_at(doc: &Document, x: i32, y: i32) -> Option<PaintEntry> {
    paint_order(doc).into_iter().rev().find(|entry| {
        entry_contains(&entry.layout, x, y) && entry.clip.contains(i64::from(x), i64::from(y))
    })
}

/// The strip entry of one of a container's scrollbars, if that bar is shown.
///
/// Looked up fresh from paint order so a drag always works against the strip
/// geometry the user currently sees, even across relayouts.
pub(crate) fn scrollbar_strip_of(
    doc: &Document,
    container: NodeId,
    vertical: bool,
) -> Option<PaintEntry> {
    paint_order(doc).into_iter().find(|entry| {
        entry.id == container && entry.scrollbar.is_some_and(|bar| bar.vertical == vertical)
    })
}

pub(crate) fn entry_contains(layout: &LayoutRect, x: i32, y: i32) -> bool {
    let right = layout.x.saturating_add(i32::from(layout.width));
    let bottom = layout.y.saturating_add(i32::from(layout.height));

    x >= layout.x && x < right && y >= layout.y && y < bottom
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

/// `resolved` is this node's already-resolved style when the caller has one.
///
/// The parent resolves each child to read its `z_index` for the sort, so the
/// value is in hand by the time the child is walked — resolving it a second time
/// here would double the resolve count for the whole tree, and style resolution
/// is the largest single cost in this walk.
fn collect_ordered_entries(
    doc: &Document,
    node_id: NodeId,
    resolved: Option<Arc<ResolvedStyle>>,
    translation: (i32, i32),
    clip: ClipRect,
    focus_path: &HashSet<NodeId>,
    entries: &mut Vec<PaintEntry>,
) {
    let Some(mut entry) = collect_entry(doc, node_id, resolved) else {
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
                let resolved = doc.resolved_style_arc(child).ok()?;
                Some((resolved.z_index, sequence, child, resolved))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(z_index, sequence, _, _)| (*z_index, *sequence));

        for (_, _, child, resolved) in children {
            collect_ordered_entries(
                doc,
                child,
                Some(resolved),
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
        ScrollbarShow::Always => Some(1.0),
        ScrollbarShow::WhenFocused => focus_path.contains(&entry.id).then_some(1.0),
        ScrollbarShow::WhenScrolling => when_scrolling_alpha(doc, entry),
        ScrollbarShow::Never => None,
    };
    let Some(alpha) = shown else {
        return Vec::new();
    };
    if viewport.width == 0 || viewport.height == 0 {
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
                alpha,
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
                alpha,
            },
        ));
    }
    strips
}

/// The visibility of a `WhenScrolling` bar right now: opaque within the hide delay
/// after the container's last scroll activity, ramping to transparent over the fade
/// duration, absent after — and pinned opaque while the bar is grabbed, however
/// long the grip is held.
fn when_scrolling_alpha(doc: &Document, entry: &PaintEntry) -> Option<f64> {
    if *lock::mutex(&doc.inner.scrollbar_grab) == Some(entry.id) {
        return Some(1.0);
    }
    let activity = lock::mutex(&doc.inner.scroll_activity)
        .get(&entry.id)
        .copied()?;
    let elapsed = doc.now().saturating_duration_since(activity);

    let delay = entry.resolved.scrollbar_hide_delay;
    if elapsed <= delay {
        return Some(1.0);
    }
    let fade = entry.resolved.scrollbar_fade_duration;
    let fading = elapsed - delay;
    if fading >= fade {
        return None;
    }
    Some(1.0 - fading.as_secs_f64() / fade.as_secs_f64())
}

fn strip_entry(entry: &PaintEntry, layout: LayoutRect, bar: ScrollbarPaint) -> PaintEntry {
    PaintEntry {
        id: entry.id,
        kind: NodeKindView::Box,
        layout,
        resolved: Arc::clone(&entry.resolved),
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

/// The scroll offset that puts the thumb at `thumb_start` — the inverse of
/// [`thumb_geometry`].
///
/// Both ends are exact in both directions: thumb at the strip start means offset
/// zero, thumb at the end of its range means the maximum offset. Between them the
/// two mappings agree up to rounding, which is what makes a grabbed thumb track
/// the cursor without drifting.
pub(crate) fn offset_for_thumb(span: u16, viewport: u16, max_scroll: u16, thumb_start: u16) -> u16 {
    let content = u32::from(viewport) + u32::from(max_scroll);
    if span == 0 || content == 0 || max_scroll == 0 {
        return 0;
    }

    let len = rounded_div(u32::from(span) * u32::from(viewport), content).clamp(1, u32::from(span))
        as u16;
    let range = u32::from(span - len);
    if range == 0 {
        return 0;
    }
    let start = u32::from(thumb_start).min(range);
    rounded_div(start * u32::from(max_scroll), range) as u16
}

fn rounded_div(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}

fn collect_entry(
    doc: &Document,
    node_id: NodeId,
    resolved: Option<Arc<ResolvedStyle>>,
) -> Option<PaintEntry> {
    let view = doc.get_node(node_id)?;
    let resolved = match resolved {
        Some(resolved) => resolved,
        None => doc.resolved_style_arc(node_id).ok()?,
    };
    if resolved.display == Display::None || resolved.opacity <= 0.0 {
        return None;
    }
    let layout = view.layout?.rect;

    Some(PaintEntry {
        id: node_id,
        kind: view.kind,
        layout,
        resolved,
        clip: ClipRect::UNBOUNDED,
        scrollbar: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{offset_for_thumb, thumb_geometry};

    #[test]
    fn thumb_mapping_is_exact_at_both_ends() {
        for (span, viewport, max_scroll) in [(10u16, 10u16, 40u16), (5, 8, 3), (24, 24, 100)] {
            let (start_at_zero, len) = thumb_geometry(span, viewport, max_scroll, 0);
            assert_eq!(start_at_zero, 0);
            assert_eq!(offset_for_thumb(span, viewport, max_scroll, 0), 0);

            let (start_at_max, _) = thumb_geometry(span, viewport, max_scroll, max_scroll);
            assert_eq!(start_at_max, span - len);
            assert_eq!(
                offset_for_thumb(span, viewport, max_scroll, span - len),
                max_scroll
            );
        }
    }

    #[test]
    fn offset_for_thumb_clamps_past_the_range() {
        // Thumb positions beyond the strip clamp to the maximum offset.
        assert_eq!(offset_for_thumb(10, 10, 40, u16::MAX), 40);
    }

    #[test]
    fn offset_for_thumb_is_monotonic() {
        let (span, viewport, max_scroll) = (12u16, 12u16, 30u16);
        let mut last = 0;
        for start in 0..span {
            let offset = offset_for_thumb(span, viewport, max_scroll, start);
            assert!(offset >= last);
            last = offset;
        }
    }

    #[test]
    fn thumb_round_trips_when_range_covers_the_offsets() {
        // With at least as many thumb positions as offsets, no information is lost:
        // forward then inverse returns the original offset.
        let (span, viewport, max_scroll) = (30u16, 20u16, 10u16);
        for offset in 0..=max_scroll {
            let (start, _) = thumb_geometry(span, viewport, max_scroll, offset);
            assert_eq!(offset_for_thumb(span, viewport, max_scroll, start), offset);
        }
    }

    #[test]
    fn degenerate_strips_map_to_zero() {
        assert_eq!(offset_for_thumb(0, 10, 40, 3), 0);
        assert_eq!(offset_for_thumb(10, 0, 0, 3), 0);
        // No scroll range at all: the thumb fills the strip and cannot move.
        assert_eq!(offset_for_thumb(10, 10, 0, 3), 0);
    }
}

use crate::document::Document;
use crate::id::NodeId;
use crate::node::{LayoutRect, NodeKindView};
use crate::render::grid::ClipRect;
use crate::style::resolution::ResolvedStyle;
use crate::style::{Display, Overflow};

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
}

pub(crate) fn paint_order(doc: &Document) -> Vec<PaintEntry> {
    let mut entries = Vec::new();
    collect_ordered_entries(doc, doc.root(), (0, 0), ClipRect::UNBOUNDED, &mut entries);
    entries
}

fn collect_ordered_entries(
    doc: &Document,
    node_id: NodeId,
    translation: (i32, i32),
    clip: ClipRect,
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
    if !culled {
        entries.push(entry);
    }
    if child_clip.is_empty() {
        return;
    }

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
        collect_ordered_entries(doc, child, child_translation, child_clip, entries);
    }
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
    })
}

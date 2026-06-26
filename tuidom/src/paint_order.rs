use crate::document::Document;
use crate::id::NodeId;
use crate::node::{LayoutRect, NodeKindView};
use crate::style::Display;
use crate::style::resolution::ResolvedStyle;

#[derive(Debug, Clone)]
pub(crate) struct PaintEntry {
    pub id: NodeId,
    pub kind: NodeKindView,
    pub layout: LayoutRect,
    pub resolved: ResolvedStyle,
}

pub(crate) fn paint_order(doc: &Document) -> Vec<PaintEntry> {
    let mut entries = Vec::new();
    collect_ordered_entries(doc, doc.root(), &mut entries);
    entries
}

fn collect_ordered_entries(doc: &Document, node_id: NodeId, entries: &mut Vec<PaintEntry>) {
    let Some(entry) = collect_entry(doc, node_id) else {
        return;
    };

    entries.push(entry);

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
        collect_ordered_entries(doc, child, entries);
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
    })
}

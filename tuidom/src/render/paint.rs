//! Tree → grid painting. Walks the DOM depth-first and fills cells.

use crate::document::Document;
use crate::id::NodeId;
use crate::node::NodeKindView;
use crate::render::grid::{Cell, Grid};

/// Paint the visible portion of the DOM tree into the grid.
pub(crate) fn paint(doc: &Document, grid: &mut Grid) {
    let root = match doc.root() {
        Some(r) => r,
        None => return,
    };

    paint_node(doc, grid, root);
}

/// Recursively paint a node and its children.
fn paint_node(doc: &Document, grid: &mut Grid, node_id: NodeId) {
    let view = match doc.get_node(node_id) {
        Some(v) => v,
        None => {
            log::info!("[paint] node {node_id:?} not found");
            return;
        }
    };

    let layout = match view.layout {
        Some(l) => l,
        None => {
            log::info!("[paint] node {node_id:?} has no layout");
            return;
        }
    };

    let resolved = doc.resolved_style(node_id);
    let alpha = resolved.opacity;

    if alpha <= 0.0 {
        return;
    }

    let bg_rgb = resolved.background.map(|c| c.to_rgb());
    let fg_rgb = resolved.color.to_rgb();

    match &view.kind {
        NodeKindView::Box => {
            log::info!("[paint] Box {node_id:?} at {},{} {}x{} alpha={alpha} bg={bg_rgb:?}", layout.x, layout.y, layout.width, layout.height);
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell { ch: ' ', fg: None, bg: Some(bg) };
                grid.fill_rect(layout.x, layout.y, layout.width, layout.height, bg_cell, alpha);
            }
        }

        NodeKindView::Text { content } => {
            log::info!("[paint] Text {node_id:?} at {},{} {}x{} alpha={alpha} bg={bg_rgb:?} fg={fg_rgb:?}", layout.x, layout.y, layout.width, layout.height);
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell { ch: ' ', fg: None, bg: Some(bg) };
                grid.fill_rect(layout.x, layout.y, layout.width, layout.height, bg_cell, alpha);
            }
            grid.write_text(layout.x, layout.y, content, Some(fg_rgb), alpha);
        }
    }

    // Paint children on top
    for child in &view.children {
        paint_node(doc, grid, *child);
    }
}

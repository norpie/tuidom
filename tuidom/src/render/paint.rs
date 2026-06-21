//! Tree → grid painting. Walks the DOM depth-first and fills cells.

use crate::document::Document;
use crate::id::NodeId;
use crate::node::NodeKindView;
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::Display;

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
        None => return,
    };

    let Ok(resolved) = doc.resolved_style(node_id) else {
        return;
    };
    if resolved.display == Display::None {
        return;
    }

    let layout = match view.layout {
        Some(l) => l,
        None => return,
    };

    let alpha = resolved.opacity;

    if alpha <= 0.0 {
        return;
    }

    let bg_rgb = resolved.background.map(|c| c.to_rgb());
    let fg_rgb = resolved.color.to_rgb();

    match &view.kind {
        NodeKindView::Box => {
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell::empty_with_bg(bg);
                grid.fill_rect(
                    layout.x,
                    layout.y,
                    layout.width,
                    layout.height,
                    bg_cell,
                    alpha,
                );
            }
        }

        NodeKindView::Text { content } => {
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell::empty_with_bg(bg);
                grid.fill_rect(
                    layout.x,
                    layout.y,
                    layout.width,
                    layout.height,
                    bg_cell,
                    alpha,
                );
            }
            grid.write_text_clipped(
                GridRect {
                    x: layout.x,
                    y: layout.y,
                    width: layout.width,
                    height: layout.height,
                },
                content,
                Some(fg_rgb),
                alpha,
            );
        }
    }

    // Paint children on top
    for child in &view.children {
        paint_node(doc, grid, *child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::LayoutRect;
    use crate::style::{Display, Length, Style};

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.cells[row]
            .iter()
            .filter(|cell| !cell.is_wide_continuation())
            .map(Cell::terminal_text)
            .collect()
    }

    #[test]
    fn child_changed_to_display_none_does_not_paint_from_stale_layout() {
        let doc = Document::new();
        let root = doc.create_box();
        let text = doc.create_text("hi");

        let mut root_style = Style::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        doc.append_child(root, text).unwrap();
        doc.set_root(root);
        doc.compute_layout(5, 1);

        let mut visible_grid = Grid::new(5, 1);
        paint(&doc, &mut visible_grid);
        assert_eq!(row_text(&visible_grid, 0), "hi   ");

        let mut hidden_style = Style::new();
        hidden_style.display(Display::None);
        doc.set_style(text, &hidden_style).unwrap();
        doc.compute_layout(5, 1);
        assert!(doc.get_node(text).unwrap().layout.is_none());

        if let Some(mut data) = doc.inner.nodes.get_mut(&text) {
            data.layout = Some(LayoutRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            });
        }

        let mut hidden_grid = Grid::new(5, 1);
        paint(&doc, &mut hidden_grid);
        assert_eq!(row_text(&hidden_grid, 0), "     ");
    }

    #[test]
    fn text_node_paints_multiline_content_clipped_to_layout() {
        let doc = Document::new();
        let text = doc.create_text("abcd\nefgh");
        doc.set_root(text);

        if let Some(mut data) = doc.inner.nodes.get_mut(&text) {
            data.layout = Some(LayoutRect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            });
        } else {
            panic!("text node should exist");
        }

        let mut grid = Grid::new(5, 3);
        paint(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "     ");
        assert_eq!(row_text(&grid, 1), " ab  ");
        assert_eq!(row_text(&grid, 2), "     ");
    }
}

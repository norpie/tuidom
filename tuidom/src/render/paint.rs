//! Tree → grid painting using z-index sorted sibling subtrees.

use crate::document::Document;
use crate::id::NodeId;
use crate::node::{LayoutRect, NodeKindView};
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::Display;
use crate::style::color::RgbCache;
use crate::style::resolution::ResolvedStyle;

/// Paint the visible portion of the DOM tree into the grid.
pub(crate) fn paint(doc: &Document, grid: &mut Grid, rgb_cache: &mut RgbCache) {
    let root = doc.root();

    let mut sequence = 0;
    if let Some(context) = collect_context(doc, root, &mut sequence) {
        paint_context(grid, &context, rgb_cache);
    }
}

#[derive(Debug)]
struct PaintNode {
    kind: NodeKindView,
    layout: LayoutRect,
    resolved: ResolvedStyle,
    sequence: u64,
    children: Vec<PaintNode>,
}

fn collect_context(doc: &Document, root: NodeId, sequence: &mut u64) -> Option<PaintNode> {
    collect_node_tree(doc, root, sequence, 0)
}

fn collect_node_tree(
    doc: &Document,
    node_id: NodeId,
    sequence: &mut u64,
    node_sequence: u64,
) -> Option<PaintNode> {
    let view = doc.get_node(node_id)?;
    let resolved = doc.resolved_style(node_id).ok()?;
    if resolved.display == Display::None || resolved.opacity <= 0.0 {
        return None;
    }
    let layout = view.layout?;

    let mut children = Vec::new();
    for child in doc.get_children(node_id) {
        *sequence += 1;
        if let Some(child_node) = collect_node_tree(doc, child, sequence, *sequence) {
            children.push(child_node);
        }
    }

    Some(PaintNode {
        kind: view.kind,
        layout,
        resolved,
        sequence: node_sequence,
        children,
    })
}

fn paint_context(grid: &mut Grid, root: &PaintNode, rgb_cache: &mut RgbCache) {
    paint_node_self(grid, root, rgb_cache);

    let mut children = root.children.iter().collect::<Vec<_>>();
    children.sort_by_key(|child| (child.resolved.z_index, child.sequence));

    for child in children {
        paint_context(grid, child, rgb_cache);
    }
}

fn paint_node_self(grid: &mut Grid, node: &PaintNode, rgb_cache: &mut RgbCache) {
    let alpha = node.resolved.opacity;
    let bg_rgb = node.resolved.background.map(|c| rgb_cache.resolve(c));
    let fg_rgb = rgb_cache.resolve(node.resolved.color);

    match &node.kind {
        NodeKindView::Box => {
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell::empty_with_bg(bg);
                grid.fill_rect(
                    node.layout.x,
                    node.layout.y,
                    node.layout.width,
                    node.layout.height,
                    bg_cell,
                    alpha,
                );
            }
        }

        NodeKindView::Text { content } => {
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell::empty_with_bg(bg);
                grid.fill_rect(
                    node.layout.x,
                    node.layout.y,
                    node.layout.width,
                    node.layout.height,
                    bg_cell,
                    alpha,
                );
            }
            grid.write_text_clipped(
                GridRect {
                    x: node.layout.x,
                    y: node.layout.y,
                    width: node.layout.width,
                    height: node.layout.height,
                },
                content,
                Some(fg_rgb),
                alpha,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::LayoutRect;
    use crate::style::color::Rgb;
    use crate::style::{Color, Display, Length, Style};

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.cells[row]
            .iter()
            .filter(|cell| !cell.is_wide_continuation())
            .map(Cell::terminal_text)
            .collect()
    }

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    fn set_layout(doc: &Document, node: NodeId, layout: LayoutRect) {
        crate::lock::rw_write(&doc.inner.layout_rects).insert(node, layout);
    }

    fn one_cell() -> LayoutRect {
        LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }
    }

    fn set_one_cell_layouts(doc: &Document, nodes: &[NodeId]) {
        for node in nodes {
            set_layout(doc, *node, one_cell());
        }
    }

    fn set_background(doc: &Document, node: NodeId, color: Color) {
        let mut style = Style::new();
        style.background(color);
        doc.set_style(node, &style).unwrap();
    }

    fn set_background_z(doc: &Document, node: NodeId, color: Color, z_index: i32) {
        let mut style = Style::new();
        style.background(color);
        style.z_index(z_index);
        doc.set_style(node, &style).unwrap();
    }

    fn set_background_z_context(doc: &Document, node: NodeId, color: Color, z_index: i32) {
        let mut style = Style::new();
        style.background(color);
        style.z_index(z_index);
        style.stacking_context(true);
        doc.set_style(node, &style).unwrap();
    }

    fn paint_doc(doc: &Document, grid: &mut Grid) {
        let mut rgb_cache = RgbCache::new();
        paint(doc, grid, &mut rgb_cache);
    }

    fn painted_bg(doc: &Document) -> Option<Rgb> {
        let mut grid = Grid::new(1, 1);
        paint_doc(doc, &mut grid);
        grid.cells[0][0].bg
    }

    #[test]
    fn default_paint_order_matches_dom_order() {
        let doc = Document::new();
        let root = doc.root();
        let first = doc.create_box();
        let second = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background(&doc, first, Color::red());
        set_background(&doc, second, Color::blue());

        doc.append_child(root, first).unwrap();
        doc.append_child(root, second).unwrap();
        set_one_cell_layouts(&doc, &[root, first, second]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn higher_z_index_paints_above_later_dom_sibling() {
        let doc = Document::new();
        let root = doc.root();
        let high = doc.create_box();
        let low = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, high, Color::blue(), 10);
        set_background_z(&doc, low, Color::red(), 0);

        doc.append_child(root, high).unwrap();
        doc.append_child(root, low).unwrap();
        set_one_cell_layouts(&doc, &[root, high, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn lower_z_index_paints_below_earlier_dom_sibling() {
        let doc = Document::new();
        let root = doc.root();
        let normal = doc.create_box();
        let low = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, normal, Color::blue(), 0);
        set_background_z(&doc, low, Color::red(), -1);

        doc.append_child(root, normal).unwrap();
        doc.append_child(root, low).unwrap();
        set_one_cell_layouts(&doc, &[root, normal, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn descendant_z_index_does_not_escape_parent_subtree() {
        let doc = Document::new();
        let root = doc.root();
        let parent = doc.create_box();
        let child = doc.create_box();
        let sibling = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, parent, Color::red(), 0);
        set_background_z(&doc, child, Color::green(), 999);
        set_background_z(&doc, sibling, Color::blue(), 1);

        doc.append_child(root, parent).unwrap();
        doc.append_child(parent, child).unwrap();
        doc.append_child(root, sibling).unwrap();
        set_one_cell_layouts(&doc, &[root, parent, child, sibling]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn parent_self_paints_before_child_even_with_higher_z_index() {
        let doc = Document::new();
        let root = doc.root();
        let parent = doc.create_box();
        let child = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, parent, Color::red(), 10);
        set_background_z(&doc, child, Color::green(), 0);

        doc.append_child(root, parent).unwrap();
        doc.append_child(parent, child).unwrap();
        set_one_cell_layouts(&doc, &[root, parent, child]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 255, 0)));
    }

    #[test]
    fn explicit_stacking_context_subtree_paints_atomically() {
        let doc = Document::new();
        let root = doc.root();
        let context_root = doc.create_box();
        let child = doc.create_box();
        let sibling = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z_context(&doc, context_root, Color::red(), 0);
        set_background_z(&doc, child, Color::green(), 999);
        set_background_z(&doc, sibling, Color::blue(), 1);

        doc.append_child(root, context_root).unwrap();
        doc.append_child(context_root, child).unwrap();
        doc.append_child(root, sibling).unwrap();
        set_one_cell_layouts(&doc, &[root, context_root, child, sibling]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn descendants_sort_inside_parent_subtree() {
        let doc = Document::new();
        let root = doc.root();
        let context_root = doc.create_box();
        let high = doc.create_box();
        let low = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z_context(&doc, context_root, Color::red(), 0);
        set_background_z(&doc, high, Color::green(), 10);
        set_background_z(&doc, low, Color::blue(), 0);

        doc.append_child(root, context_root).unwrap();
        doc.append_child(context_root, high).unwrap();
        doc.append_child(context_root, low).unwrap();
        set_one_cell_layouts(&doc, &[root, context_root, high, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 255, 0)));
    }

    #[test]
    fn child_changed_to_display_none_does_not_paint_from_stale_layout() {
        let doc = Document::new();
        let root = doc.root();
        let text = doc.create_text("hi");

        let mut root_style = Style::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        doc.append_child(root, text).unwrap();
        doc.compute_layout(5, 1);

        let mut visible_grid = Grid::new(5, 1);
        paint_doc(&doc, &mut visible_grid);
        assert_eq!(row_text(&visible_grid, 0), "hi   ");

        let mut hidden_style = Style::new();
        hidden_style.display(Display::None);
        doc.set_style(text, &hidden_style).unwrap();
        doc.compute_layout(5, 1);
        assert!(doc.get_node(text).unwrap().layout.is_none());

        set_layout(
            &doc,
            text,
            LayoutRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
        );

        let mut hidden_grid = Grid::new(5, 1);
        paint_doc(&doc, &mut hidden_grid);
        assert_eq!(row_text(&hidden_grid, 0), "     ");
    }

    #[test]
    fn translucent_background_color_blends_without_node_opacity_and_preserves_text() {
        let doc = Document::new();
        let root = doc.root();
        let text = doc.create_text("x");
        let overlay = doc.create_box();

        let mut root_style = Style::new();
        root_style.background(Color::black());
        doc.set_style(root, &root_style).unwrap();

        let mut text_style = Style::new();
        text_style.color(Color::white());
        doc.set_style(text, &text_style).unwrap();

        let mut overlay_style = Style::new();
        overlay_style.background(Color::oklcha(1.0, 0.0, 0.0, 0.5));
        doc.set_style(overlay, &overlay_style).unwrap();

        doc.append_child(root, text).unwrap();
        doc.append_child(root, overlay).unwrap();

        let layout = LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        set_layout(&doc, root, layout);
        set_layout(&doc, text, layout);
        set_layout(&doc, overlay, layout);

        let mut grid = Grid::new(1, 1);
        paint_doc(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "x");
        assert_eq!(grid.cells[0][0].bg, Some(rgb(128, 128, 128)));
        assert_eq!(grid.cells[0][0].fg, Some(rgb(255, 255, 255)));
    }

    #[test]
    fn color_alpha_and_node_opacity_multiply() {
        let doc = Document::new();
        let root = doc.root();
        let overlay = doc.create_box();

        let mut root_style = Style::new();
        root_style.background(Color::black());
        doc.set_style(root, &root_style).unwrap();

        let mut overlay_style = Style::new();
        overlay_style.background(Color::oklcha(1.0, 0.0, 0.0, 0.5));
        overlay_style.opacity(0.5);
        doc.set_style(overlay, &overlay_style).unwrap();

        doc.append_child(root, overlay).unwrap();

        let layout = LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        set_layout(&doc, root, layout);
        set_layout(&doc, overlay, layout);

        let mut grid = Grid::new(1, 1);
        paint_doc(&doc, &mut grid);

        assert_eq!(grid.cells[0][0].bg, Some(rgb(64, 64, 64)));
    }

    #[test]
    fn translucent_foreground_color_blends_with_background() {
        let doc = Document::new();
        let root = doc.root();
        let text = doc.create_text("x");

        let mut root_style = Style::new();
        root_style.background(Color::black());
        doc.set_style(root, &root_style).unwrap();

        let mut text_style = Style::new();
        text_style.color(Color::oklcha(1.0, 0.0, 0.0, 0.5));
        doc.set_style(text, &text_style).unwrap();

        doc.append_child(root, text).unwrap();

        let layout = LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        set_layout(&doc, root, layout);
        set_layout(&doc, text, layout);

        let mut grid = Grid::new(1, 1);
        paint_doc(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "x");
        assert_eq!(grid.cells[0][0].bg, Some(rgb(0, 0, 0)));
        assert_eq!(grid.cells[0][0].fg, Some(rgb(128, 128, 128)));
    }

    #[test]
    fn text_node_paints_multiline_content_clipped_to_layout() {
        let doc = Document::new();
        let root = doc.root();
        let text = doc.create_text("abcd\nefgh");
        doc.append_child(root, text).unwrap();

        set_layout(
            &doc,
            root,
            LayoutRect {
                x: 0,
                y: 0,
                width: 5,
                height: 3,
            },
        );
        set_layout(
            &doc,
            text,
            LayoutRect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            },
        );

        let mut grid = Grid::new(5, 3);
        paint_doc(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "     ");
        assert_eq!(row_text(&grid, 1), " ab  ");
        assert_eq!(row_text(&grid, 2), "     ");
    }
}

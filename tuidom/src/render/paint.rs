//! Tree → grid painting using z-index sorted stacking contexts.

use crate::document::Document;
use crate::id::NodeId;
use crate::node::{LayoutRect, NodeKindView};
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::Display;
use crate::style::resolution::ResolvedStyle;

/// Paint the visible portion of the DOM tree into the grid.
pub(crate) fn paint(doc: &Document, grid: &mut Grid) {
    let root = match doc.root() {
        Some(r) => r,
        None => return,
    };

    let mut sequence = 0;
    if let Some(context) = collect_context(doc, root, &mut sequence) {
        paint_context(grid, &context);
    }
}

#[derive(Debug)]
struct PaintNode {
    kind: NodeKindView,
    layout: LayoutRect,
    resolved: ResolvedStyle,
}

#[derive(Debug)]
struct PaintContext {
    root: PaintNode,
    items: Vec<PaintItem>,
}

#[derive(Debug)]
enum PaintItem {
    Node {
        node: PaintNode,
        z_index: i32,
        sequence: u64,
    },
    Context {
        context: PaintContext,
        z_index: i32,
        sequence: u64,
    },
}

impl PaintItem {
    fn z_index(&self) -> i32 {
        match self {
            Self::Node { z_index, .. } | Self::Context { z_index, .. } => *z_index,
        }
    }

    fn sequence(&self) -> u64 {
        match self {
            Self::Node { sequence, .. } | Self::Context { sequence, .. } => *sequence,
        }
    }
}

fn collect_context(doc: &Document, root: NodeId, sequence: &mut u64) -> Option<PaintContext> {
    let root_node = collect_node(doc, root)?;
    let mut context = PaintContext {
        root: root_node,
        items: Vec::new(),
    };

    for child in doc.get_children(root) {
        collect_into_context(doc, child, sequence, &mut context.items);
    }

    Some(context)
}

fn collect_into_context(
    doc: &Document,
    node_id: NodeId,
    sequence: &mut u64,
    items: &mut Vec<PaintItem>,
) {
    let Some(node) = collect_node(doc, node_id) else {
        return;
    };

    *sequence += 1;
    let node_sequence = *sequence;
    let z_index = node.resolved.z_index;
    let creates_context = node.resolved.stacking_context;

    if creates_context {
        let mut nested_context = PaintContext {
            root: node,
            items: Vec::new(),
        };
        for child in doc.get_children(node_id) {
            collect_into_context(doc, child, sequence, &mut nested_context.items);
        }
        items.push(PaintItem::Context {
            context: nested_context,
            z_index,
            sequence: node_sequence,
        });
    } else {
        items.push(PaintItem::Node {
            node,
            z_index,
            sequence: node_sequence,
        });
        for child in doc.get_children(node_id) {
            collect_into_context(doc, child, sequence, items);
        }
    }
}

fn collect_node(doc: &Document, node_id: NodeId) -> Option<PaintNode> {
    let view = doc.get_node(node_id)?;
    let resolved = doc.resolved_style(node_id).ok()?;
    if resolved.display == Display::None || resolved.opacity <= 0.0 {
        return None;
    }
    let layout = view.layout?;

    Some(PaintNode {
        kind: view.kind,
        layout,
        resolved,
    })
}

fn paint_context(grid: &mut Grid, context: &PaintContext) {
    paint_node_self(grid, &context.root);

    let mut items = context.items.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| (item.z_index(), item.sequence()));

    for item in items {
        match item {
            PaintItem::Node { node, .. } => paint_node_self(grid, node),
            PaintItem::Context { context, .. } => paint_context(grid, context),
        }
    }
}

fn paint_node_self(grid: &mut Grid, node: &PaintNode) {
    let alpha = node.resolved.opacity;
    let bg_rgb = node.resolved.background.map(|c| c.to_rgb());
    let fg_rgb = node.resolved.color.to_rgb();

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
        if let Some(mut data) = doc.inner.nodes.get_mut(&node) {
            data.layout = Some(layout);
        }
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

    fn painted_bg(doc: &Document) -> Option<Rgb> {
        let mut grid = Grid::new(1, 1);
        paint(doc, &mut grid);
        grid.cells[0][0].bg
    }

    #[test]
    fn default_paint_order_matches_dom_order() {
        let doc = Document::new();
        let root = doc.create_box();
        let first = doc.create_box();
        let second = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background(&doc, first, Color::red());
        set_background(&doc, second, Color::blue());

        doc.append_child(root, first).unwrap();
        doc.append_child(root, second).unwrap();
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, first, second]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn higher_z_index_paints_above_later_dom_sibling() {
        let doc = Document::new();
        let root = doc.create_box();
        let high = doc.create_box();
        let low = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, high, Color::blue(), 10);
        set_background_z(&doc, low, Color::red(), 0);

        doc.append_child(root, high).unwrap();
        doc.append_child(root, low).unwrap();
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, high, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn lower_z_index_paints_below_earlier_dom_sibling() {
        let doc = Document::new();
        let root = doc.create_box();
        let normal = doc.create_box();
        let low = doc.create_box();

        set_background(&doc, root, Color::black());
        set_background_z(&doc, normal, Color::blue(), 0);
        set_background_z(&doc, low, Color::red(), -1);

        doc.append_child(root, normal).unwrap();
        doc.append_child(root, low).unwrap();
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, normal, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn descendant_z_index_can_participate_in_nearest_stacking_context() {
        let doc = Document::new();
        let root = doc.create_box();
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
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, parent, child, sibling]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 255, 0)));
    }

    #[test]
    fn stacking_context_prevents_descendant_z_index_bleed() {
        let doc = Document::new();
        let root = doc.create_box();
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
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, context_root, child, sibling]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 0, 255)));
    }

    #[test]
    fn descendants_sort_inside_their_stacking_context() {
        let doc = Document::new();
        let root = doc.create_box();
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
        doc.set_root(root);
        set_one_cell_layouts(&doc, &[root, context_root, high, low]);

        assert_eq!(painted_bg(&doc), Some(rgb(0, 255, 0)));
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
    fn translucent_background_color_blends_without_node_opacity_and_preserves_text() {
        let doc = Document::new();
        let root = doc.create_box();
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
        doc.set_root(root);

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
        paint(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "x");
        assert_eq!(grid.cells[0][0].bg, Some(rgb(128, 128, 128)));
        assert_eq!(grid.cells[0][0].fg, Some(rgb(255, 255, 255)));
    }

    #[test]
    fn color_alpha_and_node_opacity_multiply() {
        let doc = Document::new();
        let root = doc.create_box();
        let overlay = doc.create_box();

        let mut root_style = Style::new();
        root_style.background(Color::black());
        doc.set_style(root, &root_style).unwrap();

        let mut overlay_style = Style::new();
        overlay_style.background(Color::oklcha(1.0, 0.0, 0.0, 0.5));
        overlay_style.opacity(0.5);
        doc.set_style(overlay, &overlay_style).unwrap();

        doc.append_child(root, overlay).unwrap();
        doc.set_root(root);

        let layout = LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        set_layout(&doc, root, layout);
        set_layout(&doc, overlay, layout);

        let mut grid = Grid::new(1, 1);
        paint(&doc, &mut grid);

        assert_eq!(grid.cells[0][0].bg, Some(rgb(64, 64, 64)));
    }

    #[test]
    fn translucent_foreground_color_blends_with_background() {
        let doc = Document::new();
        let root = doc.create_box();
        let text = doc.create_text("x");

        let mut root_style = Style::new();
        root_style.background(Color::black());
        doc.set_style(root, &root_style).unwrap();

        let mut text_style = Style::new();
        text_style.color(Color::oklcha(1.0, 0.0, 0.0, 0.5));
        doc.set_style(text, &text_style).unwrap();

        doc.append_child(root, text).unwrap();
        doc.set_root(root);

        let layout = LayoutRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        set_layout(&doc, root, layout);
        set_layout(&doc, text, layout);

        let mut grid = Grid::new(1, 1);
        paint(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "x");
        assert_eq!(grid.cells[0][0].bg, Some(rgb(0, 0, 0)));
        assert_eq!(grid.cells[0][0].fg, Some(rgb(128, 128, 128)));
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

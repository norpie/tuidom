//! Tree → grid painting using z-index sorted sibling subtrees.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::node::{NodeKindView, input_display_content};
use crate::paint_order::{PaintEntry, paint_order};
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::CursorBlink;
use crate::style::color::{Rgb, RgbCache};

/// DOM painting stage timings.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DomPaintStats {
    /// Time spent collecting the visible DOM tree into a paintable snapshot.
    pub collect_time: Duration,
    /// Time spent rasterizing the collected DOM snapshot into the grid.
    pub paint_time: Duration,
}

/// Paint the visible portion of the DOM tree into the grid.
pub(crate) fn paint(
    doc: &Document,
    grid: &mut Grid,
    rgb_cache: &mut RgbCache,
    cursor_visible: bool,
) -> DomPaintStats {
    let collect_start = Instant::now();
    let entries = paint_order(doc);
    let collect_time = collect_start.elapsed();

    let focused = doc.focused();
    let paint_start = Instant::now();
    for entry in &entries {
        paint_entry(grid, entry, focused, rgb_cache, cursor_visible);
    }
    let paint_time = paint_start.elapsed();

    DomPaintStats {
        collect_time,
        paint_time,
    }
}

fn paint_entry(
    grid: &mut Grid,
    node: &PaintEntry,
    focused: Option<crate::id::NodeId>,
    rgb_cache: &mut RgbCache,
    cursor_visible: bool,
) {
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
            paint_text(grid, node, bg_rgb, fg_rgb, alpha, content);
        }

        NodeKindView::Input {
            value,
            cursor,
            multiline,
            mask,
            ..
        } => {
            let content = input_display_content(value, *multiline, *mask);
            paint_text(grid, node, bg_rgb, fg_rgb, alpha, &content);
            if focused == Some(node.id)
                && (node.resolved.cursor_blink == CursorBlink::None || cursor_visible)
            {
                paint_input_cursor(grid, node, value, *cursor, *multiline, *mask, rgb_cache);
            }
        }
    }
}

fn paint_text(
    grid: &mut Grid,
    node: &PaintEntry,
    bg_rgb: Option<Rgb>,
    fg_rgb: Rgb,
    alpha: f64,
    content: &str,
) {
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

fn paint_input_cursor(
    grid: &mut Grid,
    node: &PaintEntry,
    value: &str,
    cursor: usize,
    multiline: bool,
    mask: Option<char>,
    rgb_cache: &mut RgbCache,
) {
    if node.layout.width == 0 || node.layout.height == 0 {
        return;
    }

    let cursor = clamp_to_grapheme_boundary(value, cursor);
    let position = input_cursor_position(value, cursor, multiline, mask);
    if position.y >= i32::from(node.layout.height) || position.x >= i32::from(node.layout.width) {
        return;
    }

    grid.paint_cursor(
        node.layout.x + position.x,
        node.layout.y + position.y,
        position.width,
        node.resolved.cursor_shape,
        Some(rgb_cache.resolve(node.resolved.cursor_fg)),
        Some(rgb_cache.resolve(node.resolved.cursor_bg)),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorPosition {
    x: i32,
    y: i32,
    width: u8,
}

fn input_cursor_position(
    value: &str,
    cursor: usize,
    multiline: bool,
    mask: Option<char>,
) -> CursorPosition {
    let prefix = input_display_content(&value[..cursor], multiline, mask);
    let y = if prefix.is_empty() {
        0
    } else {
        prefix.matches('\n').count() as i32
    };
    let x = UnicodeWidthStr::width(prefix.rsplit('\n').next().unwrap_or("")) as i32;
    let width = cursor_grapheme_width(&value[cursor..], multiline, mask);
    CursorPosition { x, y, width }
}

fn cursor_grapheme_width(suffix: &str, multiline: bool, mask: Option<char>) -> u8 {
    let Some(grapheme) = suffix.graphemes(true).next() else {
        return 1;
    };
    if multiline && grapheme == "\n" {
        return 1;
    }

    let display = if let Some(mask) = mask {
        mask.to_string()
    } else if !multiline && grapheme == "\n" {
        " ".to_owned()
    } else {
        grapheme.to_owned()
    };
    UnicodeWidthStr::width(display.as_str()).clamp(1, 2) as u8
}

fn clamp_to_grapheme_boundary(content: &str, offset: usize) -> usize {
    if offset >= content.len() {
        return content.len();
    }

    content
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index <= offset)
        .last()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::NodeId;
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
        paint(doc, grid, &mut rgb_cache, true);
    }

    fn painted_bg(doc: &Document) -> Option<Rgb> {
        let mut grid = Grid::new(1, 1);
        paint_doc(doc, &mut grid);
        grid.cells[0][0].bg
    }

    #[test]
    fn default_paint_order_matches_dom_order() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let first = doc.create_box().unwrap();
        let second = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let high = doc.create_box().unwrap();
        let low = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let normal = doc.create_box().unwrap();
        let low = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        let sibling = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let parent = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let context_root = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        let sibling = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let context_root = doc.create_box().unwrap();
        let high = doc.create_box().unwrap();
        let low = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("hi").unwrap();

        let mut root_style = Style::new();
        root_style.width(Length::Pixels(5));
        root_style.height(Length::Pixels(1));
        doc.set_style(root, &root_style).unwrap();

        doc.append_child(root, text).unwrap();
        doc.compute_layout(5, 1).unwrap();

        let mut visible_grid = Grid::new(5, 1);
        paint_doc(&doc, &mut visible_grid);
        assert_eq!(row_text(&visible_grid, 0), "hi   ");

        let mut hidden_style = Style::new();
        hidden_style.display(Display::None);
        doc.set_style(text, &hidden_style).unwrap();
        doc.compute_layout(5, 1).unwrap();
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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("x").unwrap();
        let overlay = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let overlay = doc.create_box().unwrap();

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
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("x").unwrap();

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
    fn paint_clips_negative_layout_position_without_snapping_to_origin() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("hi").unwrap();

        doc.append_child(root, text).unwrap();
        set_layout(
            &doc,
            root,
            LayoutRect {
                x: 0,
                y: 0,
                width: 3,
                height: 1,
            },
        );
        set_layout(
            &doc,
            text,
            LayoutRect {
                x: -1,
                y: 0,
                width: 2,
                height: 1,
            },
        );

        let mut grid = Grid::new(3, 1);
        paint_doc(&doc, &mut grid);

        assert_eq!(row_text(&grid, 0), "i  ");
    }

    #[test]
    fn text_node_paints_multiline_content_clipped_to_layout() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("abcd\nefgh").unwrap();
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

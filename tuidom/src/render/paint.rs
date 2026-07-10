//! Tree → grid painting using z-index sorted sibling subtrees.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::document::Document;
use crate::id::NodeId;
use crate::node::{
    LayoutRect, NodeKindView, input_display_content, input_scrolled_display_content,
};
use crate::paint_order::{PaintEntry, paint_order};
use crate::render::RenderCursor;
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::color::{Rgb, RgbCache};
use crate::style::{Color, CursorShape};

/// Largest background fill observed during a profiled paint pass.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LargestFillProfile {
    /// Node id that requested the fill.
    pub node_id: crate::id::NodeId,
    /// Node kind that requested the fill.
    pub node_kind: &'static str,
    /// Requested fill x coordinate before clipping.
    pub x: i32,
    /// Requested fill y coordinate before clipping.
    pub y: i32,
    /// Requested fill width before clipping.
    pub width: u16,
    /// Requested fill height before clipping.
    pub height: u16,
    /// Requested area before clipping.
    pub requested_cells: usize,
    /// Actual grid cells touched after clipping.
    pub clipped_cells: usize,
}

/// Detailed DOM paint instrumentation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PaintProfile {
    /// Whether detailed paint instrumentation was enabled for this frame.
    pub enabled: bool,
    /// Time spent converting resolved colors to terminal RGB.
    pub rgb_resolve_time: Duration,
    /// Number of resolved colors converted or read from the RGB cache.
    pub rgb_resolves: usize,
    /// Time spent filling node backgrounds into the grid.
    pub background_fill_time: Duration,
    /// Number of background fill calls.
    pub background_fill_calls: usize,
    /// Number of grid cells touched by background fills.
    pub filled_cells: usize,
    /// Total requested background fill area before clipping.
    pub requested_fill_cells: usize,
    /// Number of fully opaque background fill calls.
    pub opaque_fill_calls: usize,
    /// Number of cells touched by fully opaque background fills.
    pub opaque_filled_cells: usize,
    /// Largest background fill in this frame.
    pub largest_fill: Option<LargestFillProfile>,
    /// Time spent writing text glyphs into the grid.
    pub text_write_time: Duration,
    /// Number of glyph heads written into the grid.
    pub glyphs_written: usize,
    /// Time spent formatting input display content before text paint.
    pub input_format_time: Duration,
}

/// DOM painting stage timings.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DomPaintStats {
    /// Time spent clearing or initializing the frame grid.
    pub grid_time: Duration,
    /// Time spent collecting the visible DOM tree into a paintable snapshot.
    pub collect_time: Duration,
    /// Time spent rasterizing the collected DOM snapshot into the grid.
    pub paint_time: Duration,
    /// Detailed instrumentation for the paint span.
    pub profile: PaintProfile,
}

/// DOM painting output.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DomPaintOutput {
    /// DOM painting timings.
    pub stats: DomPaintStats,
    /// Cursor metadata produced by the focused input, if any.
    pub cursor: Option<RenderCursor>,
}

/// Paint the visible portion of the DOM tree into the grid.
pub(crate) fn paint(
    doc: &Document,
    grid: &mut Grid,
    rgb_cache: &mut RgbCache,
    instrument: bool,
    clear_grid: bool,
) -> DomPaintOutput {
    let collect_start = Instant::now();
    let entries = paint_order(doc);
    let collect_time = collect_start.elapsed();

    let focused = doc.focused();
    let mut profile = PaintProfile {
        enabled: instrument,
        ..PaintProfile::default()
    };

    let grid_start = Instant::now();
    let skipped_background = if clear_grid {
        clear_grid_for_paint(grid, &entries, rgb_cache, &mut profile)
    } else {
        None
    };
    let grid_time = grid_start.elapsed();

    let paint_start = Instant::now();
    let mut cursor = None;
    for entry in &entries {
        if let Some(entry_cursor) = paint_entry(
            grid,
            entry,
            focused,
            rgb_cache,
            &mut profile,
            skipped_background,
        ) {
            cursor = Some(entry_cursor);
        }
    }
    let paint_time = paint_start.elapsed();

    DomPaintOutput {
        stats: DomPaintStats {
            grid_time,
            collect_time,
            paint_time,
            profile,
        },
        cursor,
    }
}

fn paint_entry(
    grid: &mut Grid,
    node: &PaintEntry,
    focused: Option<crate::id::NodeId>,
    rgb_cache: &mut RgbCache,
    profile: &mut PaintProfile,
    skipped_background: Option<NodeId>,
) -> Option<RenderCursor> {
    let alpha = node.resolved.opacity;
    let skip_background = skipped_background == Some(node.id);
    let bg_rgb = if skip_background {
        None
    } else {
        node.resolved
            .background
            .map(|c| resolve_rgb(rgb_cache, c, profile))
    };
    let fg_rgb = resolve_rgb(rgb_cache, node.resolved.color, profile);

    match &node.kind {
        NodeKindView::Box => {
            if let Some(bg) = bg_rgb {
                let bg_cell = Cell::empty_with_bg(bg);
                fill_background(grid, node, bg_cell, alpha, profile);
            }
            None
        }

        NodeKindView::Text { content } => {
            paint_text(grid, node, bg_rgb, fg_rgb, alpha, content, profile);
            None
        }

        NodeKindView::Input {
            value,
            cursor,
            multiline,
            mask,
            scroll_x,
            scroll_y,
            ..
        } => {
            let input_format_start = profile.enabled.then(Instant::now);
            let content = input_display_content(value, *multiline, *mask);
            let visible_content = input_scrolled_display_content(&content, *scroll_x, *scroll_y);
            if let Some(start) = input_format_start {
                profile.input_format_time += start.elapsed();
            }
            paint_text(grid, node, bg_rgb, fg_rgb, alpha, &visible_content, profile);
            if focused == Some(node.id) {
                input_cursor_metadata(
                    grid,
                    node,
                    InputCursorPaint {
                        value,
                        cursor: *cursor,
                        multiline: *multiline,
                        mask: *mask,
                        scroll_x: *scroll_x,
                        scroll_y: *scroll_y,
                    },
                    fg_rgb,
                )
            } else {
                None
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
    profile: &mut PaintProfile,
) {
    if let Some(bg) = bg_rgb {
        let bg_cell = Cell::empty_with_bg(bg);
        fill_background(grid, node, bg_cell, alpha, profile);
    }

    let text_start = profile.enabled.then(Instant::now);
    let glyphs = grid.write_text_clipped(
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
    if let Some(start) = text_start {
        profile.text_write_time += start.elapsed();
        profile.glyphs_written += glyphs;
    }
}

fn clear_grid_for_paint(
    grid: &mut Grid,
    entries: &[PaintEntry],
    rgb_cache: &mut RgbCache,
    profile: &mut PaintProfile,
) -> Option<NodeId> {
    for entry in entries {
        let Some(background) = entry.resolved.background else {
            if entry_has_no_self_paint(entry) {
                continue;
            }
            grid.clear();
            return None;
        };

        if entry.resolved.opacity < 1.0 || !covers_grid(entry.layout, grid) {
            grid.clear();
            return None;
        }

        let bg = resolve_rgb(rgb_cache, background, profile);
        if bg.a < 255 {
            grid.clear();
            return None;
        }

        grid.clear_with_bg(bg);
        return Some(entry.id);
    }

    grid.clear();
    None
}

fn entry_has_no_self_paint(entry: &PaintEntry) -> bool {
    match &entry.kind {
        NodeKindView::Box => true,
        NodeKindView::Text { content } => content.is_empty(),
        NodeKindView::Input { .. } => false,
    }
}

fn covers_grid(layout: LayoutRect, grid: &Grid) -> bool {
    let left = i64::from(layout.x);
    let top = i64::from(layout.y);
    let right = left + i64::from(layout.width);
    let bottom = top + i64::from(layout.height);

    left <= 0 && top <= 0 && right >= i64::from(grid.width) && bottom >= i64::from(grid.height)
}

fn fill_background(
    grid: &mut Grid,
    node: &PaintEntry,
    bg_cell: Cell,
    alpha: f64,
    profile: &mut PaintProfile,
) {
    let requested_cells = usize::from(node.layout.width) * usize::from(node.layout.height);
    let opaque_fill = alpha >= 1.0 && bg_cell.bg.is_some_and(|bg| bg.a == 255);
    let fill_start = profile.enabled.then(Instant::now);
    let cells = grid.fill_rect(
        node.layout.x,
        node.layout.y,
        node.layout.width,
        node.layout.height,
        bg_cell,
        alpha,
    );
    if let Some(start) = fill_start {
        profile.background_fill_time += start.elapsed();
        profile.background_fill_calls += 1;
        profile.filled_cells += cells;
        profile.requested_fill_cells += requested_cells;
        if opaque_fill {
            profile.opaque_fill_calls += 1;
            profile.opaque_filled_cells += cells;
        }
        record_largest_fill(profile, node, requested_cells, cells);
    }
}

fn record_largest_fill(
    profile: &mut PaintProfile,
    node: &PaintEntry,
    requested_cells: usize,
    clipped_cells: usize,
) {
    if profile
        .largest_fill
        .is_some_and(|largest| largest.clipped_cells >= clipped_cells)
    {
        return;
    }

    profile.largest_fill = Some(LargestFillProfile {
        node_id: node.id,
        node_kind: node_kind_label(&node.kind),
        x: node.layout.x,
        y: node.layout.y,
        width: node.layout.width,
        height: node.layout.height,
        requested_cells,
        clipped_cells,
    });
}

fn node_kind_label(kind: &NodeKindView) -> &'static str {
    match kind {
        NodeKindView::Box => "box",
        NodeKindView::Text { .. } => "text",
        NodeKindView::Input { .. } => "input",
    }
}

fn resolve_rgb(rgb_cache: &mut RgbCache, color: Color, profile: &mut PaintProfile) -> Rgb {
    let resolve_start = profile.enabled.then(Instant::now);
    let rgb = rgb_cache.resolve(color);
    if let Some(start) = resolve_start {
        profile.rgb_resolve_time += start.elapsed();
        profile.rgb_resolves += 1;
    }
    rgb
}

struct InputCursorPaint<'a> {
    value: &'a str,
    cursor: usize,
    multiline: bool,
    mask: Option<char>,
    scroll_x: u16,
    scroll_y: u16,
}

fn input_cursor_metadata(
    grid: &mut Grid,
    node: &PaintEntry,
    input: InputCursorPaint<'_>,
    color: Rgb,
) -> Option<RenderCursor> {
    if node.layout.width == 0 || node.layout.height == 0 {
        return None;
    }

    let cursor = clamp_to_grapheme_boundary(input.value, input.cursor);
    let position = input_cursor_position(input.value, cursor, input.multiline, input.mask);
    let x = position.x - i32::from(input.scroll_x);
    let y = position.y - i32::from(input.scroll_y);
    let input_clipped =
        x < 0 || y < 0 || y >= i32::from(node.layout.height) || x >= i32::from(node.layout.width);

    let screen_x = node.layout.x + x;
    let screen_y = node.layout.y + y;
    let screen_clipped = screen_x < 0
        || screen_y < 0
        || screen_x >= i32::from(grid.width)
        || screen_y >= i32::from(grid.height);
    let visible = !input_clipped && !screen_clipped;
    if visible && node.resolved.cursor_shape == CursorShape::Block {
        invert_cursor_cell(grid, screen_x, screen_y, color);
    }

    Some(RenderCursor {
        x: screen_x,
        y: screen_y,
        shape: node.resolved.cursor_shape,
        color,
        visible,
    })
}

fn invert_cursor_cell(grid: &mut Grid, x: i32, y: i32, cursor_color: Rgb) {
    if x < 0 || y < 0 || x >= i32::from(grid.width) || y >= i32::from(grid.height) {
        return;
    }

    let cell = &mut grid.cells[y as usize][x as usize];
    let fg = cell.fg;
    let bg = cell.bg;
    cell.fg = bg;
    cell.bg = fg.or(Some(cursor_color));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorPosition {
    x: i32,
    y: i32,
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
    CursorPosition { x, y }
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
        paint(doc, grid, &mut rgb_cache, false, false);
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

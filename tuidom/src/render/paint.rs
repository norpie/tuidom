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
use crate::performance::{LargestFillProfile, PaintProfile};
use crate::render::RenderCursor;
use crate::render::grid::{Cell, Grid, GridRect};
use crate::style::color::{Rgb, RgbCache};
use crate::style::{Color, CursorShape};

/// Base cell state used to clear a frame before painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FrameClearBase {
    /// Empty terminal-default cells.
    #[default]
    Default,
    /// Empty cells with a shared background color.
    Background(Rgb),
}

/// DOM painting stage timings.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DomPaintStats {
    /// Base cell state used to clear the frame before painting.
    pub clear_base: FrameClearBase,
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
    let clear_result = if clear_grid {
        clear_grid_for_paint(grid, &entries, rgb_cache, &mut profile)
    } else {
        ClearGridResult::default()
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
            clear_result.skipped_background,
        ) {
            cursor = Some(entry_cursor);
        }
    }
    let paint_time = paint_start.elapsed();

    DomPaintOutput {
        stats: DomPaintStats {
            clear_base: clear_result.base,
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

    let cursor = match &node.kind {
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
    };

    // Every node kind can carry a border, and it is drawn over the node's own background:
    // layout already reserved its cells, so no content of this node contends for them.
    paint_border(grid, node, fg_rgb, alpha, rgb_cache, profile);

    cursor
}

fn paint_border(
    grid: &mut Grid,
    node: &PaintEntry,
    fg_rgb: Rgb,
    alpha: f64,
    rgb_cache: &mut RgbCache,
    profile: &mut PaintProfile,
) {
    let border = node.resolved.border;
    if !border.sides.any() {
        return;
    }

    // An unset border color follows the node's foreground, like CSS's `currentColor`.
    let color = match node.resolved.border_color {
        Some(color) => resolve_rgb(rgb_cache, color, profile),
        None => fg_rgb,
    };

    let border_start = profile.enabled.then(Instant::now);
    let glyphs = grid.write_border(
        GridRect {
            x: node.layout.x,
            y: node.layout.y,
            width: node.layout.width,
            height: node.layout.height,
        },
        border,
        Some(color),
        alpha,
    );
    if let Some(start) = border_start {
        profile.text_write_time += start.elapsed();
        profile.glyphs_written += glyphs;
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

    let content_rect = node.layout.content_rect(&node.resolved);
    let text_start = profile.enabled.then(Instant::now);
    let glyphs = grid.write_text_clipped(
        GridRect {
            x: content_rect.x,
            y: content_rect.y,
            width: content_rect.width,
            height: content_rect.height,
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

#[derive(Debug, Clone, Copy, Default)]
struct ClearGridResult {
    base: FrameClearBase,
    skipped_background: Option<NodeId>,
}

fn clear_grid_for_paint(
    grid: &mut Grid,
    entries: &[PaintEntry],
    rgb_cache: &mut RgbCache,
    profile: &mut PaintProfile,
) -> ClearGridResult {
    for entry in entries {
        let Some(background) = entry.resolved.background else {
            if entry_has_no_self_paint(entry) {
                continue;
            }
            grid.clear();
            return ClearGridResult::default();
        };

        if entry.resolved.opacity < 1.0 || !covers_grid(entry.layout, grid) {
            grid.clear();
            return ClearGridResult::default();
        }

        let bg = resolve_rgb(rgb_cache, background, profile);
        if bg.a < 255 {
            grid.clear();
            return ClearGridResult::default();
        }

        grid.clear_with_bg(bg);
        return ClearGridResult {
            base: FrameClearBase::Background(bg),
            skipped_background: Some(entry.id),
        };
    }

    grid.clear();
    ClearGridResult::default()
}

fn entry_has_no_self_paint(entry: &PaintEntry) -> bool {
    // A bordered node paints cells even with no background and no text, so it cannot be
    // skipped past when hoisting a later opaque background into the frame clear — the border
    // would then be painted after the clear, on top of a background that should cover it.
    if entry.resolved.border.sides.any() {
        return false;
    }

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
    let content_rect = node.layout.content_rect(&node.resolved);
    if content_rect.width == 0 || content_rect.height == 0 {
        return None;
    }

    let cursor = clamp_to_grapheme_boundary(input.value, input.cursor);
    let position = input_cursor_position(input.value, cursor, input.multiline, input.mask);
    let x = position.x - i32::from(input.scroll_x);
    let y = position.y - i32::from(input.scroll_y);
    let input_clipped =
        x < 0 || y < 0 || y >= i32::from(content_rect.height) || x >= i32::from(content_rect.width);

    let screen_x = content_rect.x + x;
    let screen_y = content_rect.y + y;
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

    grid.touch_span(y as usize, x as usize, x as usize + 1);
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
    use crate::style::{Border, BorderCharset, Color, Display, Length, Style};

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

    /// Paints the way the terminal renderer does: reusing a grid, which is the only path
    /// that runs the frame-clear fast path. `HeadlessRuntime` always paints into a fresh
    /// grid, so it never exercises this.
    fn paint_doc_clearing(doc: &Document, grid: &mut Grid) {
        let mut rgb_cache = RgbCache::new();
        paint(doc, grid, &mut rgb_cache, false, true);
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

    /// The frame-clear fast path hoists a later opaque full-screen background into the grid
    /// clear, skipping earlier entries that paint nothing. A bordered box paints something
    /// even with no background and no text — if it were skipped, its border would be painted
    /// after the clear, on top of the background that is supposed to cover it.
    #[test]
    fn bordered_entry_is_not_skipped_when_hoisting_a_background_into_the_frame_clear() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let bordered = doc.create_box().unwrap();
        let cover = doc.create_box().unwrap();
        doc.append_child(root, bordered).unwrap();
        doc.append_child(root, cover).unwrap();

        let mut border_style = Style::new();
        border_style.border(Border::new(BorderCharset::single()));
        doc.set_style(bordered, &border_style).unwrap();
        set_background(&doc, cover, Color::blue());

        let full = LayoutRect {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        };
        for node in [root, bordered, cover] {
            set_layout(&doc, node, full);
        }

        let mut grid = Grid::new(4, 3);
        paint_doc_clearing(&doc, &mut grid);

        // `cover` paints after `bordered`, so nothing of the border may survive.
        assert_eq!(row_text(&grid, 0), "    ");
        assert_eq!(row_text(&grid, 1), "    ");
        assert_eq!(row_text(&grid, 2), "    ");
        assert_eq!(grid.cells[0][0].bg, Some(rgb(0, 0, 255)));
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

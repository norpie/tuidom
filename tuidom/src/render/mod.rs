//! Renderer — orchestrates paint → diff → flush each frame.

pub(crate) mod diff;
pub(crate) mod grid;
pub(crate) mod paint;
mod terminal;

use std::io;
use std::time::Duration;

use terminal::Terminal;
pub(crate) use terminal::{flush_changes_into, flush_full_into, restore_for_panic};

use crate::document::Document;
use crate::lock;
use crate::performance::{DiffProfile, FlushMode, PaintProfile, RenderMetrics};
use crate::render::paint::FrameClearBase;
use crate::style::CursorShape;
use crate::style::color::{Rgb, RgbCache};

/// Timings for backend-neutral grid rendering.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GridRenderStats {
    /// Time spent creating the frame grid.
    pub grid_time: Duration,
    /// Time spent collecting the visible DOM tree into a paintable snapshot.
    pub dom_collect_time: Duration,
    /// Time spent painting DOM nodes into the grid.
    pub dom_paint_time: Duration,
    /// Detailed instrumentation for DOM painting.
    pub paint_profile: PaintProfile,
}

/// Cursor metadata produced by rendering a focused input node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderCursor {
    /// Screen x coordinate in terminal cells.
    pub x: i32,
    /// Screen y coordinate in terminal cells.
    pub y: i32,
    /// Cursor shape requested by style.
    pub shape: CursorShape,
    /// Cursor color derived from the focused node's resolved foreground color.
    pub color: Rgb,
    /// Whether the cursor should be shown after input/layout clipping.
    pub visible: bool,
}

/// Backend-neutral render metadata for an existing frame grid.
#[derive(Debug)]
pub(crate) struct RenderGridOutput {
    /// Optional cursor metadata for the frame.
    pub cursor: Option<RenderCursor>,
    /// Base cell state used to clear the frame before painting.
    pub clear_base: FrameClearBase,
    /// Timings for backend-neutral grid rendering.
    pub stats: GridRenderStats,
}

/// Paint a laid-out document into an existing grid without flushing to a terminal.
pub(crate) fn render_into_grid(
    doc: &Document,
    grid: &mut grid::Grid,
    rgb_cache: &mut RgbCache,
) -> RenderGridOutput {
    render_grid(doc, grid, rgb_cache, true)
}

fn render_grid(
    doc: &Document,
    grid: &mut grid::Grid,
    rgb_cache: &mut RgbCache,
    clear_grid: bool,
) -> RenderGridOutput {
    let instrument_paint = lock::mutex(&doc.inner.performance).detail().is_detailed();
    let dom_output = paint::paint(doc, grid, rgb_cache, instrument_paint, clear_grid);

    RenderGridOutput {
        cursor: dom_output.cursor,
        clear_base: dom_output.stats.clear_base,
        stats: GridRenderStats {
            grid_time: dom_output.stats.grid_time,
            dom_collect_time: dom_output.stats.collect_time,
            dom_paint_time: dom_output.stats.paint_time,
            paint_profile: dom_output.stats.profile,
        },
    }
}

const FULL_REDRAW_CHANGE_THRESHOLD_NUMERATOR: usize = 1;
const FULL_REDRAW_CHANGE_THRESHOLD_DENOMINATOR: usize = 3;

pub(crate) fn should_flush_full(changed_cells: usize, total_cells: usize) -> bool {
    total_cells > 0
        && changed_cells.saturating_mul(FULL_REDRAW_CHANGE_THRESHOLD_DENOMINATOR)
            > total_cells.saturating_mul(FULL_REDRAW_CHANGE_THRESHOLD_NUMERATOR)
}

/// Orchestrates the render pipeline: paint, diff, and flush to terminal.
pub(crate) struct Renderer {
    terminal: Terminal,
    old_grid: grid::Grid,
    new_grid: grid::Grid,
    old_clear_base: FrameClearBase,
    rgb_cache: RgbCache,
}

impl Renderer {
    /// Create a new renderer with screen-sized grids.
    pub fn new(width: u16, height: u16) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new()?,
            old_grid: grid::Grid::new(width, height),
            new_grid: grid::Grid::new(width, height),
            old_clear_base: FrameClearBase::default(),
            rgb_cache: RgbCache::new(),
        })
    }

    /// Render a single frame: layout (already done), paint, diff, flush.
    pub fn render_frame(&mut self, doc: &Document) -> io::Result<RenderMetrics> {
        let output = render_into_grid(doc, &mut self.new_grid, &mut self.rgb_cache);
        let grid_stats = output.stats;
        let cursor = output.cursor;

        let clear_base_changed = self.old_clear_base != output.clear_base;
        let dirty_spans = (!clear_base_changed)
            .then(|| (self.old_grid.touched_spans(), self.new_grid.touched_spans()));
        let instrument_diff = lock::mutex(&doc.inner.performance).detail().is_detailed();
        let diff_start = std::time::Instant::now();
        let diff_output = diff::diff_profiled_with_hints(
            &self.old_grid,
            &self.new_grid,
            instrument_diff,
            dirty_spans,
        );
        let diff_time = diff_start.elapsed();
        let changes = diff_output.changes;

        let total_cells = (self.new_grid.width as usize) * (self.new_grid.height as usize);
        let full_redraw = should_flush_full(changes.len(), total_cells);
        let flush_start = std::time::Instant::now();
        if full_redraw {
            self.terminal.flush_full(&self.new_grid, cursor)?;
        } else {
            self.terminal.flush_changes(&changes, cursor)?;
        }
        let flush_time = flush_start.elapsed();
        let cells_flushed = if full_redraw {
            total_cells
        } else {
            changes.len()
        };
        let flush_mode = if full_redraw {
            FlushMode::FullRedraw
        } else {
            FlushMode::Changes
        };

        std::mem::swap(&mut self.old_grid, &mut self.new_grid);
        self.old_clear_base = output.clear_base;

        Ok(RenderMetrics {
            diff_dirty_cells: changes.len(),
            cells_flushed,
            flush_mode,
            grid_time: grid_stats.grid_time,
            dom_collect_time: grid_stats.dom_collect_time,
            dom_paint_time: grid_stats.dom_paint_time,
            paint_profile: grid_stats.paint_profile,
            diff_time,
            diff_profile: diff_output.profile,
            flush_time,
        })
    }

    /// Handle terminal resize — clears and full-redraws the screen.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.old_grid = grid::Grid::new(width, height);
        self.new_grid = grid::Grid::new(width, height);
        self.old_clear_base = FrameClearBase::default();
    }

    /// Render a full-screen redraw (e.g. after resize) — skips diffing.
    pub fn render_full(&mut self, doc: &Document) -> io::Result<RenderMetrics> {
        let output = render_into_grid(doc, &mut self.new_grid, &mut self.rgb_cache);
        let grid_stats = output.stats;
        let cursor = output.cursor;

        let flush_start = std::time::Instant::now();
        self.terminal.flush_full(&self.new_grid, cursor)?;
        let flush_time = flush_start.elapsed();

        let cells = (self.new_grid.width as usize) * (self.new_grid.height as usize);
        std::mem::swap(&mut self.old_grid, &mut self.new_grid);
        self.old_clear_base = output.clear_base;

        Ok(RenderMetrics {
            diff_dirty_cells: cells,
            cells_flushed: cells,
            flush_mode: FlushMode::FullRedraw,
            grid_time: grid_stats.grid_time,
            dom_collect_time: grid_stats.dom_collect_time,
            dom_paint_time: grid_stats.dom_paint_time,
            paint_profile: grid_stats.paint_profile,
            diff_time: Duration::ZERO,
            diff_profile: DiffProfile::default(),
            flush_time,
        })
    }
}

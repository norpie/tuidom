//! Renderer — orchestrates paint → diff → flush each frame.

pub(crate) mod diff;
pub(crate) mod grid;
mod paint;
mod terminal;

use std::io;
use std::time::Duration;

use terminal::Terminal;

use crate::document::Document;
use crate::lock;
use crate::style::CursorShape;
use crate::style::color::{Rgb, RgbCache};

/// Breakdown of time spent in each render phase.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RenderStats {
    /// Number of cells flushed.
    pub cells_changed: usize,
    /// Whether this frame was a full redraw instead of a diffed update.
    pub full_redraw: bool,
    /// Time spent creating/clearing the frame grid.
    pub grid_time: Duration,
    /// Time spent collecting the visible DOM tree into a paintable snapshot.
    pub dom_collect_time: Duration,
    /// Time spent painting DOM nodes into the grid.
    pub dom_paint_time: Duration,
    /// Detailed instrumentation for DOM painting.
    pub paint_profile: paint::PaintProfile,
    /// Time spent painting the debug overlay into the grid.
    pub overlay_paint_time: Duration,
    /// Time spent diffing (old vs new).
    pub diff_time: Duration,
    /// Time spent flushing to terminal.
    pub flush_time: Duration,
}

impl RenderStats {
    /// Total time spent inside the renderer for this frame.
    pub fn render_time(self) -> Duration {
        self.grid_time
            + self.dom_collect_time
            + self.dom_paint_time
            + self.overlay_paint_time
            + self.diff_time
            + self.flush_time
    }
}

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
    pub paint_profile: paint::PaintProfile,
    /// Time spent painting the debug overlay into the grid.
    pub overlay_paint_time: Duration,
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

/// Backend-neutral render output for one frame.
#[derive(Debug)]
pub(crate) struct RenderFrame {
    /// Painted terminal cell grid.
    pub grid: grid::Grid,
    /// Optional cursor metadata for the frame.
    pub cursor: Option<RenderCursor>,
    /// Timings for backend-neutral grid rendering.
    pub stats: GridRenderStats,
}

/// Paint a laid-out document into a fresh frame without flushing to a terminal.
pub(crate) fn render_to_grid(
    doc: &Document,
    width: u16,
    height: u16,
    rgb_cache: &mut RgbCache,
) -> RenderFrame {
    let grid_start = std::time::Instant::now();
    let mut grid = grid::Grid::new(width, height);
    let grid_time = grid_start.elapsed();

    let instrument_paint = lock::mutex(&doc.inner.debug_overlay).enabled;
    let dom_output = paint::paint(doc, &mut grid, rgb_cache, instrument_paint);

    let mut overlay_paint_time = Duration::ZERO;
    {
        let overlay = lock::mutex(&doc.inner.debug_overlay);
        if overlay.enabled {
            let overlay_paint_start = std::time::Instant::now();
            overlay.render(&mut grid);
            overlay_paint_time = overlay_paint_start.elapsed();
        }
    }

    RenderFrame {
        grid,
        cursor: dom_output.cursor,
        stats: GridRenderStats {
            grid_time,
            dom_collect_time: dom_output.stats.collect_time,
            dom_paint_time: dom_output.stats.paint_time,
            paint_profile: dom_output.stats.profile,
            overlay_paint_time,
        },
    }
}

/// Orchestrates the render pipeline: paint, diff, and flush to terminal.
pub(crate) struct Renderer {
    terminal: Terminal,
    old_grid: grid::Grid,
    new_grid: grid::Grid,
    rgb_cache: RgbCache,
}

impl Renderer {
    /// Create a new renderer with screen-sized grids.
    pub fn new(width: u16, height: u16) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new()?,
            old_grid: grid::Grid::new(width, height),
            new_grid: grid::Grid::new(width, height),
            rgb_cache: RgbCache::new(),
        })
    }

    /// Render a single frame: layout (already done), paint, diff, flush.
    pub fn render_frame(&mut self, doc: &Document) -> io::Result<RenderStats> {
        let frame = render_to_grid(
            doc,
            self.old_grid.width,
            self.old_grid.height,
            &mut self.rgb_cache,
        );
        let grid_stats = frame.stats;
        let cursor = frame.cursor;
        self.new_grid = frame.grid;

        let diff_start = std::time::Instant::now();
        let changes = diff::diff(&self.old_grid, &self.new_grid);
        let diff_time = diff_start.elapsed();

        let flush_start = std::time::Instant::now();
        self.terminal.flush_changes(&changes, cursor)?;
        let flush_time = flush_start.elapsed();

        std::mem::swap(&mut self.old_grid, &mut self.new_grid);

        Ok(RenderStats {
            cells_changed: changes.len(),
            full_redraw: false,
            grid_time: grid_stats.grid_time,
            dom_collect_time: grid_stats.dom_collect_time,
            dom_paint_time: grid_stats.dom_paint_time,
            paint_profile: grid_stats.paint_profile,
            overlay_paint_time: grid_stats.overlay_paint_time,
            diff_time,
            flush_time,
        })
    }

    /// Handle terminal resize — clears and full-redraws the screen.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.old_grid = grid::Grid::new(width, height);
        self.new_grid = grid::Grid::new(width, height);
    }

    /// Render a full-screen redraw (e.g. after resize) — skips diffing.
    pub fn render_full(&mut self, doc: &Document) -> io::Result<RenderStats> {
        let frame = render_to_grid(
            doc,
            self.old_grid.width,
            self.old_grid.height,
            &mut self.rgb_cache,
        );
        let grid_stats = frame.stats;
        let cursor = frame.cursor;
        self.new_grid = frame.grid;

        let flush_start = std::time::Instant::now();
        self.terminal.flush_full(&self.new_grid, cursor)?;
        let flush_time = flush_start.elapsed();

        let cells = (self.new_grid.width as usize) * (self.new_grid.height as usize);
        std::mem::swap(&mut self.old_grid, &mut self.new_grid);

        Ok(RenderStats {
            cells_changed: cells,
            full_redraw: true,
            grid_time: grid_stats.grid_time,
            dom_collect_time: grid_stats.dom_collect_time,
            dom_paint_time: grid_stats.dom_paint_time,
            paint_profile: grid_stats.paint_profile,
            overlay_paint_time: grid_stats.overlay_paint_time,
            diff_time: Duration::ZERO,
            flush_time,
        })
    }
}

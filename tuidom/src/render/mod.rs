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

/// Breakdown of time spent in each render phase.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RenderStats {
    /// Number of cells flushed.
    pub cells_changed: usize,
    /// Whether this frame was a full redraw instead of a diffed update.
    pub full_redraw: bool,
    /// Time spent creating/clearing the frame grid.
    pub grid_time: Duration,
    /// Time spent painting DOM nodes into the grid.
    pub dom_paint_time: Duration,
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
            + self.dom_paint_time
            + self.overlay_paint_time
            + self.diff_time
            + self.flush_time
    }
}

/// Orchestrates the render pipeline: paint, diff, and flush to terminal.
pub(crate) struct Renderer {
    terminal: Terminal,
    old_grid: grid::Grid,
    new_grid: grid::Grid,
}

impl Renderer {
    /// Create a new renderer with screen-sized grids.
    pub fn new(width: u16, height: u16) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new()?,
            old_grid: grid::Grid::new(width, height),
            new_grid: grid::Grid::new(width, height),
        })
    }

    /// Render a single frame: layout (already done), paint, diff, flush.
    pub fn render_frame(&mut self, doc: &Document) -> io::Result<RenderStats> {
        // 1. Create/clear frame grid
        let grid_start = std::time::Instant::now();
        self.new_grid = grid::Grid::new(self.old_grid.width, self.old_grid.height);
        let grid_time = grid_start.elapsed();

        // 2. Paint DOM
        let dom_paint_start = std::time::Instant::now();
        paint::paint(doc, &mut self.new_grid);
        let dom_paint_time = dom_paint_start.elapsed();

        // 3. Paint debug overlay
        let mut overlay_paint_time = Duration::ZERO;
        {
            let overlay = lock::mutex(&doc.inner.debug_overlay);
            if overlay.enabled {
                let overlay_paint_start = std::time::Instant::now();
                overlay.render(&mut self.new_grid);
                overlay_paint_time = overlay_paint_start.elapsed();
            }
        }

        // 4. Diff
        let diff_start = std::time::Instant::now();
        let changes = diff::diff(&self.old_grid, &self.new_grid);
        let diff_time = diff_start.elapsed();

        // 5. Flush
        let flush_start = std::time::Instant::now();
        self.terminal.flush_changes(&changes)?;
        let flush_time = flush_start.elapsed();

        // 6. Swap grids for next frame
        std::mem::swap(&mut self.old_grid, &mut self.new_grid);

        Ok(RenderStats {
            cells_changed: changes.len(),
            full_redraw: false,
            grid_time,
            dom_paint_time,
            overlay_paint_time,
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
        let grid_start = std::time::Instant::now();
        self.new_grid = grid::Grid::new(self.old_grid.width, self.old_grid.height);
        let grid_time = grid_start.elapsed();

        let dom_paint_start = std::time::Instant::now();
        paint::paint(doc, &mut self.new_grid);
        let dom_paint_time = dom_paint_start.elapsed();

        let mut overlay_paint_time = Duration::ZERO;
        {
            let overlay = lock::mutex(&doc.inner.debug_overlay);
            if overlay.enabled {
                let overlay_paint_start = std::time::Instant::now();
                overlay.render(&mut self.new_grid);
                overlay_paint_time = overlay_paint_start.elapsed();
            }
        }

        let flush_start = std::time::Instant::now();
        self.terminal.flush_full(&self.new_grid)?;
        let flush_time = flush_start.elapsed();

        let cells = (self.new_grid.width as usize) * (self.new_grid.height as usize);
        std::mem::swap(&mut self.old_grid, &mut self.new_grid);

        Ok(RenderStats {
            cells_changed: cells,
            full_redraw: true,
            grid_time,
            dom_paint_time,
            overlay_paint_time,
            diff_time: Duration::ZERO,
            flush_time,
        })
    }
}

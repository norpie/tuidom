//! Renderer — orchestrates paint → diff → flush each frame.

pub(crate) mod diff;
pub(crate) mod grid;
mod paint;
mod terminal;

use std::io;

use terminal::Terminal;

use crate::document::Document;

/// Orchestrates the render pipeline: paint, diff, and flush to terminal.
pub(crate) struct Renderer {
    terminal: Terminal,
    old_grid: grid::Grid,
    new_grid: grid::Grid,
}

impl Renderer {
    /// Create a new renderer with screen-sized grids.
    ///
    /// Initializes the terminal (alternate screen, raw mode, hidden cursor).
    pub fn new(width: u16, height: u16) -> io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new()?,
            old_grid: grid::Grid::new(width, height),
            new_grid: grid::Grid::new(width, height),
        })
    }

    /// Render a single frame: layout (already done), paint, diff, flush.
    ///
    /// Returns the number of cells that changed (for debug metrics).
    pub fn render_frame(&mut self, doc: &Document) -> io::Result<usize> {
        // 1. Paint current DOM state into new grid
        self.new_grid = grid::Grid::new(self.old_grid.width, self.old_grid.height);
        paint::paint(doc, &mut self.new_grid);

        // 2. Paint debug overlay on top if enabled
        {
            let overlay = doc.inner.debug_overlay.lock().unwrap();
            if overlay.enabled {
                overlay.render(&mut self.new_grid);
            }
        }

        // 3. Diff against previous frame
        let changes = diff::diff(&self.old_grid, &self.new_grid);
        let cells_changed = changes.len();

        // 4. Send only changes to terminal
        self.terminal.flush_changes(&changes)?;

        // 5. Swap grids for next frame
        std::mem::swap(&mut self.old_grid, &mut self.new_grid);

        Ok(cells_changed)
    }

    /// Handle terminal resize.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.old_grid.resize(width, height);
        self.new_grid.resize(width, height);
    }
}

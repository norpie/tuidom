//! Cell buffer — the 2D grid representing the virtual screen.

use crate::style::color::Rgb;

/// A single terminal character position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cell {
    /// The displayed character.
    pub ch: char,
    /// Foreground color. `None` means terminal default.
    pub fg: Option<Rgb>,
    /// Background color. `None` means terminal default.
    pub bg: Option<Rgb>,
}

impl Cell {
    /// Create an empty cell (space, no colors).
    pub fn empty() -> Self {
        Self {
            ch: ' ',
            fg: None,
            bg: None,
        }
    }

    /// Check if this cell differs from another (for diffing).
    pub fn differs_from(&self, other: &Self) -> bool {
        self.ch != other.ch || self.fg != other.fg || self.bg != other.bg
    }
}

/// A 2D buffer of [`Cell`]s representing a single frame's screen state.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    /// Row-major cell storage: cells[row][col].
    pub cells: Vec<Vec<Cell>>,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

impl Grid {
    /// Create a new grid filled with empty cells.
    pub fn new(width: u16, height: u16) -> Self {
        let cells = vec![vec![Cell::empty(); width as usize]; height as usize];
        Self {
            cells,
            width,
            height,
        }
    }

    /// Resize the grid, preserving existing content where possible.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells.resize(height as usize, vec![Cell::empty(); width as usize]);
        for row in self.cells.iter_mut() {
            row.resize(width as usize, Cell::empty());
        }
    }

    /// Set a cell at the given position. Silently ignores out-of-bounds.
    pub fn put(&mut self, x: u16, y: u16, cell: Cell) {
        if x < self.width && y < self.height {
            self.cells[y as usize][x as usize] = cell;
        }
    }

    /// Fill a rectangular region with a cell value.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, cell: Cell) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for row in y..y_end {
            for col in x..x_end {
                self.cells[row as usize][col as usize] = cell;
            }
        }
    }

    /// Write text at a position. Truncates at grid edge.
    pub fn write_text(&mut self, x: u16, y: u16, text: &str, fg: Option<Rgb>, bg: Option<Rgb>) {
        for (i, ch) in text.chars().enumerate() {
            if x + i as u16 >= self.width {
                break;
            }
            let cell = Cell { ch, fg, bg };
            self.put(x + i as u16, y, cell);
        }
    }
}

//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};

use crate::render::diff::CellChange;
use crate::render::grid::Grid;
use crate::style::color::Rgb;

/// Wraps stdout with crossterm setup and teardown.
pub(crate) struct Terminal {
    stdout: Stdout,
}

impl Terminal {
    /// Initialize the terminal: raw mode, alternate screen, hide cursor.
    pub fn new() -> io::Result<Self> {
        let mut stdout = io::stdout();
        enable_raw_mode()?;
        queue!(stdout, EnterAlternateScreen, Hide)?;
        stdout.flush()?;
        Ok(Self { stdout })
    }

    /// Flush only the changed cells to the terminal.
    pub fn flush_changes(&mut self, changes: &[CellChange]) -> io::Result<()> {
        for change in changes {
            queue_cell(&mut self.stdout, change.x, change.y, &change.cell)?;
        }
        self.stdout.flush()
    }

    /// Flush the entire grid — used on resize.
    ///
    /// Batches all commands with `queue!` then flushes once.
    pub fn flush_full(&mut self, grid: &Grid) -> io::Result<()> {
        use crossterm::terminal::{Clear, ClearType};
        queue!(self.stdout, Clear(ClearType::All))?;

        for (y, row) in grid.cells.iter().enumerate() {
            let y = y as u16;
            let mut x = 0u16;
            while x < grid.width {
                let cell = &row[x as usize];
                queue!(
                    self.stdout,
                    MoveTo(x, y),
                    SetForegroundColor(to_crossterm_color(cell.fg)),
                    SetBackgroundColor(to_crossterm_color(cell.bg)),
                )?;

                // Find run of same-style cells
                let run_start = x;
                while x < grid.width
                    && row[x as usize].fg == cell.fg
                    && row[x as usize].bg == cell.bg
                {
                    x += 1;
                }
                let run_text: String =
                    (run_start..x).map(|i| row[i as usize].ch).collect();
                queue!(self.stdout, Print(run_text))?;
            }
        }
        self.stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = queue!(self.stdout, Show, LeaveAlternateScreen);
        let _ = self.stdout.flush();
        let _ = disable_raw_mode();
    }
}

/// Queue a single cell to stdout (no flush).
fn queue_cell(stdout: &mut Stdout, x: u16, y: u16, cell: &crate::render::grid::Cell) -> io::Result<()> {
    queue!(
        stdout,
        MoveTo(x, y),
        SetForegroundColor(to_crossterm_color(cell.fg)),
        SetBackgroundColor(to_crossterm_color(cell.bg)),
        Print(cell.ch),
    )
}

/// Convert optional [`Rgb`] to a crossterm color.
fn to_crossterm_color(color: Option<Rgb>) -> crossterm::style::Color {
    match color {
        Some(rgb) => crossterm::style::Color::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
        None => crossterm::style::Color::Reset,
    }
}

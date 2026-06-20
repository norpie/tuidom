//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::render::diff::CellChange;
use crate::render::grid::Cell;
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
                if cell.is_wide_continuation() {
                    x += 1;
                    continue;
                }

                queue!(
                    self.stdout,
                    MoveTo(x, y),
                    SetForegroundColor(to_crossterm_color(cell.fg)),
                    SetBackgroundColor(to_crossterm_color(cell.bg)),
                )?;

                let fg = cell.fg;
                let bg = cell.bg;
                let mut run_text = String::new();
                while x < grid.width {
                    let cell = &row[x as usize];
                    if cell.is_wide_continuation() {
                        x += 1;
                        continue;
                    }
                    if cell.fg != fg || cell.bg != bg {
                        break;
                    }

                    run_text.push_str(cell.terminal_text());
                    x = x.saturating_add(cell.content_width() as u16);
                }

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
fn queue_cell(stdout: &mut Stdout, x: u16, y: u16, cell: &Cell) -> io::Result<()> {
    if cell.is_wide_continuation() {
        return Ok(());
    }

    queue!(
        stdout,
        MoveTo(x, y),
        SetForegroundColor(to_crossterm_color(cell.fg)),
        SetBackgroundColor(to_crossterm_color(cell.bg)),
        Print(cell.terminal_text()),
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

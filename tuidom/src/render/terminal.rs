//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::execute;
use crossterm::style::{Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};

use crate::render::diff::CellChange;
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
        execute!(stdout, EnterAlternateScreen, Hide)?;
        Ok(Self { stdout })
    }

    /// Flush only the changed cells to the terminal.
    pub fn flush_changes(&mut self, changes: &[CellChange]) -> io::Result<()> {
        for change in changes {
            let fg = to_crossterm_color(change.cell.fg);
            let bg = to_crossterm_color(change.cell.bg);

            execute!(
                self.stdout,
                MoveTo(change.x, change.y),
                SetForegroundColor(fg),
                SetBackgroundColor(bg),
                Print(change.cell.ch),
            )?;
        }
        self.stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
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

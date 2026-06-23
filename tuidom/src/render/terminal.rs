//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
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
        let mut setup = TerminalSetup::new(io::stdout());
        setup.enable_raw_mode()?;
        setup.enter_alternate_screen()?;
        setup.hide_cursor()?;

        Ok(Self {
            stdout: setup.finish()?,
        })
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
        restore_terminal(&mut self.stdout, true, true, true);
    }
}

struct TerminalSetup {
    stdout: Option<Stdout>,
    raw_mode_enabled: bool,
    alternate_screen_entered: bool,
    cursor_hidden: bool,
}

impl TerminalSetup {
    fn new(stdout: Stdout) -> Self {
        Self {
            stdout: Some(stdout),
            raw_mode_enabled: false,
            alternate_screen_entered: false,
            cursor_hidden: false,
        }
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        self.raw_mode_enabled = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, EnterAlternateScreen)?;
        self.alternate_screen_entered = true;
        self.stdout()?.flush()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, Hide)?;
        self.cursor_hidden = true;
        self.stdout()?.flush()
    }

    fn stdout(&mut self) -> io::Result<&mut Stdout> {
        self.stdout
            .as_mut()
            .ok_or_else(|| io::Error::other("terminal setup missing stdout"))
    }

    fn finish(mut self) -> io::Result<Stdout> {
        self.raw_mode_enabled = false;
        self.alternate_screen_entered = false;
        self.cursor_hidden = false;
        self.stdout
            .take()
            .ok_or_else(|| io::Error::other("terminal setup missing stdout"))
    }
}

impl Drop for TerminalSetup {
    fn drop(&mut self) {
        if let Some(stdout) = self.stdout.as_mut() {
            restore_terminal(
                stdout,
                self.cursor_hidden,
                self.alternate_screen_entered,
                self.raw_mode_enabled,
            );
        }
    }
}

fn restore_terminal(
    stdout: &mut Stdout,
    cursor_hidden: bool,
    alternate_screen_entered: bool,
    raw_mode_enabled: bool,
) {
    if cursor_hidden {
        let _ = queue!(stdout, Show);
    }
    if alternate_screen_entered {
        let _ = queue!(stdout, LeaveAlternateScreen);
    }
    let _ = queue!(stdout, ResetColor, SetAttribute(Attribute::Reset));
    let _ = stdout.flush();
    if raw_mode_enabled {
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

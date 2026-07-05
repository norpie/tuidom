//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::queue;
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::render::RenderCursor;
use crate::render::diff::CellChange;
use crate::render::grid::Cell;
use crate::render::grid::Grid;
use crate::style::CursorShape;
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
        setup.enable_mouse_capture()?;
        setup.hide_cursor()?;

        Ok(Self {
            stdout: setup.finish()?,
        })
    }

    /// Flush only the changed cells to the terminal.
    pub fn flush_changes(
        &mut self,
        changes: &[CellChange],
        cursor: Option<RenderCursor>,
    ) -> io::Result<()> {
        queue!(self.stdout, Hide)?;
        for change in changes {
            queue_cell(&mut self.stdout, change.x, change.y, &change.cell)?;
        }
        queue_cursor(&mut self.stdout, cursor)?;
        self.stdout.flush()
    }

    /// Flush the entire grid — used on resize.
    ///
    /// Batches all commands with `queue!` then flushes once.
    pub fn flush_full(&mut self, grid: &Grid, cursor: Option<RenderCursor>) -> io::Result<()> {
        use crossterm::terminal::{Clear, ClearType};
        queue!(self.stdout, Hide, Clear(ClearType::All))?;

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
        queue_cursor(&mut self.stdout, cursor)?;
        self.stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore_terminal(&mut self.stdout, true, true, true, true);
    }
}

struct TerminalSetup {
    stdout: Option<Stdout>,
    raw_mode_enabled: bool,
    alternate_screen_entered: bool,
    cursor_hidden: bool,
    mouse_capture_enabled: bool,
}

impl TerminalSetup {
    fn new(stdout: Stdout) -> Self {
        Self {
            stdout: Some(stdout),
            raw_mode_enabled: false,
            alternate_screen_entered: false,
            cursor_hidden: false,
            mouse_capture_enabled: false,
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

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, EnableMouseCapture)?;
        self.mouse_capture_enabled = true;
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
        self.mouse_capture_enabled = false;
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
                self.mouse_capture_enabled,
                self.alternate_screen_entered,
                self.raw_mode_enabled,
            );
        }
    }
}

fn restore_terminal(
    stdout: &mut Stdout,
    cursor_hidden: bool,
    mouse_capture_enabled: bool,
    alternate_screen_entered: bool,
    raw_mode_enabled: bool,
) {
    if cursor_hidden {
        let _ = queue!(stdout, Show);
    }
    if mouse_capture_enabled {
        let _ = queue!(stdout, DisableMouseCapture);
    }
    if alternate_screen_entered {
        let _ = queue!(stdout, LeaveAlternateScreen);
    }
    let _ = queue!(
        stdout,
        SetCursorStyle::DefaultUserShape,
        Print("\x1b]112\x07"),
        ResetColor,
        SetAttribute(Attribute::Reset)
    );
    let _ = stdout.flush();
    if raw_mode_enabled {
        let _ = disable_raw_mode();
    }
}

fn queue_cursor(stdout: &mut Stdout, cursor: Option<RenderCursor>) -> io::Result<()> {
    let Some(cursor) = cursor else {
        queue!(stdout, Hide)?;
        return Ok(());
    };

    if !cursor.visible || cursor.x < 0 || cursor.y < 0 {
        queue!(stdout, Hide)?;
        return Ok(());
    }

    let Ok(x) = u16::try_from(cursor.x) else {
        queue!(stdout, Hide)?;
        return Ok(());
    };
    let Ok(y) = u16::try_from(cursor.y) else {
        queue!(stdout, Hide)?;
        return Ok(());
    };
    if cursor.shape == CursorShape::Block {
        queue!(stdout, Hide)?;
        return Ok(());
    }

    queue!(
        stdout,
        cursor_style(cursor),
        Print(cursor_color_sequence(cursor.color)),
        MoveTo(x, y),
        Show
    )
}

fn cursor_color_sequence(color: Rgb) -> String {
    format!("\x1b]12;#{:02x}{:02x}{:02x}\x07", color.r, color.g, color.b)
}

fn cursor_style(cursor: RenderCursor) -> SetCursorStyle {
    match cursor.shape {
        CursorShape::Block => SetCursorStyle::SteadyBlock,
        CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
        CursorShape::Bar => SetCursorStyle::SteadyBar,
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

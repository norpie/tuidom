//! Terminal I/O — crossterm initialization, flush, and cleanup.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
};
use crossterm::queue;
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::panic::install_hook;
use crate::render::RenderCursor;
use crate::render::diff::CellChange;
use crate::render::grid::{Cell, CellAttrs, Grid};
use crate::style::CursorShape;
use crate::style::color::Rgb;

/// Terminal modes currently active on the real terminal.
///
/// Global because the panic hook has no handle to [`Terminal`] — it runs wherever
/// the panic did. Setup guards, the live terminal, and the hook all restore from
/// this one source of truth, and [`take_active`] hands the bits to exactly one
/// caller, so a panic followed by an unwinding `Drop` cannot restore twice.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

/// Whether a [`Terminal`] already owns the process's terminal.
///
/// [`ACTIVE`] describes the terminal, not any one owner, so a second `Terminal`
/// would share it: whichever dropped first would restore modes the other was
/// still using, and the survivor would render into a cooked normal screen. That
/// configuration cannot work anyway — two renderers diff against separate grids
/// and would fight over every cell — so it is refused rather than counted.
static INSTANCE_HELD: AtomicBool = AtomicBool::new(false);

const RAW_MODE: u8 = 1 << 0;
const ALTERNATE_SCREEN: u8 = 1 << 1;
const MOUSE_CAPTURE: u8 = 1 << 2;
const CURSOR_HIDDEN: u8 = 1 << 3;
const FOCUS_CHANGE: u8 = 1 << 4;

fn mark_active(flag: u8) {
    ACTIVE.fetch_or(flag, Ordering::SeqCst);
}

/// Claim the active modes, leaving none behind for a second restorer.
fn take_active() -> u8 {
    ACTIVE.swap(0, Ordering::SeqCst)
}

/// Wraps stdout with crossterm setup and teardown.
pub(crate) struct Terminal {
    stdout: Stdout,
}

impl Terminal {
    /// Initialize the terminal: raw mode, alternate screen, hide cursor.
    ///
    /// Fails if another [`Terminal`] is already live — one process drives one
    /// terminal, so a second concurrent `Document::run()` is refused here rather
    /// than left to corrupt the screen.
    pub fn new() -> io::Result<Self> {
        if INSTANCE_HELD
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(io::Error::other(
                "a terminal is already active; only one Document may run() per process",
            ));
        }

        // Installed before the first mode is turned on, so a panic partway through
        // setup is covered by the same restore path as a panic during the run.
        install_hook();

        let mut setup = TerminalSetup::new(io::stdout());
        // Every `?` here drops `setup`, restoring whatever was turned on; the
        // instance claim has to be released on those paths too.
        let started = (|| {
            setup.enable_raw_mode()?;
            setup.enter_alternate_screen()?;
            setup.enable_mouse_capture()?;
            setup.enable_focus_change()?;
            setup.hide_cursor()?;
            setup.finish()
        })();

        match started {
            Ok(stdout) => Ok(Self { stdout }),
            Err(err) => {
                INSTANCE_HELD.store(false, Ordering::SeqCst);
                Err(err)
            }
        }
    }

    /// Flush only the changed cells to the terminal.
    pub fn flush_changes(
        &mut self,
        changes: &[CellChange],
        cursor: Option<RenderCursor>,
        bell: bool,
    ) -> io::Result<()> {
        flush_changes_into(&mut self.stdout, changes, cursor, bell)
    }

    /// Flush the entire grid — used on resize.
    pub fn flush_full(
        &mut self,
        grid: &Grid,
        cursor: Option<RenderCursor>,
        bell: bool,
    ) -> io::Result<()> {
        flush_full_into(&mut self.stdout, grid, cursor, bell)
    }
}

/// Queue a pending bell ahead of a frame's own output.
///
/// First in the flush so the bell is not held hostage by however long the frame's
/// cell writes take, and so it still goes out when the frame writes nothing.
fn queue_bell<W: Write>(out: &mut W, bell: bool) -> io::Result<()> {
    if bell {
        queue!(out, Print("\x07"))?;
    }
    Ok(())
}

/// Encode a set of cell changes as terminal output into `out`.
///
/// Split from [`Terminal`] so the flush path can be driven against any writer —
/// a headless byte sink runs exactly the code a real terminal gets, which is
/// what makes the flush side measurable and assertable without a tty.
pub(crate) fn flush_changes_into<W: Write>(
    out: &mut W,
    changes: &[CellChange],
    cursor: Option<RenderCursor>,
    bell: bool,
) -> io::Result<()> {
    queue_bell(out, bell)?;
    // Attributes are sticky, so each flush starts from a known state and then emits only
    // transitions. `Attribute::Reset` also resets colors, which is harmless here because
    // every cell write re-specifies its own fg and bg.
    queue!(out, Hide, SetAttribute(Attribute::Reset))?;
    let mut attrs = CellAttrs::default();
    for change in changes {
        queue_cell(out, change.x, change.y, &change.cell, &mut attrs)?;
    }
    queue_cursor(out, cursor)?;
    out.flush()
}

/// Encode an entire grid as terminal output into `out`.
///
/// Batches all commands with `queue!` then flushes once.
pub(crate) fn flush_full_into<W: Write>(
    out: &mut W,
    grid: &Grid,
    cursor: Option<RenderCursor>,
    bell: bool,
) -> io::Result<()> {
    use crossterm::terminal::{Clear, ClearType};
    queue_bell(out, bell)?;
    queue!(
        out,
        Hide,
        Clear(ClearType::All),
        SetAttribute(Attribute::Reset)
    )?;
    let mut current_attrs = CellAttrs::default();

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
                out,
                MoveTo(x, y),
                SetForegroundColor(to_crossterm_color(cell.fg)),
                SetBackgroundColor(to_crossterm_color(cell.bg)),
            )?;

            let fg = cell.fg;
            let bg = cell.bg;
            let attrs = cell.attrs;
            queue_attr_transitions(out, &mut current_attrs, attrs)?;

            let mut run_text = String::new();
            while x < grid.width {
                let cell = &row[x as usize];
                if cell.is_wide_continuation() {
                    x += 1;
                    continue;
                }
                // A run is one span of identical colors *and* attributes — attributes stay
                // on until turned off, so a change has to break the run.
                if cell.fg != fg || cell.bg != bg || cell.attrs != attrs {
                    break;
                }

                run_text.push_str(cell.terminal_text());
                x = x.saturating_add(cell.content_width() as u16);
            }

            queue!(out, Print(run_text))?;
        }
    }
    queue_cursor(out, cursor)?;
    out.flush()
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore_terminal(&mut self.stdout);
        INSTANCE_HELD.store(false, Ordering::SeqCst);
    }
}

struct TerminalSetup {
    stdout: Option<Stdout>,
}

impl TerminalSetup {
    fn new(stdout: Stdout) -> Self {
        Self {
            stdout: Some(stdout),
        }
    }

    fn enable_raw_mode(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        mark_active(RAW_MODE);
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, EnterAlternateScreen)?;
        mark_active(ALTERNATE_SCREEN);
        self.stdout()?.flush()
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, EnableMouseCapture)?;
        mark_active(MOUSE_CAPTURE);
        self.stdout()?.flush()
    }

    /// Ask the terminal to report window focus changes.
    ///
    /// Terminals that do not support it ignore the sequence and simply never send
    /// the events, so this needs no capability check.
    fn enable_focus_change(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, EnableFocusChange)?;
        mark_active(FOCUS_CHANGE);
        self.stdout()?.flush()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout()?, Hide)?;
        mark_active(CURSOR_HIDDEN);
        self.stdout()?.flush()
    }

    fn stdout(&mut self) -> io::Result<&mut Stdout> {
        self.stdout
            .as_mut()
            .ok_or_else(|| io::Error::other("terminal setup missing stdout"))
    }

    /// Hand the initialized terminal over, disarming this guard.
    ///
    /// The active bitset deliberately stays set: those modes are still on, and
    /// undoing them passes to [`Terminal`]'s own `Drop`. Taking the stdout handle
    /// is what disarms the guard — with none left, its `Drop` restores nothing.
    fn finish(mut self) -> io::Result<Stdout> {
        self.stdout
            .take()
            .ok_or_else(|| io::Error::other("terminal setup missing stdout"))
    }
}

impl Drop for TerminalSetup {
    fn drop(&mut self) {
        if let Some(stdout) = self.stdout.as_mut() {
            restore_terminal(stdout);
        }
    }
}

/// Restore the terminal modes that are still active, exactly once.
fn restore_terminal(stdout: &mut Stdout) {
    let state = take_active();
    // Nothing was ever turned on — or another restorer already claimed it. Writing
    // resets to a terminal this process never touched would be scribbling on it.
    if state == 0 {
        return;
    }

    let _ = queue_restore(stdout, state);
    let _ = stdout.flush();
    if state & RAW_MODE != 0 {
        let _ = disable_raw_mode();
    }
}

/// Restore the terminal from the panic hook.
///
/// Takes a fresh stdout handle because the hook runs wherever the panic did, with
/// no access to the live [`Terminal`]. With no terminal set up the bitset is empty
/// and this writes nothing, which is what makes the hook safe to leave installed.
pub(crate) fn restore_for_panic() {
    restore_terminal(&mut io::stdout());
}

/// Queue the teardown sequence for the modes in `state`.
///
/// Split from [`restore_terminal`] so the emitted sequence can be asserted without
/// a tty, the same way the flush path is. Raw mode is absent by design: it is a
/// syscall rather than a sequence, so it stays with the caller.
fn queue_restore<W: Write>(out: &mut W, state: u8) -> io::Result<()> {
    if state & CURSOR_HIDDEN != 0 {
        queue!(out, Show)?;
    }
    if state & MOUSE_CAPTURE != 0 {
        queue!(out, DisableMouseCapture)?;
    }
    if state & FOCUS_CHANGE != 0 {
        queue!(out, DisableFocusChange)?;
    }
    if state & ALTERNATE_SCREEN != 0 {
        queue!(out, LeaveAlternateScreen)?;
    }
    queue!(
        out,
        SetCursorStyle::DefaultUserShape,
        Print("\x1b]112\x07"),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )
}

fn queue_cursor<W: Write>(stdout: &mut W, cursor: Option<RenderCursor>) -> io::Result<()> {
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
///
/// `current` is the attribute state the terminal is already in, and is updated as
/// transitions are emitted.
fn queue_cell<W: Write>(
    stdout: &mut W,
    x: u16,
    y: u16,
    cell: &Cell,
    current: &mut CellAttrs,
) -> io::Result<()> {
    if cell.is_wide_continuation() {
        return Ok(());
    }

    queue!(
        stdout,
        MoveTo(x, y),
        SetForegroundColor(to_crossterm_color(cell.fg)),
        SetBackgroundColor(to_crossterm_color(cell.bg)),
    )?;
    queue_attr_transitions(stdout, current, cell.attrs)?;
    queue!(stdout, Print(cell.terminal_text()))
}

/// Emit only the SGR changes between the terminal's current attribute state and `target`.
///
/// A blanket `Attribute::Reset` per cell is not an option: SGR 0 also resets colors, so it
/// would fight the color writes.
fn queue_attr_transitions<W: Write>(
    stdout: &mut W,
    current: &mut CellAttrs,
    target: CellAttrs,
) -> io::Result<()> {
    if target.bold != current.bold {
        let attr = if target.bold {
            Attribute::Bold
        } else {
            Attribute::NormalIntensity
        };
        queue!(stdout, SetAttribute(attr))?;
    }
    if target.italic != current.italic {
        let attr = if target.italic {
            Attribute::Italic
        } else {
            Attribute::NoItalic
        };
        queue!(stdout, SetAttribute(attr))?;
    }
    if target.underline != current.underline {
        let attr = if target.underline {
            Attribute::Underlined
        } else {
            Attribute::NoUnderline
        };
        queue!(stdout, SetAttribute(attr))?;
    }

    *current = target;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mode that shows up as an escape sequence. Raw mode is a syscall, so
    /// it is deliberately not part of what [`queue_restore`] emits.
    const SEQUENCED: [u8; 4] = [CURSOR_HIDDEN, MOUSE_CAPTURE, FOCUS_CHANGE, ALTERNATE_SCREEN];

    fn restore_bytes(state: u8) -> String {
        let mut out = Vec::new();
        queue_restore(&mut out, state).expect("writing to a Vec cannot fail");
        String::from_utf8(out).expect("restore sequence is valid utf-8")
    }

    #[test]
    fn each_active_mode_adds_its_own_teardown() {
        let tail = restore_bytes(0);
        for flag in SEQUENCED {
            let emitted = restore_bytes(flag);
            assert!(
                emitted.len() > tail.len(),
                "mode {flag:#06b} emitted no teardown"
            );
            assert!(
                emitted.ends_with(&tail),
                "the unconditional reset tail must come last, after mode {flag:#06b}"
            );
        }
    }

    #[test]
    fn inactive_modes_are_left_alone() {
        let all: u8 = SEQUENCED.iter().fold(0, |acc, flag| acc | flag);
        for flag in SEQUENCED {
            let without = restore_bytes(all & !flag);
            assert!(
                without.len() < restore_bytes(all).len(),
                "mode {flag:#06b} was torn down despite being inactive"
            );
        }
    }

    /// The one sequence worth pinning literally: failing to leave the alternate
    /// screen is what strands a user looking at a dead frame.
    #[test]
    fn leaving_the_alternate_screen_emits_its_sequence() {
        assert!(restore_bytes(ALTERNATE_SCREEN).contains("\x1b[?1049l"));
        assert!(!restore_bytes(CURSOR_HIDDEN).contains("\x1b[?1049l"));
    }

    #[test]
    fn raw_mode_alone_emits_only_the_reset_tail() {
        assert_eq!(restore_bytes(RAW_MODE), restore_bytes(0));
    }
}

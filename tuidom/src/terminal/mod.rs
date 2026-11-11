//! Every field here is a report. The engine draws the same way whatever it says.

mod env;
mod merge;
mod probe;
mod reply;
mod tmux;
#[cfg(unix)]
mod tty;

pub(crate) use probe::{pixel_size, probe};

use crate::style::ResolvedColor;

/// What is known about the terminal the document runs in.
///
/// Environment variables give a first guess, and terminal replies replace it where the
/// terminal answered. `Default` is the answer for an environment that says nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalInfo {
    /// The terminal emulator's name, such as `kitty` or `WezTerm`.
    ///
    /// Inside a multiplexer this is the terminal outside it, and `None` unless that terminal
    /// answered through the multiplexer: the environment inside one describes whichever
    /// terminal started it, which may not be the one attached now.
    pub name: Option<String>,
    /// The terminal emulator's version, as it reports it.
    pub version: Option<String>,
    /// The multiplexer between the application and the terminal, if any.
    pub multiplexer: Option<Multiplexer>,
    /// How many colors the terminal can show.
    pub color_depth: ColorDepth,
    /// Whether `NO_COLOR` is set. A preference the user stated, not a terminal capability.
    pub no_color: bool,
    /// Which image protocols the terminal supports.
    pub graphics: Graphics,
    /// The terminal's default foreground color, if it reported one.
    pub foreground: Option<ResolvedColor>,
    /// The terminal's default background color, if it reported one.
    pub background: Option<ResolvedColor>,
    /// The window and cell size in pixels, if the terminal reports it.
    pub pixel_size: Option<PixelSize>,
    /// Whether the session runs over SSH.
    pub remote: bool,
    /// Whether the terminal answered queries. `false` means every field came from the
    /// environment.
    pub probed: bool,
}

/// A terminal multiplexer between the application and the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multiplexer {
    /// Which multiplexer it is.
    pub kind: MultiplexerKind,
    /// Its version, as it reports it.
    pub version: Option<String>,
    /// Whether escape sequences wrapped for passthrough reach the outer terminal.
    ///
    /// Image protocols need it. tmux turns it off by default (`allow-passthrough`).
    pub passthrough: bool,
}

/// The multiplexers tuidom recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplexerKind {
    /// tmux.
    Tmux,
    /// GNU screen.
    Screen,
    /// Zellij.
    Zellij,
}

/// How many colors a terminal can show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorDepth {
    /// The 16 standard ANSI colors, assumed when nothing says more.
    #[default]
    Ansi16,
    /// The 256-color palette.
    Ansi256,
    /// 24-bit RGB.
    TrueColor,
}

/// The image protocols a terminal supports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Graphics {
    /// The kitty graphics protocol.
    pub kitty: bool,
    /// Sixel.
    pub sixel: bool,
    /// iTerm2's inline images (OSC 1337), which has no query, so it is known only by name.
    pub iterm2: bool,
}

/// The terminal window's size in pixels, and one cell's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    /// Window width in pixels.
    pub width: u16,
    /// Window height in pixels.
    pub height: u16,
    /// Cell width in pixels.
    pub cell_width: u16,
    /// Cell height in pixels.
    pub cell_height: u16,
}

/// Terminals whose name, from `TERM`, `TERM_PROGRAM` or XTVERSION, is enough to know they show
/// 24-bit color.
const TRUECOLOR_TERMINALS: &[&str] = &[
    "alacritty",
    "foot",
    "ghostty",
    "iTerm.app",
    "iTerm2",
    "kitty",
    "vscode",
    "WezTerm",
    "xterm-ghostty",
    "xterm-kitty",
];

fn is_truecolor_terminal(name: &str) -> bool {
    TRUECOLOR_TERMINALS.contains(&name)
}

/// Terminals known to support the kitty graphics protocol, by version-string name or `TERM`.
/// Used where the protocol cannot be queried: inside tmux, and before the probe.
fn speaks_kitty(name: &str) -> bool {
    matches!(
        name,
        "kitty" | "WezTerm" | "ghostty" | "xterm-kitty" | "xterm-ghostty"
    )
}

fn speaks_iterm2(name: &str) -> bool {
    matches!(name, "iTerm.app" | "iTerm2" | "WezTerm")
}

impl TerminalInfo {
    /// What the process environment alone says about the terminal.
    ///
    /// Available before [`Document::run`](crate::Document::run) and without a terminal, but
    /// a guess: the terminal is never asked.
    pub fn from_env() -> Self {
        env::detect(|name| std::env::var(name).ok())
    }

    /// This info with `pixel_size` in place, or `None` when that changes nothing.
    pub(crate) fn with_pixel_size(&self, pixel_size: Option<PixelSize>) -> Option<Self> {
        (self.pixel_size != pixel_size).then(|| Self {
            pixel_size,
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: PixelSize = PixelSize {
        width: 800,
        height: 600,
        cell_width: 10,
        cell_height: 20,
    };

    #[test]
    fn a_pixel_size_change_gives_new_info() {
        let info = TerminalInfo::default();
        let resized = info.with_pixel_size(Some(SIZE)).unwrap();
        assert_eq!(resized.pixel_size, Some(SIZE));
        assert_eq!(resized.with_pixel_size(None), Some(TerminalInfo::default()));
    }

    #[test]
    fn an_unchanged_pixel_size_gives_nothing() {
        assert_eq!(TerminalInfo::default().with_pixel_size(None), None);
        let info = TerminalInfo {
            pixel_size: Some(SIZE),
            ..TerminalInfo::default()
        };
        assert_eq!(info.with_pixel_size(Some(SIZE)), None);
    }
}

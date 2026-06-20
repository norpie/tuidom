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
}

/// Blend `src` over `dst` using `alpha` for both fg and bg colors.
fn blend_cell(dst: &Cell, src: &Cell, alpha: f64) -> Cell {
    Cell {
        ch: src.ch,
        fg: blend_fg(dst.fg, src.fg, alpha, dst.bg),
        bg: blend_color(dst.bg, src.bg, alpha),
    }
}

/// Blend a source foreground color over a destination foreground color.
///
/// When the destination is transparent (None), fades toward the cell's
/// background color instead of black.
fn blend_fg(dst: Option<Rgb>, src: Option<Rgb>, alpha: f64, cell_bg: Option<Rgb>) -> Option<Rgb> {
    match (dst, src) {
        (None, None) => None,
        (None, Some(s)) => {
            // Fade toward the background color behind us
            let target = cell_bg.unwrap_or(Rgb { r: 0, g: 0, b: 0, a: 255 });
            Some(Rgb {
                r: lerp_u8(target.r, s.r, alpha),
                g: lerp_u8(target.g, s.g, alpha),
                b: lerp_u8(target.b, s.b, alpha),
                a: s.a,
            })
        }
        (Some(d), None) => Some(d),
        (Some(d), Some(s)) => Some(Rgb {
            r: lerp_u8(d.r, s.r, alpha),
            g: lerp_u8(d.g, s.g, alpha),
            b: lerp_u8(d.b, s.b, alpha),
            a: d.a.max(s.a),
        }),
    }
}

/// Blend a source color over a destination color (for backgrounds).
fn blend_color(dst: Option<Rgb>, src: Option<Rgb>, alpha: f64) -> Option<Rgb> {
    match (dst, src) {
        (None, None) => None,
        (None, Some(s)) => Some(Rgb {
            r: lerp_u8(0, s.r, alpha),
            g: lerp_u8(0, s.g, alpha),
            b: lerp_u8(0, s.b, alpha),
            a: s.a,
        }),
        (Some(d), None) => Some(d),
        (Some(d), Some(s)) => Some(Rgb {
            r: lerp_u8(d.r, s.r, alpha),
            g: lerp_u8(d.g, s.g, alpha),
            b: lerp_u8(d.b, s.b, alpha),
            a: d.a.max(s.a),
        }),
    }
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round() as u8
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

    /// Fill a rectangular region with a cell value, blending by `alpha`.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, cell: Cell, alpha: f64) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let mut samples = Vec::new();
        for row in y..y_end {
            for col in x..x_end {
                let dst = &self.cells[row as usize][col as usize];
                let blended = blend_cell(dst, &cell, alpha);
                if row == y && (col == x || col == x + w / 2 || col == x_end - 1) {
                    samples.push(format!("  ({col},{row}) dst={dst:?} src={cell:?} alpha={alpha:.6} → {blended:?}"));
                }
                self.cells[row as usize][col as usize] = blended;
            }
        }
        if !samples.is_empty() {
            log::info!("[fill_rect] {x},{y} {w}x{h} alpha={alpha:.6} cell={cell:?}");
            for s in &samples { log::info!("{s}"); }
        }
    }

    /// Write text at a position, blending fg by `alpha` toward the cell's bg.
    /// Bg is left as-is (assumes the background was already filled by `fill_rect`).
    pub fn write_text(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
    ) {
        for (i, ch) in text.chars().enumerate() {
            if x + i as u16 >= self.width {
                break;
            }
            let dst = &self.cells[y as usize][(x + i as u16) as usize];
            let cell = Cell {
                ch,
                fg: blend_fg(dst.fg, fg, alpha, dst.bg),
                bg: dst.bg,
            };
            self.cells[y as usize][(x + i as u16) as usize] = cell;
        }
    }
}

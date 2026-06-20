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
            let target = cell_bg.unwrap_or(Rgb {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            });
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

fn clamp_alpha(alpha: f64) -> f64 {
    if alpha.is_nan() {
        0.0
    } else {
        alpha.clamp(0.0, 1.0)
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

    /// Fill a rectangular region with a cell value, blending by `alpha`.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, cell: Cell, alpha: f64) {
        if x >= self.width || y >= self.height || w == 0 || h == 0 {
            return;
        }

        let alpha = clamp_alpha(alpha);
        if alpha <= 0.0 {
            return;
        }

        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        for row in y..y_end {
            for col in x..x_end {
                let dst = &self.cells[row as usize][col as usize];
                self.cells[row as usize][col as usize] = blend_cell(dst, &cell, alpha);
            }
        }
    }

    /// Write one line of text at a position, clipped to the screen width.
    /// Bg is left as-is (assumes the background was already filled by `fill_rect`).
    pub fn write_text(&mut self, x: u16, y: u16, text: &str, fg: Option<Rgb>, alpha: f64) {
        if x >= self.width || y >= self.height {
            return;
        }

        let max_width = self.width - x;
        let line = text.lines().next().unwrap_or("");
        self.write_text_line_clipped(x, y, max_width, line, fg, alpha);
    }

    /// Write multiline text clipped to a rectangular region.
    /// Bg is left as-is (assumes the background was already filled by `fill_rect`).
    pub fn write_text_clipped(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
    ) {
        if x >= self.width || y >= self.height || w == 0 || h == 0 {
            return;
        }

        let max_width = w.min(self.width - x);
        let max_height = h.min(self.height - y);
        for (line_index, line) in text.lines().take(max_height as usize).enumerate() {
            self.write_text_line_clipped(x, y + line_index as u16, max_width, line, fg, alpha);
        }
    }

    fn write_text_line_clipped(
        &mut self,
        x: u16,
        y: u16,
        max_width: u16,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
    ) {
        let alpha = clamp_alpha(alpha);
        if alpha <= 0.0 || max_width == 0 {
            return;
        }

        let row = y as usize;
        let start = x as usize;
        let end = start + max_width as usize;

        for (offset, ch) in text.chars().take(end - start).enumerate() {
            let col = start + offset;
            let dst = &self.cells[row][col];
            let cell = Cell {
                ch,
                fg: blend_fg(dst.fg, fg, alpha, dst.bg),
                bg: dst.bg,
            };
            self.cells[row][col] = cell;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.cells[row].iter().map(|cell| cell.ch).collect()
    }

    #[test]
    fn fill_rect_ignores_offscreen_and_handles_overflow() {
        let blue = rgb(0, 0, 255);
        let cell = Cell { ch: ' ', fg: None, bg: Some(blue) };
        let mut grid = Grid::new(2, 2);

        grid.fill_rect(5, 0, 1, 1, cell, 1.0);
        assert_eq!(grid.cells, vec![vec![Cell::empty(); 2]; 2]);

        grid.fill_rect(1, 1, u16::MAX, u16::MAX, cell, 1.0);
        assert_eq!(grid.cells[1][1].bg, Some(blue));
        assert_eq!(grid.cells[0][0], Cell::empty());
    }

    #[test]
    fn fill_rect_clamps_alpha() {
        let red = rgb(255, 0, 0);
        let cell = Cell { ch: ' ', fg: None, bg: Some(red) };
        let mut grid = Grid::new(1, 1);

        grid.fill_rect(0, 0, 1, 1, cell, f64::NAN);
        assert_eq!(grid.cells[0][0], Cell::empty());

        grid.fill_rect(0, 0, 1, 1, cell, 2.0);
        assert_eq!(grid.cells[0][0].bg, Some(red));
    }

    #[test]
    fn write_text_ignores_offscreen_y() {
        let mut grid = Grid::new(3, 1);
        let before = grid.clone();

        grid.write_text(0, 1, "abc", Some(rgb(255, 255, 255)), 1.0);
        assert_eq!(grid.cells, before.cells);
    }

    #[test]
    fn write_text_stops_at_newline() {
        let mut grid = Grid::new(5, 1);

        grid.write_text(0, 0, "ab\ncd", Some(rgb(255, 255, 255)), 1.0);
        assert_eq!(row_text(&grid, 0), "ab   ");
    }

    #[test]
    fn write_text_clipped_respects_width() {
        let mut grid = Grid::new(5, 1);

        grid.write_text_clipped(1, 0, 2, 1, "abcd", Some(rgb(255, 255, 255)), 1.0);
        assert_eq!(row_text(&grid, 0), " ab  ");
    }

    #[test]
    fn write_text_clipped_respects_height_and_newlines() {
        let mut grid = Grid::new(4, 3);

        grid.write_text_clipped(0, 0, 4, 2, "ab\ncd\nef", Some(rgb(255, 255, 255)), 1.0);
        assert_eq!(row_text(&grid, 0), "ab  ");
        assert_eq!(row_text(&grid, 1), "cd  ");
        assert_eq!(row_text(&grid, 2), "    ");
    }
}

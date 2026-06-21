//! Cell buffer — the 2D grid representing the virtual screen.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::style::color::Rgb;

/// Text content stored in a single terminal cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellContent {
    /// Empty cell content. Rendered as a space.
    Empty,
    /// A grapheme cluster starting at this cell.
    Glyph {
        /// Grapheme cluster text.
        text: String,
        /// Terminal cell width. Currently 1 or 2.
        width: u8,
    },
    /// Second cell occupied by a width-2 glyph.
    WideContinuation,
}

impl CellContent {
    fn terminal_text(&self) -> &str {
        match self {
            Self::Empty => " ",
            Self::Glyph { text, .. } => text,
            Self::WideContinuation => "",
        }
    }

    fn width(&self) -> u8 {
        match self {
            Self::Glyph { width, .. } => *width,
            _ => 1,
        }
    }
}

/// A rectangular region in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GridRect {
    /// Left edge in terminal cells.
    pub x: u16,
    /// Top edge in terminal cells.
    pub y: u16,
    /// Width in terminal cells.
    pub width: u16,
    /// Height in terminal cells.
    pub height: u16,
}

/// A single terminal character position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    /// Display content occupying this terminal cell.
    pub content: CellContent,
    /// Foreground color. `None` means terminal default.
    pub fg: Option<Rgb>,
    /// Background color. `None` means terminal default.
    pub bg: Option<Rgb>,
}

impl Cell {
    /// Create an empty cell (space, no colors).
    pub fn empty() -> Self {
        Self {
            content: CellContent::Empty,
            fg: None,
            bg: None,
        }
    }

    /// Create an empty cell with a background color.
    pub fn empty_with_bg(bg: Rgb) -> Self {
        Self {
            content: CellContent::Empty,
            fg: None,
            bg: Some(bg),
        }
    }

    /// Text to print for this cell when flushing to the terminal.
    pub fn terminal_text(&self) -> &str {
        self.content.terminal_text()
    }

    /// Whether this cell is the continuation of a wide glyph.
    pub fn is_wide_continuation(&self) -> bool {
        matches!(self.content, CellContent::WideContinuation)
    }

    /// Width occupied by this cell's terminal text.
    pub fn content_width(&self) -> u8 {
        self.content.width()
    }
}

/// Blend `src` over `dst` using `alpha` for both fg and bg colors.
fn blend_cell(dst: &Cell, src: &Cell, alpha: f64, replace_content: bool) -> Cell {
    Cell {
        content: if replace_content {
            src.content.clone()
        } else {
            dst.content.clone()
        },
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

        let replaces_content = !matches!(cell.content, CellContent::Empty) || alpha >= 1.0;
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        for row in y..y_end {
            for col in x..x_end {
                if replaces_content {
                    self.clear_text_span_at(row as usize, col as usize);
                }
                let dst = &self.cells[row as usize][col as usize];
                self.cells[row as usize][col as usize] =
                    blend_cell(dst, &cell, alpha, replaces_content);
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
    pub fn write_text_clipped(&mut self, rect: GridRect, text: &str, fg: Option<Rgb>, alpha: f64) {
        if rect.x >= self.width || rect.y >= self.height || rect.width == 0 || rect.height == 0 {
            return;
        }

        let max_width = rect.width.min(self.width - rect.x);
        let max_height = rect.height.min(self.height - rect.y);
        for (line_index, line) in text.lines().take(max_height as usize).enumerate() {
            self.write_text_line_clipped(
                rect.x,
                rect.y + line_index as u16,
                max_width,
                line,
                fg,
                alpha,
            );
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
        let mut col = x as usize;
        let end = col + max_width as usize;

        for grapheme in text.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme).min(2);
            if width == 0 {
                continue;
            }
            if col + width > end {
                break;
            }

            self.write_glyph(row, col, grapheme, width as u8, fg, alpha);
            col += width;
        }
    }

    fn write_glyph(
        &mut self,
        row: usize,
        col: usize,
        text: &str,
        width: u8,
        fg: Option<Rgb>,
        alpha: f64,
    ) {
        self.clear_text_span_at(row, col);
        if width == 2 {
            self.clear_text_span_at(row, col + 1);
        }

        let dst = &self.cells[row][col];
        let glyph_cell = Cell {
            content: CellContent::Glyph {
                text: text.to_string(),
                width,
            },
            fg: blend_fg(dst.fg, fg, alpha, dst.bg),
            bg: dst.bg,
        };
        self.cells[row][col] = glyph_cell;

        if width == 2 {
            self.cells[row][col + 1].content = CellContent::WideContinuation;
            self.cells[row][col + 1].fg = None;
        }
    }

    fn clear_text_span_at(&mut self, row: usize, col: usize) {
        if row >= self.height as usize || col >= self.width as usize {
            return;
        }

        match self.cells[row][col].content.clone() {
            CellContent::Glyph { width: 2, .. } => {
                self.clear_one_cell_text(row, col);
                if col + 1 < self.width as usize {
                    self.clear_one_cell_text(row, col + 1);
                }
            }
            CellContent::WideContinuation => {
                if col > 0 {
                    self.clear_one_cell_text(row, col - 1);
                }
                self.clear_one_cell_text(row, col);
            }
            _ => self.clear_one_cell_text(row, col),
        }
    }

    fn clear_one_cell_text(&mut self, row: usize, col: usize) {
        self.cells[row][col].content = CellContent::Empty;
        self.cells[row][col].fg = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.cells[row]
            .iter()
            .filter(|cell| !cell.is_wide_continuation())
            .map(Cell::terminal_text)
            .collect()
    }

    #[test]
    fn fill_rect_ignores_offscreen_and_handles_overflow() {
        let blue = rgb(0, 0, 255);
        let cell = Cell::empty_with_bg(blue);
        let mut grid = Grid::new(2, 2);

        grid.fill_rect(5, 0, 1, 1, cell.clone(), 1.0);
        assert_eq!(grid.cells, vec![vec![Cell::empty(); 2]; 2]);

        grid.fill_rect(1, 1, u16::MAX, u16::MAX, cell, 1.0);
        assert_eq!(grid.cells[1][1].bg, Some(blue));
        assert_eq!(grid.cells[0][0], Cell::empty());
    }

    #[test]
    fn translucent_empty_fill_blends_background_without_erasing_text() {
        let white = rgb(255, 255, 255);
        let blue = rgb(0, 0, 255);
        let red = rgb(255, 0, 0);
        let mut grid = Grid::new(2, 1);

        grid.fill_rect(0, 0, 2, 1, Cell::empty_with_bg(blue), 1.0);
        grid.write_text(0, 0, "hi", Some(white), 1.0);
        grid.fill_rect(0, 0, 2, 1, Cell::empty_with_bg(red), 0.5);

        assert_eq!(row_text(&grid, 0), "hi");
        assert_eq!(grid.cells[0][0].fg, Some(white));
        assert_eq!(grid.cells[0][0].bg, Some(rgb(128, 0, 128)));
        assert_eq!(grid.cells[0][1].fg, Some(white));
        assert_eq!(grid.cells[0][1].bg, Some(rgb(128, 0, 128)));
    }

    #[test]
    fn fill_rect_clamps_alpha() {
        let red = rgb(255, 0, 0);
        let cell = Cell::empty_with_bg(red);
        let mut grid = Grid::new(1, 1);

        grid.fill_rect(0, 0, 1, 1, cell.clone(), f64::NAN);
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

        grid.write_text_clipped(
            GridRect {
                x: 1,
                y: 0,
                width: 2,
                height: 1,
            },
            "abcd",
            Some(rgb(255, 255, 255)),
            1.0,
        );
        assert_eq!(row_text(&grid, 0), " ab  ");
    }

    #[test]
    fn write_text_clipped_respects_height_and_newlines() {
        let mut grid = Grid::new(4, 3);

        grid.write_text_clipped(
            GridRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            "ab\ncd\nef",
            Some(rgb(255, 255, 255)),
            1.0,
        );
        assert_eq!(row_text(&grid, 0), "ab  ");
        assert_eq!(row_text(&grid, 1), "cd  ");
        assert_eq!(row_text(&grid, 2), "    ");
    }

    #[test]
    fn ascii_glyphs_are_width_one() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(0, 0, "abc", Some(rgb(255, 255, 255)), 1.0);

        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][2].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn wide_glyph_occupies_two_cells() {
        let mut grid = Grid::new(4, 1);

        grid.write_text(0, 0, "a界b", Some(rgb(255, 255, 255)), 1.0);

        assert_eq!(row_text(&grid, 0), "a界b");
        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 2, .. }
        ));
        assert!(matches!(
            grid.cells[0][2].content,
            CellContent::WideContinuation
        ));
        assert!(matches!(
            grid.cells[0][3].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn wide_glyph_is_skipped_at_clip_boundary() {
        let mut grid = Grid::new(2, 1);

        grid.write_text_clipped(
            GridRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            "a界",
            Some(rgb(255, 255, 255)),
            1.0,
        );
        assert_eq!(row_text(&grid, 0), "a ");
    }

    #[test]
    fn combining_grapheme_occupies_one_cell() {
        let mut grid = Grid::new(2, 1);

        grid.write_text(0, 0, "e\u{301}x", Some(rgb(255, 255, 255)), 1.0);
        assert_eq!(row_text(&grid, 0), "e\u{301}x");
        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn overwriting_wide_head_clears_continuation() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        grid.write_text(0, 0, "a", Some(rgb(255, 255, 255)), 1.0);

        assert_eq!(row_text(&grid, 0), "a  ");
        assert!(matches!(grid.cells[0][1].content, CellContent::Empty));
    }

    #[test]
    fn overwriting_wide_continuation_clears_head() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        grid.write_text(1, 0, "a", Some(rgb(255, 255, 255)), 1.0);

        assert_eq!(row_text(&grid, 0), " a ");
        assert!(matches!(grid.cells[0][0].content, CellContent::Empty));
    }

    #[test]
    fn fill_rect_clears_wide_span_it_overlaps() {
        let blue = rgb(0, 0, 255);
        let mut grid = Grid::new(3, 1);

        grid.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        grid.fill_rect(1, 0, 1, 1, Cell::empty_with_bg(blue), 1.0);

        assert_eq!(row_text(&grid, 0), "   ");
        assert!(matches!(grid.cells[0][0].content, CellContent::Empty));
        assert_eq!(grid.cells[0][1].bg, Some(blue));
    }
}

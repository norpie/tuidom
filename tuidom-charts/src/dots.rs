//! A grid of dots over a block of cells, for plotting lines and points finer than a cell.

use tuidom::canvas::{CanvasBuffer, CanvasStyle};
use tuidom::style::ResolvedColor;

/// How dots are drawn, which sets how many fit in a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Marker {
    /// Braille patterns, 2 by 4 dots a cell. A cell has one color: the last drawn into it.
    #[default]
    Braille,
    /// Half blocks, 1 by 2 dots a cell, each half with its own color. For fonts that draw
    /// braille badly.
    HalfBlock,
    /// A `•` per cell.
    Dot,
}

impl Marker {
    fn per_cell(self) -> (u32, u32) {
        match self {
            Self::Braille => (2, 4),
            Self::HalfBlock => (1, 2),
            Self::Dot => (1, 1),
        }
    }
}

/// Dots over `columns` by `rows` cells, drawn into a canvas buffer with [`DotGrid::draw`].
///
/// Dot `(0, 0)` is the top-left one.
#[derive(Debug, Clone, PartialEq)]
pub struct DotGrid {
    columns: u16,
    rows: u16,
    marker: Marker,
    cells: Vec<DotCell>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct DotCell {
    bits: u8,
    /// One color, or for half blocks the top half's and the bottom half's.
    colors: [Option<ResolvedColor>; 2],
}

/// Braille dot bits by column and row within a cell.
const BRAILLE: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

impl DotGrid {
    /// An empty grid over `columns` by `rows` cells.
    pub fn new(columns: u16, rows: u16, marker: Marker) -> Self {
        Self {
            columns,
            rows,
            marker,
            cells: vec![DotCell::default(); usize::from(columns) * usize::from(rows)],
        }
    }

    /// The grid's size in dots.
    pub fn dots(&self) -> (u32, u32) {
        let (across, down) = self.marker.per_cell();
        (
            u32::from(self.columns) * across,
            u32::from(self.rows) * down,
        )
    }

    /// Light the dot at `x`, `y` in `color`. Dots outside the grid are ignored.
    pub fn set(&mut self, x: u32, y: u32, color: ResolvedColor) {
        let (width, height) = self.dots();
        if x >= width || y >= height {
            return;
        }
        let (across, down) = self.marker.per_cell();
        let index = (y / down) as usize * usize::from(self.columns) + (x / across) as usize;
        let Some(cell) = self.cells.get_mut(index) else {
            return;
        };
        let (dx, dy) = ((x % across) as usize, (y % down) as usize);
        match self.marker {
            Marker::Braille => {
                cell.bits |= BRAILLE[dx][dy];
                cell.colors[0] = Some(color);
            }
            Marker::HalfBlock => {
                cell.bits |= 1 << dy;
                cell.colors[dy] = Some(color);
            }
            Marker::Dot => {
                cell.bits = 1;
                cell.colors[0] = Some(color);
            }
        }
    }

    /// Light the dots on the segment between two points in dot coordinates, which may lie
    /// outside the grid. Nothing is drawn when either point is not finite.
    pub fn line(&mut self, from: (f64, f64), to: (f64, f64), color: ResolvedColor) {
        let (width, height) = self.dots();
        if width == 0 || height == 0 {
            return;
        }
        // Each dot covers half a dot either side of its center.
        let (right, bottom) = (f64::from(width) - 0.5, f64::from(height) - 0.5);
        let Some((from, to)) = clip(from, to, (-0.5, -0.5), (right, bottom)) else {
            return;
        };
        let dot = |point: (f64, f64)| {
            (
                (point.0.round() as i64).clamp(0, i64::from(width) - 1),
                (point.1.round() as i64).clamp(0, i64::from(height) - 1),
            )
        };
        let ((mut x, mut y), (end_x, end_y)) = (dot(from), dot(to));
        let (dx, dy) = ((end_x - x).abs(), -(end_y - y).abs());
        let (step_x, step_y) = ((end_x - x).signum(), (end_y - y).signum());
        let mut error = dx + dy;
        loop {
            if let (Ok(dot_x), Ok(dot_y)) = (u32::try_from(x), u32::try_from(y)) {
                self.set(dot_x, dot_y, color);
            }
            if x == end_x && y == end_y {
                break;
            }
            let doubled = 2 * error;
            if doubled >= dy {
                error += dy;
                x += step_x;
            }
            if doubled <= dx {
                error += dx;
                y += step_y;
            }
        }
    }

    /// Write the cells with a lit dot into `buffer`, with the grid's top-left cell at `x`, `y`.
    /// Cells with none are left as they are.
    pub fn draw(&self, buffer: &mut CanvasBuffer, x: u16, y: u16) {
        for (index, cell) in self.cells.iter().enumerate() {
            if cell.bits == 0 {
                continue;
            }
            let column = (index % usize::from(self.columns.max(1))) as u16;
            let row = (index / usize::from(self.columns.max(1))) as u16;
            let (Some(cell_x), Some(cell_y)) = (x.checked_add(column), y.checked_add(row)) else {
                continue;
            };
            let (symbol, style) = self.glyph(cell);
            buffer.set(cell_x, cell_y, &symbol, style);
        }
    }

    fn glyph(&self, cell: &DotCell) -> (String, CanvasStyle) {
        let fg = |color| CanvasStyle {
            fg: color,
            ..CanvasStyle::default()
        };
        match self.marker {
            Marker::Braille => (
                char::from_u32(0x2800 + u32::from(cell.bits))
                    .unwrap_or(' ')
                    .to_string(),
                fg(cell.colors[0]),
            ),
            Marker::HalfBlock => match cell.colors {
                [Some(top), Some(bottom)] if top == bottom => ("█".into(), fg(Some(top))),
                [Some(top), Some(bottom)] => (
                    "▀".into(),
                    CanvasStyle {
                        fg: Some(top),
                        bg: Some(bottom),
                        ..CanvasStyle::default()
                    },
                ),
                [Some(top), None] => ("▀".into(), fg(Some(top))),
                [None, bottom] => ("▄".into(), fg(bottom)),
            },
            Marker::Dot => ("•".into(), fg(cell.colors[0])),
        }
    }
}

/// The part of the segment inside the rectangle from `low` to `high`, or `None` when none of it
/// is, or a point is not finite.
fn clip(
    from: (f64, f64),
    to: (f64, f64),
    low: (f64, f64),
    high: (f64, f64),
) -> Option<((f64, f64), (f64, f64))> {
    if ![from.0, from.1, to.0, to.1]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (mut enter, mut leave) = (0.0f64, 1.0f64);
    // Liang–Barsky: each edge as `p * t <= q`, narrowing the part of the segment inside.
    for (p, q) in [
        (-dx, from.0 - low.0),
        (dx, high.0 - from.0),
        (-dy, from.1 - low.1),
        (dy, high.1 - from.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                enter = enter.max(t);
            } else {
                leave = leave.min(t);
            }
        }
    }
    let point = |t: f64| (from.0 + t * dx, from.1 + t * dy);
    (enter <= leave).then_some((point(enter), point(leave)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(buffer: &CanvasBuffer) -> Vec<String> {
        (0..buffer.height())
            .map(|y| {
                (0..buffer.width())
                    .map(|x| match buffer.get(x, y) {
                        Some(("", _)) | None => " ",
                        Some((symbol, _)) => symbol,
                    })
                    .collect()
            })
            .collect()
    }

    fn drawn(grid: &DotGrid) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(grid.columns, grid.rows);
        grid.draw(&mut buffer, 0, 0);
        rows(&buffer)
    }

    fn fg(buffer: &CanvasBuffer, x: u16, y: u16) -> Option<ResolvedColor> {
        buffer.get(x, y).unwrap().1.fg
    }

    #[test]
    fn markers_set_the_dots_per_cell() {
        assert_eq!(DotGrid::new(3, 2, Marker::Braille).dots(), (6, 8));
        assert_eq!(DotGrid::new(3, 2, Marker::HalfBlock).dots(), (3, 4));
        assert_eq!(DotGrid::new(3, 2, Marker::Dot).dots(), (3, 2));
    }

    #[test]
    fn braille_dots_combine_in_their_cell() {
        let mut grid = DotGrid::new(2, 1, Marker::Braille);
        grid.set(0, 0, ResolvedColor::red());
        assert_eq!(drawn(&grid), ["⠁ "]);
        grid.set(1, 3, ResolvedColor::red());
        assert_eq!(drawn(&grid), ["⢁ "]);
        grid.set(0, 3, ResolvedColor::red());
        grid.set(1, 0, ResolvedColor::red());
        grid.set(2, 1, ResolvedColor::red());
        assert_eq!(drawn(&grid), ["⣉⠂"]);
    }

    #[test]
    fn dots_outside_the_grid_are_ignored() {
        let mut grid = DotGrid::new(1, 1, Marker::Braille);
        grid.set(2, 0, ResolvedColor::red());
        grid.set(0, 4, ResolvedColor::red());
        assert_eq!(drawn(&grid), [" "]);
    }

    #[test]
    fn the_last_color_drawn_into_a_cell_wins() {
        let mut grid = DotGrid::new(1, 1, Marker::Braille);
        grid.set(0, 0, ResolvedColor::red());
        grid.set(1, 3, ResolvedColor::blue());
        let mut buffer = CanvasBuffer::new(1, 1);
        grid.draw(&mut buffer, 0, 0);
        assert_eq!(fg(&buffer, 0, 0), Some(ResolvedColor::blue()));
    }

    #[test]
    fn half_blocks_keep_a_color_per_half() {
        let mut grid = DotGrid::new(4, 1, Marker::HalfBlock);
        grid.set(0, 0, ResolvedColor::red());
        grid.set(1, 1, ResolvedColor::red());
        grid.set(2, 0, ResolvedColor::red());
        grid.set(2, 1, ResolvedColor::red());
        grid.set(3, 0, ResolvedColor::red());
        grid.set(3, 1, ResolvedColor::blue());
        let mut buffer = CanvasBuffer::new(4, 1);
        grid.draw(&mut buffer, 0, 0);
        assert_eq!(rows(&buffer), ["▀▄█▀"]);
        assert_eq!(buffer.get(2, 0).unwrap().1.bg, None);
        let split = buffer.get(3, 0).unwrap().1;
        assert_eq!(split.fg, Some(ResolvedColor::red()));
        assert_eq!(split.bg, Some(ResolvedColor::blue()));
    }

    #[test]
    fn dot_markers_draw_a_bullet() {
        let mut grid = DotGrid::new(2, 1, Marker::Dot);
        grid.set(1, 0, ResolvedColor::red());
        assert_eq!(drawn(&grid), [" •"]);
    }

    #[test]
    fn lines_light_every_dot_between_their_ends() {
        let mut grid = DotGrid::new(4, 1, Marker::HalfBlock);
        grid.line((0.0, 1.0), (3.0, 1.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["▄▄▄▄"]);

        let mut grid = DotGrid::new(1, 2, Marker::HalfBlock);
        grid.line((0.0, 3.0), (0.0, 0.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["█", "█"]);

        let mut grid = DotGrid::new(4, 4, Marker::Dot);
        grid.line((0.0, 3.0), (3.0, 0.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["   •", "  • ", " •  ", "•   "]);
    }

    #[test]
    fn a_line_of_one_point_lights_that_dot() {
        let mut grid = DotGrid::new(2, 1, Marker::Dot);
        grid.line((1.2, 0.0), (1.2, 0.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), [" •"]);
    }

    #[test]
    fn lines_are_clipped_to_the_grid() {
        let mut grid = DotGrid::new(4, 4, Marker::Dot);
        grid.line((-3.0, 6.0), (6.0, -3.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["   •", "  • ", " •  ", "•   "]);

        let mut grid = DotGrid::new(3, 1, Marker::Dot);
        grid.line((-1e9, 0.0), (1e9, 0.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["•••"]);

        let mut grid = DotGrid::new(3, 1, Marker::Dot);
        grid.line((-5.0, 0.0), (-1.0, 0.0), ResolvedColor::red());
        grid.line((0.0, 2.0), (2.0, 2.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["   "]);
    }

    #[test]
    fn a_line_with_a_point_not_finite_draws_nothing() {
        let mut grid = DotGrid::new(3, 1, Marker::Dot);
        grid.line((0.0, 0.0), (f64::NAN, 0.0), ResolvedColor::red());
        grid.line((f64::INFINITY, 0.0), (0.0, 0.0), ResolvedColor::red());
        assert_eq!(drawn(&grid), ["   "]);
    }

    #[test]
    fn drawing_at_an_offset_leaves_other_cells_alone() {
        let mut grid = DotGrid::new(2, 1, Marker::Dot);
        grid.set(0, 0, ResolvedColor::red());
        let mut buffer = CanvasBuffer::new(4, 2);
        buffer.text(0, 1, "abcd", CanvasStyle::default());
        buffer.set(3, 1, "x", CanvasStyle::default());
        grid.draw(&mut buffer, 2, 1);
        assert_eq!(rows(&buffer), ["    ", "ab•x"]);
        assert_eq!(fg(&buffer, 2, 1), Some(ResolvedColor::red()));

        grid.draw(&mut buffer, 3, 0);
        grid.draw(&mut buffer, u16::MAX, 0);
        assert_eq!(rows(&buffer), ["   •", "ab•x"]);
    }
}

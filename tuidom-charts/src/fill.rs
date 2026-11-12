//! Bars of block characters, filling cells from an edge.
//!
//! Upward and rightward fills end in an eighth block, so a bar's length is shown to an eighth of
//! a cell. Unicode has eighth blocks only along the bottom and left edges, so downward and
//! leftward fills end in a half block and are shown to half a cell.

use std::ops::Range;

use tuidom::canvas::{CanvasBuffer, CanvasStyle};
use tuidom::style::ResolvedColor;

const UP: [&str; 8] = ["", "▁", "▂", "▃", "▄", "▅", "▆", "▇"];
const RIGHT: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
const FULL: &str = "█";

/// Fill `width` columns from `x` upward from row `bottom`, `length` cells high to the nearest
/// eighth. Cells outside the buffer are cropped; a length that is not positive draws nothing.
pub fn fill_up(
    buffer: &mut CanvasBuffer,
    x: u16,
    bottom: u16,
    width: u16,
    length: f64,
    color: ResolvedColor,
) {
    let Some(steps) = steps(length, 8, u32::from(bottom) + 1) else {
        return;
    };
    for (offset, symbol) in cells(steps, 8, &UP) {
        let Some(row) = bottom.checked_sub(offset) else {
            break;
        };
        for column in columns(buffer, x, width) {
            buffer.set(column, row, symbol, style(color));
        }
    }
}

/// Fill `height` rows from `y` rightward from column `left`, `length` cells long to the nearest
/// eighth. Cells outside the buffer are cropped; a length that is not positive draws nothing.
pub fn fill_right(
    buffer: &mut CanvasBuffer,
    left: u16,
    y: u16,
    height: u16,
    length: f64,
    color: ResolvedColor,
) {
    let span = u32::from(buffer.width().saturating_sub(left));
    let Some(steps) = steps(length, 8, span) else {
        return;
    };
    for (offset, symbol) in cells(steps, 8, &RIGHT) {
        let Some(column) = left.checked_add(offset) else {
            break;
        };
        for row in rows(buffer, y, height) {
            buffer.set(column, row, symbol, style(color));
        }
    }
}

/// Fill `width` columns from `x` downward from row `top`, `length` cells high to the nearest
/// half. Cells outside the buffer are cropped; a length that is not positive draws nothing.
pub fn fill_down(
    buffer: &mut CanvasBuffer,
    x: u16,
    top: u16,
    width: u16,
    length: f64,
    color: ResolvedColor,
) {
    let span = u32::from(buffer.height().saturating_sub(top));
    let Some(steps) = steps(length, 2, span) else {
        return;
    };
    for (offset, symbol) in cells(steps, 2, &["", "▀"]) {
        let Some(row) = top.checked_add(offset) else {
            break;
        };
        for column in columns(buffer, x, width) {
            buffer.set(column, row, symbol, style(color));
        }
    }
}

/// Fill `height` rows from `y` leftward from column `right`, `length` cells long to the nearest
/// half. Cells outside the buffer are cropped; a length that is not positive draws nothing.
pub fn fill_left(
    buffer: &mut CanvasBuffer,
    right: u16,
    y: u16,
    height: u16,
    length: f64,
    color: ResolvedColor,
) {
    let Some(steps) = steps(length, 2, u32::from(right) + 1) else {
        return;
    };
    for (offset, symbol) in cells(steps, 2, &["", "▐"]) {
        let Some(column) = right.checked_sub(offset) else {
            break;
        };
        for row in rows(buffer, y, height) {
            buffer.set(column, row, symbol, style(color));
        }
    }
}

/// `length` in steps of `1 / per_cell`, rounded, and at most `cells` whole cells. `None` when
/// that is no steps.
fn steps(length: f64, per_cell: u32, cells: u32) -> Option<u32> {
    if !length.is_finite() {
        return None;
    }
    let most = f64::from(cells * per_cell);
    let steps = (length * f64::from(per_cell)).round().min(most) as u32;
    (length > 0.0 && steps > 0).then_some(steps)
}

/// Each cell of a fill as its distance from the starting edge and its symbol: whole cells, then
/// the part of one left over.
fn cells<'a>(
    steps: u32,
    per_cell: u32,
    parts: &'a [&'a str],
) -> impl Iterator<Item = (u16, &'a str)> {
    let whole = steps / per_cell;
    let part = parts
        .get((steps % per_cell) as usize)
        .copied()
        .unwrap_or("");
    (0..whole)
        .map(|offset| (offset, FULL))
        .chain((!part.is_empty()).then_some((whole, part)))
        .filter_map(|(offset, symbol)| Some((u16::try_from(offset).ok()?, symbol)))
}

fn columns(buffer: &CanvasBuffer, x: u16, width: u16) -> Range<u16> {
    x..x.saturating_add(width).min(buffer.width())
}

fn rows(buffer: &CanvasBuffer, y: u16, height: u16) -> Range<u16> {
    y..y.saturating_add(height).min(buffer.height())
}

fn style(color: ResolvedColor) -> CanvasStyle {
    CanvasStyle {
        fg: Some(color),
        ..CanvasStyle::default()
    }
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

    fn up(width: u16, height: u16, x: u16, bottom: u16, bar: u16, length: f64) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(width, height);
        fill_up(&mut buffer, x, bottom, bar, length, ResolvedColor::red());
        rows(&buffer)
    }

    #[test]
    fn upward_fills_end_in_an_eighth() {
        assert_eq!(up(1, 3, 0, 2, 1, 2.0), [" ", "█", "█"]);
        assert_eq!(up(1, 3, 0, 2, 1, 1.5), [" ", "▄", "█"]);
        assert_eq!(up(1, 3, 0, 2, 1, 0.125), [" ", " ", "▁"]);
        assert_eq!(up(1, 3, 0, 2, 1, 0.9), [" ", " ", "▇"]);
        assert_eq!(up(1, 3, 0, 2, 1, 0.95), [" ", " ", "█"]);
    }

    #[test]
    fn a_fill_too_short_or_not_finite_draws_nothing() {
        assert_eq!(up(1, 2, 0, 1, 1, 0.05), [" ", " "]);
        assert_eq!(up(1, 2, 0, 1, 1, 0.0), [" ", " "]);
        assert_eq!(up(1, 2, 0, 1, 1, -3.0), [" ", " "]);
        assert_eq!(up(1, 2, 0, 1, 1, f64::NAN), [" ", " "]);
        assert_eq!(up(1, 2, 0, 1, 1, f64::INFINITY), [" ", " "]);
    }

    #[test]
    fn upward_fills_span_their_width_and_are_cropped() {
        assert_eq!(up(4, 2, 1, 1, 2, 1.5), [" ▄▄ ", " ██ "]);
        assert_eq!(up(3, 2, 2, 1, 5, 9.0), ["  █", "  █"]);
        assert_eq!(up(2, 2, 0, 0, 1, 1.5), ["█ ", "  "]);
        assert_eq!(up(2, 2, 0, 5, 1, 1.5), ["  ", "  "]);
    }

    #[test]
    fn rightward_fills_end_in_an_eighth() {
        let mut buffer = CanvasBuffer::new(4, 3);
        fill_right(&mut buffer, 1, 0, 2, 1.375, ResolvedColor::red());
        fill_right(&mut buffer, 0, 2, 1, 10.0, ResolvedColor::red());
        assert_eq!(rows(&buffer), [" █▍ ", " █▍ ", "████"]);
        assert_eq!(buffer.get(2, 0).unwrap().1.fg, Some(ResolvedColor::red()));
    }

    #[test]
    fn downward_fills_end_in_a_half() {
        let mut buffer = CanvasBuffer::new(2, 4);
        fill_down(&mut buffer, 0, 1, 1, 1.5, ResolvedColor::red());
        fill_down(&mut buffer, 1, 0, 1, 1.2, ResolvedColor::red());
        assert_eq!(rows(&buffer), [" █", "█ ", "▀ ", "  "]);

        let mut buffer = CanvasBuffer::new(1, 2);
        fill_down(&mut buffer, 0, 1, 1, 5.0, ResolvedColor::red());
        assert_eq!(rows(&buffer), [" ", "█"]);
    }

    #[test]
    fn leftward_fills_end_in_a_half() {
        let mut buffer = CanvasBuffer::new(4, 2);
        fill_left(&mut buffer, 2, 0, 1, 1.5, ResolvedColor::red());
        fill_left(&mut buffer, 3, 1, 1, 9.0, ResolvedColor::red());
        assert_eq!(rows(&buffer), [" ▐█ ", "████"]);
    }
}

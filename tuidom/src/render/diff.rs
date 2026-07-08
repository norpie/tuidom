//! Frame diffing — compare old vs. new grid to find changed cells.
//!
//! Wide glyphs occupy multiple terminal cells, so dirty cells are expanded
//! through both the old and new wide spans before flushing.

use crate::render::grid::Cell;
use crate::render::grid::CellContent;
use crate::render::grid::Grid;
use crate::style::color::Rgb;

/// A single changed cell with its position.
#[derive(Debug, Clone)]
pub(crate) struct CellChange {
    /// Column.
    pub x: u16,
    /// Row.
    pub y: u16,
    /// The new cell value to write.
    pub cell: Cell,
}

/// Max squared color distance to consider two colors "the same".
/// Must be 0 — even a 1-bit difference in any channel matters.
const COLOR_THRESHOLD: i32 = 0;

/// Compare old and new grids, returning only the cells that changed.
pub(crate) fn diff(old: &Grid, new: &Grid) -> Vec<CellChange> {
    let width = new.width as usize;
    let height = new.height as usize;
    let mut dirty_row = vec![false; width];
    let mut changes = Vec::new();

    for (y, new_row) in new.cells.iter().enumerate().take(height) {
        if old.cells.get(y).is_some_and(|old_row| old_row == new_row) {
            continue;
        }

        dirty_row.fill(false);
        for (x, new_cell) in new_row.iter().enumerate().take(width) {
            let changed = old
                .cells
                .get(y)
                .and_then(|old_row| old_row.get(x))
                .is_none_or(|old_cell| cells_differ(old_cell, new_cell));

            if changed {
                mark_span_in_row(&mut dirty_row, old, x, y);
                mark_span_in_row(&mut dirty_row, new, x, y);
            }
        }

        for (x, is_dirty) in dirty_row.iter().enumerate().take(width) {
            if *is_dirty {
                changes.push(CellChange {
                    x: x as u16,
                    y: y as u16,
                    cell: new.cells[y][x].clone(),
                });
            }
        }
    }

    changes
}

fn cells_differ(old: &Cell, new: &Cell) -> bool {
    old.content != new.content
        || color_diff(old.fg, new.fg) > COLOR_THRESHOLD
        || color_diff(old.bg, new.bg) > COLOR_THRESHOLD
}

fn mark_span_in_row(dirty: &mut [bool], grid: &Grid, x: usize, y: usize) {
    if y >= grid.height as usize || x >= grid.width as usize {
        mark(dirty, x);
        return;
    }

    match &grid.cells[y][x].content {
        CellContent::Glyph { width: 2, .. } => {
            mark(dirty, x);
            mark(dirty, x + 1);
        }
        CellContent::WideContinuation => {
            if x > 0 {
                mark(dirty, x - 1);
            }
            mark(dirty, x);
        }
        _ => mark(dirty, x),
    }
}

fn mark(dirty: &mut [bool], x: usize) {
    if x < dirty.len() {
        dirty[x] = true;
    }
}

/// Squared Euclidean distance between two optional colors.
fn color_diff(a: Option<Rgb>, b: Option<Rgb>) -> i32 {
    match (a, b) {
        (None, None) => 0,
        (None, Some(_)) | (Some(_), None) => i32::MAX,
        (Some(a), Some(b)) => {
            let dr = a.r as i32 - b.r as i32;
            let dg = a.g as i32 - b.g as i32;
            let db = a.b as i32 - b.b as i32;
            dr * dr + dg * dg + db * db
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    #[test]
    fn diff_marks_both_cells_when_wide_glyph_is_added() {
        let old = Grid::new(3, 1);
        let mut new = Grid::new(3, 1);
        new.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);

        let changes = diff(&old, &new);
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }

    #[test]
    fn diff_marks_both_cells_when_wide_glyph_is_removed() {
        let mut old = Grid::new(3, 1);
        old.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        let new = Grid::new(3, 1);

        let changes = diff(&old, &new);
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }
}

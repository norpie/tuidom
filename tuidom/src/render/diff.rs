//! Frame diffing — compare old vs. new grid to find changed cells.
//!
//! Uses a color distance threshold to avoid sending near-identical colors
//! (e.g. from alpha blending on intermediate animation frames).

use crate::render::grid::Cell;
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
    let mut changes = Vec::new();

    let height = old.height.min(new.height) as usize;
    let width = old.width.min(new.width) as usize;

    for y in 0..height {
        let old_row = &old.cells[y];
        let new_row = &new.cells[y];
        for x in 0..width {
            let o = &old_row[x];
            let n = &new_row[x];

            if o.ch != n.ch
                || color_diff(o.fg, n.fg) > COLOR_THRESHOLD
                || color_diff(o.bg, n.bg) > COLOR_THRESHOLD
            {
                changes.push(CellChange {
                    x: x as u16,
                    y: y as u16,
                    cell: *n,
                });
            }
        }
    }

    // Handle newly visible areas (grid grew)
    for y in height..new.height as usize {
        for x in 0..new.width as usize {
            changes.push(CellChange {
                x: x as u16,
                y: y as u16,
                cell: new.cells[y][x],
            });
        }
    }
    for y in 0..height {
        for x in width..new.width as usize {
            changes.push(CellChange {
                x: x as u16,
                y: y as u16,
                cell: new.cells[y][x],
            });
        }
    }

    changes
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

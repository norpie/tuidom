//! Frame diffing — compare old vs. new grid to find changed cells.

use crate::render::grid::Cell;
use crate::render::grid::Grid;

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

/// Compare old and new grids, returning only the cells that changed.
pub(crate) fn diff(old: &Grid, new: &Grid) -> Vec<CellChange> {
    let mut changes = Vec::new();

    // Use the smaller of the two dimensions
    let height = old.height.min(new.height) as usize;
    let width = old.width.min(new.width) as usize;

    for y in 0..height {
        let old_row = &old.cells[y];
        let new_row = &new.cells[y];
        for x in 0..width {
            if old_row[x].differs_from(&new_row[x]) {
                changes.push(CellChange {
                    x: x as u16,
                    y: y as u16,
                    cell: new_row[x],
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

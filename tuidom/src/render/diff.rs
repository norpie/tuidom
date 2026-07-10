//! Frame diffing — compare old vs. new grid to find changed cells.
//!
//! Wide glyphs occupy multiple terminal cells, so dirty cells are expanded
//! through both the old and new wide spans before flushing.

use crate::render::grid::Cell;
use crate::render::grid::CellContent;
use std::time::{Duration, Instant};

use crate::render::grid::Grid;

/// Detailed frame diff instrumentation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiffProfile {
    /// Whether detailed diff instrumentation was enabled for this frame.
    pub enabled: bool,
    /// Number of rows considered in the new grid.
    pub rows: usize,
    /// Number of rows skipped by exact row equality.
    pub unchanged_rows: usize,
    /// Number of rows that required cell-level scanning.
    pub changed_rows: usize,
    /// Number of cells compared inside changed rows.
    pub cells_compared: usize,
    /// Number of dirty cells emitted.
    pub dirty_cells: usize,
    /// Time spent checking row equality.
    pub row_equality_time: Duration,
    /// Time spent scanning cells in changed rows.
    pub cell_scan_time: Duration,
    /// Time spent emitting dirty cells from dirty row buffers.
    pub emit_time: Duration,
}

/// Frame diff output.
#[derive(Debug)]
pub(crate) struct DiffOutput {
    /// Changed cells to flush.
    pub changes: Vec<CellChange>,
    /// Detailed diff instrumentation.
    pub profile: DiffProfile,
}

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

/// Compare old and new grids with optional instrumentation.
pub(crate) fn diff_profiled(old: &Grid, new: &Grid, instrument: bool) -> DiffOutput {
    let width = new.width as usize;
    let height = new.height as usize;
    let mut dirty_row = vec![false; width];
    let mut changes = Vec::new();
    let mut profile = DiffProfile {
        enabled: instrument,
        ..DiffProfile::default()
    };

    for (y, new_row) in new.cells.iter().enumerate().take(height) {
        if profile.enabled {
            profile.rows += 1;
        }

        let row_equality_start = profile.enabled.then(Instant::now);
        let row_unchanged = old.cells.get(y).is_some_and(|old_row| old_row == new_row);
        if let Some(start) = row_equality_start {
            profile.row_equality_time += start.elapsed();
        }
        if row_unchanged {
            if profile.enabled {
                profile.unchanged_rows += 1;
            }
            continue;
        }
        if profile.enabled {
            profile.changed_rows += 1;
        }

        dirty_row.fill(false);
        let cell_scan_start = profile.enabled.then(Instant::now);
        for (x, new_cell) in new_row.iter().enumerate().take(width) {
            if profile.enabled {
                profile.cells_compared += 1;
            }
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
        if let Some(start) = cell_scan_start {
            profile.cell_scan_time += start.elapsed();
        }

        let emit_start = profile.enabled.then(Instant::now);
        for (x, is_dirty) in dirty_row.iter().enumerate().take(width) {
            if *is_dirty {
                changes.push(CellChange {
                    x: x as u16,
                    y: y as u16,
                    cell: new.cells[y][x].clone(),
                });
            }
        }
        if let Some(start) = emit_start {
            profile.emit_time += start.elapsed();
        }
    }

    if profile.enabled {
        profile.dirty_cells = changes.len();
    }

    DiffOutput { changes, profile }
}

fn cells_differ(old: &Cell, new: &Cell) -> bool {
    old.content != new.content || old.fg != new.fg || old.bg != new.bg
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::color::Rgb;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    #[test]
    fn diff_marks_both_cells_when_wide_glyph_is_added() {
        let old = Grid::new(3, 1);
        let mut new = Grid::new(3, 1);
        new.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);

        let changes = diff_profiled(&old, &new, false).changes;
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }

    #[test]
    fn diff_marks_both_cells_when_wide_glyph_is_removed() {
        let mut old = Grid::new(3, 1);
        old.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        let new = Grid::new(3, 1);

        let changes = diff_profiled(&old, &new, false).changes;
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }
}

//! Frame diffing — compare old vs. new grid to find changed cells.
//!
//! Wide glyphs occupy multiple terminal cells, so dirty cells are expanded
//! through both the old and new wide spans before flushing.

use crate::performance::DiffProfile;
use crate::render::grid::Cell;
use crate::render::grid::CellContent;
use std::time::Instant;

use crate::render::grid::{Grid, TouchedSpan};

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

/// Compare old and new grids with optional dirty row hints and instrumentation.
pub(crate) fn diff_profiled_with_hints(
    old: &Grid,
    new: &Grid,
    instrument: bool,
    dirty_spans: Option<(&[Option<TouchedSpan>], &[Option<TouchedSpan>])>,
) -> DiffOutput {
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

        let scan_span = dirty_spans.and_then(|(old_spans, new_spans)| {
            union_spans(
                span_hint(old_spans, y, width),
                span_hint(new_spans, y, width),
            )
        });
        let Some(scan_span) =
            scan_span.or_else(|| dirty_spans.is_none().then_some(full_span(width)))
        else {
            if profile.enabled {
                profile.hint_skipped_rows += 1;
            }
            continue;
        };

        let row_equality_start = profile.enabled.then(Instant::now);
        let row_unchanged = old
            .cells
            .get(y)
            .and_then(|old_row| old_row.get(scan_span.start..scan_span.end))
            .is_some_and(|old_cells| old_cells == &new_row[scan_span.start..scan_span.end]);
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
        for (offset, new_cell) in new_row[scan_span.start..scan_span.end].iter().enumerate() {
            let x = scan_span.start + offset;
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

fn span_hint(spans: &[Option<TouchedSpan>], row: usize, width: usize) -> Option<TouchedSpan> {
    spans.get(row).copied().unwrap_or(Some(full_span(width)))
}

fn full_span(width: usize) -> TouchedSpan {
    TouchedSpan {
        start: 0,
        end: width,
    }
}

fn union_spans(a: Option<TouchedSpan>, b: Option<TouchedSpan>) -> Option<TouchedSpan> {
    match (a, b) {
        (None, None) => None,
        (Some(span), None) | (None, Some(span)) => Some(span),
        (Some(a), Some(b)) => Some(TouchedSpan {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }),
    }
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

        let changes = diff_profiled_with_hints(
            &old,
            &new,
            false,
            Some((old.touched_spans(), new.touched_spans())),
        )
        .changes;
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }

    #[test]
    fn diff_marks_both_cells_when_wide_glyph_is_removed() {
        let mut old = Grid::new(3, 1);
        old.write_text(0, 0, "界", Some(rgb(255, 255, 255)), 1.0);
        let new = Grid::new(3, 1);

        let changes = diff_profiled_with_hints(
            &old,
            &new,
            false,
            Some((old.touched_spans(), new.touched_spans())),
        )
        .changes;
        let coords: Vec<(u16, u16)> = changes.iter().map(|c| (c.x, c.y)).collect();

        assert_eq!(coords, vec![(0, 0), (1, 0)]);
    }
}

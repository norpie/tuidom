//! Images drawn over their marker cells with sixel or iTerm2: which a frame draws, and their
//! bytes.
//!
//! An image is drawn again only when the frame has a marker cell for it that the last frame did
//! not, since that is the only way part of it can be missing from the screen. Text written over
//! an image replaces that part of it, so covering one needs no redraw.

use std::collections::{HashMap, HashSet};

use crate::image::iterm2::InlineImage;
use crate::image::sixel::SixelImage;
use crate::image::{ImageDrawing, PlacedImage, emit};
use crate::render::diff::CellChange;
use crate::render::grid::{CellContent, Grid};

/// One image to draw at a screen cell, and the cells inside its rectangle to write again after
/// it, since the image covered them.
#[derive(Debug)]
pub(crate) struct ImageDraw {
    pub x: u16,
    pub y: u16,
    pub bytes: Vec<u8>,
    pub cover: Vec<CellChange>,
}

/// The encoders' resampled images for the ids on screen, kept so a redraw only encodes a crop.
#[derive(Debug, Default)]
pub(crate) struct PlacedImages {
    sixel: HashMap<u32, ([u8; 3], SixelImage)>,
    inline: HashMap<u32, InlineImage>,
}

/// Where an image's marker cells are: its origin on screen, and the first and last column and
/// row of it that have one.
#[derive(Debug, Clone, Copy)]
struct Markers {
    origin: (i32, i32),
    first: (u16, u16),
    last: (u16, u16),
}

impl PlacedImages {
    /// The images `grid` needs drawn: every one with a marker cell among `changes`, or every
    /// one on screen when `changes` is `None` because the whole grid is being written. `placed`
    /// is in paint order, which the draws keep, so an image over another is drawn after it.
    pub fn draws(
        &mut self,
        grid: &Grid,
        changes: Option<&[CellChange]>,
        placed: &[PlacedImage],
        tmux: bool,
    ) -> Vec<ImageDraw> {
        let showing: HashSet<u32> = placed.iter().map(|image| image.id).collect();
        self.sixel.retain(|id, _| showing.contains(id));
        self.inline.retain(|id, _| showing.contains(id));

        let mut redraw: HashSet<u32> = match changes {
            Some(changes) => changes
                .iter()
                .filter_map(|change| marker_id(&change.cell.content))
                .collect(),
            None => showing,
        };
        if redraw.is_empty() {
            return Vec::new();
        }
        let markers = markers(grid);
        // Drawing an image covers whatever lies in its rectangle, other images' cells included,
        // so those are drawn again too.
        loop {
            let covered: Vec<u32> = markers
                .iter()
                .filter(|(id, _)| !redraw.contains(id))
                .filter(|(id, _)| {
                    redraw.iter().any(|drawn| {
                        markers
                            .get(drawn)
                            .is_some_and(|drawn| overlaps(grid, drawn, **id))
                    })
                })
                .map(|(id, _)| *id)
                .collect();
            if covered.is_empty() {
                break;
            }
            redraw.extend(covered);
        }

        placed
            .iter()
            .filter(|image| redraw.contains(&image.id))
            .filter_map(|image| {
                let found = markers.get(&image.id)?;
                let x = u16::try_from(found.origin.0 + i32::from(found.first.0)).ok()?;
                let y = u16::try_from(found.origin.1 + i32::from(found.first.1)).ok()?;
                let crop = (
                    found.first.0,
                    found.first.1,
                    found.last.0 - found.first.0 + 1,
                    found.last.1 - found.first.1 + 1,
                );
                let bytes = self.encode(image, crop, tmux)?;
                Some(ImageDraw {
                    x,
                    y,
                    bytes,
                    cover: cover(grid, x, y, crop.2, crop.3),
                })
            })
            .collect()
    }

    /// The bytes drawing `crop` of `image`, in its cells: column, row, columns and rows.
    fn encode(
        &mut self,
        image: &PlacedImage,
        crop: (u16, u16, u16, u16),
        tmux: bool,
    ) -> Option<Vec<u8>> {
        let fitting = &image.fitting;
        match image.drawing {
            ImageDrawing::Sixel => {
                let (cell_width, cell_height) = (u32::from(image.cell.0), u32::from(image.cell.1));
                let cached = self
                    .sixel
                    .get(&image.id)
                    .filter(|(background, _)| *background == image.background);
                if cached.is_none() {
                    let sixel = SixelImage::new(
                        &image.image,
                        fitting.source,
                        u32::from(fitting.width) * cell_width,
                        u32::from(fitting.height) * cell_height,
                        image.background,
                    );
                    self.sixel.insert(image.id, (image.background, sixel));
                }
                let (_, sixel) = self.sixel.get(&image.id)?;
                // tmux draws sixel itself, so it goes to tmux as it is.
                sixel.encode((
                    u32::from(crop.0) * cell_width,
                    u32::from(crop.1) * cell_height,
                    u32::from(crop.2) * cell_width,
                    u32::from(crop.3) * cell_height,
                ))
            }
            ImageDrawing::ITerm2 => {
                let inline = self
                    .inline
                    .entry(image.id)
                    .or_insert_with(|| InlineImage::new(&image.image, fitting, image.cell));
                let sequence = inline.encode(crop)?;
                let mut out = Vec::with_capacity(sequence.len());
                emit(&mut out, &sequence, tmux);
                Some(out)
            }
            ImageDrawing::Placeholders | ImageDrawing::HalfBlock => None,
        }
    }
}

fn marker_id(content: &CellContent) -> Option<u32> {
    match content {
        CellContent::Image { id, .. } => Some(*id),
        _ => None,
    }
}

fn markers(grid: &Grid) -> HashMap<u32, Markers> {
    let mut found: HashMap<u32, Markers> = HashMap::new();
    for (y, row) in grid.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let CellContent::Image { id, column, row } = cell.content else {
                continue;
            };
            found
                .entry(id)
                .and_modify(|markers| {
                    markers.first = (markers.first.0.min(column), markers.first.1.min(row));
                    markers.last = (markers.last.0.max(column), markers.last.1.max(row));
                })
                .or_insert(Markers {
                    origin: (x as i32 - i32::from(column), y as i32 - i32::from(row)),
                    first: (column, row),
                    last: (column, row),
                });
        }
    }
    found
}

/// Whether the rectangle `drawn` is drawn over holds a marker cell of image `id`.
fn overlaps(grid: &Grid, drawn: &Markers, id: u32) -> bool {
    let left = drawn.origin.0 + i32::from(drawn.first.0);
    let top = drawn.origin.1 + i32::from(drawn.first.1);
    let right = drawn.origin.0 + i32::from(drawn.last.0);
    let bottom = drawn.origin.1 + i32::from(drawn.last.1);
    (top..=bottom).any(|y| {
        (left..=right).any(|x| grid_cell_content(grid, x, y).and_then(marker_id) == Some(id))
    })
}

fn grid_cell_content(grid: &Grid, x: i32, y: i32) -> Option<&CellContent> {
    let row = grid.cells.get(usize::try_from(y).ok()?)?;
    Some(&row.get(usize::try_from(x).ok()?)?.content)
}

/// The cells in the rectangle that are not image markers, with the head of a wide glyph whose
/// second half is in it, since writing that half alone prints nothing.
fn cover(grid: &Grid, x: u16, y: u16, width: u16, height: u16) -> Vec<CellChange> {
    let mut cells = Vec::new();
    for row in y..y.saturating_add(height).min(grid.height) {
        for column in x..x.saturating_add(width).min(grid.width) {
            let cell = &grid.cells[usize::from(row)][usize::from(column)];
            let column = match cell.content {
                CellContent::Image { .. } => continue,
                CellContent::WideContinuation if column == x && column > 0 => column - 1,
                _ => column,
            };
            cells.push(CellChange {
                x: column,
                y: row,
                cell: grid.cells[usize::from(row)][usize::from(column)].clone(),
            });
        }
    }
    cells
}

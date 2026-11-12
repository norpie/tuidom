//! Decoded pixels for an image node to show.

use std::fmt;
use std::sync::Arc;

use crate::canvas::{CanvasBuffer, CanvasStyle};
use crate::style::ResolvedColor;
use crate::{Result, TuidomError};

/// How images are drawn, set for the whole document with
/// [`Document::set_image_protocol`](crate::Document::set_image_protocol).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Kitty placeholders where [`Graphics::placeholders`](crate::terminal::Graphics::placeholders)
    /// says the terminal supports them, half-blocks everywhere else, including before the
    /// terminal has been probed.
    #[default]
    Auto,
    /// Kitty placeholders whatever the terminal is detected as, for one known to support them
    /// that detection misses.
    Kitty,
    /// Half-block characters colored from the pixels, which any color terminal can show.
    HalfBlock,
}

/// What a frame draws images with, once [`ImageProtocol::Auto`] is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageDrawing {
    Placeholders,
    HalfBlock,
}

/// How an image fills its node's content box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// The whole image as large as fits with its aspect ratio kept, centered. The node's
    /// background shows in the space left over.
    #[default]
    Contain,
    /// The image keeps its aspect ratio and covers the whole box, cropped evenly on the
    /// sides that overflow.
    Cover,
    /// The whole image stretched to the box.
    Fill,
}

/// The cell size assumed when the terminal has not reported one, in pixels.
pub(crate) const ASSUMED_CELL_PIXELS: (u16, u16) = (8, 16);

/// Where an image lands in a content box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageFitting {
    /// Cells from the content box's left edge.
    pub x: u16,
    /// Cells from the content box's top edge.
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
    /// The part of the image shown, in pixels: x, y, width, height.
    pub source: (u32, u32, u32, u32),
}

/// Place an image of `image` pixels in a content box of `content` cells, each `cell` pixels.
/// `None` when either is empty, since nothing is drawn.
pub(crate) fn fit_image(
    image: (u32, u32),
    cell: (u16, u16),
    content: (u16, u16),
    fit: ImageFit,
) -> Option<ImageFitting> {
    let (image_width, image_height) = image;
    let (columns, rows) = content;
    if image_width == 0 || image_height == 0 || columns == 0 || rows == 0 {
        return None;
    }
    let cell_width = f64::from(cell.0.max(1));
    let cell_height = f64::from(cell.1.max(1));
    let box_width = f64::from(columns) * cell_width;
    let box_height = f64::from(rows) * cell_height;
    let horizontal = box_width / f64::from(image_width);
    let vertical = box_height / f64::from(image_height);
    let whole = (0, 0, image_width, image_height);

    Some(match fit {
        ImageFit::Fill => ImageFitting {
            x: 0,
            y: 0,
            width: columns,
            height: rows,
            source: whole,
        },
        ImageFit::Contain => {
            let scale = horizontal.min(vertical);
            let width = cells(f64::from(image_width) * scale / cell_width, columns);
            let height = cells(f64::from(image_height) * scale / cell_height, rows);
            ImageFitting {
                x: (columns - width) / 2,
                y: (rows - height) / 2,
                width,
                height,
                source: whole,
            }
        }
        ImageFit::Cover => {
            let scale = horizontal.max(vertical);
            let shown_width = pixels(box_width / scale, image_width);
            let shown_height = pixels(box_height / scale, image_height);
            ImageFitting {
                x: 0,
                y: 0,
                width: columns,
                height: rows,
                source: (
                    (image_width - shown_width) / 2,
                    (image_height - shown_height) / 2,
                    shown_width,
                    shown_height,
                ),
            }
        }
    })
}

/// An image's size in cells at its own resolution, rounded up so no pixel is cut.
pub(crate) fn intrinsic_cells(image: (u32, u32), cell: (u16, u16)) -> (f32, f32) {
    let axis = |pixels: u32, cell: u16| (f64::from(pixels) / f64::from(cell.max(1))).ceil() as f32;
    (axis(image.0, cell.0), axis(image.1, cell.1))
}

fn cells(value: f64, limit: u16) -> u16 {
    (value.round() as u16).clamp(1, limit)
}

fn pixels(value: f64, limit: u32) -> u32 {
    (value.round() as u32).clamp(1, limit)
}

/// An image node's half-block cells and what they were made from.
#[derive(Debug)]
pub(crate) struct HalfBlockImage {
    image: Arc<ImageData>,
    size: (u16, u16),
    source: (u32, u32, u32, u32),
    cells: Arc<CanvasBuffer>,
}

impl HalfBlockImage {
    /// The cells for `image` at `fitting`, reusing `cached` when it was made from the same
    /// image at the same size and crop.
    pub(crate) fn for_fitting(
        cached: Option<&Self>,
        image: &Arc<ImageData>,
        fitting: ImageFitting,
    ) -> Self {
        let cells = match cached {
            Some(cached)
                if Arc::ptr_eq(&cached.image, image)
                    && cached.size == (fitting.width, fitting.height)
                    && cached.source == fitting.source =>
            {
                cached.cells.clone()
            }
            _ => Arc::new(image.half_blocks(&fitting)),
        };
        Self {
            image: image.clone(),
            size: (fitting.width, fitting.height),
            source: fitting.source,
            cells,
        }
    }

    pub(crate) fn cells(&self) -> &Arc<CanvasBuffer> {
        &self.cells
    }
}

/// An image as RGBA pixels, eight bits per channel, rows top to bottom.
///
/// A zero-sized image is allowed and shows nothing.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ImageData {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl fmt::Debug for ImageData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageData")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ImageData {
    /// An image from `width` by `height` RGBA pixels.
    ///
    /// Errors with [`TuidomError::InvalidImageData`] unless `rgba` holds exactly four bytes
    /// per pixel.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(usize::MAX);
        if rgba.len() != expected {
            return Err(TuidomError::InvalidImageData {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Decode a PNG, JPEG, GIF or WebP file's bytes. A GIF gives its first frame.
    #[cfg(feature = "decode")]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let decoded = ::image::load_from_memory(bytes)
            .map_err(|error| TuidomError::ImageDecode(error.to_string()))?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        Self::from_rgba(width, height, decoded.into_raw())
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The `source` region (x, y, width, height) scaled to `width` by `height`, each pixel the
    /// average of the area it covers. Color is averaged weighted by alpha, so transparent
    /// pixels do not tint their neighbors.
    pub(crate) fn resample(
        &self,
        source: (u32, u32, u32, u32),
        width: u32,
        height: u32,
    ) -> Vec<[u8; 4]> {
        let mut out = vec![[0; 4]; width as usize * height as usize];
        let (left, top, source_width, source_height) = source;
        if source_width == 0
            || source_height == 0
            || left.saturating_add(source_width) > self.width
            || top.saturating_add(source_height) > self.height
        {
            return out;
        }
        let x_spans = spans(left, source_width, width);
        let y_spans = spans(top, source_height, height);
        for (ty, y_span) in y_spans.iter().enumerate() {
            for (tx, x_span) in x_spans.iter().enumerate() {
                let mut sum = [0.0f64; 4];
                let mut area = 0.0;
                for &(sy, wy) in y_span {
                    for &(sx, wx) in x_span {
                        let weight = wx * wy;
                        let [r, g, b, a] = self.pixel(sx, sy).map(f64::from);
                        let alpha = a * weight;
                        sum[0] += r * alpha;
                        sum[1] += g * alpha;
                        sum[2] += b * alpha;
                        sum[3] += alpha;
                        area += weight;
                    }
                }
                if sum[3] > 0.0 {
                    out[ty * width as usize + tx] = [
                        channel(sum[0] / sum[3]),
                        channel(sum[1] / sum[3]),
                        channel(sum[2] / sum[3]),
                        channel(sum[3] / area),
                    ];
                }
            }
        }
        out
    }

    /// The fitted image as half-block cells, two pixels to a cell: `▀` with the top pixel as
    /// glyph color and the bottom as background. A transparent half leaves the background
    /// unset, so what is behind shows there.
    pub(crate) fn half_blocks(&self, fitting: &ImageFitting) -> CanvasBuffer {
        let (columns, rows) = (fitting.width, fitting.height);
        let width = u32::from(columns);
        let pixels = self.resample(fitting.source, width, u32::from(rows) * 2);
        let mut buffer = CanvasBuffer::new(columns, rows);
        for row in 0..rows {
            for column in 0..columns {
                let at = |half: u16| {
                    let index = (usize::from(row) * 2 + usize::from(half)) * width as usize
                        + usize::from(column);
                    pixels.get(index).copied().unwrap_or_default()
                };
                let (top, bottom) = (at(0), at(1));
                let (symbol, fg, bg) = match (top[3], bottom[3]) {
                    (0, 0) => continue,
                    (0, _) => ("▄", bottom, None),
                    (_, 0) => ("▀", top, None),
                    _ => ("▀", top, Some(bottom)),
                };
                let style = CanvasStyle {
                    fg: Some(pixel_color(fg)),
                    bg: bg.map(pixel_color),
                    ..CanvasStyle::default()
                };
                buffer.set(column, row, symbol, style);
            }
        }
        buffer
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let start = (y as usize * self.width as usize + x as usize) * 4;
        match self.rgba.get(start..start + 4) {
            Some(&[r, g, b, a]) => [r, g, b, a],
            _ => [0; 4],
        }
    }
}

/// For each of `target` pixels along an axis, the `length` source pixels from `start` it
/// covers and how much of each, in source pixels.
fn spans(start: u32, length: u32, target: u32) -> Vec<Vec<(u32, f64)>> {
    let scale = f64::from(length) / f64::from(target);
    (0..target)
        .map(|t| {
            let from = f64::from(t) * scale;
            let to = f64::from(t + 1) * scale;
            let first = from.floor() as u32;
            let last = (to.ceil() as u32).min(length);
            (first..last)
                .filter_map(|s| {
                    let covered = to.min(f64::from(s + 1)) - from.max(f64::from(s));
                    (covered > 0.0).then_some((start + s, covered))
                })
                .collect()
        })
        .collect()
}

fn pixel_color([r, g, b, a]: [u8; 4]) -> ResolvedColor {
    ResolvedColor::from_srgba(r, g, b, a)
}

fn channel(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, pixels: &[[u8; 4]]) -> ImageData {
        ImageData::from_rgba(width, height, pixels.concat()).unwrap()
    }

    const CELL: (u16, u16) = (8, 16);

    #[test]
    fn contain_keeps_the_aspect_ratio_and_centers_with_the_odd_cell_last() {
        let wide = fit_image((160, 32), CELL, (10, 10), ImageFit::Contain).unwrap();
        assert_eq!((wide.x, wide.y, wide.width, wide.height), (0, 4, 10, 1));
        assert_eq!(wide.source, (0, 0, 160, 32));

        let tall = fit_image((8, 160), CELL, (5, 5), ImageFit::Contain).unwrap();
        assert_eq!((tall.x, tall.y, tall.width, tall.height), (2, 0, 1, 5));
    }

    #[test]
    fn fill_stretches_the_whole_image_over_the_box() {
        let fitting = fit_image((160, 32), CELL, (4, 7), ImageFit::Fill).unwrap();
        assert_eq!(
            fitting,
            ImageFitting {
                x: 0,
                y: 0,
                width: 4,
                height: 7,
                source: (0, 0, 160, 32),
            }
        );
    }

    #[test]
    fn cover_fills_the_box_with_a_centered_crop() {
        let fitting = fit_image((160, 160), CELL, (10, 2), ImageFit::Cover).unwrap();
        assert_eq!(
            (fitting.x, fitting.y, fitting.width, fitting.height),
            (0, 0, 10, 2)
        );
        assert_eq!(fitting.source, (0, 48, 160, 64));
    }

    #[test]
    fn nothing_fits_an_empty_image_or_box() {
        for fit in [ImageFit::Contain, ImageFit::Cover, ImageFit::Fill] {
            assert_eq!(fit_image((0, 10), CELL, (4, 4), fit), None);
            assert_eq!(fit_image((10, 10), CELL, (4, 0), fit), None);
        }
    }

    #[test]
    fn intrinsic_size_rounds_partial_cells_up() {
        assert_eq!(intrinsic_cells((80, 64), CELL), (10.0, 4.0));
        assert_eq!(intrinsic_cells((81, 65), CELL), (11.0, 5.0));
    }

    #[test]
    fn bytes_must_match_the_size() {
        assert_eq!(
            ImageData::from_rgba(2, 1, vec![0; 7]),
            Err(TuidomError::InvalidImageData {
                expected: 8,
                actual: 7
            })
        );
        assert!(ImageData::from_rgba(0, 3, Vec::new()).is_ok());
    }

    #[test]
    fn resampling_to_the_same_size_keeps_every_pixel() {
        let pixels = [
            [1, 2, 3, 255],
            [40, 50, 60, 128],
            [7, 8, 9, 0],
            [200, 0, 100, 255],
        ];
        let resampled = image(2, 2, &pixels).resample((0, 0, 2, 2), 2, 2);
        assert_eq!(resampled[..2], pixels[..2]);
        assert_eq!(resampled[2], [0; 4]);
        assert_eq!(resampled[3], pixels[3]);
    }

    #[test]
    fn downscaling_averages_the_covered_pixels() {
        let source = image(
            2,
            2,
            &[
                [0, 0, 0, 255],
                [100, 0, 0, 255],
                [0, 200, 0, 255],
                [0, 0, 40, 255],
            ],
        );
        assert_eq!(source.resample((0, 0, 2, 2), 1, 1), [[25, 50, 10, 255]]);

        let row = image(3, 1, &[[0, 0, 0, 255], [90, 0, 0, 255], [180, 0, 0, 255]]);
        assert_eq!(
            row.resample((0, 0, 3, 1), 2, 1),
            [[30, 0, 0, 255], [150, 0, 0, 255]]
        );
    }

    #[test]
    fn transparent_pixels_do_not_tint_the_average() {
        let source = image(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 0]]);
        assert_eq!(source.resample((0, 0, 2, 1), 1, 1), [[255, 0, 0, 128]]);
    }

    #[test]
    fn resampling_reads_only_the_source_region() {
        let source = image(3, 1, &[[200, 0, 0, 255], [0, 100, 0, 255], [0, 0, 50, 255]]);
        assert_eq!(source.resample((1, 0, 2, 1), 1, 1), [[0, 50, 25, 255]]);
    }

    #[test]
    fn upscaling_repeats_pixels() {
        let source = image(2, 1, &[[10, 20, 30, 255], [40, 50, 60, 255]]);
        assert_eq!(
            source.resample((0, 0, 2, 1), 4, 2),
            [
                [10, 20, 30, 255],
                [10, 20, 30, 255],
                [40, 50, 60, 255],
                [40, 50, 60, 255],
            ]
            .repeat(2)
        );
    }

    #[test]
    fn an_empty_image_resamples_to_transparent() {
        let empty = ImageData::from_rgba(0, 0, Vec::new()).unwrap();
        assert_eq!(empty.resample((0, 0, 0, 0), 2, 1), [[0; 4]; 2]);
        let outside = image(1, 1, &[[9, 9, 9, 255]]);
        assert_eq!(outside.resample((0, 0, 2, 1), 1, 1), [[0; 4]]);
    }

    #[cfg(feature = "decode")]
    #[test]
    fn a_png_decodes_to_its_pixels() {
        use std::io::Cursor;

        use ::image::{ImageFormat, RgbaImage};

        let pixels = [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0]];
        let mut png = Vec::new();
        RgbaImage::from_raw(3, 1, pixels.concat())
            .unwrap()
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        assert_eq!(ImageData::from_bytes(&png), Ok(image(3, 1, &pixels)));
        assert!(matches!(
            ImageData::from_bytes(b"not an image"),
            Err(TuidomError::ImageDecode(_))
        ));
    }
}

//! Decoded pixels for an image node to show.

use std::fmt;

use crate::{Result, TuidomError};

/// An image as RGBA pixels, eight bits per channel, rows top to bottom.
///
/// A zero-sized image is allowed and shows nothing.
#[derive(Clone, PartialEq, Eq)]
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

    /// The image scaled to `width` by `height`, each pixel the average of the source area it
    /// covers. Color is averaged weighted by alpha, so transparent pixels do not tint their
    /// neighbors.
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn resample(&self, width: u32, height: u32) -> Vec<[u8; 4]> {
        let mut out = vec![[0; 4]; width as usize * height as usize];
        if self.width == 0 || self.height == 0 {
            return out;
        }
        let x_spans = spans(self.width, width);
        let y_spans = spans(self.height, height);
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

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let start = (y as usize * self.width as usize + x as usize) * 4;
        match self.rgba.get(start..start + 4) {
            Some(&[r, g, b, a]) => [r, g, b, a],
            _ => [0; 4],
        }
    }
}

/// For each of `target` cells along an axis of `source` pixels, the source pixels it covers
/// and how much of each, in source pixels.
fn spans(source: u32, target: u32) -> Vec<Vec<(u32, f64)>> {
    let scale = f64::from(source) / f64::from(target);
    (0..target)
        .map(|t| {
            let start = f64::from(t) * scale;
            let end = f64::from(t + 1) * scale;
            let first = start.floor() as u32;
            let last = (end.ceil() as u32).min(source);
            (first..last)
                .filter_map(|s| {
                    let covered = end.min(f64::from(s + 1)) - start.max(f64::from(s));
                    (covered > 0.0).then_some((s, covered))
                })
                .collect()
        })
        .collect()
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
        let resampled = image(2, 2, &pixels).resample(2, 2);
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
        assert_eq!(source.resample(1, 1), [[25, 50, 10, 255]]);

        let row = image(3, 1, &[[0, 0, 0, 255], [90, 0, 0, 255], [180, 0, 0, 255]]);
        assert_eq!(row.resample(2, 1), [[30, 0, 0, 255], [150, 0, 0, 255]]);
    }

    #[test]
    fn transparent_pixels_do_not_tint_the_average() {
        let source = image(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 0]]);
        assert_eq!(source.resample(1, 1), [[255, 0, 0, 128]]);
    }

    #[test]
    fn upscaling_repeats_pixels() {
        let source = image(2, 1, &[[10, 20, 30, 255], [40, 50, 60, 255]]);
        assert_eq!(
            source.resample(4, 2),
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
        assert_eq!(empty.resample(2, 1), [[0; 4]; 2]);
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

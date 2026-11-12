//! iTerm2 inline images for images drawn over cells.
//!
//! The terminal stretches the PNG it is sent over the cells it is given, so an image is sent at
//! its own resolution where that is smaller than the cells' pixels, and is never enlarged here.
#![cfg_attr(not(test), expect(dead_code))]

use flate2::Crc;

use super::{ImageData, ImageFitting, base64, compress};

/// An image resampled for inline image output, at a whole number of pixels per cell so any
/// crop along cell edges is whole pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineImage {
    width: u32,
    per_cell: (u32, u32),
    pixels: Vec<[u8; 4]>,
}

impl InlineImage {
    /// `image` as `fitting` shows it, with cells of `cell` pixels. Each cell gets as many pixels
    /// as the source has for it, rounded up, and at most the cell's own.
    pub fn new(image: &ImageData, fitting: &ImageFitting, cell: (u16, u16)) -> Self {
        let (_, _, source_width, source_height) = fitting.source;
        let per_cell = (
            source_width
                .div_ceil(u32::from(fitting.width.max(1)))
                .clamp(1, u32::from(cell.0.max(1))),
            source_height
                .div_ceil(u32::from(fitting.height.max(1)))
                .clamp(1, u32::from(cell.1.max(1))),
        );
        let width = u32::from(fitting.width) * per_cell.0;
        let height = u32::from(fitting.height) * per_cell.1;
        Self {
            width,
            per_cell,
            pixels: image.resample(fitting.source, width, height),
        }
    }

    /// The inline image sequence drawing `crop` of the image, as column, row, columns and rows
    /// of its cells, stretched over that many cells. `None` when the crop is empty or outside
    /// the image.
    pub fn encode(&self, crop: (u16, u16, u16, u16)) -> Option<Vec<u8>> {
        let (column, row, columns, rows) = crop;
        let (cell_width, cell_height) = self.per_cell;
        let left = u32::from(column) * cell_width;
        let top = u32::from(row) * cell_height;
        let width = u32::from(columns) * cell_width;
        let height = u32::from(rows) * cell_height;
        let image_height = self.pixels.len() as u32 / self.width.max(1);
        if width == 0 || height == 0 || left + width > self.width || top + height > image_height {
            return None;
        }
        let mut scanlines = Vec::with_capacity((height * (width * 4 + 1)) as usize);
        for y in top..top + height {
            let start = (y * self.width + left) as usize;
            scanlines.push(0);
            scanlines.extend_from_slice(self.pixels[start..start + width as usize].as_flattened());
        }
        let png = png(width, height, &scanlines)?;
        Some(
            format!(
                "\x1b]1337;File=inline=1;size={};width={columns};height={rows};\
                 preserveAspectRatio=0:{}\x07",
                png.len(),
                base64(&png)
            )
            .into_bytes(),
        )
    }
}

/// An 8-bit RGBA PNG of `scanlines`: each row its filter byte, always none, then its pixels.
fn png(width: u32, height: u32, scanlines: &[u8]) -> Option<Vec<u8>> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, color type 6 (RGBA), default compression, filtering and no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &compress(scanlines)?);
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sent {
        arguments: Vec<(String, String)>,
        png: Vec<u8>,
    }

    impl Sent {
        fn argument(&self, key: &str) -> &str {
            self.arguments
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
                .unwrap()
        }
    }

    fn base64_decode(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let values: Vec<u32> = text
            .bytes()
            .filter(|&byte| byte != b'=')
            .map(|byte| ALPHABET.iter().position(|&a| a == byte).unwrap() as u32)
            .collect();
        let mut out = Vec::new();
        for group in values.chunks(4) {
            let bits = group
                .iter()
                .enumerate()
                .fold(0, |bits, (index, value)| bits | value << (18 - 6 * index));
            for index in 0..group.len() - 1 {
                out.push((bits >> (16 - 8 * index)) as u8);
            }
        }
        out
    }

    fn parse(bytes: &[u8]) -> Sent {
        let text = std::str::from_utf8(bytes).unwrap();
        let body = text
            .strip_prefix("\x1b]1337;File=")
            .and_then(|body| body.strip_suffix('\x07'))
            .unwrap();
        let (arguments, payload) = body.split_once(':').unwrap();
        Sent {
            arguments: arguments
                .split(';')
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap();
                    (key.to_string(), value.to_string())
                })
                .collect(),
            png: base64_decode(payload),
        }
    }

    fn image(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> ImageData {
        let rgba = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| pixel(x, y))
            .collect();
        ImageData::from_rgba(width, height, rgba).unwrap()
    }

    fn fitting(width: u16, height: u16, source: (u32, u32, u32, u32)) -> ImageFitting {
        ImageFitting {
            x: 0,
            y: 0,
            width,
            height,
            source,
        }
    }

    #[test]
    fn the_sequence_names_the_cells_and_the_png_size() {
        let image = image(4, 4, |_, _| [255, 0, 0, 255]);
        let inline = InlineImage::new(&image, &fitting(2, 1, (0, 0, 4, 4)), (8, 16));
        let sent = parse(&inline.encode((0, 0, 2, 1)).unwrap());
        assert_eq!(sent.argument("inline"), "1");
        assert_eq!(sent.argument("width"), "2");
        assert_eq!(sent.argument("height"), "1");
        assert_eq!(sent.argument("preserveAspectRatio"), "0");
        assert_eq!(sent.argument("size"), sent.png.len().to_string());
    }

    #[test]
    fn a_small_image_is_sent_at_its_own_size_and_a_large_one_at_the_cells() {
        let small = image(4, 6, |_, _| [0, 0, 255, 255]);
        let inline = InlineImage::new(&small, &fitting(2, 3, (0, 0, 4, 6)), (8, 16));
        assert_eq!(inline.per_cell, (2, 2));

        let large = image(400, 300, |_, _| [0, 0, 255, 255]);
        let inline = InlineImage::new(&large, &fitting(10, 5, (0, 0, 400, 300)), (8, 16));
        assert_eq!(inline.per_cell, (8, 16));
    }

    #[test]
    fn an_empty_or_outside_crop_sends_nothing() {
        let image = image(4, 4, |_, _| [255; 4]);
        let inline = InlineImage::new(&image, &fitting(2, 2, (0, 0, 4, 4)), (8, 16));
        assert_eq!(inline.encode((0, 0, 0, 2)), None);
        assert_eq!(inline.encode((1, 0, 2, 2)), None);
        assert_eq!(inline.encode((0, 2, 2, 1)), None);
    }

    #[cfg(feature = "decode")]
    mod decoded {
        use super::*;

        fn decoded(bytes: &[u8]) -> ImageData {
            ImageData::from_bytes(&parse(bytes).png).unwrap()
        }

        #[test]
        fn the_png_holds_the_pixels_with_their_alpha() {
            let pixel = |x: u32, y: u32| [(x * 60) as u8, (y * 60) as u8, 7, (40 + x * 70) as u8];
            let image = image(4, 4, pixel);
            let inline = InlineImage::new(&image, &fitting(2, 2, (0, 0, 4, 4)), (8, 16));
            let png = decoded(&inline.encode((0, 0, 2, 2)).unwrap());
            assert_eq!((png.width(), png.height()), (4, 4));
            assert_eq!(png, image);
        }

        #[test]
        fn a_crop_is_that_part_of_the_image() {
            let pixel = |x: u32, y: u32| [(x * 30) as u8, (y * 30) as u8, 0, 255];
            let image = image(8, 6, pixel);
            let inline = InlineImage::new(&image, &fitting(4, 3, (0, 0, 8, 6)), (8, 16));
            let png = decoded(&inline.encode((1, 1, 2, 2)).unwrap());
            let expected = self::image(4, 4, |x, y| pixel(x + 2, y + 2));
            assert_eq!(png, expected);
        }
    }
}

//! Sixel output for images drawn over cells.
//!
//! Sixel draws six pixel rows at a time, so every height here is a multiple of six: a band
//! reaching past an image's last cell row would draw into the row below it.
#![cfg_attr(not(test), expect(dead_code))]

use std::fmt::Write;

use super::ImageData;

const BAND: u32 = 6;
const MAX_COLORS: usize = 256;
/// Colors are counted at five bits per channel before the palette is chosen.
const CHANNEL_BITS: u32 = 5;
const BUCKETS: usize = 1 << (3 * CHANNEL_BITS);

/// An image resampled for sixel output and reduced to at most 256 colors, from which any crop
/// can be encoded without resampling again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SixelImage {
    width: u32,
    height: u32,
    palette: Vec<[u8; 3]>,
    /// A palette index per pixel, row by row; `None` where the pixel is transparent.
    pixels: Vec<Option<u8>>,
}

impl SixelImage {
    /// `source` of `image` resampled to `width` by `height` pixels, the height rounded down to
    /// a multiple of six. Partly transparent pixels are blended with `background`, since sixel
    /// pixels are either drawn or not; fully transparent ones are left undrawn.
    pub fn new(
        image: &ImageData,
        source: (u32, u32, u32, u32),
        width: u32,
        height: u32,
        background: [u8; 3],
    ) -> Self {
        let height = height / BAND * BAND;
        let colors: Vec<Option<[u8; 3]>> = image
            .resample(source, width, height)
            .into_iter()
            .map(|[r, g, b, a]| match a {
                0 => None,
                255 => Some([r, g, b]),
                _ => Some(blend([r, g, b], background, a)),
            })
            .collect();
        let (palette, lookup) = palette(&colors);
        let pixels = colors
            .iter()
            .map(|color| color.map(|color| lookup[bucket(color)]))
            .collect();
        Self {
            width,
            height,
            palette,
            pixels,
        }
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The sixel sequence drawing `crop` of the image, as x, y, width and height in its pixels,
    /// with the height rounded down to a multiple of six. `None` when that leaves nothing.
    pub fn encode(&self, crop: (u32, u32, u32, u32)) -> Option<Vec<u8>> {
        let (left, top, width, height) = crop;
        let width = width.min(self.width.saturating_sub(left)) as usize;
        let height = height.min(self.height.saturating_sub(top)) / BAND * BAND;
        if width == 0 || height == 0 {
            return None;
        }

        // P2=1 leaves pixels no color is drawn on as they were.
        let mut out = format!("\x1bP0;1;0q\"1;1;{width};{height}");
        for (index, [r, g, b]) in self.palette.iter().enumerate() {
            let _ = write!(
                out,
                "#{index};2;{};{};{}",
                percent(*r),
                percent(*g),
                percent(*b)
            );
        }

        let colors = self.palette.len();
        let mut bits = vec![0u8; colors * width];
        let mut present = vec![false; colors];
        let bands = (top..top + height).step_by(BAND as usize);
        let last_band = bands.len().saturating_sub(1);
        for (band, band_top) in bands.enumerate() {
            for dy in 0..BAND {
                let start = ((band_top + dy) * self.width + left) as usize;
                for (x, pixel) in self.pixels[start..start + width].iter().enumerate() {
                    if let Some(index) = pixel {
                        let index = usize::from(*index);
                        present[index] = true;
                        bits[index * width + x] |= 1 << dy;
                    }
                }
            }
            let mut first = true;
            for (index, used) in present.iter_mut().enumerate() {
                if !*used {
                    continue;
                }
                *used = false;
                if !first {
                    out.push('$');
                }
                first = false;
                let _ = write!(out, "#{index}");
                let row = &mut bits[index * width..(index + 1) * width];
                let end = row
                    .iter()
                    .rposition(|&sixel| sixel != 0)
                    .map_or(0, |x| x + 1);
                push_runs(&mut out, &row[..end]);
                row.fill(0);
            }
            // A line feed after the last band would move the cursor into the row below.
            if band < last_band {
                out.push('-');
            }
        }
        out.push_str("\x1b\\");
        Some(out.into_bytes())
    }
}

fn blend(color: [u8; 3], background: [u8; 3], alpha: u8) -> [u8; 3] {
    let alpha = u32::from(alpha);
    [0, 1, 2].map(|channel| {
        let mixed =
            u32::from(color[channel]) * alpha + u32::from(background[channel]) * (255 - alpha);
        ((mixed + 127) / 255) as u8
    })
}

fn percent(channel: u8) -> u32 {
    (u32::from(channel) * 100 + 127) / 255
}

fn bucket([r, g, b]: [u8; 3]) -> usize {
    let shift = 8 - CHANNEL_BITS;
    (usize::from(r >> shift) << (2 * CHANNEL_BITS))
        | (usize::from(g >> shift) << CHANNEL_BITS)
        | usize::from(b >> shift)
}

fn bucket_channel(bucket: usize, channel: usize) -> usize {
    (bucket >> ((2 - channel) * CHANNEL_BITS as usize)) & ((1 << CHANNEL_BITS) - 1)
}

/// Choose at most 256 colors by median cut over the opaque pixels' buckets. Returns the palette
/// and the palette index of every bucket. A bucket holding a single color keeps it exactly.
fn palette(colors: &[Option<[u8; 3]>]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let mut count = vec![0u64; BUCKETS];
    let mut sum = vec![[0u64; 3]; BUCKETS];
    for &color in colors.iter().flatten() {
        let bucket = bucket(color);
        count[bucket] += 1;
        for (total, channel) in sum[bucket].iter_mut().zip(color) {
            *total += u64::from(channel);
        }
    }

    let used: Vec<usize> = (0..BUCKETS).filter(|&bucket| count[bucket] > 0).collect();
    let mut boxes = if used.is_empty() { vec![] } else { vec![used] };
    while boxes.len() < MAX_COLORS {
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, buckets)| buckets.len() > 1)
            .map(|(index, buckets)| {
                let (channel, range) = widest_channel(buckets);
                (index, channel, range)
            })
            .max_by_key(|&(_, _, range)| range);
        let Some((index, channel, _)) = widest else {
            break;
        };
        let mut low = boxes.swap_remove(index);
        low.sort_unstable_by_key(|&bucket| bucket_channel(bucket, channel));
        let total: u64 = low.iter().map(|&bucket| count[bucket]).sum();
        let mut seen = 0;
        let median = low
            .iter()
            .position(|&bucket| {
                seen += count[bucket];
                seen * 2 >= total
            })
            .unwrap_or(0);
        let high = low.split_off((median + 1).clamp(1, low.len() - 1));
        boxes.push(low);
        boxes.push(high);
    }

    let mut lookup = vec![0u8; BUCKETS];
    let palette = boxes
        .iter()
        .enumerate()
        .map(|(index, buckets)| {
            let mut pixels = 0;
            let mut total = [0u64; 3];
            for &bucket in buckets {
                lookup[bucket] = index as u8;
                pixels += count[bucket];
                for (total, channel) in total.iter_mut().zip(sum[bucket]) {
                    *total += channel;
                }
            }
            total.map(|total| ((total + pixels / 2) / pixels.max(1)) as u8)
        })
        .collect();
    (palette, lookup)
}

fn widest_channel(buckets: &[usize]) -> (usize, usize) {
    (0..3)
        .map(|channel| {
            let values = buckets
                .iter()
                .map(|&bucket| bucket_channel(bucket, channel));
            let range = values.clone().max().unwrap_or(0) - values.min().unwrap_or(0);
            (channel, range)
        })
        .max_by_key(|&(_, range)| range)
        .unwrap_or((0, 0))
}

/// Append sixel characters for `sixels`, a run of four or more as `!count` and the character.
fn push_runs(out: &mut String, sixels: &[u8]) {
    let mut rest = sixels;
    while let Some(&sixel) = rest.first() {
        let run = rest.iter().take_while(|&&next| next == sixel).count();
        let character = char::from(63 + sixel);
        if run > 3 {
            let _ = write!(out, "!{run}{character}");
        } else {
            for _ in 0..run {
                out.push(character);
            }
        }
        rest = &rest[run..];
    }
}

#[cfg(test)]
mod tests {
    use std::iter::Peekable;
    use std::str::Chars;

    use super::*;

    struct Decoded {
        width: usize,
        height: usize,
        pixels: Vec<Option<[u8; 3]>>,
    }

    impl Decoded {
        fn at(&self, x: usize, y: usize) -> Option<[u8; 3]> {
            self.pixels[y * self.width + x]
        }
    }

    fn number(chars: &mut Peekable<Chars>) -> usize {
        let mut value = 0;
        while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
            value = value * 10 + digit as usize;
            chars.next();
        }
        value
    }

    /// A sixel reader covering what the encoder writes.
    fn decode(bytes: &[u8]) -> Decoded {
        let text = std::str::from_utf8(bytes).unwrap();
        let body = text
            .strip_prefix("\x1bP0;1;0q")
            .and_then(|body| body.strip_suffix("\x1b\\"))
            .unwrap();
        let mut chars = body.chars().peekable();
        assert_eq!(chars.next(), Some('"'));
        let numbers: Vec<usize> = (0..4)
            .map(|index| {
                if index > 0 {
                    assert_eq!(chars.next(), Some(';'));
                }
                number(&mut chars)
            })
            .collect();
        let (width, height) = (numbers[2], numbers[3]);
        let mut decoded = Decoded {
            width,
            height,
            pixels: vec![None; width * height],
        };
        let mut palette = vec![[0u8; 3]; 256];
        let (mut x, mut y, mut color) = (0, 0, 0);
        let draw = |decoded: &mut Decoded, x: usize, y: usize, color: [u8; 3], sixel: u8| {
            for dy in 0..6 {
                if sixel & 1 << dy != 0 {
                    decoded.pixels[(y + dy) * decoded.width + x] = Some(color);
                }
            }
        };
        while let Some(c) = chars.next() {
            match c {
                '#' => {
                    color = number(&mut chars);
                    if chars.peek() == Some(&';') {
                        chars.next();
                        assert_eq!(number(&mut chars), 2);
                        let rgb = [0, 1, 2].map(|_| {
                            assert_eq!(chars.next(), Some(';'));
                            ((number(&mut chars) * 255 + 50) / 100) as u8
                        });
                        palette[color] = rgb;
                    }
                }
                '!' => {
                    let run = number(&mut chars);
                    let sixel = chars.next().unwrap() as u8 - 63;
                    for _ in 0..run {
                        draw(&mut decoded, x, y, palette[color], sixel);
                        x += 1;
                    }
                }
                '$' => x = 0,
                '-' => {
                    x = 0;
                    y += 6;
                }
                '?'..='~' => {
                    draw(&mut decoded, x, y, palette[color], c as u8 - 63);
                    x += 1;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        decoded
    }

    fn image(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> ImageData {
        let rgba = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| pixel(x, y))
            .collect();
        ImageData::from_rgba(width, height, rgba).unwrap()
    }

    fn sixel(image: &ImageData, background: [u8; 3]) -> SixelImage {
        let (width, height) = (image.width(), image.height());
        SixelImage::new(image, (0, 0, width, height), width, height, background)
    }

    const COLORS: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ];

    fn close(actual: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| a.abs_diff(e) <= tolerance)
    }

    #[test]
    fn a_few_flat_colors_decode_exactly() {
        let image = image(5, 12, |x, y| COLORS[((x + y) % 4) as usize]);
        let decoded = decode(&sixel(&image, [0; 3]).encode((0, 0, 5, 12)).unwrap());
        assert_eq!((decoded.width, decoded.height), (5, 12));
        for y in 0..12 {
            for x in 0..5 {
                let [r, g, b, _] = COLORS[(x + y) % 4];
                assert_eq!(decoded.at(x, y), Some([r, g, b]), "{x},{y}");
            }
        }
    }

    #[test]
    fn the_height_is_rounded_down_to_whole_bands() {
        let image = image(3, 14, |_, _| COLORS[0]);
        let sixel = SixelImage::new(&image, (0, 0, 3, 14), 3, 14, [0; 3]);
        assert_eq!(sixel.height(), 12);
        let bytes = sixel.encode((0, 0, 3, 14)).unwrap();
        assert_eq!(decode(&bytes).height, 12);
        assert!(!String::from_utf8(bytes).unwrap().ends_with("-\x1b\\"));
    }

    #[test]
    fn transparent_pixels_are_left_undrawn() {
        let image = image(
            4,
            6,
            |x, _| {
                if x % 2 == 0 { [0, 0, 0, 0] } else { COLORS[2] }
            },
        );
        let decoded = decode(&sixel(&image, [255; 3]).encode((0, 0, 4, 6)).unwrap());
        for y in 0..6 {
            assert_eq!(decoded.at(0, y), None);
            assert_eq!(decoded.at(1, y), Some([0, 0, 255]));
        }
    }

    #[test]
    fn partly_transparent_pixels_are_blended_with_the_background() {
        let image = image(2, 6, |_, _| [255, 0, 0, 128]);
        let decoded = decode(&sixel(&image, [0, 0, 255]).encode((0, 0, 2, 6)).unwrap());
        let blended = decoded.at(1, 3).unwrap();
        assert!(close(blended, [128, 0, 127], 3), "{blended:?}");
    }

    #[test]
    fn more_than_256_colors_are_reduced_to_a_close_palette() {
        let gradient = |x: u32, y: u32| [(x * 4) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255];
        let image = image(64, 48, gradient);
        let sixel = sixel(&image, [0; 3]);
        assert_eq!(sixel.palette.len(), 256);
        let decoded = decode(&sixel.encode((0, 0, 64, 48)).unwrap());
        for y in 0..48 {
            for x in 0..64 {
                let [r, g, b, _] = gradient(x, y);
                let actual = decoded.at(x as usize, y as usize).unwrap();
                assert!(close(actual, [r, g, b], 24), "{x},{y}: {actual:?}");
            }
        }
    }

    #[test]
    fn a_crop_decodes_as_that_part_of_the_whole() {
        let image = image(9, 18, |x, y| COLORS[((x * 3 + y) % 4) as usize]);
        let sixel = sixel(&image, [0; 3]);
        let whole = decode(&sixel.encode((0, 0, 9, 18)).unwrap());
        let crop = decode(&sixel.encode((2, 5, 4, 12)).unwrap());
        assert_eq!((crop.width, crop.height), (4, 12));
        for y in 0..12 {
            for x in 0..4 {
                assert_eq!(crop.at(x, y), whole.at(x + 2, y + 5), "{x},{y}");
            }
        }
    }

    #[test]
    fn a_crop_shorter_than_a_band_or_outside_the_image_draws_nothing() {
        let sixel = sixel(&image(4, 12, |_, _| COLORS[0]), [0; 3]);
        assert_eq!(sixel.encode((0, 0, 4, 5)), None);
        assert_eq!(sixel.encode((4, 0, 4, 12)), None);
        assert_eq!(sixel.encode((0, 8, 4, 12)), None);
    }

    #[test]
    fn long_runs_are_compressed() {
        let bytes = sixel(&image(40, 6, |_, _| COLORS[3]), [0; 3])
            .encode((0, 0, 40, 6))
            .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("#0!40~"), "{text:?}");
    }
}

use palette::{IntoColor, Oklch, Srgb};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Final color format sent to the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// A color in the OKLCH color space, backed by [`palette::Oklch`].
///
/// All colors are stored internally as OKLCH (Lightness, Chroma, Hue, Alpha).
/// Conversion to RGB happens only at render time, with results cached globally.
///
/// # Creating colors
///
/// ```ignore
/// Color::white()
/// Color::black()
/// Color::oklch(0.55, 0.20, 260.0)  // Purple-blue
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Lightness (0–1)
    l: f32,
    /// Chroma (0–~0.37)
    c: f32,
    /// Hue in degrees (0–360)
    h: f32,
    /// Alpha (0–1)
    a: f32,
}

impl Color {
    /// White.
    pub fn white() -> Self {
        Self { l: 1.0, c: 0.0, h: 0.0, a: 1.0 }
    }

    /// Black.
    pub fn black() -> Self {
        Self { l: 0.0, c: 0.0, h: 0.0, a: 1.0 }
    }

    /// Red.
    pub fn red() -> Self {
        Self::from_srgb(255, 0, 0)
    }

    /// Green.
    pub fn green() -> Self {
        Self::from_srgb(0, 255, 0)
    }

    /// Blue.
    pub fn blue() -> Self {
        Self::from_srgb(0, 0, 255)
    }

    /// Cyan.
    pub fn cyan() -> Self {
        Self::from_srgb(0, 255, 255)
    }

    /// Magenta.
    pub fn magenta() -> Self {
        Self::from_srgb(255, 0, 255)
    }

    /// Yellow.
    pub fn yellow() -> Self {
        Self::from_srgb(255, 255, 0)
    }

    /// Create a color from OKLCH components.
    ///
    /// - `l`: lightness (0–1)
    /// - `c`: chroma (0–~0.37)
    /// - `h`: hue in degrees (0–360)
    pub fn oklch(l: f64, c: f64, h: f64) -> Self {
        Self { l: l as f32, c: c as f32, h: h as f32, a: 1.0 }
    }

    /// Create a color from OKLCH with alpha.
    pub fn oklcha(l: f64, c: f64, h: f64, a: f64) -> Self {
        Self { l: l as f32, c: c as f32, h: h as f32, a: a as f32 }
    }

    /// Convert to terminal-ready RGB.
    ///
    /// Results are cached globally — repeated conversions of the same
    /// color hit the cache.
    pub(crate) fn to_rgb(self) -> Rgb {
        let key = CacheKey {
            l: self.l.to_bits(),
            c: self.c.to_bits(),
            h: self.h.to_bits(),
            a: self.a.to_bits(),
        };

        {
            let cache = RGB_CACHE.lock().expect("RGB cache lock poisoned");
            if let Some(&rgb) = cache.get(&key) {
                return rgb;
            }
        }

        let oklch = Oklch::new(self.l, self.c, self.h);
        let srgb: Srgb = oklch.into_color();
        let srgb_u8: Srgb<u8> = srgb.into_format();

        let rgb = Rgb {
            r: srgb_u8.red,
            g: srgb_u8.green,
            b: srgb_u8.blue,
            a: (self.a * 255.0).round().clamp(0.0, 255.0) as u8,
        };

        RGB_CACHE
            .lock()
            .expect("RGB cache lock poisoned")
            .insert(key, rgb);

        rgb
    }

    /// Create a [`Color`] from sRGB components.
    fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        let srgb: Srgb = Srgb::new(r, g, b).into_format();
        let oklch: Oklch = srgb.into_color();
        Self {
            l: oklch.l,
            c: oklch.chroma,
            h: oklch.hue.into_degrees(),
            a: 1.0,
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::black()
    }
}

// ---------------------------------------------------------------------------
// Conversion cache
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    l: u32,
    c: u32,
    h: u32,
    a: u32,
}

static RGB_CACHE: LazyLock<Mutex<HashMap<CacheKey, Rgb>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_is_white() {
        let rgb = Color::white().to_rgb();
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 255);
        assert_eq!(rgb.b, 255);
    }

    #[test]
    fn black_is_black() {
        let rgb = Color::black().to_rgb();
        assert_eq!(rgb.r, 0);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn named_colors_roundtrip() {
        // Colors defined from sRGB should convert back correctly.
        for (color, r, g, b) in [
            (Color::red(), 255, 0, 0),
            (Color::green(), 0, 255, 0),
            (Color::blue(), 0, 0, 255),
            (Color::cyan(), 0, 255, 255),
            (Color::magenta(), 255, 0, 255),
            (Color::yellow(), 255, 255, 0),
        ] {
            let rgb = color.to_rgb();
            assert_eq!((rgb.r, rgb.g, rgb.b), (r, g, b), "color: {color:?}");
        }
    }

    #[test]
    fn cache_hit() {
        let c = Color::oklch(0.5, 0.1, 180.0);
        let rgb1 = c.to_rgb();
        let rgb2 = c.to_rgb();
        assert_eq!(rgb1.r, rgb2.r);
        assert_eq!(rgb1.g, rgb2.g);
        assert_eq!(rgb1.b, rgb2.b);
    }
}

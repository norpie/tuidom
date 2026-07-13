use std::collections::HashMap;

use palette::{IntoColor, Oklch, Srgb};

/// Final color format sent to the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// A concrete color in the OKLCH color space, backed by [`palette::Oklch`].
///
/// This is what a [`Color`] evaluates to once variables and node-relative references have been
/// resolved, and it is what [`ResolvedStyle`](crate::style::ResolvedStyle) holds. Conversion to
/// RGB happens only at render time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedColor {
    /// Lightness (0–1)
    l: f32,
    /// Chroma (0–~0.37)
    c: f32,
    /// Hue in degrees (0–360)
    h: f32,
    /// Alpha (0–1)
    a: f32,
}

impl ResolvedColor {
    /// White.
    pub fn white() -> Self {
        Self {
            l: 1.0,
            c: 0.0,
            h: 0.0,
            a: 1.0,
        }
    }

    /// Black.
    pub fn black() -> Self {
        Self {
            l: 0.0,
            c: 0.0,
            h: 0.0,
            a: 1.0,
        }
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
        Self {
            l: l as f32,
            c: c as f32,
            h: h as f32,
            a: 1.0,
        }
    }

    /// Create a color from OKLCH with alpha.
    pub fn oklcha(l: f64, c: f64, h: f64, a: f64) -> Self {
        Self {
            l: l as f32,
            c: c as f32,
            h: h as f32,
            a: a as f32,
        }
    }

    /// Create a [`ResolvedColor`] from sRGB components.
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

impl Default for ResolvedColor {
    fn default() -> Self {
        Self::black()
    }
}

/// A color as written in a [`Style`](crate::style::Style).
///
/// A color is an expression, not a value: it can name a variable or refer to the node it is used
/// on, neither of which means anything until the style is resolved against a specific node. It
/// evaluates to a [`ResolvedColor`] during style resolution.
///
/// # Creating colors
///
/// ```ignore
/// Color::white()
/// Color::black()
/// Color::oklch(0.55, 0.20, 260.0)  // Purple-blue
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Color {
    /// A concrete OKLCH color.
    Literal(ResolvedColor),
}

impl Color {
    /// White.
    pub fn white() -> Self {
        Self::Literal(ResolvedColor::white())
    }

    /// Black.
    pub fn black() -> Self {
        Self::Literal(ResolvedColor::black())
    }

    /// Red.
    pub fn red() -> Self {
        Self::Literal(ResolvedColor::red())
    }

    /// Green.
    pub fn green() -> Self {
        Self::Literal(ResolvedColor::green())
    }

    /// Blue.
    pub fn blue() -> Self {
        Self::Literal(ResolvedColor::blue())
    }

    /// Cyan.
    pub fn cyan() -> Self {
        Self::Literal(ResolvedColor::cyan())
    }

    /// Magenta.
    pub fn magenta() -> Self {
        Self::Literal(ResolvedColor::magenta())
    }

    /// Yellow.
    pub fn yellow() -> Self {
        Self::Literal(ResolvedColor::yellow())
    }

    /// Create a color from OKLCH components.
    ///
    /// - `l`: lightness (0–1)
    /// - `c`: chroma (0–~0.37)
    /// - `h`: hue in degrees (0–360)
    pub fn oklch(l: f64, c: f64, h: f64) -> Self {
        Self::Literal(ResolvedColor::oklch(l, c, h))
    }

    /// Create a color from OKLCH with alpha.
    pub fn oklcha(l: f64, c: f64, h: f64, a: f64) -> Self {
        Self::Literal(ResolvedColor::oklcha(l, c, h, a))
    }

    /// Evaluate this color to a concrete [`ResolvedColor`].
    pub(crate) fn eval(&self) -> Option<ResolvedColor> {
        match self {
            Self::Literal(color) => Some(*color),
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::black()
    }
}

impl From<ResolvedColor> for Color {
    fn from(color: ResolvedColor) -> Self {
        Self::Literal(color)
    }
}

// ---------------------------------------------------------------------------
// Conversion cache
// ---------------------------------------------------------------------------

/// Read-through cache for converting resolved colors to terminal RGB.
#[derive(Debug, Default)]
pub(crate) struct RgbCache {
    entries: HashMap<CacheKey, Rgb>,
}

impl RgbCache {
    /// Create an empty RGB cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to terminal-ready RGB, caching repeated conversions.
    pub fn resolve(&mut self, color: ResolvedColor) -> Rgb {
        let key = CacheKey::from(color);
        if let Some(&rgb) = self.entries.get(&key) {
            return rgb;
        }

        let rgb = color_to_rgb(color);
        self.entries.insert(key, rgb);
        rgb
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    l: u32,
    c: u32,
    h: u32,
    a: u32,
}

impl From<ResolvedColor> for CacheKey {
    fn from(color: ResolvedColor) -> Self {
        Self {
            l: color.l.to_bits(),
            c: color.c.to_bits(),
            h: color.h.to_bits(),
            a: color.a.to_bits(),
        }
    }
}

fn color_to_rgb(color: ResolvedColor) -> Rgb {
    let oklch = Oklch::new(color.l, color.c, color.h);
    let srgb: Srgb = oklch.into_color();
    let srgb_u8: Srgb<u8> = srgb.into_format();

    Rgb {
        r: srgb_u8.red,
        g: srgb_u8.green,
        b: srgb_u8.blue,
        a: (color.a * 255.0).round().clamp(0.0, 255.0) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_is_white() {
        let mut cache = RgbCache::new();
        let rgb = cache.resolve(ResolvedColor::white());
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 255);
        assert_eq!(rgb.b, 255);
    }

    #[test]
    fn black_is_black() {
        let mut cache = RgbCache::new();
        let rgb = cache.resolve(ResolvedColor::black());
        assert_eq!(rgb.r, 0);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn named_colors_roundtrip() {
        // Colors defined from sRGB should convert back correctly.
        let mut cache = RgbCache::new();
        for (color, r, g, b) in [
            (ResolvedColor::red(), 255, 0, 0),
            (ResolvedColor::green(), 0, 255, 0),
            (ResolvedColor::blue(), 0, 0, 255),
            (ResolvedColor::cyan(), 0, 255, 255),
            (ResolvedColor::magenta(), 255, 0, 255),
            (ResolvedColor::yellow(), 255, 255, 0),
        ] {
            let rgb = cache.resolve(color);
            assert_eq!((rgb.r, rgb.g, rgb.b), (r, g, b), "color: {color:?}");
        }
    }

    #[test]
    fn cache_hit() {
        let c = ResolvedColor::oklch(0.5, 0.1, 180.0);
        let mut cache = RgbCache::new();

        let rgb1 = cache.resolve(c);
        let rgb2 = cache.resolve(c);

        assert_eq!(rgb1, rgb2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn literal_colors_evaluate_to_themselves() {
        assert_eq!(Color::red().eval(), Some(ResolvedColor::red()));
        assert_eq!(
            Color::oklcha(0.5, 0.1, 180.0, 0.5).eval(),
            Some(ResolvedColor::oklcha(0.5, 0.1, 180.0, 0.5))
        );
    }
}

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
        Self::new(l as f32, c as f32, h as f32, 1.0)
    }

    /// Create a color from OKLCH with alpha.
    pub fn oklcha(l: f64, c: f64, h: f64, a: f64) -> Self {
        Self::new(l as f32, c as f32, h as f32, a as f32)
    }

    /// Create a color, canonicalizing its hue.
    ///
    /// Hue is an angle, so 400°, 40°, and -320° name one color. Storing it canonically is what
    /// makes two equal colors compare equal and share one [`RgbCache`] entry, however they were
    /// built — sRGB conversion produces hues in -180..180, while an operation may land anywhere.
    fn new(l: f32, c: f32, h: f32, a: f32) -> Self {
        Self {
            l,
            c,
            h: h.rem_euclid(360.0),
            a,
        }
    }

    /// Create a [`ResolvedColor`] from sRGB components.
    fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        let srgb: Srgb = Srgb::new(r, g, b).into_format();
        let oklch: Oklch = srgb.into_color();
        Self::new(oklch.l, oklch.chroma, oklch.hue.into_degrees(), 1.0)
    }

    /// Whether this color has no meaningful hue, so mixing must borrow the other color's.
    fn is_achromatic(self) -> bool {
        self.c <= ACHROMATIC_CHROMA
    }

    /// Blend toward `other`, component-wise in OKLCH.
    pub(crate) fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        // A gray has no hue to interpolate, so interpolating one would swing the result through
        // unrelated hues: mixing white into blue would pass through green. Borrow the other
        // color's hue instead, and let chroma alone carry the transition.
        let (from, to) = match (self.is_achromatic(), other.is_achromatic()) {
            (true, false) => (other.h, other.h),
            (false, true) => (self.h, self.h),
            _ => (self.h, other.h),
        };

        // Take the short way around the hue circle.
        let mut delta = to.rem_euclid(360.0) - from.rem_euclid(360.0);
        if delta > 180.0 {
            delta -= 360.0;
        } else if delta < -180.0 {
            delta += 360.0;
        }

        Self {
            l: lerp(self.l, other.l, t),
            c: lerp(self.c, other.c, t),
            h: (from + delta * t).rem_euclid(360.0),
            a: lerp(self.a, other.a, t),
        }
    }
}

/// Chroma at or below which a color is a gray and its hue carries no information.
const ACHROMATIC_CHROMA: f32 = 1e-4;

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// An operation applied to a [`Color`], evaluated during style resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorOp {
    /// Raise lightness by an absolute amount.
    Lighten(f32),
    /// Lower lightness by an absolute amount.
    Darken(f32),
    /// Replace lightness.
    WithLightness(f32),
    /// Replace chroma.
    WithChroma(f32),
    /// Replace hue, in degrees.
    WithHue(f32),
    /// Replace alpha.
    WithAlpha(f32),
    /// Blend toward another color, `t` of the way.
    Mix {
        /// The color to blend toward. Itself an expression, so you can mix two variables.
        other: Box<Color>,
        /// How far to blend, 0–1.
        t: f32,
    },
}

impl ColorOp {
    /// Apply this operation to a concrete color.
    ///
    /// Returns `None` when the operation names something unresolvable — only [`Mix`], whose
    /// operand is itself an expression, can do that.
    ///
    /// [`Mix`]: ColorOp::Mix
    fn apply(&self, base: ResolvedColor, ctx: &ColorContext) -> Option<ResolvedColor> {
        Some(match self {
            // Lightness steps are absolute rather than proportional: OKLCH's L is perceptually
            // uniform, which is the whole reason for the color space, so `darken(0.1)` should be
            // the same visual step wherever it starts from.
            Self::Lighten(amount) => ResolvedColor {
                l: (base.l + amount).clamp(0.0, 1.0),
                ..base
            },
            Self::Darken(amount) => ResolvedColor {
                l: (base.l - amount).clamp(0.0, 1.0),
                ..base
            },
            Self::WithLightness(l) => ResolvedColor {
                l: l.clamp(0.0, 1.0),
                ..base
            },
            // Chroma has no fixed ceiling — the sRGB conversion gamut-clips it at render time.
            Self::WithChroma(c) => ResolvedColor {
                c: c.max(0.0),
                ..base
            },
            Self::WithHue(h) => ResolvedColor {
                h: h.rem_euclid(360.0),
                ..base
            },
            Self::WithAlpha(a) => ResolvedColor {
                a: a.clamp(0.0, 1.0),
                ..base
            },
            Self::Mix { other, t } => base.mix(other.eval(ctx)?, *t),
        })
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
    /// A named color variable, resolved from the node's inherited variable scope.
    Var(String),
    /// The background the node sits on — its own if it has one, otherwise the nearest ancestor's,
    /// falling back to the document's declared terminal background.
    CurrentBg,
    /// The node's own resolved foreground color.
    CurrentFg,
    /// Another color with an operation applied to it.
    Derived {
        /// The color the operation applies to.
        base: Box<Color>,
        /// The operation.
        op: ColorOp,
    },
}

/// What a [`Color`] expression is evaluated against.
///
/// A color expression means nothing on its own — it names variables that only exist relative to
/// some node in the tree. This is that node's view.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ColorContext<'a> {
    /// The variables in scope, inherited from ancestors and the document.
    pub vars: &'a HashMap<String, ResolvedColor>,
    /// What [`Color::CurrentBg`] resolves to here.
    pub current_bg: ResolvedColor,
    /// What [`Color::CurrentFg`] resolves to here.
    pub current_fg: ResolvedColor,
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

    /// Reference a named color variable.
    ///
    /// The name resolves against the variable scope of whatever node the style is used on:
    /// variables declared on the node's ancestors, falling back to the document's. A name nothing
    /// defines makes the whole expression unresolvable, and the property using it falls back to
    /// its default.
    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    /// Apply an operation to this color, producing a derived color.
    fn derive(self, op: ColorOp) -> Self {
        Self::Derived {
            base: Box::new(self),
            op,
        }
    }

    /// Raise lightness by an absolute amount.
    pub fn lighten(self, amount: f64) -> Self {
        self.derive(ColorOp::Lighten(amount as f32))
    }

    /// Lower lightness by an absolute amount.
    pub fn darken(self, amount: f64) -> Self {
        self.derive(ColorOp::Darken(amount as f32))
    }

    /// Replace lightness (0–1).
    pub fn with_lightness(self, l: f64) -> Self {
        self.derive(ColorOp::WithLightness(l as f32))
    }

    /// Replace chroma (0–~0.37).
    pub fn with_chroma(self, c: f64) -> Self {
        self.derive(ColorOp::WithChroma(c as f32))
    }

    /// Replace hue, in degrees.
    pub fn with_hue(self, h: f64) -> Self {
        self.derive(ColorOp::WithHue(h as f32))
    }

    /// Replace alpha (0–1).
    pub fn with_alpha(self, a: f64) -> Self {
        self.derive(ColorOp::WithAlpha(a as f32))
    }

    /// Blend toward another color, `t` of the way (0–1).
    pub fn mix(self, other: Color, t: f64) -> Self {
        self.derive(ColorOp::Mix {
            other: Box::new(other),
            t: t as f32,
        })
    }

    /// Evaluate this color to a concrete [`ResolvedColor`] against a node's context.
    ///
    /// `None` means the expression names something that does not resolve.
    pub(crate) fn eval(&self, ctx: &ColorContext) -> Option<ResolvedColor> {
        match self {
            Self::Literal(color) => Some(*color),
            Self::Var(name) => ctx.vars.get(name).copied(),
            Self::CurrentBg => Some(ctx.current_bg),
            Self::CurrentFg => Some(ctx.current_fg),
            Self::Derived { base, op } => op.apply(base.eval(ctx)?, ctx),
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

    fn eval_in(color: &Color, vars: &HashMap<String, ResolvedColor>) -> Option<ResolvedColor> {
        color.eval(&ColorContext {
            vars,
            current_bg: ResolvedColor::black(),
            current_fg: ResolvedColor::white(),
        })
    }

    fn eval(color: Color) -> ResolvedColor {
        eval_in(&color, &HashMap::new()).expect("expression should resolve")
    }

    #[test]
    fn literal_colors_evaluate_to_themselves() {
        assert_eq!(eval(Color::red()), ResolvedColor::red());
        assert_eq!(
            eval(Color::oklcha(0.5, 0.1, 180.0, 0.5)),
            ResolvedColor::oklcha(0.5, 0.1, 180.0, 0.5)
        );
    }

    #[test]
    fn a_variable_resolves_from_the_scope() {
        let vars = HashMap::from([("--primary".to_string(), ResolvedColor::red())]);
        assert_eq!(
            eval_in(&Color::var("--primary"), &vars),
            Some(ResolvedColor::red())
        );
    }

    #[test]
    fn an_undefined_variable_makes_the_whole_expression_unresolvable() {
        let vars = HashMap::new();
        assert_eq!(eval_in(&Color::var("--nope"), &vars), None);
        // Not "darken of some fallback" — a typo'd name must not half-apply a derivation.
        assert_eq!(eval_in(&Color::var("--nope").darken(0.1), &vars), None);
        assert_eq!(
            eval_in(&Color::white().mix(Color::var("--nope"), 0.5), &vars),
            None
        );
    }

    #[test]
    fn derivations_apply_to_variables() {
        let vars = HashMap::from([(
            "--primary".to_string(),
            ResolvedColor::oklch(0.5, 0.1, 180.0),
        )]);
        let resolved = eval_in(&Color::var("--primary").darken(0.2), &vars)
            .expect("defined variable should resolve");
        assert!((resolved.l - 0.3).abs() < 1e-6);
    }

    #[test]
    fn hue_is_stored_canonically() {
        // One hue, three spellings. They must be one color — otherwise equal colors compare
        // unequal and the RGB cache holds a duplicate entry for each spelling.
        assert_eq!(
            ResolvedColor::oklch(0.5, 0.1, 400.0),
            ResolvedColor::oklch(0.5, 0.1, 40.0)
        );
        assert_eq!(
            ResolvedColor::oklch(0.5, 0.1, -320.0),
            ResolvedColor::oklch(0.5, 0.1, 40.0)
        );
    }

    #[test]
    fn lighten_and_darken_step_lightness() {
        let base = Color::oklch(0.5, 0.1, 180.0);
        assert_eq!(eval(base.clone().lighten(0.2)).l, 0.7);
        assert_eq!(eval(base.darken(0.2)).l, 0.3);
    }

    #[test]
    fn lightness_steps_clamp_at_the_ends() {
        assert_eq!(eval(Color::oklch(0.9, 0.1, 180.0).lighten(0.5)).l, 1.0);
        assert_eq!(eval(Color::oklch(0.1, 0.1, 180.0).darken(0.5)).l, 0.0);
    }

    #[test]
    fn with_operations_replace_single_components() {
        let base = Color::oklch(0.5, 0.1, 180.0);
        assert_eq!(eval(base.clone().with_lightness(0.8)).l, 0.8);
        assert_eq!(eval(base.clone().with_chroma(0.2)).c, 0.2);
        assert_eq!(eval(base.clone().with_alpha(0.25)).a, 0.25);
        // Hue wraps rather than clamping — 400° is 40°.
        assert_eq!(eval(base.with_hue(400.0)).h, 40.0);
    }

    #[test]
    fn derivations_chain() {
        let derived = Color::oklch(0.5, 0.1, 180.0)
            .darken(0.2)
            .with_alpha(0.5)
            .lighten(0.1);
        let resolved = eval(derived);
        assert!((resolved.l - 0.4).abs() < 1e-6);
        assert_eq!(resolved.a, 0.5);
    }

    #[test]
    fn mix_interpolates_component_wise() {
        let a = Color::oklcha(0.2, 0.0, 0.0, 1.0);
        let b = Color::oklcha(0.8, 0.0, 0.0, 0.0);
        let mixed = eval(a.mix(b, 0.5));
        assert!((mixed.l - 0.5).abs() < 1e-6);
        assert!((mixed.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mix_takes_the_short_way_around_the_hue_circle() {
        // 350° to 10° is 20° forward, not 340° backward.
        let mixed = eval(Color::oklch(0.5, 0.1, 350.0).mix(Color::oklch(0.5, 0.1, 10.0), 0.5));
        assert!((mixed.h - 0.0).abs() < 1e-4, "hue was {}", mixed.h);
    }

    #[test]
    fn mixing_with_a_gray_keeps_the_chromatic_hue() {
        // A gray has no hue. Interpolating its nominal 0° would swing the result through unrelated
        // hues — white mixed into blue would come out green — so chroma alone carries the mix.
        let blue = ResolvedColor::blue();
        let mixed = eval(Color::white().mix(Color::blue(), 0.5));
        assert!(
            (mixed.h - blue.h).abs() < 1e-4,
            "expected blue's hue {}, got {}",
            blue.h,
            mixed.h
        );
        assert!((mixed.c - blue.c / 2.0).abs() < 1e-6);
    }

    #[test]
    fn mix_clamps_its_ratio() {
        let a = Color::oklch(0.2, 0.0, 0.0);
        let b = Color::oklch(0.8, 0.0, 0.0);
        assert_eq!(eval(a.clone().mix(b.clone(), 2.0)).l, 0.8);
        assert_eq!(eval(a.mix(b, -1.0)).l, 0.2);
    }
}

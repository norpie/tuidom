//! Animation types — easing curves, transition properties, and configuration.

pub(crate) mod driver;
mod keyframes;
pub(crate) mod value;

pub use keyframes::{AnimatableProperty, AnimationDirection, AnimationHandle, KeyframeAnimation};
pub(crate) use keyframes::{ResolvedKeyframes, ResolvedTrack};

use std::time::Duration;

/// Easing / interpolation curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Constant rate.
    Linear,
    /// Slow start, then fast.
    EaseIn,
    /// Fast start, then slow.
    EaseOut,
    /// Slow start and end, fast middle.
    EaseInOut,
    /// A CSS-style cubic bézier curve through `(0,0)`, `(x1,y1)`, `(x2,y2)`, `(1,1)`.
    ///
    /// The x coordinates are clamped to 0–1 so progress along the curve stays a
    /// function of time, exactly as `cubic-bezier()` requires.
    CubicBezier(f64, f64, f64, f64),
}

/// Properties that can be transitioned (animated).
///
/// One entry per interpolable style property. Discrete properties (border style,
/// booleans, text content) are unrepresentable here, so a non-animatable
/// transition cannot be configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionProperty {
    /// Opacity (0–1).
    Opacity,
    /// Background color, interpolated in OKLCH. Animates only between two set
    /// backgrounds — a change to or from transparent snaps.
    Background,
    /// Foreground text color, interpolated in OKLCH.
    Foreground,
    /// Border color, interpolated in OKLCH. Animates only between two explicitly
    /// set border colors — an unset border color follows `Foreground` instead.
    BorderColor,
    /// Absolute-position offsets. Animates only between two `Position::Absolute`
    /// values — a change to or from `Flow` snaps.
    Position,
    /// Width. Animates cell-to-cell or percent-to-percent — a change across
    /// units, or to or from `Auto`, snaps.
    Width,
    /// Height, with the same unit rules as `Width`.
    Height,
    /// Padding, all four edges together.
    Padding,
    /// Margin, all four edges together.
    Margin,
}

impl TransitionProperty {
    /// Whether an in-flight value of this property moves layout, so animation
    /// ticks must feed the layout engine rather than only the paint pass.
    pub(crate) fn affects_layout(self) -> bool {
        matches!(
            self,
            Self::Position | Self::Width | Self::Height | Self::Padding | Self::Margin
        )
    }
}

/// Configuration for a property transition.
///
/// Set on a node to declare that changes to this property should animate
/// over the given duration with the given easing.
#[derive(Debug, Clone)]
pub struct TransitionConfig {
    /// Which property this config applies to.
    pub property: TransitionProperty,
    /// How long the transition lasts.
    pub duration: Duration,
    /// The easing curve.
    pub easing: Easing,
}

impl TransitionConfig {
    /// Create a transition configuration for any animatable property.
    pub fn new(property: TransitionProperty, duration: Duration, easing: Easing) -> Self {
        Self {
            property,
            duration,
            easing,
        }
    }

    /// Convenience constructor for an opacity transition.
    pub fn opacity(duration: Duration, easing: Easing) -> Self {
        Self::new(TransitionProperty::Opacity, duration, easing)
    }
}

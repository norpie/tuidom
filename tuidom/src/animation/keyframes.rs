//! Keyframe animations — multi-step property animations with iteration control.

use std::time::Duration;

use crate::animation::value::AnimatedValue;
use crate::animation::{Easing, TransitionProperty};
use crate::style::{Color, EdgeInsets, Length};

/// A typed property value inside a keyframe.
///
/// One variant per animatable property, carrying its value — a non-animatable
/// property (border style, text content, a boolean) is unrepresentable, so a
/// keyframe cannot be written that the engine would have to ignore.
///
/// Colors are [`Color`] expressions, evaluated once against the animated node's
/// scope when [`Document::animate`](crate::Document::animate) is called; an
/// expression that does not resolve there contributes nothing. A `Length::Auto`
/// size is likewise skipped — like CSS, `auto` cannot be interpolated.
#[derive(Debug, Clone)]
pub enum AnimatableProperty {
    /// Opacity (0–1).
    Opacity(f64),
    /// Background color.
    Background(Color),
    /// Foreground text color.
    Foreground(Color),
    /// Border color.
    BorderColor(Color),
    /// Absolute-position offsets from the parent's box origin.
    Position {
        /// Horizontal offset in terminal cells.
        x: i32,
        /// Vertical offset in terminal cells.
        y: i32,
    },
    /// Width.
    Width(Length),
    /// Height.
    Height(Length),
    /// Padding, all four edges.
    Padding(EdgeInsets),
    /// Margin, all four edges.
    Margin(EdgeInsets),
}

impl AnimatableProperty {
    /// The transition property this value belongs to.
    pub(crate) fn property(&self) -> TransitionProperty {
        match self {
            Self::Opacity(_) => TransitionProperty::Opacity,
            Self::Background(_) => TransitionProperty::Background,
            Self::Foreground(_) => TransitionProperty::Foreground,
            Self::BorderColor(_) => TransitionProperty::BorderColor,
            Self::Position { .. } => TransitionProperty::Position,
            Self::Width(_) => TransitionProperty::Width,
            Self::Height(_) => TransitionProperty::Height,
            Self::Padding(_) => TransitionProperty::Padding,
            Self::Margin(_) => TransitionProperty::Margin,
        }
    }
}

/// Direction keyframes play in across iterations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationDirection {
    /// Every iteration plays from 0% to 100%.
    #[default]
    Normal,
    /// Every iteration plays from 100% to 0%.
    Reverse,
    /// Odd iterations play forward, even iterations play backward.
    Alternate,
}

/// A multi-step property animation: keyframes at percentages, played over a
/// duration for a number of iterations.
///
/// Built with the fluent methods and started with
/// [`Document::animate`](crate::Document::animate). Easing applies per keyframe
/// segment, like CSS. A property missing from the 0% or 100% keyframe uses the
/// node's underlying resolved value as the implicit endpoint.
///
/// While an animation runs, its values apply on top of any transition overrides
/// — animations win on conflict, as in CSS. When it ends it is removed and the
/// node returns to its underlying style; hold the end state by setting it as
/// the node's style from an `on_animation_end` handler.
#[derive(Debug, Clone)]
pub struct KeyframeAnimation {
    pub(crate) keyframes: Vec<(f64, Vec<AnimatableProperty>)>,
    pub(crate) duration: Duration,
    pub(crate) easing: Easing,
    /// `None` means infinite.
    pub(crate) iterations: Option<u32>,
    pub(crate) direction: AnimationDirection,
}

impl KeyframeAnimation {
    /// Create an animation with the given duration and no keyframes yet.
    ///
    /// Defaults: linear easing, one iteration, normal direction.
    pub fn new(duration: Duration) -> Self {
        Self {
            keyframes: Vec::new(),
            duration,
            easing: Easing::Linear,
            iterations: Some(1),
            direction: AnimationDirection::Normal,
        }
    }

    /// Shorthand for a two-state animation: `from` at 0%, `to` at 100%.
    pub fn from_to(
        duration: Duration,
        from: impl IntoIterator<Item = AnimatableProperty>,
        to: impl IntoIterator<Item = AnimatableProperty>,
    ) -> Self {
        Self::new(duration).keyframe(0.0, from).keyframe(100.0, to)
    }

    /// Add a keyframe at `percent` (0–100, clamped) with the given values.
    pub fn keyframe(
        mut self,
        percent: f64,
        values: impl IntoIterator<Item = AnimatableProperty>,
    ) -> Self {
        let percent = if percent.is_finite() {
            percent.clamp(0.0, 100.0)
        } else {
            0.0
        };
        self.keyframes.push((percent, values.into_iter().collect()));
        self
    }

    /// Set the easing curve, applied per keyframe segment.
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Set a finite iteration count. Zero iterations end immediately.
    pub fn iterations(mut self, count: u32) -> Self {
        self.iterations = Some(count);
        self
    }

    /// Repeat forever, until cancelled or the node is removed.
    pub fn infinite(mut self) -> Self {
        self.iterations = None;
        self
    }

    /// Set the playback direction across iterations.
    pub fn direction(mut self, direction: AnimationDirection) -> Self {
        self.direction = direction;
        self
    }
}

/// Opaque handle to a running keyframe animation.
///
/// Returned by [`Document::animate`](crate::Document::animate) and passed to
/// the pause/resume/cancel methods. Carries the document identity, so a handle
/// from one document never controls an animation in another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnimationHandle {
    pub(crate) document_id: u64,
    pub(crate) id: u64,
}

/// One property's keyframe track, resolved and sorted: colors evaluated, values
/// typed, percentages normalized to 0–1.
pub(crate) type ResolvedTrack = Vec<(f64, AnimatedValue)>;

/// A keyframe animation with every value resolved at `animate` time.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedKeyframes {
    pub tracks: Vec<(TransitionProperty, ResolvedTrack)>,
    pub duration: Duration,
    pub easing: Easing,
    pub iterations: Option<u32>,
    pub direction: AnimationDirection,
}

impl ResolvedKeyframes {
    /// Whether any track animates a layout-affecting property.
    pub fn affects_layout(&self) -> bool {
        self.tracks
            .iter()
            .any(|(property, _)| property.affects_layout())
    }
}

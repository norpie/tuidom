//! Animation types — easing curves, transition properties, and configuration.

pub(crate) mod driver;

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
}

/// Properties that can be transitioned (animated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionProperty {
    /// Opacity (0–1).
    Opacity,
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
    /// Convenience constructor for an opacity transition.
    pub fn opacity(duration: Duration, easing: Easing) -> Self {
        Self {
            property: TransitionProperty::Opacity,
            duration,
            easing,
        }
    }
}

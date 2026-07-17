//! Typed animated values — extraction from and application to resolved styles,
//! and per-type interpolation.

use crate::animation::TransitionProperty;
use crate::style::resolution::ResolvedStyle;
use crate::style::{EdgeInsets, Length, Position, ResolvedColor};

/// A value in flight between two resolved-style states.
///
/// One variant per interpolation shape, not per property: width and height are
/// both a cell count, padding and margin both four of them. A property maps to
/// a variant through [`extract_animated_value`], and two values interpolate only
/// when they share a variant — a `Pixels` width cannot animate to a `Percent`
/// one, so that change snaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnimatedValue {
    /// A unit scalar (opacity).
    Float(f64),
    /// A concrete OKLCH color.
    Color(ResolvedColor),
    /// A terminal-cell count, kept fractional in flight and rounded on application.
    Cells(f64),
    /// A percentage of the parent's content area.
    Percent(f64),
    /// Absolute-position offsets from the parent's box origin.
    Offset { x: f64, y: f64 },
    /// Four edge sizes, kept fractional in flight and rounded on application.
    Insets {
        top: f64,
        right: f64,
        bottom: f64,
        left: f64,
    },
}

impl AnimatedValue {
    /// Interpolate toward `other` at `t` (0–1).
    ///
    /// Returns `None` for mismatched variants — the caller never starts such a
    /// transition, so this is a defensive dead end rather than a reachable path.
    pub fn lerp(self, other: Self, t: f64) -> Option<Self> {
        match (self, other) {
            (Self::Float(a), Self::Float(b)) => Some(Self::Float(lerp(a, b, t))),
            (Self::Color(a), Self::Color(b)) => Some(Self::Color(a.mix(b, t as f32))),
            (Self::Cells(a), Self::Cells(b)) => Some(Self::Cells(lerp(a, b, t))),
            (Self::Percent(a), Self::Percent(b)) => Some(Self::Percent(lerp(a, b, t))),
            (Self::Offset { x: ax, y: ay }, Self::Offset { x: bx, y: by }) => Some(Self::Offset {
                x: lerp(ax, bx, t),
                y: lerp(ay, by, t),
            }),
            (
                Self::Insets {
                    top: at,
                    right: ar,
                    bottom: ab,
                    left: al,
                },
                Self::Insets {
                    top: bt,
                    right: br,
                    bottom: bb,
                    left: bl,
                },
            ) => Some(Self::Insets {
                top: lerp(at, bt, t),
                right: lerp(ar, br, t),
                bottom: lerp(ab, bb, t),
                left: lerp(al, bl, t),
            }),
            _ => None,
        }
    }

    /// Whether two values share a variant and can interpolate.
    pub fn compatible(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    /// Whether two values are close enough to make a transition pointless.
    pub fn approx_eq(self, other: Self) -> bool {
        match (self, other) {
            (Self::Float(a), Self::Float(b))
            | (Self::Cells(a), Self::Cells(b))
            | (Self::Percent(a), Self::Percent(b)) => (a - b).abs() < f64::EPSILON,
            (Self::Color(a), Self::Color(b)) => a == b,
            (Self::Offset { x: ax, y: ay }, Self::Offset { x: bx, y: by }) => {
                (ax - bx).abs() < f64::EPSILON && (ay - by).abs() < f64::EPSILON
            }
            (
                Self::Insets {
                    top: at,
                    right: ar,
                    bottom: ab,
                    left: al,
                },
                Self::Insets {
                    top: bt,
                    right: br,
                    bottom: bb,
                    left: bl,
                },
            ) => {
                (at - bt).abs() < f64::EPSILON
                    && (ar - br).abs() < f64::EPSILON
                    && (ab - bb).abs() < f64::EPSILON
                    && (al - bl).abs() < f64::EPSILON
            }
            _ => false,
        }
    }
}

/// Read a property's current animatable value from a resolved style.
///
/// `None` means the property has no interpolable value in this state — an unset
/// background, an `Auto` size, a `Flow` position — and a change to or from such
/// a state snaps instead of transitioning, the way CSS cannot animate `auto`.
pub(crate) fn extract_animated_value(
    resolved: &ResolvedStyle,
    property: TransitionProperty,
) -> Option<AnimatedValue> {
    match property {
        TransitionProperty::Opacity => Some(AnimatedValue::Float(resolved.opacity)),
        TransitionProperty::Background => resolved.background.map(AnimatedValue::Color),
        TransitionProperty::Foreground => Some(AnimatedValue::Color(resolved.color)),
        TransitionProperty::BorderColor => resolved.border_color.map(AnimatedValue::Color),
        TransitionProperty::Position => match resolved.position {
            Position::Absolute { x, y } => Some(AnimatedValue::Offset {
                x: f64::from(x),
                y: f64::from(y),
            }),
            Position::Flow => None,
        },
        TransitionProperty::Width => length_value(resolved.width),
        TransitionProperty::Height => length_value(resolved.height),
        TransitionProperty::Padding => Some(insets_value(resolved.padding)),
        TransitionProperty::Margin => Some(insets_value(resolved.margin)),
    }
}

/// Write an in-flight value over its property in a resolved style.
///
/// Cell-valued properties round here, at the last moment: interpolation stays
/// fractional so a slow transition still progresses between cell boundaries.
pub(crate) fn apply_animated_value(
    resolved: &mut ResolvedStyle,
    property: TransitionProperty,
    value: AnimatedValue,
) {
    match (property, value) {
        (TransitionProperty::Opacity, AnimatedValue::Float(v)) => resolved.opacity = v,
        (TransitionProperty::Background, AnimatedValue::Color(c)) => resolved.background = Some(c),
        (TransitionProperty::Foreground, AnimatedValue::Color(c)) => resolved.color = c,
        (TransitionProperty::BorderColor, AnimatedValue::Color(c)) => {
            resolved.border_color = Some(c)
        }
        (TransitionProperty::Position, AnimatedValue::Offset { x, y }) => {
            resolved.position = Position::Absolute {
                x: round_cells_i32(x),
                y: round_cells_i32(y),
            }
        }
        (TransitionProperty::Width, AnimatedValue::Cells(v)) => {
            resolved.width = Length::Pixels(round_cells_u16(v))
        }
        (TransitionProperty::Width, AnimatedValue::Percent(v)) => {
            resolved.width = Length::Percent(v)
        }
        (TransitionProperty::Height, AnimatedValue::Cells(v)) => {
            resolved.height = Length::Pixels(round_cells_u16(v))
        }
        (TransitionProperty::Height, AnimatedValue::Percent(v)) => {
            resolved.height = Length::Percent(v)
        }
        (TransitionProperty::Padding, AnimatedValue::Insets { .. }) => {
            resolved.padding = round_insets(value)
        }
        (TransitionProperty::Margin, AnimatedValue::Insets { .. }) => {
            resolved.margin = round_insets(value)
        }
        // A mismatched pairing cannot be produced by the driver; ignore rather
        // than corrupt the style.
        _ => {}
    }
}

fn length_value(length: Length) -> Option<AnimatedValue> {
    match length {
        Length::Pixels(v) => Some(AnimatedValue::Cells(f64::from(v))),
        Length::Percent(v) => Some(AnimatedValue::Percent(v)),
        Length::Auto => None,
    }
}

fn insets_value(insets: EdgeInsets) -> AnimatedValue {
    AnimatedValue::Insets {
        top: f64::from(insets.top),
        right: f64::from(insets.right),
        bottom: f64::from(insets.bottom),
        left: f64::from(insets.left),
    }
}

fn round_insets(value: AnimatedValue) -> EdgeInsets {
    let AnimatedValue::Insets {
        top,
        right,
        bottom,
        left,
    } = value
    else {
        return EdgeInsets::ZERO;
    };
    EdgeInsets::new(
        round_cells_u16(top),
        round_cells_u16(right),
        round_cells_u16(bottom),
        round_cells_u16(left),
    )
}

fn round_cells_u16(value: f64) -> u16 {
    value.round().clamp(0.0, f64::from(u16::MAX)) as u16
}

fn round_cells_i32(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

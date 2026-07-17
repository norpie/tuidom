//! Animation driver — manages transition state, interpolation, and tick scheduling.

use std::collections::HashMap;
use std::time::Instant;

use crate::animation::value::{AnimatedValue, extract_animated_value};
use crate::animation::{Easing, TransitionConfig, TransitionProperty};
use crate::id::NodeId;
use crate::style::resolution::ResolvedStyle;

// ---------------------------------------------------------------------------
// Transition state
// ---------------------------------------------------------------------------

/// An active transition for a single node + property.
#[derive(Debug)]
struct TransitionState {
    node_id: NodeId,
    property: TransitionProperty,
    from: AnimatedValue,
    to: AnimatedValue,
    started: Instant,
    config: TransitionConfig,
}

impl TransitionState {
    /// Current progress (0–1). Clamped.
    fn progress(&self, now: Instant) -> f64 {
        let elapsed = now.duration_since(self.started).as_secs_f64();
        let total = self.config.duration.as_secs_f64();
        if total <= 0.0 {
            1.0
        } else {
            (elapsed / total).clamp(0.0, 1.0)
        }
    }

    /// Eased progress at the current time — the fraction of the value distance covered.
    fn eased_progress(&self, now: Instant) -> f64 {
        apply_easing(self.progress(now), self.config.easing)
    }

    /// Interpolated value at the current time.
    ///
    /// `from` and `to` always share a variant — the driver never starts a
    /// transition between incompatible values — so the fallback is unreachable.
    fn value(&self, now: Instant) -> AnimatedValue {
        self.from
            .lerp(self.to, self.eased_progress(now))
            .unwrap_or(self.to)
    }

    /// Whether this transition has completed.
    fn is_done(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

// ---------------------------------------------------------------------------
// Animation driver
// ---------------------------------------------------------------------------

/// A transition that ran to completion, reported by [`AnimationDriver::cleanup`].
///
/// Interrupted and removed transitions are discarded, not finished — only a
/// transition that settled on its target produces one of these.
pub(crate) struct FinishedTransition {
    /// The node whose property finished transitioning.
    pub node_id: NodeId,
    /// The property that finished.
    pub property: TransitionProperty,
}

/// Manages all active transitions and tick scheduling.
pub(crate) struct AnimationDriver {
    /// All active transitions.
    active: Vec<TransitionState>,
}

impl AnimationDriver {
    /// Create a new empty driver.
    pub fn new() -> Self {
        Self { active: Vec::new() }
    }

    /// Called after a style change to check for transitionable properties.
    pub fn style_changed(
        &mut self,
        node_id: NodeId,
        old_resolved: &ResolvedStyle,
        new_resolved: &ResolvedStyle,
        configs: &HashMap<TransitionProperty, TransitionConfig>,
        now: Instant,
    ) {
        for (&property, config) in configs {
            let old_val = extract_animated_value(old_resolved, property);
            let new_val = extract_animated_value(new_resolved, property);

            // A state with no interpolable value on either side — an unset
            // background, an `Auto` size, a `Flow` position — snaps: any
            // in-flight transition is dropped, none is started.
            let (Some(old_val), Some(new_val)) = (old_val, new_val) else {
                if old_val.is_some() != new_val.is_some() {
                    self.remove(node_id, property);
                }
                continue;
            };

            // A unit change (cells to percent) has no path between its values.
            if !old_val.compatible(new_val) {
                self.remove(node_id, property);
                continue;
            }

            // No base change for this property: leave any in-flight transition
            // alone — an unrelated property changing must not disturb it.
            if old_val.approx_eq(new_val) {
                continue;
            }

            let mut config = config.clone();

            // An interrupted transition hands over its current displayed value:
            // the old base value is the interrupted transition's *target*, and
            // starting from it would jump the node to the far end mid-flight.
            let interrupted = self
                .active
                .iter()
                .find(|t| t.node_id == node_id && t.property == property);
            let from = interrupted.map_or(old_val, |t| t.value(now));

            // A pure reversal — heading back to where the interrupted transition
            // started — covers only the distance already traveled, so it gets only
            // the matching share of the duration. Reversing a barely started fade
            // at full duration would crawl compared to the flick it undoes.
            if let Some(t) = interrupted
                && new_val.approx_eq(t.from)
            {
                config.duration = config.duration.mul_f64(t.eased_progress(now));
            }

            self.remove(node_id, property);

            // A target the display already sits on has nothing to animate; a
            // zero-distance transition would only hold the render loop active.
            if !from.approx_eq(new_val) {
                self.active.push(TransitionState {
                    node_id,
                    property,
                    from,
                    to: new_val,
                    started: now,
                    config,
                });
            }
        }
    }

    /// Get overrides that should be applied to resolved style for a node.
    pub fn overrides_for(
        &self,
        node_id: NodeId,
        now: Instant,
    ) -> HashMap<TransitionProperty, AnimatedValue> {
        let mut overrides = HashMap::new();

        for t in &self.active {
            if t.node_id == node_id {
                overrides.insert(t.property, t.value(now));
            }
        }

        overrides
    }

    /// Return whether any transitions are currently active.
    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }

    /// Nodes with an in-flight transition on a layout-affecting property — the
    /// nodes whose interpolated style must be fed to the layout engine this frame.
    pub fn layout_animating_nodes(&self) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        for t in &self.active {
            if t.property.affects_layout() && !nodes.contains(&t.node_id) {
                nodes.push(t.node_id);
            }
        }
        nodes
    }

    /// Remove all active transitions for a removed node.
    pub fn remove_node(&mut self, node_id: NodeId) {
        self.active
            .retain(|transition| transition.node_id != node_id);
    }

    /// Remove completed transitions, reporting each one so the caller can fire
    /// its end event.
    pub fn cleanup(&mut self, now: Instant) -> Vec<FinishedTransition> {
        let mut finished = Vec::new();
        self.active.retain(|t| {
            if t.is_done(now) {
                finished.push(FinishedTransition {
                    node_id: t.node_id,
                    property: t.property,
                });
                false
            } else {
                true
            }
        });
        finished
    }

    fn remove(&mut self, node_id: NodeId, property: TransitionProperty) {
        self.active
            .retain(|t| !(t.node_id == node_id && t.property == property));
    }
}

// ---------------------------------------------------------------------------
// Easing math
// ---------------------------------------------------------------------------

fn apply_easing(t: f64, easing: Easing) -> f64 {
    match easing {
        Easing::Linear => t,
        Easing::EaseIn => t * t * t,
        Easing::EaseOut => {
            let t = 1.0 - t;
            1.0 - t * t * t
        }
        Easing::EaseInOut => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                let t = -2.0 * t + 2.0;
                1.0 - t * t * t / 2.0
            }
        }
        Easing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(t, x1, y1, x2, y2),
    }
}

/// Evaluate a CSS-style cubic bézier timing function at time fraction `t`.
///
/// The curve runs from `(0,0)` to `(1,1)` with control points `(x1,y1)` and
/// `(x2,y2)`; x is time, y is progress. Control x values are clamped to 0–1 so
/// the curve stays a function of time. The parameter where the curve's x equals
/// `t` is found by Newton iteration, falling back to bisection where the slope
/// is too flat for Newton to converge.
fn cubic_bezier(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let x1 = x1.clamp(0.0, 1.0);
    let x2 = x2.clamp(0.0, 1.0);

    let mut s = t;
    for _ in 0..8 {
        let error = bezier_component(s, x1, x2) - t;
        if error.abs() < 1e-7 {
            return bezier_component(s, y1, y2);
        }
        let slope = bezier_derivative(s, x1, x2);
        if slope.abs() < 1e-6 {
            break;
        }
        s = (s - error / slope).clamp(0.0, 1.0);
    }

    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..32 {
        s = (lo + hi) / 2.0;
        if bezier_component(s, x1, x2) < t {
            lo = s;
        } else {
            hi = s;
        }
    }
    bezier_component(s, y1, y2)
}

/// One coordinate of the bézier at parameter `s`, with endpoints 0 and 1.
fn bezier_component(s: f64, c1: f64, c2: f64) -> f64 {
    let inv = 1.0 - s;
    3.0 * inv * inv * s * c1 + 3.0 * inv * s * s * c2 + s * s * s
}

/// Derivative of [`bezier_component`] with respect to `s`.
fn bezier_derivative(s: f64, c1: f64, c2: f64) -> f64 {
    let inv = 1.0 - s;
    3.0 * inv * inv * c1 + 6.0 * inv * s * (c2 - c1) + 3.0 * s * s * (1.0 - c2)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::animation::TransitionConfig;
    use crate::style::ResolvedColor;

    fn opacity_style(opacity: f64) -> ResolvedStyle {
        ResolvedStyle {
            opacity,
            ..ResolvedStyle::default()
        }
    }

    fn opacity_config() -> HashMap<TransitionProperty, TransitionConfig> {
        HashMap::from([(
            TransitionProperty::Opacity,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )])
    }

    fn opacity_override(driver: &AnimationDriver, node: NodeId, now: Instant) -> Option<f64> {
        match driver
            .overrides_for(node, now)
            .get(&TransitionProperty::Opacity)
        {
            Some(AnimatedValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    #[test]
    fn fresh_transition_starts_from_the_old_base_value() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );

        assert_eq!(opacity_override(&driver, node, start), Some(0.0));
        let mid = opacity_override(&driver, node, start + Duration::from_millis(500));
        assert!((mid.unwrap_or(f64::NAN) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn interrupting_a_transition_continues_from_the_displayed_value() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let midpoint = start + Duration::from_millis(500);

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );
        // Reverse mid-flight: the old base is the interrupted target (1.0), but the
        // display sits at 0.5 — the reversal must start there, not jump to 1.0.
        // As a pure reversal it also runs at half the duration (500ms).
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(0.0),
            &opacity_config(),
            midpoint,
        );

        let at_reversal = opacity_override(&driver, node, midpoint);
        assert!((at_reversal.unwrap_or(f64::NAN) - 0.5).abs() < 1e-9);
        let later = opacity_override(&driver, node, midpoint + Duration::from_millis(250));
        assert!((later.unwrap_or(f64::NAN) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn a_pure_reversal_gets_a_matching_share_of_the_duration() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let quarter = start + Duration::from_millis(250);

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );
        // Reverse a quarter of the way in: the way back covers a quarter of the
        // distance, so it gets a quarter of the duration.
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(0.0),
            &opacity_config(),
            quarter,
        );

        let halfway_back = opacity_override(&driver, node, quarter + Duration::from_millis(125));
        assert!((halfway_back.unwrap_or(f64::NAN) - 0.125).abs() < 1e-9);

        let finished = driver.cleanup(quarter + Duration::from_millis(250));
        assert_eq!(finished.len(), 1);
        assert!(!driver.has_active());
    }

    #[test]
    fn retargeting_keeps_the_full_duration() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let quarter = start + Duration::from_millis(250);

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );
        // A new target that is not the interrupted start is no reversal: the full
        // configured duration applies from the displayed value.
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(0.5),
            &opacity_config(),
            quarter,
        );

        let mid = opacity_override(&driver, node, quarter + Duration::from_millis(500));
        assert!((mid.unwrap_or(f64::NAN) - 0.375).abs() < 1e-9);
        assert!(
            driver
                .cleanup(quarter + Duration::from_millis(900))
                .is_empty()
        );
        assert!(driver.has_active());
    }

    #[test]
    fn cleanup_reports_each_finished_transition_once() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );

        assert!(
            driver
                .cleanup(start + Duration::from_millis(500))
                .is_empty()
        );

        let finished = driver.cleanup(start + Duration::from_secs(1));
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].node_id, node);
        assert_eq!(finished[0].property, TransitionProperty::Opacity);

        assert!(driver.cleanup(start + Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn a_target_matching_the_displayed_value_ends_the_transition() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let midpoint = start + Duration::from_millis(500);

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(0.5),
            &opacity_config(),
            midpoint,
        );

        assert!(!driver.has_active());
        assert_eq!(opacity_override(&driver, node, midpoint), None);
    }

    #[test]
    fn an_unrelated_style_change_leaves_a_transition_running() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let midpoint = start + Duration::from_millis(500);

        driver.style_changed(
            node,
            &opacity_style(0.0),
            &opacity_style(1.0),
            &opacity_config(),
            start,
        );
        // A style change that does not touch opacity re-signals with equal base
        // values; the in-flight transition must keep its original timeline.
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(1.0),
            &opacity_config(),
            midpoint,
        );

        let mid = opacity_override(&driver, node, midpoint);
        assert!((mid.unwrap_or(f64::NAN) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_color_transition_passes_through_the_oklch_midpoint() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();
        let red = ResolvedColor::red();
        let blue = ResolvedColor::blue();

        let from_style = ResolvedStyle {
            background: Some(red),
            ..ResolvedStyle::default()
        };
        let to_style = ResolvedStyle {
            background: Some(blue),
            ..ResolvedStyle::default()
        };
        let configs = HashMap::from([(
            TransitionProperty::Background,
            TransitionConfig::new(
                TransitionProperty::Background,
                Duration::from_secs(1),
                Easing::Linear,
            ),
        )]);

        driver.style_changed(node, &from_style, &to_style, &configs, start);

        let overrides = driver.overrides_for(node, start + Duration::from_millis(500));
        let mid = overrides.get(&TransitionProperty::Background).copied();
        assert_eq!(mid, Some(AnimatedValue::Color(red.mix(blue, 0.5))));
        assert_ne!(mid, Some(AnimatedValue::Color(red)));
        assert_ne!(mid, Some(AnimatedValue::Color(blue)));
    }

    #[test]
    fn a_unit_change_snaps_instead_of_transitioning() {
        let mut driver = AnimationDriver::new();
        let node = NodeId::new(1);
        let start = Instant::now();

        let from_style = ResolvedStyle {
            width: crate::style::Length::Pixels(4),
            ..ResolvedStyle::default()
        };
        let to_style = ResolvedStyle {
            width: crate::style::Length::Percent(50.0),
            ..ResolvedStyle::default()
        };
        let configs = HashMap::from([(
            TransitionProperty::Width,
            TransitionConfig::new(
                TransitionProperty::Width,
                Duration::from_secs(1),
                Easing::Linear,
            ),
        )]);

        driver.style_changed(node, &from_style, &to_style, &configs, start);

        assert!(!driver.has_active());
    }

    #[test]
    fn cubic_bezier_easing_matches_the_curve() {
        // A bézier with both control points on the diagonal is the identity.
        let linear = Easing::CubicBezier(0.25, 0.25, 0.75, 0.75);
        for t in [0.0, 0.1, 0.35, 0.5, 0.82, 1.0] {
            assert!((apply_easing(t, linear) - t).abs() < 1e-4);
        }

        // Endpoints are exact for any curve.
        let ease = Easing::CubicBezier(0.25, 0.1, 0.25, 1.0);
        assert_eq!(apply_easing(0.0, ease), 0.0);
        assert_eq!(apply_easing(1.0, ease), 1.0);

        // The CSS `ease` curve is fast in the middle: well above linear at t=0.5,
        // and monotonically increasing throughout.
        let mid = apply_easing(0.5, ease);
        assert!(mid > 0.7 && mid < 0.9);
        let mut previous = 0.0;
        for step in 1..=20 {
            let value = apply_easing(f64::from(step) / 20.0, ease);
            assert!(value >= previous);
            previous = value;
        }
    }
}

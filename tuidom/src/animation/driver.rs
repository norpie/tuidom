//! Animation driver — manages transition state, interpolation, and tick scheduling.

use std::collections::HashMap;
use std::time::Instant;

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
    from: f64,
    to: f64,
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

    /// Interpolated value at the current time.
    fn value(&self, now: Instant) -> f64 {
        let t = self.progress(now);
        lerp(self.from, self.to, apply_easing(t, self.config.easing))
    }

    /// Whether this transition has completed.
    fn is_done(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

// ---------------------------------------------------------------------------
// Animation driver
// ---------------------------------------------------------------------------

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
    ///
    /// Returns `true` if a new transition was started.
    pub fn style_changed(
        &mut self,
        node_id: NodeId,
        old_resolved: &ResolvedStyle,
        new_resolved: &ResolvedStyle,
        configs: &HashMap<TransitionProperty, TransitionConfig>,
        now: Instant,
    ) -> bool {
        let had_active = !self.active.is_empty();

        // Check opacity
        if let Some(config) = configs.get(&TransitionProperty::Opacity).cloned() {
            let old_val = old_resolved.opacity;
            let new_val = new_resolved.opacity;
            if (old_val - new_val).abs() >= f64::EPSILON {
                // An interrupted transition hands over its current displayed value:
                // the old base value is the interrupted transition's *target*, and
                // starting from it would jump the node to the far end mid-flight.
                let from = self
                    .active
                    .iter()
                    .find(|t| t.node_id == node_id && t.property == TransitionProperty::Opacity)
                    .map_or(old_val, |t| t.value(now));
                self.active.retain(|t| {
                    !(t.node_id == node_id && t.property == TransitionProperty::Opacity)
                });
                // A target the display already sits on has nothing to animate; a
                // zero-distance transition would only hold the render loop active.
                if (from - new_val).abs() >= f64::EPSILON {
                    self.active.push(TransitionState {
                        node_id,
                        property: TransitionProperty::Opacity,
                        from,
                        to: new_val,
                        started: now,
                        config,
                    });
                }
            }
        }

        !self.active.is_empty() && !had_active
    }

    /// Get overrides that should be applied to resolved style for a node.
    pub fn overrides_for(&self, node_id: NodeId, now: Instant) -> HashMap<TransitionProperty, f64> {
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

    /// Remove all active transitions for a removed node.
    pub fn remove_node(&mut self, node_id: NodeId) {
        self.active
            .retain(|transition| transition.node_id != node_id);
    }

    /// Remove completed transitions. Returns `true` if any remain active.
    pub fn cleanup(&mut self, now: Instant) -> bool {
        self.active.retain(|t| !t.is_done(now));
        !self.active.is_empty()
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
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::animation::TransitionConfig;

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
        driver
            .overrides_for(node, now)
            .get(&TransitionProperty::Opacity)
            .copied()
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
        driver.style_changed(
            node,
            &opacity_style(1.0),
            &opacity_style(0.0),
            &opacity_config(),
            midpoint,
        );

        let at_reversal = opacity_override(&driver, node, midpoint);
        assert!((at_reversal.unwrap_or(f64::NAN) - 0.5).abs() < 1e-9);
        let later = opacity_override(&driver, node, midpoint + Duration::from_millis(500));
        assert!((later.unwrap_or(f64::NAN) - 0.25).abs() < 1e-9);
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
}

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
        // Check opacity
        if let Some(mut config) = configs.get(&TransitionProperty::Opacity).cloned() {
            let old_val = old_resolved.opacity;
            let new_val = new_resolved.opacity;
            if (old_val - new_val).abs() >= f64::EPSILON {
                // An interrupted transition hands over its current displayed value:
                // the old base value is the interrupted transition's *target*, and
                // starting from it would jump the node to the far end mid-flight.
                let interrupted = self
                    .active
                    .iter()
                    .find(|t| t.node_id == node_id && t.property == TransitionProperty::Opacity);
                let from = interrupted.map_or(old_val, |t| t.value(now));
                // A pure reversal — heading back to where the interrupted transition
                // started — covers only the distance already traveled, so it gets only
                // the matching share of the duration. Reversing a barely started fade
                // at full duration would crawl compared to the flick it undoes.
                if let Some(t) = interrupted
                    && (new_val - t.from).abs() < f64::EPSILON
                    && (t.to - t.from).abs() >= f64::EPSILON
                {
                    let covered = ((from - t.from) / (t.to - t.from)).abs().clamp(0.0, 1.0);
                    config.duration = config.duration.mul_f64(covered);
                }
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
}

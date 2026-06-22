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
        configs: &std::collections::HashMap<TransitionProperty, TransitionConfig>,
    ) -> bool {
        let had_active = !self.active.is_empty();

        // Check opacity
        if let Some(config) = configs.get(&TransitionProperty::Opacity).cloned() {
            let old_val = old_resolved.opacity;
            let new_val = new_resolved.opacity;
            if (old_val - new_val).abs() >= f64::EPSILON {
                // Remove existing, add new
                self.active.retain(|t| {
                    !(t.node_id == node_id && t.property == TransitionProperty::Opacity)
                });
                self.active.push(TransitionState {
                    node_id,
                    property: TransitionProperty::Opacity,
                    from: old_val,
                    to: new_val,
                    started: Instant::now(),
                    config,
                });
            }
        }

        !self.active.is_empty() && !had_active
    }

    /// Get overrides that should be applied to resolved style for a node.
    pub fn overrides_for(&self, node_id: NodeId) -> HashMap<TransitionProperty, f64> {
        let now = Instant::now();
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

    /// Remove completed transitions. Returns `true` if any remain active.
    pub fn cleanup(&mut self) -> bool {
        let now = Instant::now();
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

//! Animation driver — manages transition state, interpolation, and the tick task.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tokio::time::sleep_until;

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
        Self {
            active: Vec::new(),
        }
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

    /// Remove completed transitions. Returns `true` if any remain active.
    pub fn cleanup(&mut self) -> bool {
        let now = Instant::now();
        self.active.retain(|t| !t.is_done(now));
        !self.active.is_empty()
    }

    /// Determine the next deadline for a frame, if any animations are active.
    fn next_deadline(&self) -> Option<tokio::time::Instant> {
        let now = Instant::now();

        self.active
            .iter()
            .map(|t| {
                let remaining =
                    t.config
                        .duration
                        .saturating_sub(now.duration_since(t.started));
                // Clamp: at least 8ms, at most 100ms between frames
                let step = remaining
                    .min(Duration::from_millis(100))
                    .max(Duration::from_millis(8));
                let deadline = now + step;
                tokio::time::Instant::from_std(deadline)
            })
            .min()
    }
}

// ---------------------------------------------------------------------------
// Tick task
// ---------------------------------------------------------------------------

/// Spawn a background task that drives animation frames and notifies the
/// render loop on each tick. Exits when all animations complete.
pub(crate) fn spawn_tick_task(
    driver: Arc<Mutex<AnimationDriver>>,
    config_changed: Arc<Notify>,
    anim_tick: Arc<Notify>,
) {
    tokio::spawn(async move {
        loop {
            // Compute next deadline
            let deadline = {
                let d = driver.lock().unwrap();
                d.next_deadline()
            };

            let Some(deadline) = deadline else {
                // No active animations — send one last tick and exit
                anim_tick.notify_one();
                break;
            };

            tokio::select! {
                _ = sleep_until(deadline) => {
                    let mut d = driver.lock().unwrap();
                    let has_active = d.cleanup();
                    anim_tick.notify_one();
                    if !has_active {
                        break;
                    }
                }
                _ = config_changed.notified() => {
                    // Config changed — recompute deadline
                    continue;
                }
            }
        }
    });
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

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::animation::value::{AnimatedValue, insets_value, length_value};
use crate::animation::{
    AnimatableProperty, AnimationHandle, KeyframeAnimation, ResolvedKeyframes, TransitionProperty,
};
use crate::document::Document;
use crate::error::Result;
use crate::id::NodeId;
use crate::lock;
use crate::style::color::ColorContext;

impl Document {
    /// Start a keyframe animation on a node.
    ///
    /// Color expressions in the keyframes are evaluated once, here, against the
    /// node's current color scope — a variable redeclared later does not retint
    /// a running animation. An expression that does not resolve contributes
    /// nothing, like an undefined variable in a style.
    ///
    /// The animation's values apply on top of the node's style and any running
    /// transitions until it ends, is cancelled, or the node is removed. Control
    /// it through the returned handle with
    /// [`pause_animation`](Self::pause_animation),
    /// [`resume_animation`](Self::resume_animation), and
    /// [`cancel_animation`](Self::cancel_animation).
    ///
    /// Returns [`TuidomError::NodeNotFound`](crate::TuidomError::NodeNotFound)
    /// if `node` does not exist.
    pub fn animate(&self, node: NodeId, animation: KeyframeAnimation) -> Result<AnimationHandle> {
        let resolved = self.resolved_base_style(node)?;
        let ctx = ColorContext {
            vars: &resolved.color_vars,
            current_bg: resolved.effective_background,
            current_fg: resolved.color,
        };

        let mut tracks: Vec<(TransitionProperty, Vec<(f64, AnimatedValue)>)> = Vec::new();
        let mut by_property: HashMap<TransitionProperty, usize> = HashMap::new();
        for (percent, values) in &animation.keyframes {
            let offset = percent / 100.0;
            for value in values {
                let Some(resolved_value) = resolve_keyframe_value(value, &ctx) else {
                    continue;
                };
                let property = value.property();
                let index = *by_property.entry(property).or_insert_with(|| {
                    tracks.push((property, Vec::new()));
                    tracks.len() - 1
                });
                tracks[index].1.push((offset, resolved_value));
            }
        }
        for (_, track) in &mut tracks {
            track.sort_by(|a, b| a.0.total_cmp(&b.0));
        }

        let handle = AnimationHandle {
            document_id: self.inner.document_id,
            id: self.inner.next_animation_id.fetch_add(1, Ordering::Relaxed),
        };
        lock::mutex(&self.inner.animation).animate(
            handle.id,
            node,
            ResolvedKeyframes {
                tracks,
                duration: animation.duration,
                easing: animation.easing,
                iterations: animation.iterations,
                direction: animation.direction,
            },
            self.now(),
        );

        self.inner.anim_config_changed.notify_one();
        self.inner.notify.notify_one();
        Ok(handle)
    }

    /// Freeze a running animation at its current values.
    ///
    /// The frozen values keep applying, but no frames are driven for a paused
    /// animation — the renderer idles until it resumes. Returns `false` when the
    /// handle names no running animation of this document, or one already paused.
    pub fn pause_animation(&self, handle: AnimationHandle) -> bool {
        if handle.document_id != self.inner.document_id {
            return false;
        }
        lock::mutex(&self.inner.animation).pause_animation(handle.id, self.now())
    }

    /// Resume a paused animation where it left off.
    ///
    /// Returns `false` when the handle names no paused animation of this document.
    pub fn resume_animation(&self, handle: AnimationHandle) -> bool {
        if handle.document_id != self.inner.document_id {
            return false;
        }
        let resumed = lock::mutex(&self.inner.animation).resume_animation(handle.id, self.now());
        if resumed {
            self.inner.anim_config_changed.notify_one();
        }
        resumed
    }

    /// Cancel a running animation, returning the node to its underlying style.
    ///
    /// No end event fires for a cancelled animation. Returns `false` when the
    /// handle names no running animation of this document.
    pub fn cancel_animation(&self, handle: AnimationHandle) -> bool {
        if handle.document_id != self.inner.document_id {
            return false;
        }
        let Some(node) = lock::mutex(&self.inner.animation).cancel_animation(handle.id) else {
            return false;
        };
        // The layout engine may hold the animation's last interpolated value;
        // settle it back on the underlying style.
        if let Ok(resolved) = self.resolved_style(node) {
            let _ = lock::mutex(&self.inner.layout).set_style(node, &resolved);
        }
        self.inner.notify.notify_one();
        true
    }
}

/// Resolve one keyframe value to its typed animated form.
///
/// `None` — an unresolvable color expression or an `Auto` length — drops the
/// value from its track rather than failing the animation.
fn resolve_keyframe_value(value: &AnimatableProperty, ctx: &ColorContext) -> Option<AnimatedValue> {
    match value {
        AnimatableProperty::Opacity(v) => Some(AnimatedValue::Float(*v)),
        AnimatableProperty::Background(color)
        | AnimatableProperty::Foreground(color)
        | AnimatableProperty::BorderColor(color) => color.eval(ctx).map(AnimatedValue::Color),
        AnimatableProperty::Position { x, y } => Some(AnimatedValue::Offset {
            x: f64::from(*x),
            y: f64::from(*y),
        }),
        AnimatableProperty::Width(length) | AnimatableProperty::Height(length) => {
            length_value(*length)
        }
        AnimatableProperty::Padding(insets) | AnimatableProperty::Margin(insets) => {
            Some(insets_value(*insets))
        }
    }
}

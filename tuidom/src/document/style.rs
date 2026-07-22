use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use crate::animation::driver::KeyframeEventKind;
use crate::animation::value::apply_animated_value;
use crate::animation::{AnimationHandle, TransitionConfig};
use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;
use crate::runtime_event::RuntimeEvent;
use crate::style::color::ColorContext;
use crate::style::resolution::{ColorScope, ResolvedStyle, StyleDefaults};
use crate::style::{Color, ResolvedColor, Style};

impl Document {
    /// Set a transition configuration for a node.
    ///
    /// When the given property changes (via [`update_style`] or [`set_style`]),
    /// the engine will animate the change over the specified duration and easing.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn set_transition(&self, id: NodeId, config: TransitionConfig) -> Result<()> {
        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };

        data.transition_configs.insert(config.property, config);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Color variables
    // ------------------------------------------------------------------

    /// Declare a document-level color variable.
    ///
    /// Document variables sit beneath every node's scope, so any node can reference one with
    /// [`Color::var`] unless an ancestor shadows the name.
    ///
    /// They are the bottom of the chain, and so are evaluated against an empty scope: a document
    /// variable may be a literal or a derivation of one, but a [`Color::var`] inside it has nothing
    /// to resolve against and never resolves.
    pub fn set_color_var(&self, name: impl Into<String>, value: Color) {
        lock::mutex(&self.inner.color_vars).insert(name.into(), value);
        self.invalidate_resolved_style(self.root());
        self.inner.notify.notify_one();
    }

    /// Get a document-level color variable.
    pub fn color_var(&self, name: &str) -> Option<Color> {
        lock::mutex(&self.inner.color_vars).get(name).cloned()
    }

    /// Remove a document-level color variable.
    pub fn remove_color_var(&self, name: &str) {
        lock::mutex(&self.inner.color_vars).remove(name);
        self.invalidate_resolved_style(self.root());
        self.inner.notify.notify_one();
    }

    // ------------------------------------------------------------------
    // Terminal background
    // ------------------------------------------------------------------

    /// Declare the terminal's background color.
    ///
    /// The real one is unknowable without querying the terminal, so tuidom asks you to state it.
    /// It is what [`Color::CurrentBg`] resolves to on a node with no background anywhere in its
    /// ancestry, and what a translucent color blends toward over an unpainted cell. It defaults to
    /// black.
    ///
    /// It is an assumption used for color math, not a color that gets painted: cells with no
    /// background still emit the terminal default, so an unstyled app keeps showing the user's real
    /// background whatever this is set to.
    ///
    /// It is evaluated against the empty scope, so it may be a literal or a derivation of one — a
    /// [`Color::var`] inside it has nothing to resolve against.
    pub fn set_terminal_background(&self, color: Color) {
        *lock::mutex(&self.inner.terminal_background) = color;
        self.invalidate_resolved_style(self.root());
        self.inner.notify.notify_one();
    }

    /// The terminal background color the document assumes.
    pub fn terminal_background(&self) -> Color {
        lock::mutex(&self.inner.terminal_background).clone()
    }

    /// The declared terminal background, evaluated against the empty scope.
    pub(crate) fn resolved_terminal_background(&self) -> ResolvedColor {
        let declared = lock::mutex(&self.inner.terminal_background).clone();
        declared
            .eval(&ColorContext {
                vars: &HashMap::new(),
                current_bg: ResolvedColor::black(),
                current_fg: ResolvedColor::white(),
            })
            .unwrap_or_else(ResolvedColor::black)
    }

    /// The document's color variables, evaluated against the empty scope.
    ///
    /// The document sits at the bottom of the chain, so its variables see no other variables —
    /// only the terminal background, which a variable may usefully derive a surface color from.
    fn color_scope(&self) -> ColorScope {
        let declared = lock::mutex(&self.inner.color_vars);
        if declared.is_empty() {
            return ColorScope::default();
        }

        let empty = HashMap::new();
        let ctx = ColorContext {
            vars: &empty,
            current_bg: self.resolved_terminal_background(),
            current_fg: ResolvedColor::white(),
        };
        Arc::new(
            declared
                .iter()
                .filter_map(|(name, expr)| Some((name.clone(), expr.eval(&ctx)?)))
                .collect(),
        )
    }

    // ------------------------------------------------------------------
    // Style
    // ------------------------------------------------------------------

    /// Set the inline style for a node.
    ///
    /// This replaces any previously set style, invalidates the resolved
    /// style cache, and signals the animation driver if any transitionable
    /// properties changed.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn set_style(&self, id: NodeId, style: &Style) -> Result<()> {
        let old_resolved = self.resolved_base_style(id)?;

        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        data.style = style.clone();
        drop(data);

        self.invalidate_resolved_style(id);
        self.sync_layout_subtree_styles(id)?;
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved)?;
        // After the change's own effects, so a blur listener sees the settled style. The
        // node guard is long dropped, so dispatching from here is safe.
        self.settle_focus_if_hidden(id);
        Ok(())
    }

    /// Get a node's declared inline style.
    ///
    /// This is what was written with [`set_style`](Self::set_style), before inheritance,
    /// defaults, pseudo-states, or animation overrides — so every property keeps its
    /// [`StyleValue`](crate::style::StyleValue) state, and `Unset` stays distinguishable
    /// from a value that happens to equal the default. That distinction is the whole
    /// reason to read this rather than
    /// [`resolved_style`](Self::resolved_style), which has collapsed it by construction.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn style(&self, id: NodeId) -> Result<Style> {
        match self.inner.nodes.get(&id) {
            Some(data) => Ok(data.style.clone()),
            None => Err(TuidomError::NodeNotFound { id }),
        }
    }

    /// Update a node's style in-place via a closure.
    ///
    /// Invalidates the resolved style cache, triggers a re-render, and signals
    /// the animation driver if any transitionable properties changed.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn update_style(&self, id: NodeId, f: impl FnOnce(&mut Style)) -> Result<()> {
        // Capture old resolved values before the mutation
        let old_resolved = self.resolved_base_style(id)?;

        let Some(mut data) = self.inner.nodes.get_mut(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        let mut style = data.style.clone();
        let result = catch_unwind(AssertUnwindSafe(|| f(&mut style)));
        if let Err(payload) = result {
            tracing::error!("style update callback panicked for {id:?}");
            resume_unwind(payload);
        }
        data.style = style;
        drop(data);

        self.invalidate_resolved_style(id);
        self.sync_layout_subtree_styles(id)?;
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved)?;
        self.settle_focus_if_hidden(id);
        Ok(())
    }

    /// Get the fully resolved style for a node, including animation overrides.
    ///
    /// Returns the cached value if available, otherwise computes it by
    /// applying explicit values, explicit inheritance, and document defaults.
    ///
    /// During active animations, property values are overridden with the
    /// interpolated animation value.
    ///
    /// Returns [`TuidomError::NodeNotFound`] if `id` does not exist.
    pub fn resolved_style(&self, id: NodeId) -> Result<ResolvedStyle> {
        self.resolved_style_arc(id).map(|r| (*r).clone())
    }

    /// The same value as [`Document::resolved_style`], shared rather than copied.
    ///
    /// `ResolvedStyle` is several hundred bytes, so a caller that only reads it —
    /// the paint-order walk resolves every node and again every child — should
    /// take the `Arc` and leave the payload where it is.
    pub(crate) fn resolved_style_arc(&self, id: NodeId) -> Result<Arc<ResolvedStyle>> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_style_unlocked(id)
    }

    pub(crate) fn resolved_style_unlocked(&self, id: NodeId) -> Result<Arc<ResolvedStyle>> {
        let base = self.resolved_base_style_unlocked(id)?;

        // Apply animation overrides: transitions first, then keyframe values on
        // top — animations win on conflict, as in CSS. Keyframes sample against
        // the transition-adjusted style, so an implicit endpoint tracks the
        // underlying value it will hand back to.
        let (overrides, keyframe_overrides) = {
            let driver = lock::mutex(&self.inner.animation);
            // Reading the clock is a lock of its own, and it was paid on every
            // resolve — thousands a frame — to serve the handful of nodes that
            // animate. A node in neither animation list cannot produce an
            // override, so this returns exactly what the full path would.
            if !driver.animates(id) {
                return Ok(base);
            }
            let now = self.now();
            let overrides = driver.overrides_for(id, now);
            let keyframe_overrides = driver.keyframe_overrides_for(id, now, &base);
            (overrides, keyframe_overrides)
        };

        // Nothing animating means the cached value *is* the answer, so the
        // common node hands back its `Arc` untouched. Only a node with a live
        // override pays the copy `make_mut` does to detach it from the cache.
        if overrides.is_empty() && keyframe_overrides.is_empty() {
            return Ok(base);
        }

        let mut resolved = base;
        let target = Arc::make_mut(&mut resolved);
        for (prop, val) in overrides {
            apply_animated_value(target, prop, val);
        }
        for (prop, val) in keyframe_overrides {
            apply_animated_value(target, prop, val);
        }

        Ok(resolved)
    }

    /// Get the base resolved style without animation overrides.
    ///
    /// Used internally by the animation driver to read target values.
    pub(crate) fn resolved_base_style(&self, id: NodeId) -> Result<Arc<ResolvedStyle>> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_base_style_unlocked(id)
    }

    pub(crate) fn resolved_base_style_unlocked(&self, id: NodeId) -> Result<Arc<ResolvedStyle>> {
        // Counted here rather than at the `resolved_style_unlocked` entry point so the
        // recursive walk to an uncached parent is counted too — on a cold cache that walk
        // is most of the work, and a count that hid it would understate the cost it exists
        // to expose.
        self.inner.style_counters.record_resolve();

        // Check cache
        {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            if let Some(resolved) = &*lock::rw_read(&node.resolved_style) {
                return Ok(Arc::clone(resolved));
            }
        }

        // Cache miss — compute
        self.inner.style_counters.record_miss();
        let parent = self.get_parent_unlocked(id);
        let parent_resolved = parent
            .map(|pid| self.resolved_base_style_unlocked(pid))
            .transpose()?;

        let defaults = if id == self.root() {
            StyleDefaults::root(self.color_scope(), self.resolved_terminal_background())
        } else {
            StyleDefaults::default()
        };
        let mut resolved = {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            if id == self.root() {
                ResolvedStyle::compute_with_defaults(&node, parent_resolved.as_deref(), &defaults)
            } else {
                ResolvedStyle::compute(&node, parent_resolved.as_deref())
            }
        };

        // Merge order is base → focus → active → disabled, so disabled wins on conflict.
        // Disabling blurs and deactivates the node, so disabled rarely coexists with the
        // others; the order is a safety net rather than a common path.
        let pseudo = lock::mutex(&self.inner.pseudo_styles).get(&id).cloned();
        if let Some(pseudo) = pseudo {
            if self.focused() == Some(id)
                && let Some(focus_style) = &pseudo.focus
            {
                resolved.apply_overrides(focus_style, parent_resolved.as_deref(), &defaults);
            }
            if self.active() == Some(id)
                && let Some(active_style) = &pseudo.active
            {
                resolved.apply_overrides(active_style, parent_resolved.as_deref(), &defaults);
            }
            if let Some(disabled_style) = &pseudo.disabled
                && self.is_effectively_disabled_unlocked(id)
            {
                resolved.apply_overrides(disabled_style, parent_resolved.as_deref(), &defaults);
            }
        }

        let Some(node) = self.inner.nodes.get(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        let resolved = Arc::new(resolved);
        *lock::rw_write(&node.resolved_style) = Some(Arc::clone(&resolved));
        Ok(resolved)
    }

    /// Signal the animation driver about a style change and spawn tick task if needed.
    ///
    /// The diff runs on merged resolved styles, so pseudo-state changes (focus,
    /// active, disabled) transition exactly like explicit style changes.
    pub(super) fn signal_animation(&self, id: NodeId, old_resolved: &ResolvedStyle) -> Result<()> {
        // Read transition configs before locking the driver
        let configs = {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            node.transition_configs.clone()
        };

        // Nothing configured means nothing can start: skip the resolve and the
        // driver lock, so the hot focus/hover path stays cheap.
        if configs.is_empty() {
            return Ok(());
        }

        // Compute the new resolved value BEFORE locking the driver
        let new_resolved = self.resolved_base_style(id)?;

        let mut driver = lock::mutex(&self.inner.animation);
        driver.style_changed(id, old_resolved, &new_resolved, &configs, self.now());
        drop(driver);

        self.inner.anim_config_changed.notify_one();

        Ok(())
    }

    /// Remove finished transitions and animations, settling layout-affecting
    /// ones, and return the runtime events to dispatch for them.
    ///
    /// The layout engine holds the last interpolated value a tick pushed; a
    /// finished layout transition or animation pushes the settled style once
    /// more so layout rests exactly on the underlying value.
    pub(crate) fn run_animation_upkeep(&self) -> Vec<RuntimeEvent> {
        // Counts what this tick *settled*, not what is live: the driver exposes no live
        // counts, and "finished two transitions this frame" is the more useful number
        // anyway — a tick that settles nothing is the common, uninteresting case.
        let span = tracing::debug_span!(
            "animation_upkeep",
            finished_transitions = tracing::field::Empty,
            keyframe_events = tracing::field::Empty,
        );
        let _guard = span.enter();

        let now = self.now();
        let (finished, keyframe_events) = {
            let mut driver = lock::mutex(&self.inner.animation);
            (driver.cleanup(now), driver.keyframe_upkeep(now))
        };
        span.record("finished_transitions", finished.len());
        span.record("keyframe_events", keyframe_events.len());

        let mut events = Vec::new();
        let mut settle = Vec::new();
        for transition in finished {
            if transition.property.affects_layout() {
                settle.push(transition.node_id);
            }
            events.push(RuntimeEvent::TransitionEnd {
                node: transition.node_id,
                property: transition.property,
            });
        }
        for event in keyframe_events {
            let handle = AnimationHandle {
                document_id: self.inner.document_id,
                id: event.animation_id,
            };
            match event.kind {
                KeyframeEventKind::Iteration { iteration } => {
                    events.push(RuntimeEvent::AnimationIteration {
                        node: event.node_id,
                        handle,
                        iteration,
                    });
                }
                KeyframeEventKind::End => {
                    settle.push(event.node_id);
                    events.push(RuntimeEvent::AnimationEnd {
                        node: event.node_id,
                        handle,
                    });
                }
            }
        }

        for node in settle {
            let Ok(resolved) = self.resolved_style(node) else {
                continue;
            };
            let _ = lock::mutex(&self.inner.layout).set_style(node, &resolved);
        }
        events
    }

    /// Invalidate the resolved style cache for a node and all descendants.
    pub(crate) fn invalidate_resolved_style(&self, id: NodeId) {
        if let Some(node) = self.inner.nodes.get(&id) {
            *lock::rw_write(&node.resolved_style) = None;
            let children = node.children.clone();
            drop(node); // release lock before recursing
            for child in children {
                self.invalidate_resolved_style(child);
            }
        }
    }
}

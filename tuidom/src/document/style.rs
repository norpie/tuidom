use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use crate::animation::TransitionConfig;
use crate::animation::driver::FinishedTransition;
use crate::animation::value::apply_animated_value;
use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;
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

        self.signal_animation(id, &old_resolved)
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
            log::error!("style update callback panicked for {id:?}");
            resume_unwind(payload);
        }
        data.style = style;
        drop(data);

        self.invalidate_resolved_style(id);
        self.sync_layout_subtree_styles(id)?;
        self.inner.notify.notify_one();

        self.signal_animation(id, &old_resolved)
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
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_style_unlocked(id)
    }

    pub(crate) fn resolved_style_unlocked(&self, id: NodeId) -> Result<ResolvedStyle> {
        let mut resolved = self.resolved_base_style_unlocked(id)?;

        // Apply animation overrides
        {
            let driver = lock::mutex(&self.inner.animation);
            for (prop, val) in driver.overrides_for(id, self.now()) {
                apply_animated_value(&mut resolved, prop, val);
            }
        }

        Ok(resolved)
    }

    /// Get the base resolved style without animation overrides.
    ///
    /// Used internally by the animation driver to read target values.
    pub(crate) fn resolved_base_style(&self, id: NodeId) -> Result<ResolvedStyle> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);
        self.resolved_base_style_unlocked(id)
    }

    pub(crate) fn resolved_base_style_unlocked(&self, id: NodeId) -> Result<ResolvedStyle> {
        // Check cache
        {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            if let Some(resolved) = &*lock::rw_read(&node.resolved_style) {
                return Ok(resolved.clone());
            }
        }

        // Cache miss — compute
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
                ResolvedStyle::compute_with_defaults(&node, parent_resolved.as_ref(), &defaults)
            } else {
                ResolvedStyle::compute(&node, parent_resolved.as_ref())
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
                resolved.apply_overrides(focus_style, parent_resolved.as_ref(), &defaults);
            }
            if self.active() == Some(id)
                && let Some(active_style) = &pseudo.active
            {
                resolved.apply_overrides(active_style, parent_resolved.as_ref(), &defaults);
            }
            if let Some(disabled_style) = &pseudo.disabled
                && self.is_effectively_disabled_unlocked(id)
            {
                resolved.apply_overrides(disabled_style, parent_resolved.as_ref(), &defaults);
            }
        }

        let Some(node) = self.inner.nodes.get(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        *lock::rw_write(&node.resolved_style) = Some(resolved.clone());
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

    /// Remove finished transitions, settling layout-affecting ones.
    ///
    /// The layout engine holds the last interpolated value a tick pushed; a
    /// finished layout transition pushes the settled style once more so layout
    /// rests exactly on the target. Returns the finished transitions so the
    /// caller can dispatch their end events.
    pub(crate) fn run_animation_upkeep(&self) -> Vec<FinishedTransition> {
        let finished = lock::mutex(&self.inner.animation).cleanup(self.now());
        for transition in &finished {
            if !transition.property.affects_layout() {
                continue;
            }
            let Ok(resolved) = self.resolved_style(transition.node_id) else {
                continue;
            };
            let _ = lock::mutex(&self.inner.layout).set_style(transition.node_id, &resolved);
        }
        finished
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

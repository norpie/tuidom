use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use crate::animation::{TransitionConfig, TransitionProperty};
use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;
use crate::style::Style;
use crate::style::resolution::{ResolvedStyle, StyleDefaults};

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
            for (prop, val) in driver.overrides_for(id) {
                match prop {
                    TransitionProperty::Opacity => resolved.opacity = val,
                }
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

        let Some(node) = self.inner.nodes.get(&id) else {
            return Err(TuidomError::NodeNotFound { id });
        };
        let resolved = if id == self.root() {
            ResolvedStyle::compute_with_defaults(
                &node,
                parent_resolved.as_ref(),
                &StyleDefaults::root(),
            )
        } else {
            ResolvedStyle::compute(&node, parent_resolved.as_ref())
        };

        *lock::rw_write(&node.resolved_style) = Some(resolved.clone());
        Ok(resolved)
    }

    /// Signal the animation driver about a style change and spawn tick task if needed.
    fn signal_animation(&self, id: NodeId, old_resolved: &ResolvedStyle) -> Result<()> {
        // Read transition configs before locking the driver
        let configs = {
            let Some(node) = self.inner.nodes.get(&id) else {
                return Err(TuidomError::NodeNotFound { id });
            };
            node.transition_configs.clone()
        };

        // Compute the new resolved value BEFORE locking the driver
        let new_resolved = self.resolved_base_style(id)?;

        let mut driver = lock::mutex(&self.inner.animation);
        driver.style_changed(id, old_resolved, &new_resolved, &configs);
        drop(driver);

        self.inner.anim_config_changed.notify_one();

        Ok(())
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

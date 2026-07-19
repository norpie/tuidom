use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::event::{FocusKeys, KeyCode};
use crate::id::{NodeId, NodeSet};
use crate::inner::PseudoStyles;
use crate::lock;
use crate::node::LayoutRect;
use crate::paint_order::paint_order;
use crate::style::{Display, Style};

impl Document {
    /// Set whether a node can receive focus.
    ///
    /// If focusability is removed from the currently focused node, focus is
    /// blurred and blur listeners are dispatched.
    pub fn set_focusable(&self, node: NodeId, focusable: bool) -> Result<()> {
        self.ensure_focus_node_exists(node)?;

        if focusable {
            lock::mutex(&self.inner.focusable_nodes).insert(node);
        } else {
            lock::mutex(&self.inner.focusable_nodes).remove(&node);
            if self.focused() == Some(node) {
                self.blur();
            }
        }

        Ok(())
    }

    /// Return whether a node can receive focus.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn is_focusable(&self, node: NodeId) -> Result<bool> {
        self.ensure_focus_node_exists(node)?;
        Ok(lock::mutex(&self.inner.focusable_nodes).contains(&node))
    }

    /// Move focus to a focusable node.
    ///
    /// Dispatches blur listeners for the previously focused node, followed by
    /// focus listeners for `node`. Calling this for the already-focused node is
    /// a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document, is not focusable, or
    /// is effectively disabled.
    pub fn focus(&self, node: NodeId) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        if !self.can_focus(node) {
            return Err(TuidomError::NodeNotFocusable { id: node });
        }

        let previous = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let active = contexts.active_mut();
            if active.focused == Some(node) {
                return Ok(());
            }
            active.focused.replace(node)
        };

        self.transition_focus(previous, Some(node))
    }

    /// Clear the current focus, if any.
    ///
    /// Dispatches blur listeners for the previously focused node.
    pub fn blur(&self) {
        let previous = lock::mutex(&self.inner.focus_contexts)
            .active_mut()
            .focused
            .take();
        if let Err(err) = self.transition_focus(previous, None) {
            log::error!("failed to refresh focus style after blur: {err}");
        }
    }

    /// Return the currently focused node, if one exists.
    ///
    /// Focus is scoped to the active focus context, so this is the focused node *within* an
    /// open modal-like context, not a document-wide value.
    pub fn focused(&self) -> Option<NodeId> {
        lock::mutex(&self.inner.focus_contexts).active().focused
    }

    /// Replace the document-level focus key bindings.
    pub fn set_focus_keys(&self, keys: FocusKeys) {
        *lock::mutex(&self.inner.focus_keys) = keys;
    }

    /// Return the document-level focus key bindings.
    pub fn focus_keys(&self) -> FocusKeys {
        lock::mutex(&self.inner.focus_keys).clone()
    }

    /// Set the style merged into a node's resolved style while it is focused.
    ///
    /// Unset properties do not override the node's base style. Inherited
    /// properties resolve from the focused node's parent, matching normal style
    /// inheritance semantics.
    pub fn set_focus_style(&self, node: NodeId, style: &Style) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        lock::mutex(&self.inner.pseudo_styles)
            .entry(node)
            .or_default()
            .focus = Some(style.clone());
        if self.focused() == Some(node) {
            self.refresh_focus_style_effect(node)?;
            self.inner.notify.notify_one();
        }
        Ok(())
    }

    /// Clear a node's focus style.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn clear_focus_style(&self, node: NodeId) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        self.clear_pseudo_style(node, |pseudo| pseudo.focus = None);
        if self.focused() == Some(node) {
            self.refresh_focus_style_effect(node)?;
            self.inner.notify.notify_one();
        }
        Ok(())
    }

    pub(crate) fn apply_focus_default_action(&self, code: KeyCode) {
        let Some(action) = self.focus_action_for_key(code) else {
            return;
        };

        match action {
            FocusAction::Next => self.focus_next(),
            FocusAction::Previous => self.focus_previous(),
            FocusAction::Up => self.focus_spatial(Direction::Up),
            FocusAction::Down => self.focus_spatial(Direction::Down),
            FocusAction::Left => self.focus_spatial(Direction::Left),
            FocusAction::Right => self.focus_spatial(Direction::Right),
            FocusAction::Blur => self.blur(),
        }
    }

    pub(crate) fn focus_target_from_hit(&self, hit: NodeId) -> Option<NodeId> {
        // A disabled or inert node swallows the interaction instead of handing it to an
        // interactive ancestor, matching how those targets drop events.
        if self.blocks_interaction(hit) {
            return None;
        }

        let focusable = lock::mutex(&self.inner.focusable_nodes).clone();
        let mut current = Some(hit);
        while let Some(node) = current {
            if focusable.contains(&node) && self.inner.nodes.contains_key(&node) {
                return Some(node);
            }
            current = self.get_parent(node);
        }
        None
    }

    /// Whether a node is allowed to take focus right now.
    pub(super) fn can_focus(&self, node: NodeId) -> bool {
        self.inner.nodes.contains_key(&node)
            && lock::mutex(&self.inner.focusable_nodes).contains(&node)
            && !self.blocks_interaction(node)
    }

    /// Move focus from `previous` to `next`, refreshing both styles and dispatching blur
    /// before focus.
    ///
    /// Callers update the focus context stack first; this settles the side effects, so
    /// context pushes and pops share one focus-change path with plain `focus`/`blur`.
    pub(super) fn transition_focus(
        &self,
        previous: Option<NodeId>,
        next: Option<NodeId>,
    ) -> Result<()> {
        if previous == next {
            return Ok(());
        }

        if let Some(previous) = previous {
            self.refresh_focus_style_effect(previous)?;
            self.dispatch_blur_to(previous);
        }
        if let Some(next) = next {
            self.refresh_focus_style_effect(next)?;
            self.dispatch_focus_to(next);
        }

        self.inner.notify.notify_one();
        Ok(())
    }

    /// Drop one pseudo-style, discarding the entry once no pseudo-style remains.
    pub(super) fn clear_pseudo_style(&self, node: NodeId, clear: impl FnOnce(&mut PseudoStyles)) {
        let mut pseudo_styles = lock::mutex(&self.inner.pseudo_styles);
        let Some(pseudo) = pseudo_styles.get_mut(&node) else {
            return;
        };
        clear(pseudo);
        if pseudo.is_empty() {
            pseudo_styles.remove(&node);
        }
    }

    pub(super) fn remove_focus_side_state(&self, node: NodeId) {
        lock::mutex(&self.inner.focusable_nodes).remove(&node);
        lock::mutex(&self.inner.pseudo_styles).remove(&node);

        let removed_active = {
            let mut active = lock::mutex(&self.inner.active_node);
            if *active == Some(node) {
                *active = None;
                true
            } else {
                false
            }
        };

        let removed_focus = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let was_focused = contexts.active().focused == Some(node);
            // An outer context may still remember this node as its focus, so clear every
            // level rather than only the active one.
            for context in contexts.iter_mut() {
                if context.focused == Some(node) {
                    context.focused = None;
                }
            }
            was_focused
        };

        if removed_focus || removed_active {
            self.inner.notify.notify_one();
        }
    }

    pub(super) fn ensure_focus_node_exists(&self, node: NodeId) -> Result<()> {
        if self.inner.nodes.contains_key(&node) {
            Ok(())
        } else {
            Err(TuidomError::NodeNotFound { id: node })
        }
    }

    fn refresh_focus_style_effect(&self, node: NodeId) -> Result<()> {
        if !self.inner.nodes.contains_key(&node) {
            return Ok(());
        }
        // The cache still holds the pre-change merged style; capturing it before
        // invalidating lets the animation driver diff the focus change like any
        // other style change.
        let old_resolved = self.resolved_base_style(node)?;
        self.invalidate_resolved_style(node);
        self.sync_layout_subtree_styles(node)?;
        self.signal_animation(node, &old_resolved)
    }

    fn focus_action_for_key(&self, code: KeyCode) -> Option<FocusAction> {
        let keys = self.focus_keys();
        if keys.next.contains(&code) {
            Some(FocusAction::Next)
        } else if keys.previous.contains(&code) {
            Some(FocusAction::Previous)
        } else if keys.up.contains(&code) {
            Some(FocusAction::Up)
        } else if keys.down.contains(&code) {
            Some(FocusAction::Down)
        } else if keys.left.contains(&code) {
            Some(FocusAction::Left)
        } else if keys.right.contains(&code) {
            Some(FocusAction::Right)
        } else if keys.blur.contains(&code) {
            Some(FocusAction::Blur)
        } else {
            None
        }
    }

    fn focus_next(&self) {
        let focusable = self.focusable_in_dom_order();
        let Some(next) = next_focus_target(self.focused(), &focusable) else {
            return;
        };
        if let Err(err) = self.focus(next) {
            log::error!("focus default action failed: {err}");
        }
    }

    fn focus_previous(&self) {
        let focusable = self.focusable_in_dom_order();
        let Some(previous) = previous_focus_target(self.focused(), &focusable) else {
            return;
        };
        if let Err(err) = self.focus(previous) {
            log::error!("focus default action failed: {err}");
        }
    }

    /// Focusable nodes in DOM order, scoped to the active focus context.
    ///
    /// Walking from the context node rather than the document root is what keeps tab order
    /// inside an open modal-like context — no filtering pass is needed.
    pub(super) fn focusable_in_dom_order(&self) -> Vec<NodeId> {
        let focusable = lock::mutex(&self.inner.focusable_nodes).clone();
        let mut nodes = Vec::new();
        self.collect_focusable_in_dom_order(self.active_focus_context(), &focusable, &mut nodes);
        nodes
    }

    fn collect_focusable_in_dom_order(
        &self,
        node: NodeId,
        focusable: &NodeSet,
        nodes: &mut Vec<NodeId>,
    ) {
        // A disabled node disables its whole subtree, so prune instead of descending.
        if lock::mutex(&self.inner.disabled_nodes).contains(&node) {
            return;
        }

        // A hidden node hides its whole subtree too. Spatial navigation already skips these
        // because they have no layout rect; without this, tab would disagree and walk into
        // controls that are not on screen.
        if self
            .resolved_style(node)
            .is_ok_and(|resolved| resolved.display == Display::None)
        {
            return;
        }

        if focusable.contains(&node) && self.inner.nodes.contains_key(&node) {
            nodes.push(node);
        }

        for child in self.get_children(node) {
            self.collect_focusable_in_dom_order(child, focusable, nodes);
        }
    }

    fn focus_spatial(&self, direction: Direction) {
        let Some(current) = self.focused() else {
            return;
        };
        let Some(next) = self.spatial_focus_target(current, direction) else {
            return;
        };
        if let Err(err) = self.focus(next) {
            log::error!("spatial focus default action failed: {err}");
        }
    }

    fn spatial_focus_target(&self, current: NodeId, direction: Direction) -> Option<NodeId> {
        let focusable = lock::mutex(&self.inner.focusable_nodes).clone();
        let entries = paint_order(self);
        let current_layout = entries
            .iter()
            .find(|entry| entry.id == current)
            .map(|entry| entry.layout)?;

        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.id != current
                    && focusable.contains(&entry.id)
                    && entry.layout.width > 0
                    && entry.layout.height > 0
                    && !self.blocks_interaction(entry.id)
            })
            .filter_map(|(paint_rank, entry)| {
                let distance = directional_distance(current_layout, entry.layout, direction)?;
                Some(FocusCandidate {
                    node: entry.id,
                    distance,
                    paint_rank,
                })
            })
            .min_by(|left, right| left.cmp_for_focus(right))
            .map(|candidate| candidate.node)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusAction {
    Next,
    Previous,
    Up,
    Down,
    Left,
    Right,
    Blur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusCandidate {
    node: NodeId,
    distance: (i64, i64),
    paint_rank: usize,
}

impl FocusCandidate {
    fn cmp_for_focus(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .cmp(&other.distance)
            .then_with(|| other.paint_rank.cmp(&self.paint_rank))
    }
}

fn next_focus_target(current: Option<NodeId>, focusable: &[NodeId]) -> Option<NodeId> {
    match current {
        None => focusable.first().copied(),
        Some(current) => {
            let index = focusable.iter().position(|node| *node == current)?;
            focusable.get(index + 1).copied()
        }
    }
}

fn previous_focus_target(current: Option<NodeId>, focusable: &[NodeId]) -> Option<NodeId> {
    match current {
        None => focusable.last().copied(),
        Some(current) => {
            let index = focusable.iter().position(|node| *node == current)?;
            index
                .checked_sub(1)
                .and_then(|index| focusable.get(index).copied())
        }
    }
}

fn directional_distance(
    current: LayoutRect,
    candidate: LayoutRect,
    direction: Direction,
) -> Option<(i64, i64)> {
    let current_edges = RectEdges::from_layout(current);
    let candidate_edges = RectEdges::from_layout(candidate);

    match direction {
        Direction::Up => {
            if candidate_edges.bottom > current_edges.top {
                return None;
            }
            Some((
                current_edges.top - candidate_edges.bottom,
                center_distance(current_edges.center_x, candidate_edges.center_x),
            ))
        }
        Direction::Down => {
            if candidate_edges.top < current_edges.bottom {
                return None;
            }
            Some((
                candidate_edges.top - current_edges.bottom,
                center_distance(current_edges.center_x, candidate_edges.center_x),
            ))
        }
        Direction::Left => {
            if candidate_edges.right > current_edges.left {
                return None;
            }
            Some((
                current_edges.left - candidate_edges.right,
                center_distance(current_edges.center_y, candidate_edges.center_y),
            ))
        }
        Direction::Right => {
            if candidate_edges.left < current_edges.right {
                return None;
            }
            Some((
                candidate_edges.left - current_edges.right,
                center_distance(current_edges.center_y, candidate_edges.center_y),
            ))
        }
    }
}

fn center_distance(left: i64, right: i64) -> i64 {
    (left - right).abs()
}

#[derive(Debug, Clone, Copy)]
struct RectEdges {
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
    center_x: i64,
    center_y: i64,
}

impl RectEdges {
    fn from_layout(layout: LayoutRect) -> Self {
        let left = i64::from(layout.x);
        let top = i64::from(layout.y);
        let right = left + i64::from(layout.width);
        let bottom = top + i64::from(layout.height);
        Self {
            left,
            right,
            top,
            bottom,
            center_x: left + i64::from(layout.width) / 2,
            center_y: top + i64::from(layout.height) / 2,
        }
    }
}

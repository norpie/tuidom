use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;

impl Document {
    /// The node whose subtree currently traps focus.
    ///
    /// Focus, tab order, and spatial navigation are all scoped to this node's subtree. This is
    /// the innermost open context whose node is not hidden: a context whose node or an
    /// ancestor has `display: none` traps nothing until it is shown again. With no such
    /// context open this is the document root, so the whole tree is in scope.
    pub fn active_focus_context(&self) -> NodeId {
        let hidden = self.hidden_focus_contexts();
        lock::mutex(&self.inner.focus_contexts)
            .active(&hidden)
            .context
    }

    /// The number of open focus contexts, counting the permanent root context and any
    /// context whose node is hidden.
    ///
    /// A depth of 1 means nothing traps focus.
    pub fn focus_context_depth(&self) -> usize {
        lock::mutex(&self.inner.focus_contexts).depth()
    }

    /// Trap focus inside a stacking context's subtree.
    ///
    /// This is the mechanism behind modals and dropdowns. While the context is open, nodes
    /// outside its subtree are inert: they cannot be focused, are skipped by tab and spatial
    /// navigation, and swallow targeted events. Focus moves to the first focusable node in
    /// the context, and the previously focused node is remembered for
    /// [`pop_focus_context`](Self::pop_focus_context).
    ///
    /// The node must be a stacking context. Trapping focus in a subtree that a sibling can
    /// paint over would leave the user interacting with something they cannot see.
    ///
    /// While the node is hidden the context is open but traps nothing, and focus stays where
    /// it is; it takes effect when the node is shown. Each context remembers its own focused
    /// node, so hiding and showing it again returns focus to where it was inside.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist, is not a stacking context, or is already
    /// an open focus context.
    pub fn push_focus_context(&self, node: NodeId) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        if !self.resolved_style(node)?.stacking_context {
            return Err(TuidomError::NotAStackingContext { id: node });
        }

        let hidden = self.hidden_focus_contexts();
        let previous = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            if contexts.contains(node) {
                return Err(TuidomError::FocusContextAlreadyOpen { id: node });
            }
            let previous = contexts.active(&hidden).focused;
            contexts.push(node);
            previous
        };
        if self.is_effectively_hidden(node) {
            return Ok(());
        }
        lock::mutex(&self.inner.focus_contexts).settled = node;

        // The context is active from here on, so this searches inside it.
        let next = self.focusable_in_dom_order().first().copied();
        if let Some(next) = next {
            let hidden = self.hidden_focus_contexts();
            lock::mutex(&self.inner.focus_contexts)
                .active_mut(&hidden)
                .focused = Some(next);
        }

        self.transition_focus(previous, next)
    }

    /// Close the innermost focus context and restore the focus it interrupted.
    ///
    /// Focus returns to the node that was focused when the context opened, provided it still
    /// exists, is still focusable, and is not disabled. Otherwise focus is left cleared
    /// rather than jumping to some other node the user never selected. The innermost context
    /// is closed even while its node is hidden, in which case focus was already elsewhere.
    ///
    /// Returns the node whose context was closed.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is open — the root context is permanent.
    pub fn pop_focus_context(&self) -> Result<NodeId> {
        let (closed, previous) = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let previous = contexts.focused_in(contexts.settled);
            match contexts.pop() {
                Some(closed) => (closed.context, previous),
                None => return Err(TuidomError::CannotPopRootFocusContext),
            }
        };

        self.restore_focus_in_active_context(previous, false)?;
        Ok(closed)
    }

    /// Whether a node is shut out of interaction because a focus context elsewhere traps it.
    ///
    /// Inert nodes cannot be focused, are skipped by tab and spatial navigation, and swallow
    /// input events. Unlike a disabled node, an inert node merges no extra style — content
    /// behind a modal keeps its own appearance.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist in this document.
    pub fn is_inert(&self, node: NodeId) -> Result<bool> {
        self.ensure_focus_node_exists(node)?;
        Ok(self.is_inert_unlocked(node))
    }

    pub(crate) fn is_inert_unlocked(&self, node: NodeId) -> bool {
        let context = self.active_focus_context();
        if context == self.inner.root {
            // The root context spans the whole document, so nothing is inert.
            return false;
        }
        !self.is_self_or_descendant(context, node)
    }

    /// Whether a node is shut out of interaction, whether by being disabled or by being
    /// inert.
    ///
    /// The two states block the same things; only disabled also merges a style.
    pub(crate) fn blocks_interaction(&self, node: NodeId) -> bool {
        self.is_effectively_disabled_unlocked(node) || self.is_inert_unlocked(node)
    }

    /// The nodes of open contexts that are hidden, which the active context skips.
    ///
    /// Takes no tree lock, so it is safe from a caller holding one, and resolves styles with
    /// the context stack unlocked.
    pub(crate) fn hidden_focus_contexts(&self) -> Vec<NodeId> {
        let nested = lock::mutex(&self.inner.focus_contexts).nested_nodes();
        nested
            .into_iter()
            .filter(|&node| self.is_effectively_hidden_unlocked(node))
            .collect()
    }

    /// Close focus contexts whose node has left the tree, then re-validate focus.
    ///
    /// A modal-like component that is removed without being popped would otherwise trap
    /// focus inside a subtree that no longer exists, which nothing could recover from.
    /// Runs after tree mutation so focus handlers are never dispatched under the tree lock.
    pub(super) fn settle_focus_contexts(&self) {
        let previous = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let previous = contexts.focused_in(contexts.settled);
            contexts.prune(|node| self.inner.nodes.contains_key(&node));
            previous
        };

        if let Err(err) = self.restore_focus_in_active_context(previous, false) {
            tracing::error!("failed to settle focus contexts: {err}");
        }
    }

    /// Move focus if a change hid or showed a context's node and so changed which context is
    /// active: from what the previously active context had focused to what the new one
    /// remembers, or to its first focusable node when it remembers nothing, as a push would.
    /// The context left behind keeps its memory for when it is active again.
    pub(super) fn settle_active_focus_context(&self) {
        let hidden = self.hidden_focus_contexts();
        let previous = {
            let contexts = lock::mutex(&self.inner.focus_contexts);
            if contexts.active(&hidden).context == contexts.settled {
                return;
            }
            contexts.focused_in(contexts.settled)
        };

        if let Err(err) = self.restore_focus_in_active_context(previous, true) {
            tracing::error!("failed to settle the active focus context: {err}");
        }
    }

    /// Re-focus whatever the now-active context remembers, dropping the memory if the node
    /// is no longer a valid focus target, and record that context as settled. With
    /// `first_if_none`, a nested context remembering nothing focuses its first focusable node.
    fn restore_focus_in_active_context(
        &self,
        previous: Option<NodeId>,
        first_if_none: bool,
    ) -> Result<()> {
        let hidden = self.hidden_focus_contexts();
        let (active, remembered) = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let active = contexts.active(&hidden).context;
            contexts.settled = active;
            (active, contexts.active(&hidden).focused)
        };

        // `can_focus` consults the context stack itself, so it must be called with the lock
        // released.
        let mut restored = remembered.filter(|node| self.can_focus(*node));
        if restored.is_none() && first_if_none && active != self.inner.root {
            restored = self.focusable_in_dom_order().first().copied();
        }
        if restored != remembered {
            lock::mutex(&self.inner.focus_contexts)
                .active_mut(&hidden)
                .focused = restored;
        }

        self.transition_focus(previous, restored)
    }
}

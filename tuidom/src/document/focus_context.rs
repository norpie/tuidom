use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::lock;

impl Document {
    /// The node whose subtree currently traps focus.
    ///
    /// Focus, tab order, and spatial navigation are all scoped to this node's subtree. With
    /// no modal-like context open this is the document root, so the whole tree is in scope.
    pub fn active_focus_context(&self) -> NodeId {
        lock::mutex(&self.inner.focus_contexts).active().context
    }

    /// The number of open focus contexts, counting the permanent root context.
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
    /// # Errors
    ///
    /// Returns an error if `node` does not exist, is not a stacking context, or is already
    /// an open focus context.
    pub fn push_focus_context(&self, node: NodeId) -> Result<()> {
        self.ensure_focus_node_exists(node)?;
        if !self.resolved_style(node)?.stacking_context {
            return Err(TuidomError::NotAStackingContext { id: node });
        }

        let previous = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            if contexts.contains(node) {
                return Err(TuidomError::FocusContextAlreadyOpen { id: node });
            }
            let previous = contexts.active().focused;
            contexts.push(node);
            previous
        };

        // The context is active from here on, so this searches inside it.
        let next = self.focusable_in_dom_order().first().copied();
        if let Some(next) = next {
            lock::mutex(&self.inner.focus_contexts).active_mut().focused = Some(next);
        }

        self.transition_focus(previous, next)
    }

    /// Close the innermost focus context and restore the focus it interrupted.
    ///
    /// Focus returns to the node that was focused when the context opened, provided it still
    /// exists, is still focusable, and is not disabled. Otherwise focus is left cleared
    /// rather than jumping to some other node the user never selected.
    ///
    /// Returns the node whose context was closed.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is open — the root context is permanent.
    pub fn pop_focus_context(&self) -> Result<NodeId> {
        let (closed, previous) = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            match contexts.pop() {
                Some(closed) => (closed.context, closed.focused),
                None => return Err(TuidomError::CannotPopRootFocusContext),
            }
        };

        self.restore_focus_in_active_context(previous)?;
        Ok(closed)
    }

    /// Close focus contexts whose node has left the tree, then re-validate focus.
    ///
    /// A modal-like component that is removed without being popped would otherwise trap
    /// focus inside a subtree that no longer exists, which nothing could recover from.
    /// Runs after tree mutation so focus handlers are never dispatched under the tree lock.
    pub(super) fn settle_focus_contexts(&self) {
        let previous = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let previous = contexts.active().focused;
            contexts.prune(|node| self.inner.nodes.contains_key(&node));
            previous
        };

        if let Err(err) = self.restore_focus_in_active_context(previous) {
            log::error!("failed to settle focus contexts: {err}");
        }
    }

    /// Re-focus whatever the now-active context remembers, dropping the memory if the node
    /// is no longer a valid focus target.
    fn restore_focus_in_active_context(&self, previous: Option<NodeId>) -> Result<()> {
        let restored = {
            let mut contexts = lock::mutex(&self.inner.focus_contexts);
            let active = contexts.active_mut();
            if active.focused.is_some_and(|node| !self.can_focus(node)) {
                active.focused = None;
            }
            active.focused
        };

        self.transition_focus(previous, restored)
    }
}

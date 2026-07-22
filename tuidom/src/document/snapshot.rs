//! Consistent point-in-time views of a document, for inspectors and debugging tools.
//!
//! Every value here is reachable one accessor at a time — [`get_node`](Document::get_node),
//! [`resolved_style`](Document::resolved_style), [`focused`](Document::focused) and the
//! rest. What a snapshot adds is that they are read **together**, under one tree guard, so
//! nothing mutates between them.
//!
//! That is not a nicety. Walking a tree with per-call locking lets a mutation land
//! mid-walk, and the results do not merely go stale — they go impossible: a parent listing
//! a child that no longer exists, or a node's layout from before a change beside its style
//! from after. An inspector built on that shows its user a tree that never existed, and the
//! bug looks like it lives in the application.

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::{Document, SelectionPoint};
use crate::id::NodeId;
use crate::lock;
use crate::node::{LayoutView, NodeKindView, ScrollOffset};
use crate::style::Style;
use crate::style::resolution::ResolvedStyle;

/// A consistent view of a whole document: its tree, and the document-level runtime state
/// that gives the tree meaning.
///
/// Taken under a single tree read guard, so the nodes are mutually consistent and the
/// document-level fields agree with them — a `focused` id is present in `nodes`, and a
/// `selection` endpoint names a node that is really there.
///
/// See [`Document::snapshot`] for what it costs and when not to take one.
#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    /// The document's permanent root node.
    pub root: NodeId,
    /// Every node reachable from [`root`](Self::root), in depth-first document order —
    /// each node immediately before its subtree, siblings in child order.
    ///
    /// This is the tree, so a node created but never appended is absent. That makes
    /// `nodes.len()` differ from [`node_count`](Document::node_count), and the difference
    /// is exactly the number of orphans; [`Document::snapshot_node`] reaches those.
    pub nodes: Vec<NodeSnapshot>,
    /// The focused node within the active focus context, if any.
    pub focused: Option<NodeId>,
    /// The node currently being pressed, if any.
    pub active: Option<NodeId>,
    /// The node whose subtree currently traps focus — the document root when nothing does.
    pub focus_context: NodeId,
    /// Number of open focus contexts, counting the permanent root context. A depth of 1
    /// means nothing traps focus.
    pub focus_context_depth: usize,
    /// The document text selection as a document-ordered `(start, end)` pair.
    pub selection: Option<(SelectionPoint, SelectionPoint)>,
}

/// One node's full state: what it is, how it is styled, where it landed, and what the
/// runtime currently thinks about it.
///
/// The runtime flags at the bottom live on `DocumentInner` keyed by [`NodeId`] rather than
/// on the node itself, and each sits behind its own lock. Joining them onto the node is
/// most of what a snapshot is for.
#[derive(Debug, Clone)]
pub struct NodeSnapshot {
    /// The node's ID.
    pub id: NodeId,
    /// Parent node, if any. `None` for the root, and for an orphan.
    pub parent: Option<NodeId>,
    /// Ordered list of child node IDs.
    pub children: Vec<NodeId>,
    /// The node kind, with text content and input state as of this snapshot.
    pub kind: NodeKindView,
    /// Arbitrary string attributes.
    pub attrs: HashMap<String, String>,
    /// Computed layout, or `None` if layout has not run since the node was added.
    pub layout: Option<LayoutView>,
    /// The declared inline style, with every property's
    /// [`StyleValue`](crate::style::StyleValue) state intact.
    ///
    /// Paired with [`resolved`](Self::resolved) this is the declared-versus-computed view:
    /// `resolved` says what the engine used, `style` says how much of that the node asked
    /// for, and an `Unset` here against a non-default there means the value was inherited
    /// or defaulted rather than written.
    pub style: Style,
    /// The fully resolved style the engine actually used: inheritance walked, defaults
    /// applied, pseudo-states merged, animation overrides on top.
    ///
    /// Shared rather than copied. `ResolvedStyle` is several hundred bytes and the style
    /// cache already holds it behind an `Arc`, so a snapshot of a large tree costs pointers
    /// instead of a few megabytes of duplicated style.
    pub resolved: Arc<ResolvedStyle>,
    /// Current scroll offset. `(0, 0)` for anything never scrolled.
    pub scroll: ScrollOffset,
    /// Whether this node can receive focus at all.
    ///
    /// Independent of whether it *could* right now — a focusable node that is
    /// [`effectively_disabled`](Self::effectively_disabled) or [`inert`](Self::inert) still
    /// reads `true` here, since this is the property that was set on it rather than the
    /// verdict those two produce.
    pub focusable: bool,
    /// Whether this node holds focus in the active focus context.
    pub focused: bool,
    /// Whether this node is the one currently being pressed.
    pub active: bool,
    /// Whether this node is itself marked disabled, ignoring its ancestors.
    pub disabled: bool,
    /// Whether this node is disabled directly *or* through an ancestor. This is the one
    /// that decides whether it can be interacted with.
    pub effectively_disabled: bool,
    /// Whether this node is outside the active focus context, and so shut out of
    /// interaction without being styled any differently.
    pub inert: bool,
}

impl Document {
    /// Take a consistent snapshot of the whole document.
    ///
    /// The tree is walked under one read guard, so every node in the result agrees with
    /// every other: each listed child is present, each non-root node's parent is present,
    /// and the document-level fields refer to nodes in the same view. Reading the same
    /// values through individual accessors gives none of that.
    ///
    /// # Cost
    ///
    /// O(n) in the tree, allocating per node — a `Style`, an attribute map, and a child
    /// list each. This is an on-demand debugging surface, not a per-frame one; taking it
    /// from `on_post_frame` on every frame will cost more than the frame did.
    ///
    /// # Deadlock
    ///
    /// Takes the tree lock, so — like anything that touches the tree — it must not be
    /// called while a node guard is held. In practice that rules out one place:
    /// [`update_style`](Self::update_style)'s closure, which runs with the node borrowed.
    /// Calling it from an event listener is fine; listeners are dispatched with no guard
    /// held, which is what makes them safe to run engine code from at all.
    pub fn snapshot(&self) -> DocumentSnapshot {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);

        // Every side table is copied out under one acquisition each, rather than queried
        // per node. Two reasons, and the second is the one that matters: a per-node query
        // takes a lock per node, and `is_inert`/`is_effectively_disabled` each walk to the
        // root, making the whole walk O(n·depth) instead of O(n).
        let (focused, focus_context, focus_context_depth) = {
            let contexts = lock::mutex(&self.inner.focus_contexts);
            let active = contexts.active();
            (active.focused, active.context, contexts.depth())
        };
        let active = *lock::mutex(&self.inner.active_node);
        let selection = self.selection_unlocked();
        let scroll_offsets = lock::mutex(&self.inner.scroll_offsets).clone();
        let disabled_nodes = lock::mutex(&self.inner.disabled_nodes).clone();
        let focusable_nodes = lock::mutex(&self.inner.focusable_nodes).clone();
        let layouts = lock::rw_read(&self.inner.layout_snapshot).clone();

        let now = self.now();
        let root = self.inner.root;
        let mut nodes = Vec::new();

        // Preorder walk. `disabled` and `in_context` are carried down rather than
        // recomputed, which is what turns two rootward walks per node into two booleans.
        let mut stack = vec![(root, false, root == focus_context)];
        while let Some((id, parent_disabled, parent_in_context)) = stack.pop() {
            // Cloned out of the guard before anything downstream runs. `resolved_style_unlocked`
            // reads the same node, and holding a guard across it is the crate's central hazard.
            let Some((kind, parent, children, attrs, style)) =
                self.inner.nodes.get(&id).map(|data| {
                    (
                        data.kind.to_view(now),
                        data.parent,
                        data.children.clone(),
                        data.attrs.clone(),
                        data.style.clone(),
                    )
                })
            else {
                continue;
            };

            // Unreachable while the tree guard is held: the only failure is a missing node,
            // and removal needs the write lock. Skipping rather than fabricating a default
            // keeps the snapshot from ever reporting a style the engine did not use.
            let Ok(resolved) = self.resolved_style_unlocked(id) else {
                continue;
            };

            let disabled = disabled_nodes.contains(&id);
            let effectively_disabled = parent_disabled || disabled;
            let in_context = parent_in_context || id == focus_context;

            stack.extend(
                children
                    .iter()
                    .rev()
                    .map(|&child| (child, effectively_disabled, in_context)),
            );

            nodes.push(NodeSnapshot {
                id,
                parent,
                children,
                kind,
                attrs,
                layout: layouts.get(&id).and_then(|l| self.layout_view(id, *l)),
                style,
                resolved,
                scroll: scroll_offsets.get(&id).copied().unwrap_or_default(),
                focusable: focusable_nodes.contains(&id),
                focused: focused == Some(id),
                active: active == Some(id),
                disabled,
                effectively_disabled,
                inert: !in_context,
            });
        }

        DocumentSnapshot {
            root,
            nodes,
            focused,
            active,
            focus_context,
            focus_context_depth,
            selection,
        }
    }

    /// Take a snapshot of a single node, or `None` if it does not exist.
    ///
    /// Unlike [`snapshot`](Self::snapshot) this reaches orphans, since it addresses the
    /// node directly instead of walking down from the root.
    ///
    /// Carries the same deadlock constraint: it takes the tree lock, so it must not be
    /// called while a node guard is held.
    pub fn snapshot_node(&self, id: NodeId) -> Option<NodeSnapshot> {
        let _tree_guard = lock::rw_read(&self.inner.tree_mutation);

        let (kind, parent, children, attrs, style) = self.inner.nodes.get(&id).map(|data| {
            (
                data.kind.to_view(self.now()),
                data.parent,
                data.children.clone(),
                data.attrs.clone(),
                data.style.clone(),
            )
        })?;
        let resolved = self.resolved_style_unlocked(id).ok()?;

        // The rootward walks `snapshot` derives during its descent have to be paid for
        // here, since one node carries no context from its ancestors.
        let effectively_disabled = self.is_effectively_disabled_unlocked(id);
        let inert = self.is_inert_unlocked(id);

        Some(NodeSnapshot {
            id,
            parent,
            children,
            kind,
            attrs,
            layout: lock::rw_read(&self.inner.layout_snapshot)
                .get(&id)
                .copied()
                .and_then(|l| self.layout_view(id, l)),
            style,
            resolved,
            scroll: self.scroll_offset(id),
            focusable: lock::mutex(&self.inner.focusable_nodes).contains(&id),
            focused: self.focused() == Some(id),
            active: *lock::mutex(&self.inner.active_node) == Some(id),
            disabled: lock::mutex(&self.inner.disabled_nodes).contains(&id),
            effectively_disabled,
            inert,
        })
    }
}

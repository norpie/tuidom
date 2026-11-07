//! Handles from a component body to the nodes its view creates, and to the document the body
//! runs in.

use std::ops::Range;

use tuidom::animation::{AnimationHandle, KeyframeAnimation};
use tuidom::{Document, LayoutView, NodeId, Result, ScrollOffset};

use crate::runtime::{Signal, provide_context, state, untracked, use_context};

/// A handle to the node an element mounts, for the engine operations a view cannot declare.
///
/// Create one with [`node_ref`] in a component body and bind it to an element with
/// [`Element::node_ref`](crate::Element::node_ref) (`ref:` in `view!`). It holds the node and
/// its [`Document`] while that element is mounted and is empty otherwise. It carries the
/// document itself because event handlers run outside any scope, where [`use_document`] finds
/// nothing.
///
/// Reads are tracked: an effect that reads [`node`](Self::node), [`layout`](Self::layout), or
/// another getter reruns when the ref fills or empties. It does not rerun when the value read
/// changes, such as focus moving or layout shifting. Commands (focusing, scrolling, animating,
/// setting the cursor) are untracked, so calling one inside an effect does not subscribe it.
///
/// On an empty ref every command does nothing and every read returns `None` (`false` for
/// [`is_focused`](Self::is_focused)). A command the engine refuses, such as setting the cursor of
/// a box, is logged and ignored. A ref bound to two mounted elements holds whichever mounted last.
pub struct NodeRef {
    slot: Signal<Option<Bound>>,
}

impl Clone for NodeRef {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for NodeRef {}

#[derive(Clone)]
struct Bound {
    doc: Document,
    node: NodeId,
}

// A `NodeId` already names its document, so the node alone decides equality.
impl PartialEq for Bound {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

/// Create an empty [`NodeRef`], owned by the current scope.
pub fn node_ref() -> NodeRef {
    NodeRef { slot: state(None) }
}

impl NodeRef {
    /// The bound node, or `None` while no element holding this ref is mounted.
    pub fn node(&self) -> Option<NodeId> {
        self.bound().map(|bound| bound.node)
    }

    /// The document the bound node lives in, or `None` while the ref is empty.
    pub fn document(&self) -> Option<Document> {
        self.bound().map(|bound| bound.doc)
    }

    /// Move focus to the node.
    pub fn focus(&self) {
        self.command("focus", Document::focus);
    }

    /// Clear focus if the node holds it. Focus elsewhere is left alone.
    pub fn blur(&self) {
        if let Some(Bound { doc, node }) = self.bound_untracked()
            && doc.focused() == Some(node)
        {
            doc.blur();
        }
    }

    /// Whether the node holds focus right now.
    pub fn is_focused(&self) -> bool {
        self.bound()
            .is_some_and(|Bound { doc, node }| doc.focused() == Some(node))
    }

    /// Scroll the node to an offset in cells, clamped to how far its content can scroll.
    pub fn scroll_to(&self, x: u16, y: u16) {
        self.command("scroll", |doc, node| doc.scroll_to(node, x, y));
    }

    /// Scroll the node by a signed number of cells per axis, clamped like
    /// [`scroll_to`](Self::scroll_to).
    pub fn scroll_by(&self, dx: i32, dy: i32) {
        self.command("scroll", |doc, node| doc.scroll_by(node, dx, dy));
    }

    /// The node's scroll offset.
    pub fn scroll_offset(&self) -> Option<ScrollOffset> {
        self.bound()
            .map(|Bound { doc, node }| doc.scroll_offset(node))
    }

    /// Start a keyframe animation on the node. Pause, resume, or cancel it through
    /// [`document`](Self::document) with the returned handle.
    pub fn animate(&self, animation: KeyframeAnimation) -> Option<AnimationHandle> {
        let Bound { doc, node } = self.bound_untracked()?;
        doc.animate(node, animation)
            .inspect_err(|err| tracing::warn!(?node, "animate failed: {err}"))
            .ok()
    }

    /// The node's size and position as of the last frame.
    ///
    /// Not reactive: nothing reruns when the layout changes. It is `None` in
    /// [`on_mount`](crate::on_mount), since no frame has laid the new node out yet. To place
    /// something relative to a node, read its layout in the handler that triggers the placement.
    pub fn layout(&self) -> Option<LayoutView> {
        let Bound { doc, node } = self.bound()?;
        doc.get_node(node).and_then(|view| view.layout)
    }

    /// An input's cursor, as a byte offset into its value.
    pub fn cursor(&self) -> Option<usize> {
        self.read("cursor", Document::input_cursor)
    }

    /// Move an input's cursor to a byte offset, clamped to a grapheme boundary. Clears the
    /// selection.
    pub fn set_cursor(&self, cursor: usize) {
        self.command("set cursor", |doc, node| doc.set_input_cursor(node, cursor));
    }

    /// An input's selected byte range, or `None` when nothing is selected.
    pub fn selection(&self) -> Option<Range<usize>> {
        self.read("selection", Document::input_selection).flatten()
    }

    /// Select a byte range of an input's value.
    pub fn set_selection(&self, selection: Range<usize>) {
        self.command("set selection", |doc, node| {
            doc.set_input_selection(node, selection)
        });
    }

    /// Clear an input's selection, leaving the cursor where it is.
    pub fn clear_selection(&self) {
        self.command("clear selection", Document::clear_input_selection);
    }

    fn bound(&self) -> Option<Bound> {
        self.slot.try_with(Option::clone).flatten()
    }

    fn bound_untracked(&self) -> Option<Bound> {
        untracked(|| self.bound())
    }

    fn command(&self, what: &str, run: impl FnOnce(&Document, NodeId) -> Result<()>) {
        if let Some(Bound { doc, node }) = self.bound_untracked()
            && let Err(err) = run(&doc, node)
        {
            tracing::warn!(?node, "{what} failed: {err}");
        }
    }

    fn read<T>(&self, what: &str, get: impl FnOnce(&Document, NodeId) -> Result<T>) -> Option<T> {
        let Bound { doc, node } = self.bound()?;
        get(&doc, node)
            .inspect_err(|err| tracing::warn!(?node, "{what} failed: {err}"))
            .ok()
    }

    pub(crate) fn fill(&self, doc: &Document, node: NodeId) {
        self.slot.set(Some(Bound {
            doc: doc.clone(),
            node,
        }));
    }

    /// Empty the ref if it still holds `node`. A later mount may have taken it over already.
    pub(crate) fn release(&self, node: NodeId) {
        if untracked(|| self.node()) == Some(node) {
            self.slot.set(None);
        }
    }
}

/// The document as stored in context, under a type no user code can provide.
#[derive(Clone)]
struct DocumentContext(Document);

pub(crate) fn provide_document(doc: &Document) {
    provide_context(DocumentContext(doc.clone()));
}

/// The document the current component is mounted in.
///
/// Available in a component body and its [`on_mount`](crate::on_mount) callbacks, under
/// [`App`](crate::App) and a plain [`mount`](crate::mount) alike. `None` outside any mount,
/// including inside an event handler, which runs with no scope: capture the document in the
/// body and move it into the handler.
pub fn use_document() -> Option<Document> {
    use_context::<DocumentContext>().map(|ctx| ctx.0)
}

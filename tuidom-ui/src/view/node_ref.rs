//! Handles from a component body to the nodes its view creates, and to the document the body
//! runs in.

use std::ops::Range;

use tuidom::animation::{AnimationHandle, KeyframeAnimation};
use tuidom::{Document, LayoutRect, LayoutView, NodeId, Result, ScrollOffset};

use super::layout_watch::{self, LayoutSignals, layout_of};
use crate::runtime::{Signal, batch, provide_context, state, tracking, untracked, use_context};

/// A handle to the node an element mounts, for the engine operations a view cannot declare.
///
/// Create one with [`node_ref`] in a component body and bind it to an element with
/// [`Element::node_ref`](crate::Element::node_ref) (`ref:` in `view!`). It holds the node and
/// its [`Document`] while that element is mounted and is empty otherwise. It carries the
/// document itself because event handlers run outside any scope, where [`use_document`] finds
/// nothing.
///
/// Reads are tracked: an effect that reads [`node`](Self::node) or another getter reruns when
/// the ref fills or empties. Most do not rerun when the value read changes, such as focus
/// moving. [`layout`](Self::layout), [`screen_rect`](Self::screen_rect), and
/// [`visible_rect`](Self::visible_rect) do, one frame behind.
/// Commands (focusing, scrolling, animating, setting the cursor) are untracked, so calling one
/// inside an effect does not subscribe it.
///
/// On an empty ref every command does nothing and every read returns `None` (`false` for
/// [`is_focused`](Self::is_focused)). A command the engine refuses, such as setting the cursor of
/// a box, is logged and ignored. A ref bound to two mounted elements holds whichever mounted last.
pub struct NodeRef {
    slot: Signal<Option<Bound>>,
    watched: LayoutSignals,
}

impl Clone for NodeRef {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for NodeRef {}

/// A fresh, empty ref: what an optional `NodeRef` prop holds when the caller passes none, so the
/// component can bind it unconditionally.
impl Default for NodeRef {
    fn default() -> Self {
        node_ref()
    }
}

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
    NodeRef {
        slot: state(None),
        watched: LayoutSignals {
            layout: state(None),
            screen_rect: state(None),
            visible_rect: state(None),
        },
    }
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

    /// The node's size and position as laid out by the last frame, before any scrolling.
    ///
    /// Read inside an effect, the effect reruns when a frame lays the node out differently.
    /// Scrolling does not rerun it, since a scroll never reruns layout. The new value arrives
    /// after the frame that produced it, so something drawn from it lags one frame behind. It
    /// is `None` until the first frame after mount, so always `None` in
    /// [`on_mount`](crate::on_mount).
    ///
    /// An effect that resizes the node it reads can rerun on every frame without settling.
    pub fn layout(&self) -> Option<LayoutView> {
        self.watched_read(self.watched.layout, layout_of)
    }

    /// Where the node is drawn: its layout rect shifted by its scrolling ancestors' offsets.
    ///
    /// To place a tooltip beside the node, prefer
    /// [`Position::Anchored`](tuidom::style::Position::Anchored), which the engine resolves
    /// without the lag. Read inside an effect, the effect reruns when the node is laid out differently or an ancestor scrolls, one
    /// frame behind as with [`layout`](Self::layout). A node scrolled out of its container's
    /// viewport still has a rect, lying outside that viewport; [`visible_rect`](Self::visible_rect)
    /// says whether any of it shows.
    pub fn screen_rect(&self) -> Option<LayoutRect> {
        self.watched_read(self.watched.screen_rect, Document::screen_rect)
    }

    /// The part of the node that reaches the screen: its [`screen_rect`](Self::screen_rect)
    /// clipped by its scrolling and clipping ancestors and the terminal edge, or `None` when
    /// none of it is drawn.
    ///
    /// Read inside an effect, the effect reruns when the visible part changes, one frame behind as with
    /// [`layout`](Self::layout). A node covered by something painted over it, such as a modal,
    /// still counts as visible. See [`Document::visible_rect`] for every case that is `None`.
    pub fn visible_rect(&self) -> Option<LayoutRect> {
        self.watched_read(self.watched.visible_rect, Document::visible_rect)
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

    /// Inside an effect, read `signal` after making sure the watcher keeps it current; outside
    /// one, ask the engine, since nothing would rerun on a later change.
    fn watched_read<T: Clone + 'static>(
        &self,
        signal: Signal<Option<T>>,
        direct: impl FnOnce(&Document, NodeId) -> Option<T>,
    ) -> Option<T> {
        let Bound { doc, node } = self.bound()?;
        if !tracking() {
            return direct(&doc, node);
        }
        layout_watch::watch(&doc, node, self.watched);
        signal.try_get().flatten()
    }

    fn read<T>(&self, what: &str, get: impl FnOnce(&Document, NodeId) -> Result<T>) -> Option<T> {
        let Bound { doc, node } = self.bound()?;
        get(&doc, node)
            .inspect_err(|err| tracing::warn!(?node, "{what} failed: {err}"))
            .ok()
    }

    /// Bind the ref to `node`. A node it held before stops being watched; a reader reruns on the
    /// change and watches the new one.
    pub(crate) fn fill(&self, doc: &Document, node: NodeId) {
        let previous = self.bound_untracked();
        batch(|| {
            if let Some(previous) = previous {
                self.unbind(&previous);
            }
            self.slot.set(Some(Bound {
                doc: doc.clone(),
                node,
            }));
        });
    }

    /// Empty the ref if it still holds `node`. A later mount may have taken it over already.
    pub(crate) fn release(&self, node: NodeId) {
        if let Some(bound) = self.bound_untracked()
            && bound.node == node
        {
            batch(|| {
                self.unbind(&bound);
                self.slot.set(None);
            });
        }
    }

    fn unbind(&self, bound: &Bound) {
        layout_watch::unwatch(&bound.doc, bound.node);
        self.watched.clear();
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

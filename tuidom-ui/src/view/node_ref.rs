//! Handles from a component body to the nodes its view creates, and to the document the body
//! runs in.

use tuidom::{Document, NodeId};

use crate::runtime::{Signal, provide_context, state, untracked, use_context};

/// A handle to the node an element mounts, for the engine operations a view cannot declare.
///
/// Create one with [`node_ref`] in a component body and bind it to an element with
/// [`Element::node_ref`](crate::Element::node_ref) (`ref:` in `view!`). It holds the node and
/// its [`Document`] while that element is mounted and is empty otherwise. It carries the
/// document itself because event handlers run outside any scope, where [`use_document`] finds
/// nothing.
///
/// Reads are tracked: an effect that reads [`node`](Self::node) reruns when the ref fills or
/// empties. A ref bound to two mounted elements holds whichever mounted last.
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

    fn bound(&self) -> Option<Bound> {
        self.slot.try_with(Option::clone).flatten()
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

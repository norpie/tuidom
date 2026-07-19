//! Node handle types.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxBuildHasher;

/// A map keyed by [`NodeId`].
///
/// The default hasher is SipHash, built to resist collision attacks on
/// adversarial keys. A `NodeId` is two integers this crate allocates itself, so
/// none of that applies, and hashing it showed up as a real share of a frame —
/// the render walk looks up every node in several of these maps.
pub(crate) type NodeMap<V> = HashMap<NodeId, V, FxBuildHasher>;

/// A set of [`NodeId`]s, hashed like [`NodeMap`].
pub(crate) type NodeSet = HashSet<NodeId, FxBuildHasher>;

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a unique internal document identity.
pub(crate) fn next_document_id() -> u64 {
    NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Lightweight, `Copy` integer handle that references a node in one document's arena.
///
/// NodeIds are cheap to copy and pass around. They remain valid until the
/// referenced node is removed. Using a stale [`NodeId`] (after removal) will
/// return `None` from [`Document`] methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) document_id: u64,
    pub(crate) index: u64,
}

impl NodeId {
    /// Create a document-agnostic [`NodeId`] with the given index for tests.
    #[cfg(test)]
    pub(crate) fn new(index: u64) -> Self {
        Self {
            document_id: 0,
            index,
        }
    }

    /// Create a [`NodeId`] scoped to a specific document.
    pub(crate) fn scoped(document_id: u64, index: u64) -> Self {
        Self { document_id, index }
    }
}

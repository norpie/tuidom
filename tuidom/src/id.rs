//! Node handle types.

/// Lightweight, `Copy` integer handle that references a node in the arena.
///
/// NodeIds are cheap to copy and pass around. They remain valid until the
/// referenced node is removed. Using a stale [`NodeId`] (after removal) will
/// return `None` from [`Document`] methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) index: u64,
}

impl NodeId {
    /// Create a new [`NodeId`] with the given index.
    pub(crate) fn new(index: u64) -> Self {
        Self { index }
    }
}

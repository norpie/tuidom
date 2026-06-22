use thiserror::Error;

use crate::NodeId;

/// Result type used by tuidom APIs.
pub type Result<T> = std::result::Result<T, TuidomError>;

/// Errors returned by tuidom APIs.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TuidomError {
    /// A referenced node does not exist in the document arena.
    #[error("node {id:?} does not exist")]
    NodeNotFound {
        /// The missing node id.
        id: NodeId,
    },

    /// A tree mutation would create a parent/child cycle.
    #[error("cannot insert node {child:?} under parent {parent:?}: operation would create a cycle")]
    TreeCycle {
        /// The requested parent node.
        parent: NodeId,
        /// The requested child node.
        child: NodeId,
    },

    /// The permanent document root cannot be reparented.
    #[error("cannot reparent the document root {id:?}")]
    CannotReparentRoot {
        /// The document root node.
        id: NodeId,
    },

    /// The permanent document root cannot be removed.
    #[error("cannot remove the document root {id:?}")]
    CannotRemoveRoot {
        /// The document root node.
        id: NodeId,
    },
}

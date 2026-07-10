use thiserror::Error;

use crate::NodeId;

/// Result type used by tuidom APIs.
pub type Result<T> = std::result::Result<T, TuidomError>;

/// Errors returned by tuidom APIs.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
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

    /// A node exists but has not been marked focusable.
    #[error("node {id:?} is not focusable")]
    NodeNotFocusable {
        /// The node that cannot receive focus.
        id: NodeId,
    },

    /// A node exists but is not an input node.
    #[error("node {id:?} is not an input")]
    NodeNotInput {
        /// The node that is not an input.
        id: NodeId,
    },

    /// A node exists but is not a text node.
    #[error("node {id:?} is not text")]
    NodeNotText {
        /// The node that is not text.
        id: NodeId,
    },

    /// An attribute key is invalid.
    #[error("attribute key cannot be empty")]
    InvalidAttributeKey,

    /// The underlying layout engine reported an error.
    #[error("layout engine error: {0}")]
    Layout(#[from] taffy::tree::TaffyError),

    /// A DOM node was missing its corresponding layout engine node.
    #[error("layout mapping missing for node {id:?}")]
    LayoutMappingMissing {
        /// The DOM node without a layout mapping.
        id: NodeId,
    },
}

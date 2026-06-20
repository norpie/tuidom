//! Node data storage and public view types.

use std::collections::HashMap;

use crate::id::NodeId;
use crate::style::Style;

// ---------------------------------------------------------------------------
// Internal node storage
// ---------------------------------------------------------------------------

/// The kind of a DOM node.
#[derive(Debug, Clone)]
pub(crate) enum NodeKind {
    /// Generic container.
    Box,
    /// Static text content.
    Text { content: String },
    // Future: Input, Frames, Canvas
}

/// Internal representation of a DOM node, stored in the arena.
#[derive(Debug, Clone)]
pub(crate) struct NodeData {
    /// The node kind.
    pub kind: NodeKind,
    /// Parent node, if any.
    pub parent: Option<NodeId>,
    /// Ordered list of child nodes.
    pub children: Vec<NodeId>,
    /// Inline style.
    pub style: Style,
    /// Arbitrary string attributes.
    pub attrs: HashMap<String, String>,
}

impl NodeData {
    /// Create a new box node.
    pub fn box_node() -> Self {
        Self {
            kind: NodeKind::Box,
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            attrs: HashMap::new(),
        }
    }

    /// Create a new text node.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Text { content: content.into() },
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            attrs: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public view — returned by Document::get_node
// ---------------------------------------------------------------------------

/// Read-only snapshot of a node's public state.
#[derive(Debug, Clone)]
pub struct NodeView {
    /// The node's ID.
    pub id: NodeId,
    /// The node kind (public-facing).
    pub kind: NodeKindView,
    /// Parent node, if any.
    pub parent: Option<NodeId>,
    /// Ordered list of child node IDs.
    pub children: Vec<NodeId>,
    /// Arbitrary string attributes.
    pub attrs: HashMap<String, String>,
}

/// Public-facing node kind.
#[derive(Debug, Clone)]
pub enum NodeKindView {
    /// Generic container.
    Box,
    /// Static text content.
    Text {
        /// The text content.
        content: String,
    },
}

impl NodeKind {
    /// Convert to the public-facing view.
    pub fn to_view(&self) -> NodeKindView {
        match self {
            NodeKind::Box => NodeKindView::Box,
            NodeKind::Text { content } => NodeKindView::Text {
                content: content.clone(),
            },
        }
    }
}

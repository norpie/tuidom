//! Node data storage and public view types.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::RwLock;

use crate::animation::TransitionConfig;
use crate::animation::TransitionProperty;
use crate::id::NodeId;
use crate::style::Style;
use crate::style::resolution::ResolvedStyle;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Computed layout for a node — position and size in terminal cells.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutRect {
    /// X position in terminal cells. May be negative when content is offscreen.
    pub x: i32,
    /// Y position in terminal cells. May be negative when content is offscreen.
    pub y: i32,
    /// Width in terminal cells.
    pub width: u16,
    /// Height in terminal cells.
    pub height: u16,
}

// ---------------------------------------------------------------------------
// Internal node storage
// ---------------------------------------------------------------------------

/// Editable text input state.
#[derive(Debug, Clone)]
pub(crate) struct InputState {
    /// Stored input content. Rendering may mask this, but storage remains unmasked.
    pub content: String,
    /// Cursor byte offset into `content`, always normalized to a grapheme boundary.
    pub cursor: usize,
    /// Selected byte range in `content`, normalized to grapheme boundaries.
    pub selection: Option<Range<usize>>,
    /// Whether Enter inserts newlines.
    pub multiline: bool,
    /// Optional display mask for password-like fields.
    pub mask: Option<char>,
}

impl InputState {
    /// Create input state with the cursor at the end of the initial content.
    pub(crate) fn new(content: impl Into<String>) -> Self {
        let content = content.into();
        let cursor = content.len();
        Self {
            content,
            cursor,
            selection: None,
            multiline: false,
            mask: None,
        }
    }
}

/// The kind of a DOM node.
#[derive(Debug, Clone)]
pub(crate) enum NodeKind {
    /// Generic container.
    Box,
    /// Static text content.
    Text { content: String },
    /// Editable text input.
    Input { state: InputState },
    // Future: Frames, Canvas
}

/// Internal representation of a DOM node, stored in the arena.
#[derive(Debug)]
pub(crate) struct NodeData {
    /// The node kind.
    pub kind: NodeKind,
    /// Parent node, if any.
    pub parent: Option<NodeId>,
    /// Ordered list of child nodes.
    pub children: Vec<NodeId>,
    /// Inline style.
    pub style: Style,
    /// Cached resolved style. Set to `None` to mark as dirty.
    pub resolved_style: RwLock<Option<ResolvedStyle>>,
    /// Transition configs for animatable properties.
    pub transition_configs: HashMap<TransitionProperty, TransitionConfig>,
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
            resolved_style: RwLock::new(None),
            transition_configs: HashMap::new(),
            attrs: HashMap::new(),
        }
    }

    /// Create a new text node.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Text {
                content: content.into(),
            },
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            resolved_style: RwLock::new(None),
            transition_configs: HashMap::new(),
            attrs: HashMap::new(),
        }
    }

    /// Create a new input node.
    pub fn input(content: impl Into<String>) -> Self {
        Self {
            kind: NodeKind::Input {
                state: InputState::new(content),
            },
            parent: None,
            children: Vec::new(),
            style: Style::default(),
            resolved_style: RwLock::new(None),
            transition_configs: HashMap::new(),
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
    /// Computed layout, if layout has been run.
    pub layout: Option<LayoutRect>,
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
    /// Editable text input state snapshot.
    Input {
        /// Stored input value.
        value: String,
        /// Cursor byte offset into `value`.
        cursor: usize,
        /// Selected byte range in `value`, if any.
        selection: Option<Range<usize>>,
        /// Whether the input accepts newlines.
        multiline: bool,
        /// Optional display mask for password-like fields.
        mask: Option<char>,
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
            NodeKind::Input { state } => NodeKindView::Input {
                value: state.content.clone(),
                cursor: state.cursor,
                selection: state.selection.clone(),
                multiline: state.multiline,
                mask: state.mask,
            },
        }
    }
}

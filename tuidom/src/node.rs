//! Node data storage and public view types.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

use crate::animation::TransitionConfig;
use crate::animation::TransitionProperty;
use crate::id::NodeId;
use crate::style::resolution::ResolvedStyle;
use crate::style::{EdgeInsets, Style};

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Computed layout for a node — position and size in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

/// A node's entry in the published layout snapshot: its screen rectangle plus how far
/// its content can scroll.
///
/// The maximum scroll is published with the rect because it comes from the same layout
/// pass — taffy measures how far content extends beyond the box — and clamping a scroll
/// offset needs both under one read.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NodeLayout {
    /// The node's screen rectangle.
    pub rect: LayoutRect,
    /// Maximum horizontal scroll offset in terminal cells.
    pub max_scroll_x: u16,
    /// Maximum vertical scroll offset in terminal cells.
    pub max_scroll_y: u16,
}

/// A node's computed layout as seen through [`NodeView`].
///
/// Exposes what downstream scroll and virtualization code needs from one layout pass:
/// the border-box rect, the scrollport it bounds descendants to, and how far its content
/// can actually scroll. Max scroll is zero on any axis whose overflow is not
/// [`Overflow::Scroll`](crate::style::Overflow::Scroll), matching what
/// [`Document::scroll_to`](crate::Document::scroll_to) clamps to.
#[derive(Debug, Clone, Copy)]
pub struct LayoutView {
    /// The node's screen rectangle (border box). May be negative when offscreen.
    pub rect: LayoutRect,
    /// The scrollport: the padding box a scrolling or clipping node bounds its
    /// descendants to. Equal to `rect` deflated by the border on any node.
    pub scrollport: LayoutRect,
    /// Maximum horizontal scroll offset in terminal cells.
    pub max_scroll_x: u16,
    /// Maximum vertical scroll offset in terminal cells.
    pub max_scroll_y: u16,
}

/// A scroll container's current offset in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollOffset {
    /// Horizontal offset: how many cells of content are scrolled off the left edge.
    pub x: u16,
    /// Vertical offset: how many cells of content are scrolled off the top edge.
    pub y: u16,
}

impl LayoutRect {
    /// The rect a node paints its own content into: the layout rect deflated by the border
    /// and then by padding.
    ///
    /// A background still fills the whole layout rect, since both are space *inside* the
    /// node. Only content the node paints itself — Text and Input glyphs, and the input
    /// cursor — is inset, which puts it where taffy already places a container's children.
    pub(crate) fn content_rect(self, resolved: &ResolvedStyle) -> Self {
        self.deflate(resolved.border.insets())
            .deflate(resolved.padding)
    }

    /// The rect a scrolling or clipping node bounds its descendants' painting to: the
    /// layout rect deflated by the border. Padding stays inside the viewport — scrolled
    /// content slides through the padding, but never over the frame.
    pub(crate) fn padding_box(self, resolved: &ResolvedStyle) -> Self {
        self.deflate(resolved.border.insets())
    }

    /// The rect a node's background fills: the layout rect, minus any cell a half-block edge
    /// takes over.
    ///
    /// An edge cell is half fill and half the color behind the node, so the edge composes it
    /// in one write. A plain fill would blend the node's color in a second time and paint over
    /// the very color the outer half is there to show.
    pub(crate) fn without_half_block_edges(self, resolved: &ResolvedStyle) -> Self {
        if !resolved.draws_half_block_edges() {
            return self;
        }
        self.deflate(resolved.half_block_edges.one_cell_insets())
    }

    fn deflate(self, insets: EdgeInsets) -> Self {
        Self {
            x: self.x.saturating_add(i32::from(insets.left)),
            y: self.y.saturating_add(i32::from(insets.top)),
            width: self
                .width
                .saturating_sub(insets.left.saturating_add(insets.right)),
            height: self
                .height
                .saturating_sub(insets.top.saturating_add(insets.bottom)),
        }
    }
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
    /// Horizontal scroll offset in terminal cells.
    pub scroll_x: u16,
    /// Vertical scroll offset in terminal rows.
    pub scroll_y: u16,
    /// Display column a run of vertical motion is trying to hold, in terminal cells.
    ///
    /// Set by the first vertical motion and cleared by anything else. Without it, moving
    /// up through a short line would clamp the column and moving back down could not
    /// recover it, so the cursor would walk leftward down the content.
    pub goal_column: Option<u16>,
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
            scroll_x: 0,
            scroll_y: 0,
            goal_column: None,
        }
    }

    /// Return the content that should be measured and painted.
    pub(crate) fn display_content(&self) -> String {
        input_display_content(&self.content, self.multiline, self.mask)
    }
}

/// Build the text displayed for an input value.
pub(crate) fn input_display_content(value: &str, multiline: bool, mask: Option<char>) -> String {
    let value = if multiline {
        value.to_owned()
    } else {
        value.replace('\n', " ")
    };

    let Some(mask) = mask else {
        return value;
    };

    value
        .graphemes(true)
        .map(|grapheme| {
            if grapheme == "\n" {
                "\n".to_owned()
            } else {
                mask.to_string()
            }
        })
        .collect()
}

/// Apply input scroll offsets to display content.
pub(crate) fn input_scrolled_display_content(
    content: &str,
    scroll_x: u16,
    scroll_y: u16,
) -> String {
    content
        .lines()
        .skip(scroll_y as usize)
        .map(|line| scroll_line(line, scroll_x))
        .collect::<Vec<_>>()
        .join("\n")
}

fn scroll_line(line: &str, scroll_x: u16) -> String {
    if scroll_x == 0 {
        return line.to_owned();
    }

    let mut cells = 0_u16;
    let mut start = line.len();
    for (index, grapheme) in line.grapheme_indices(true) {
        let width = unicode_width::UnicodeWidthStr::width(grapheme).min(2) as u16;
        if width == 0 {
            continue;
        }
        let next = cells.saturating_add(width);
        if next > scroll_x {
            start = if cells < scroll_x {
                index + grapheme.len()
            } else {
                index
            };
            break;
        }
        cells = next;
    }

    line.get(start..).unwrap_or("").to_owned()
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
    /// Text content cycling on a timer.
    Frames {
        frames: Vec<String>,
        interval: Duration,
        /// When the cycle started. The current frame is computed from elapsed
        /// time, so cycling needs no per-flip mutation.
        started: Instant,
    },
    // Future: Canvas
}

/// The frame index a frames node shows at `now`.
///
/// Computed from elapsed time rather than stored, so a flip is nothing but the
/// clock passing a boundary. An empty frame list, or a zero interval (which
/// would flip infinitely fast), pins the index at zero.
pub(crate) fn frames_index(
    count: usize,
    interval: Duration,
    started: Instant,
    now: Instant,
) -> usize {
    if count == 0 || interval.is_zero() {
        return 0;
    }
    let elapsed = now.saturating_duration_since(started).as_nanos();
    (elapsed / interval.as_nanos()) as usize % count
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
    ///
    /// Behind an `Arc` because the cache is read far more than it is filled: a
    /// frame resolves every node several times over, and `ResolvedStyle` is 408
    /// bytes, so handing out copies made the hit path a memcpy over payloads
    /// too large to stay in L1. A hit is now a pointer copy and one atomic, and
    /// never touches the payload at all.
    pub resolved_style: RwLock<Option<Arc<ResolvedStyle>>>,
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

    /// Create a new frames node starting its cycle at `started`.
    pub fn frames(frames: Vec<String>, interval: Duration, started: Instant) -> Self {
        Self {
            kind: NodeKind::Frames {
                frames,
                interval,
                started,
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
    pub layout: Option<LayoutView>,
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
        /// Horizontal scroll offset in terminal cells.
        scroll_x: u16,
        /// Vertical scroll offset in terminal rows.
        scroll_y: u16,
    },
    /// Timed frame-cycling content snapshot.
    Frames {
        /// The frame contents, cycled in order.
        frames: Vec<String>,
        /// Time each frame is shown.
        interval: Duration,
        /// Index of the frame showing at the time of this snapshot.
        current: usize,
    },
}

impl NodeKind {
    /// Convert to the public-facing view, as of `now`.
    ///
    /// The instant matters only to frames nodes, whose current frame is a
    /// function of elapsed time.
    pub fn to_view(&self, now: Instant) -> NodeKindView {
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
                scroll_x: state.scroll_x,
                scroll_y: state.scroll_y,
            },
            NodeKind::Frames {
                frames,
                interval,
                started,
            } => NodeKindView::Frames {
                frames: frames.clone(),
                interval: *interval,
                current: frames_index(frames.len(), *interval, *started, now),
            },
        }
    }
}

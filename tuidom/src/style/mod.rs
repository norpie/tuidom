/// Border charsets, sides, and presets.
pub mod border;
/// Color types and OKLCH → RGB conversion.
pub mod color;
/// Resolved style computation and caching.
pub(crate) mod resolution;
/// Scrollbar visibility and drawing characters.
pub mod scrollbar;

use std::collections::HashMap;
use std::time::Duration;

pub use border::{Border, BorderCharset, Sides};
pub use color::{Color, ColorOp, ResolvedColor};
pub use scrollbar::{ScrollbarCharset, ScrollbarShow};

// ---------------------------------------------------------------------------
// StyleValue — explicit inheritance control
// ---------------------------------------------------------------------------

/// Wraps a style property value: unset/default, explicit inheritance, or explicitly set.
///
/// By default, properties are [`Unset`](Self::Unset), which means they use the document
/// default style. Use [`Inherit`](Self::Inherit) to opt into inheriting from the parent.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum StyleValue<T> {
    /// Use the document/default style value.
    #[default]
    Unset,
    /// Inherit the resolved value from the parent node.
    Inherit,
    /// An explicitly set value.
    Set(T),
}

// ---------------------------------------------------------------------------
// Length — sizing units
// ---------------------------------------------------------------------------

/// Sizing for width and height properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Fixed size in terminal cells.
    Pixels(u16),
    /// Percentage of parent's content area.
    Percent(f64),
    /// Size to content (for flex containers: size to children; for text: size to content).
    Auto,
}

/// Edge spacing in terminal cells.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInsets {
    /// Top edge size.
    pub top: u16,
    /// Right edge size.
    pub right: u16,
    /// Bottom edge size.
    pub bottom: u16,
    /// Left edge size.
    pub left: u16,
}

impl EdgeInsets {
    /// No edge spacing.
    pub const ZERO: Self = Self::all(0);

    /// Create edge spacing with the same value on every side.
    pub const fn all(value: u16) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Create edge spacing with horizontal and vertical values.
    pub const fn symmetric(horizontal: u16, vertical: u16) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Create edge spacing with explicit top, right, bottom, and left values.
    pub const fn new(top: u16, right: u16, bottom: u16, left: u16) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// How a node participates in layout.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Display {
    /// Flex container — children laid out using flexbox.
    #[default]
    Flex,
    /// Node is not rendered and does not participate in layout.
    None,
}

// ---------------------------------------------------------------------------
// Overflow
// ---------------------------------------------------------------------------

/// How content that exceeds a node's box is treated, per axis.
///
/// `Scroll` and `Clip` also change sizing: a container that clips its content is
/// allowed to be smaller than that content, where a `Visible` container is kept
/// at least large enough to contain it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// Content spills out of the box and stays visible.
    #[default]
    Visible,
    /// Content is clipped to the box and the node is scrollable on this axis.
    Scroll,
    /// Content is clipped to the box, with no scrolling and no scrollbar.
    Clip,
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// Shape requested for an input cursor.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// Filled cell cursor.
    #[default]
    Block,
    /// Underline cursor.
    Underline,
    /// Vertical bar cursor.
    Bar,
}

// ---------------------------------------------------------------------------
// Flex layout
// ---------------------------------------------------------------------------

/// Main-axis direction for flex containers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    /// Lay children out horizontally from left to right.
    #[default]
    Row,
    /// Lay children out vertically from top to bottom.
    Column,
    /// Lay children out horizontally from right to left.
    RowReverse,
    /// Lay children out vertically from bottom to top.
    ColumnReverse,
}

/// Whether flex children remain on one line or wrap onto multiple lines.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    /// Keep flex children on a single line.
    #[default]
    NoWrap,
    /// Allow flex children to wrap onto additional lines.
    Wrap,
    /// Allow flex children to wrap, stacking lines in reverse cross-axis order.
    WrapReverse,
}

/// Spacing between flex children and flex lines in terminal cells.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FlexGap {
    /// Vertical spacing between rows or wrapped flex lines.
    pub row: u16,
    /// Horizontal spacing between columns or row-direction flex items.
    pub column: u16,
}

impl FlexGap {
    /// No flex gap.
    pub const ZERO: Self = Self::all(0);

    /// Create equal row and column gaps.
    pub const fn all(value: u16) -> Self {
        Self {
            row: value,
            column: value,
        }
    }

    /// Create explicit row and column gaps.
    pub const fn new(row: u16, column: u16) -> Self {
        Self { row, column }
    }
}

// ---------------------------------------------------------------------------
// Flex alignment
// ---------------------------------------------------------------------------

/// Cross-axis alignment for flex containers.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum AlignItems {
    /// Align items at the start of the cross axis.
    FlexStart,
    /// Align items at the end of the cross axis.
    FlexEnd,
    /// Align items at the center of the cross axis.
    Center,
    /// Stretch items to fill the cross axis.
    #[default]
    Stretch,
}

/// Cross-axis alignment override for an individual flex item.
pub type AlignSelf = AlignItems;

/// Cross-axis alignment for wrapped flex lines.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum AlignContent {
    /// Pack lines at the start of the cross axis.
    FlexStart,
    /// Pack lines at the end of the cross axis.
    FlexEnd,
    /// Pack lines at the center of the cross axis.
    Center,
    /// Stretch lines to fill the cross axis.
    #[default]
    Stretch,
    /// Distribute lines evenly with space between them.
    SpaceBetween,
    /// Distribute lines evenly with space around them.
    SpaceAround,
}

/// Main-axis alignment for flex containers.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum JustifyContent {
    /// Pack items at the start of the main axis.
    #[default]
    FlexStart,
    /// Pack items at the end of the main axis.
    FlexEnd,
    /// Pack items at the center of the main axis.
    Center,
    /// Distribute items evenly with space between them.
    SpaceBetween,
    /// Distribute items evenly with space around them.
    SpaceAround,
}

// ---------------------------------------------------------------------------
// Positioning
// ---------------------------------------------------------------------------

/// Positioning mode for a node.
///
/// Absolute offsets are parent-relative rather than screen-relative: place a node
/// at screen coordinates by making it a child of the root, whose origin is `(0, 0)`.
/// This keeps anchoring (badges, tooltips, dropdowns) working across reflow without
/// downstream recomputation, and matches the paint model, where a descendant's
/// `z_index` cannot escape its parent subtree.
///
/// Published layout rectangles are screen-absolute regardless of positioning mode.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Position {
    /// Participate in normal flex layout.
    #[default]
    Flow,
    /// Remove the node from flow and offset it from its parent's box origin.
    ///
    /// Offsets are signed terminal cells; negative values place the node above or
    /// left of its parent, and the node may overflow its parent's bounds.
    Absolute {
        /// Horizontal offset from the parent's box origin.
        x: i32,
        /// Vertical offset from the parent's box origin.
        y: i32,
    },
}

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// Inline style for a node.
///
/// Known properties are typed; custom properties are stored as raw inline metadata.
/// Use the builder methods for construction:
///
/// ```ignore
/// let style = Style::new()
///     .width(Length::Percent(100.0))
///     .color(Color::white());
/// ```
///
/// For partial updates via `Document::update_style`:
///
/// ```ignore
/// doc.update_style(id, |s| {
///     s.opacity(0.5);
/// })?;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Width of the node.
    pub(crate) width: StyleValue<Length>,
    /// Height of the node.
    pub(crate) height: StyleValue<Length>,
    /// Inner spacing in terminal cells.
    pub(crate) padding: StyleValue<EdgeInsets>,
    /// Outer spacing in terminal cells.
    pub(crate) margin: StyleValue<EdgeInsets>,
    /// Border charset and drawn sides.
    pub(crate) border: StyleValue<Border>,
    /// Border color. Unset follows the node's foreground color.
    pub(crate) border_color: StyleValue<Color>,
    /// Sides whose outermost cells are drawn as a half-block edge.
    pub(crate) half_block_edges: StyleValue<Sides>,
    /// Color of a half-block edge's inner half. Unset follows the node's background color.
    pub(crate) half_block_inner_color: StyleValue<Color>,
    /// Color of a half-block edge's outer half. Unset keeps what is already painted there.
    pub(crate) half_block_outer_color: StyleValue<Color>,
    /// Display mode.
    pub(crate) display: StyleValue<Display>,
    /// Horizontal overflow behavior.
    pub(crate) overflow_x: StyleValue<Overflow>,
    /// Vertical overflow behavior.
    pub(crate) overflow_y: StyleValue<Overflow>,
    /// When this scroll container draws its scrollbars.
    pub(crate) scrollbar_show: StyleValue<ScrollbarShow>,
    /// How long a [`ScrollbarShow::WhenScrolling`] bar stays fully visible after activity.
    pub(crate) scrollbar_hide_delay: StyleValue<Duration>,
    /// How long a [`ScrollbarShow::WhenScrolling`] bar takes to fade out after its delay.
    pub(crate) scrollbar_fade_duration: StyleValue<Duration>,
    /// The characters this scroll container's bars are drawn with.
    pub(crate) scrollbar_charset: StyleValue<ScrollbarCharset>,
    /// Scrollbar track color. Unset follows the node's foreground color.
    pub(crate) scrollbar_track_color: StyleValue<Color>,
    /// Scrollbar thumb color. Unset follows the node's foreground color.
    pub(crate) scrollbar_thumb_color: StyleValue<Color>,
    /// Opacity (0–1).
    pub(crate) opacity: StyleValue<f64>,
    /// Foreground text color.
    pub(crate) color: StyleValue<Color>,
    /// Background color.
    pub(crate) background: StyleValue<Color>,
    /// Main-axis direction for flex containers.
    pub(crate) flex_direction: StyleValue<FlexDirection>,
    /// Initial main-axis size for flex items.
    pub(crate) flex_basis: StyleValue<Length>,
    /// Relative grow factor for flex items.
    pub(crate) flex_grow: StyleValue<f32>,
    /// Relative shrink factor for flex items.
    pub(crate) flex_shrink: StyleValue<f32>,
    /// Whether flex children remain on one line or wrap onto multiple lines.
    pub(crate) flex_wrap: StyleValue<FlexWrap>,
    /// Spacing between flex children and flex lines.
    pub(crate) gap: StyleValue<FlexGap>,
    /// Cross-axis alignment override for this flex item.
    pub(crate) align_self: StyleValue<AlignSelf>,
    /// Cross-axis alignment (flex container).
    pub(crate) align_items: StyleValue<AlignItems>,
    /// Cross-axis alignment for wrapped flex lines.
    pub(crate) align_content: StyleValue<AlignContent>,
    /// Main-axis alignment (flex container).
    pub(crate) justify_content: StyleValue<JustifyContent>,
    /// Positioning mode.
    pub(crate) position: StyleValue<Position>,
    /// Paint order within the current stacking context.
    pub(crate) z_index: StyleValue<i32>,
    /// Whether this node creates an isolated stacking context for descendants.
    pub(crate) stacking_context: StyleValue<bool>,
    /// Whether mouse drag selection is confined to this node's subtree.
    pub(crate) selection_boundary: StyleValue<bool>,
    /// Background for selected text. Unset swaps the glyph's colors (reverse video).
    pub(crate) selection_bg: StyleValue<Color>,
    /// Foreground for selected text. Unset swaps the glyph's colors (reverse video).
    pub(crate) selection_fg: StyleValue<Color>,
    /// Bold text.
    pub(crate) bold: StyleValue<bool>,
    /// Italic text.
    pub(crate) italic: StyleValue<bool>,
    /// Underlined text.
    pub(crate) underline: StyleValue<bool>,
    /// Input cursor shape.
    pub(crate) cursor_shape: StyleValue<CursorShape>,
    /// Raw custom style properties.
    pub(crate) custom: HashMap<String, String>,
    /// Color variables declared on this node, in scope for it and its descendants.
    pub(crate) color_vars: HashMap<String, Color>,
}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

impl Style {
    /// Create a new [`Style`] with all properties unset.
    ///
    /// Unset properties use the document/default style instead of inheriting from the parent.
    pub fn new() -> Self {
        Self {
            width: StyleValue::Unset,
            height: StyleValue::Unset,
            padding: StyleValue::Unset,
            margin: StyleValue::Unset,
            border: StyleValue::Unset,
            border_color: StyleValue::Unset,
            half_block_edges: StyleValue::Unset,
            half_block_inner_color: StyleValue::Unset,
            half_block_outer_color: StyleValue::Unset,
            display: StyleValue::Unset,
            overflow_x: StyleValue::Unset,
            overflow_y: StyleValue::Unset,
            scrollbar_show: StyleValue::Unset,
            scrollbar_hide_delay: StyleValue::Unset,
            scrollbar_fade_duration: StyleValue::Unset,
            scrollbar_charset: StyleValue::Unset,
            scrollbar_track_color: StyleValue::Unset,
            scrollbar_thumb_color: StyleValue::Unset,
            opacity: StyleValue::Unset,
            color: StyleValue::Unset,
            background: StyleValue::Unset,
            flex_direction: StyleValue::Unset,
            flex_basis: StyleValue::Unset,
            flex_grow: StyleValue::Unset,
            flex_shrink: StyleValue::Unset,
            flex_wrap: StyleValue::Unset,
            gap: StyleValue::Unset,
            align_self: StyleValue::Unset,
            align_items: StyleValue::Unset,
            align_content: StyleValue::Unset,
            justify_content: StyleValue::Unset,
            position: StyleValue::Unset,
            z_index: StyleValue::Unset,
            stacking_context: StyleValue::Unset,
            selection_boundary: StyleValue::Unset,
            selection_bg: StyleValue::Unset,
            selection_fg: StyleValue::Unset,
            bold: StyleValue::Unset,
            italic: StyleValue::Unset,
            underline: StyleValue::Unset,
            cursor_shape: StyleValue::Unset,
            custom: HashMap::new(),
            color_vars: HashMap::new(),
        }
    }

    // -- Width ----------------------------------------------------------

    /// Set the width.
    pub fn width(&mut self, value: Length) {
        self.width = StyleValue::Set(value);
    }

    /// Explicitly inherit width from the parent node.
    pub fn inherit_width(&mut self) {
        self.width = StyleValue::Inherit;
    }

    /// Reset width to the document/default style.
    pub fn unset_width(&mut self) {
        self.width = StyleValue::Unset;
    }

    // -- Height ---------------------------------------------------------

    /// Set the height.
    pub fn height(&mut self, value: Length) {
        self.height = StyleValue::Set(value);
    }

    /// Explicitly inherit height from the parent node.
    pub fn inherit_height(&mut self) {
        self.height = StyleValue::Inherit;
    }

    /// Reset height to the document/default style.
    pub fn unset_height(&mut self) {
        self.height = StyleValue::Unset;
    }

    // -- Padding --------------------------------------------------------

    /// Set the inner spacing.
    pub fn padding(&mut self, value: EdgeInsets) {
        self.padding = StyleValue::Set(value);
    }

    /// Explicitly inherit inner spacing from the parent node.
    pub fn inherit_padding(&mut self) {
        self.padding = StyleValue::Inherit;
    }

    /// Reset inner spacing to the document/default style.
    pub fn unset_padding(&mut self) {
        self.padding = StyleValue::Unset;
    }

    // -- Margin ---------------------------------------------------------

    /// Set the outer spacing.
    pub fn margin(&mut self, value: EdgeInsets) {
        self.margin = StyleValue::Set(value);
    }

    /// Explicitly inherit outer spacing from the parent node.
    pub fn inherit_margin(&mut self) {
        self.margin = StyleValue::Inherit;
    }

    /// Reset outer spacing to the document/default style.
    pub fn unset_margin(&mut self) {
        self.margin = StyleValue::Unset;
    }

    // -- Border ---------------------------------------------------------

    /// Set the border charset and drawn sides.
    ///
    /// A border occupies one cell per drawn side, taken from the node's own box, so a
    /// bordered node's content and children are inset by it. Use [`Border::none`] to remove
    /// a border a base style set — leaving this unset overrides nothing.
    pub fn border(&mut self, value: Border) {
        self.border = StyleValue::Set(value);
    }

    /// Explicitly inherit the border from the parent node.
    pub fn inherit_border(&mut self) {
        self.border = StyleValue::Inherit;
    }

    /// Reset the border to the document/default style.
    pub fn unset_border(&mut self) {
        self.border = StyleValue::Unset;
    }

    // -- Border color ---------------------------------------------------

    /// Set the border color.
    ///
    /// When unset, the border is drawn in the node's resolved foreground color, so a focus
    /// style that changes `color` moves the border with it.
    pub fn border_color(&mut self, value: Color) {
        self.border_color = StyleValue::Set(value);
    }

    /// Explicitly inherit the border color from the parent node.
    pub fn inherit_border_color(&mut self) {
        self.border_color = StyleValue::Inherit;
    }

    /// Reset the border color to the document/default style.
    pub fn unset_border_color(&mut self) {
        self.border_color = StyleValue::Unset;
    }

    // -- Half-block edges -----------------------------------------------

    /// Draw the node's outermost cells on these sides as a half-block edge.
    ///
    /// The node's fill ends halfway into those cells instead of at the cell boundary, which is
    /// how a colored area gets vertical padding that reads as balanced against its horizontal
    /// padding — a terminal cell is about twice as tall as it is wide. Unlike a border, this
    /// takes no space: it repaints cells the node already owns.
    pub fn half_block_edges(&mut self, value: Sides) {
        self.half_block_edges = StyleValue::Set(value);
    }

    /// Explicitly inherit the half-block edge sides from the parent node.
    pub fn inherit_half_block_edges(&mut self) {
        self.half_block_edges = StyleValue::Inherit;
    }

    /// Reset the half-block edge sides to the document/default style.
    pub fn unset_half_block_edges(&mut self) {
        self.half_block_edges = StyleValue::Unset;
    }

    // -- Half-block inner color -----------------------------------------

    /// Set the color of a half-block edge's inner half — the node's own side.
    ///
    /// When unset, it follows the node's resolved background color, which is what makes the
    /// edge read as the node's fill ending half a cell early. A node with neither draws no
    /// edge: there is no fill to take a half of.
    pub fn half_block_inner_color(&mut self, value: Color) {
        self.half_block_inner_color = StyleValue::Set(value);
    }

    /// Explicitly inherit the half-block inner color from the parent node.
    pub fn inherit_half_block_inner_color(&mut self) {
        self.half_block_inner_color = StyleValue::Inherit;
    }

    /// Reset the half-block inner color to the document/default style.
    pub fn unset_half_block_inner_color(&mut self) {
        self.half_block_inner_color = StyleValue::Unset;
    }

    // -- Half-block outer color -----------------------------------------

    /// Set the color of a half-block edge's outer half — the side away from the node.
    ///
    /// When unset, the outer half keeps whatever is already painted underneath, so the edge
    /// fades into the color it sits on without being told what that is.
    pub fn half_block_outer_color(&mut self, value: Color) {
        self.half_block_outer_color = StyleValue::Set(value);
    }

    /// Explicitly inherit the half-block outer color from the parent node.
    pub fn inherit_half_block_outer_color(&mut self) {
        self.half_block_outer_color = StyleValue::Inherit;
    }

    /// Reset the half-block outer color to the document/default style.
    pub fn unset_half_block_outer_color(&mut self) {
        self.half_block_outer_color = StyleValue::Unset;
    }

    // -- Text attributes ------------------------------------------------

    /// Draw this node's text in bold.
    pub fn bold(&mut self, value: bool) {
        self.bold = StyleValue::Set(value);
    }

    /// Explicitly inherit boldness from the parent node.
    pub fn inherit_bold(&mut self) {
        self.bold = StyleValue::Inherit;
    }

    /// Reset boldness to the document/default style.
    pub fn unset_bold(&mut self) {
        self.bold = StyleValue::Unset;
    }

    /// Draw this node's text in italic.
    pub fn italic(&mut self, value: bool) {
        self.italic = StyleValue::Set(value);
    }

    /// Explicitly inherit italics from the parent node.
    pub fn inherit_italic(&mut self) {
        self.italic = StyleValue::Inherit;
    }

    /// Reset italics to the document/default style.
    pub fn unset_italic(&mut self) {
        self.italic = StyleValue::Unset;
    }

    /// Underline this node's text.
    pub fn underline(&mut self, value: bool) {
        self.underline = StyleValue::Set(value);
    }

    /// Explicitly inherit underlining from the parent node.
    pub fn inherit_underline(&mut self) {
        self.underline = StyleValue::Inherit;
    }

    /// Reset underlining to the document/default style.
    pub fn unset_underline(&mut self) {
        self.underline = StyleValue::Unset;
    }

    // -- Display --------------------------------------------------------

    /// Set the display mode.
    pub fn display(&mut self, value: Display) {
        self.display = StyleValue::Set(value);
    }

    /// Explicitly inherit display mode from the parent node.
    pub fn inherit_display(&mut self) {
        self.display = StyleValue::Inherit;
    }

    /// Reset display mode to the document/default style.
    pub fn unset_display(&mut self) {
        self.display = StyleValue::Unset;
    }

    // -- Overflow -------------------------------------------------------

    /// Set the horizontal overflow behavior.
    pub fn overflow_x(&mut self, value: Overflow) {
        self.overflow_x = StyleValue::Set(value);
    }

    /// Explicitly inherit horizontal overflow behavior from the parent node.
    pub fn inherit_overflow_x(&mut self) {
        self.overflow_x = StyleValue::Inherit;
    }

    /// Reset horizontal overflow behavior to the document/default style.
    pub fn unset_overflow_x(&mut self) {
        self.overflow_x = StyleValue::Unset;
    }

    /// Set the vertical overflow behavior.
    pub fn overflow_y(&mut self, value: Overflow) {
        self.overflow_y = StyleValue::Set(value);
    }

    /// Explicitly inherit vertical overflow behavior from the parent node.
    pub fn inherit_overflow_y(&mut self) {
        self.overflow_y = StyleValue::Inherit;
    }

    /// Reset vertical overflow behavior to the document/default style.
    pub fn unset_overflow_y(&mut self) {
        self.overflow_y = StyleValue::Unset;
    }

    /// Set both overflow axes at once.
    pub fn overflow(&mut self, value: Overflow) {
        self.overflow_x = StyleValue::Set(value);
        self.overflow_y = StyleValue::Set(value);
    }

    // -- Scrollbars -----------------------------------------------------

    /// Set when this scroll container draws its scrollbars.
    pub fn scrollbar_show(&mut self, value: ScrollbarShow) {
        self.scrollbar_show = StyleValue::Set(value);
    }

    /// Explicitly inherit scrollbar visibility from the parent node.
    pub fn inherit_scrollbar_show(&mut self) {
        self.scrollbar_show = StyleValue::Inherit;
    }

    /// Reset scrollbar visibility to the document/default style.
    pub fn unset_scrollbar_show(&mut self) {
        self.scrollbar_show = StyleValue::Unset;
    }

    /// Set how long a [`ScrollbarShow::WhenScrolling`] bar stays fully visible after
    /// scroll activity before it starts fading.
    pub fn scrollbar_hide_delay(&mut self, value: Duration) {
        self.scrollbar_hide_delay = StyleValue::Set(value);
    }

    /// Explicitly inherit the scrollbar hide delay from the parent node.
    pub fn inherit_scrollbar_hide_delay(&mut self) {
        self.scrollbar_hide_delay = StyleValue::Inherit;
    }

    /// Reset the scrollbar hide delay to the document/default style.
    pub fn unset_scrollbar_hide_delay(&mut self) {
        self.scrollbar_hide_delay = StyleValue::Unset;
    }

    /// Set how long a [`ScrollbarShow::WhenScrolling`] bar takes to fade out once its
    /// hide delay has passed.
    pub fn scrollbar_fade_duration(&mut self, value: Duration) {
        self.scrollbar_fade_duration = StyleValue::Set(value);
    }

    /// Explicitly inherit the scrollbar fade duration from the parent node.
    pub fn inherit_scrollbar_fade_duration(&mut self) {
        self.scrollbar_fade_duration = StyleValue::Inherit;
    }

    /// Reset the scrollbar fade duration to the document/default style.
    pub fn unset_scrollbar_fade_duration(&mut self) {
        self.scrollbar_fade_duration = StyleValue::Unset;
    }

    /// Set the characters this container's scrollbars are drawn with.
    pub fn scrollbar_charset(&mut self, value: ScrollbarCharset) {
        self.scrollbar_charset = StyleValue::Set(value);
    }

    /// Explicitly inherit the scrollbar charset from the parent node.
    pub fn inherit_scrollbar_charset(&mut self) {
        self.scrollbar_charset = StyleValue::Inherit;
    }

    /// Reset the scrollbar charset to the document/default style.
    pub fn unset_scrollbar_charset(&mut self) {
        self.scrollbar_charset = StyleValue::Unset;
    }

    /// Set the scrollbar track color.
    pub fn scrollbar_track_color(&mut self, value: Color) {
        self.scrollbar_track_color = StyleValue::Set(value);
    }

    /// Explicitly inherit the scrollbar track color from the parent node.
    pub fn inherit_scrollbar_track_color(&mut self) {
        self.scrollbar_track_color = StyleValue::Inherit;
    }

    /// Reset the scrollbar track color to the document/default style.
    pub fn unset_scrollbar_track_color(&mut self) {
        self.scrollbar_track_color = StyleValue::Unset;
    }

    /// Set the scrollbar thumb color.
    pub fn scrollbar_thumb_color(&mut self, value: Color) {
        self.scrollbar_thumb_color = StyleValue::Set(value);
    }

    /// Explicitly inherit the scrollbar thumb color from the parent node.
    pub fn inherit_scrollbar_thumb_color(&mut self) {
        self.scrollbar_thumb_color = StyleValue::Inherit;
    }

    /// Reset the scrollbar thumb color to the document/default style.
    pub fn unset_scrollbar_thumb_color(&mut self) {
        self.scrollbar_thumb_color = StyleValue::Unset;
    }

    // -- Opacity --------------------------------------------------------

    /// Set the opacity (0–1).
    pub fn opacity(&mut self, value: f64) {
        self.opacity = StyleValue::Set(value);
    }

    /// Explicitly inherit opacity from the parent node.
    pub fn inherit_opacity(&mut self) {
        self.opacity = StyleValue::Inherit;
    }

    /// Reset opacity to the document/default style.
    pub fn unset_opacity(&mut self) {
        self.opacity = StyleValue::Unset;
    }

    // -- Color ----------------------------------------------------------

    /// Set the foreground text color.
    pub fn color(&mut self, value: Color) {
        self.color = StyleValue::Set(value);
    }

    /// Explicitly inherit foreground text color from the parent node.
    pub fn inherit_color(&mut self) {
        self.color = StyleValue::Inherit;
    }

    /// Reset foreground text color to the document/default style.
    pub fn unset_color(&mut self) {
        self.color = StyleValue::Unset;
    }

    // -- Background -----------------------------------------------------

    /// Set the background color.
    pub fn background(&mut self, value: Color) {
        self.background = StyleValue::Set(value);
    }

    /// Explicitly inherit background color from the parent node.
    pub fn inherit_background(&mut self) {
        self.background = StyleValue::Inherit;
    }

    /// Reset background color to the document/default style.
    pub fn unset_background(&mut self) {
        self.background = StyleValue::Unset;
    }

    // -- Flex Direction -------------------------------------------------

    /// Set the main-axis direction for flex containers.
    pub fn flex_direction(&mut self, value: FlexDirection) {
        self.flex_direction = StyleValue::Set(value);
    }

    /// Explicitly inherit flex direction from the parent node.
    pub fn inherit_flex_direction(&mut self) {
        self.flex_direction = StyleValue::Inherit;
    }

    /// Reset flex direction to the document/default style.
    pub fn unset_flex_direction(&mut self) {
        self.flex_direction = StyleValue::Unset;
    }

    // -- Flex Basis -----------------------------------------------------

    /// Set the initial main-axis size for this flex item.
    pub fn flex_basis(&mut self, value: Length) {
        self.flex_basis = StyleValue::Set(value);
    }

    /// Explicitly inherit flex basis from the parent node.
    pub fn inherit_flex_basis(&mut self) {
        self.flex_basis = StyleValue::Inherit;
    }

    /// Reset flex basis to the document/default style.
    pub fn unset_flex_basis(&mut self) {
        self.flex_basis = StyleValue::Unset;
    }

    // -- Flex Grow ------------------------------------------------------

    /// Set the relative grow factor for this flex item.
    pub fn flex_grow(&mut self, value: f32) {
        self.flex_grow = StyleValue::Set(value);
    }

    /// Explicitly inherit flex grow from the parent node.
    pub fn inherit_flex_grow(&mut self) {
        self.flex_grow = StyleValue::Inherit;
    }

    /// Reset flex grow to the document/default style.
    pub fn unset_flex_grow(&mut self) {
        self.flex_grow = StyleValue::Unset;
    }

    // -- Flex Shrink ----------------------------------------------------

    /// Set the relative shrink factor for this flex item.
    pub fn flex_shrink(&mut self, value: f32) {
        self.flex_shrink = StyleValue::Set(value);
    }

    /// Explicitly inherit flex shrink from the parent node.
    pub fn inherit_flex_shrink(&mut self) {
        self.flex_shrink = StyleValue::Inherit;
    }

    /// Reset flex shrink to the document/default style.
    pub fn unset_flex_shrink(&mut self) {
        self.flex_shrink = StyleValue::Unset;
    }

    // -- Flex Wrap ------------------------------------------------------

    /// Set whether flex children remain on one line or wrap onto multiple lines.
    pub fn flex_wrap(&mut self, value: FlexWrap) {
        self.flex_wrap = StyleValue::Set(value);
    }

    /// Explicitly inherit flex wrap from the parent node.
    pub fn inherit_flex_wrap(&mut self) {
        self.flex_wrap = StyleValue::Inherit;
    }

    /// Reset flex wrap to the document/default style.
    pub fn unset_flex_wrap(&mut self) {
        self.flex_wrap = StyleValue::Unset;
    }

    // -- Gap ------------------------------------------------------------

    /// Set spacing between flex children and flex lines.
    pub fn gap(&mut self, value: FlexGap) {
        self.gap = StyleValue::Set(value);
    }

    /// Explicitly inherit gap from the parent node.
    pub fn inherit_gap(&mut self) {
        self.gap = StyleValue::Inherit;
    }

    /// Reset gap to the document/default style.
    pub fn unset_gap(&mut self) {
        self.gap = StyleValue::Unset;
    }

    // -- Align Self -----------------------------------------------------

    /// Set the cross-axis alignment override for this flex item.
    pub fn align_self(&mut self, value: AlignSelf) {
        self.align_self = StyleValue::Set(value);
    }

    /// Explicitly inherit align-self from the parent node.
    pub fn inherit_align_self(&mut self) {
        self.align_self = StyleValue::Inherit;
    }

    /// Reset align-self to the document/default style.
    pub fn unset_align_self(&mut self) {
        self.align_self = StyleValue::Unset;
    }

    // -- Align Items ----------------------------------------------------

    /// Set the cross-axis alignment.
    pub fn align_items(&mut self, value: AlignItems) {
        self.align_items = StyleValue::Set(value);
    }

    /// Explicitly inherit cross-axis alignment from the parent node.
    pub fn inherit_align_items(&mut self) {
        self.align_items = StyleValue::Inherit;
    }

    /// Reset cross-axis alignment to the document/default style.
    pub fn unset_align_items(&mut self) {
        self.align_items = StyleValue::Unset;
    }

    // -- Align Content --------------------------------------------------

    /// Set the cross-axis alignment for wrapped flex lines.
    pub fn align_content(&mut self, value: AlignContent) {
        self.align_content = StyleValue::Set(value);
    }

    /// Explicitly inherit align-content from the parent node.
    pub fn inherit_align_content(&mut self) {
        self.align_content = StyleValue::Inherit;
    }

    /// Reset align-content to the document/default style.
    pub fn unset_align_content(&mut self) {
        self.align_content = StyleValue::Unset;
    }

    // -- Justify Content -------------------------------------------------

    /// Set the main-axis alignment.
    pub fn justify_content(&mut self, value: JustifyContent) {
        self.justify_content = StyleValue::Set(value);
    }

    /// Explicitly inherit main-axis alignment from the parent node.
    pub fn inherit_justify_content(&mut self) {
        self.justify_content = StyleValue::Inherit;
    }

    /// Reset main-axis alignment to the document/default style.
    pub fn unset_justify_content(&mut self) {
        self.justify_content = StyleValue::Unset;
    }

    // -- Position -------------------------------------------------------

    /// Set the positioning mode.
    pub fn position(&mut self, value: Position) {
        self.position = StyleValue::Set(value);
    }

    /// Explicitly inherit the positioning mode from the parent node.
    pub fn inherit_position(&mut self) {
        self.position = StyleValue::Inherit;
    }

    /// Reset the positioning mode to the document/default style.
    pub fn unset_position(&mut self) {
        self.position = StyleValue::Unset;
    }

    // -- Z Index --------------------------------------------------------

    /// Set the paint order within the current stacking context.
    pub fn z_index(&mut self, value: i32) {
        self.z_index = StyleValue::Set(value);
    }

    /// Explicitly inherit z-index from the parent node.
    pub fn inherit_z_index(&mut self) {
        self.z_index = StyleValue::Inherit;
    }

    /// Reset z-index to the document/default style.
    pub fn unset_z_index(&mut self) {
        self.z_index = StyleValue::Unset;
    }

    // -- Stacking Context ----------------------------------------------

    /// Set whether this node creates an isolated stacking context.
    pub fn stacking_context(&mut self, value: bool) {
        self.stacking_context = StyleValue::Set(value);
    }

    /// Explicitly inherit stacking context behavior from the parent node.
    pub fn inherit_stacking_context(&mut self) {
        self.stacking_context = StyleValue::Inherit;
    }

    /// Reset stacking context behavior to the document/default style.
    pub fn unset_stacking_context(&mut self) {
        self.stacking_context = StyleValue::Unset;
    }

    // -- Selection Boundary --------------------------------------------

    /// Set whether mouse drag selection is confined to this node's subtree.
    pub fn selection_boundary(&mut self, value: bool) {
        self.selection_boundary = StyleValue::Set(value);
    }

    /// Explicitly inherit selection boundary behavior from the parent node.
    pub fn inherit_selection_boundary(&mut self) {
        self.selection_boundary = StyleValue::Inherit;
    }

    /// Reset selection boundary behavior to the document/default style.
    pub fn unset_selection_boundary(&mut self) {
        self.selection_boundary = StyleValue::Unset;
    }

    // -- Selection Colors ----------------------------------------------

    /// Set the background color for selected text.
    pub fn selection_bg(&mut self, value: Color) {
        self.selection_bg = StyleValue::Set(value);
    }

    /// Explicitly inherit the selected-text background from the parent node.
    pub fn inherit_selection_bg(&mut self) {
        self.selection_bg = StyleValue::Inherit;
    }

    /// Reset the selected-text background to the document/default style.
    pub fn unset_selection_bg(&mut self) {
        self.selection_bg = StyleValue::Unset;
    }

    /// Set the foreground color for selected text.
    pub fn selection_fg(&mut self, value: Color) {
        self.selection_fg = StyleValue::Set(value);
    }

    /// Explicitly inherit the selected-text foreground from the parent node.
    pub fn inherit_selection_fg(&mut self) {
        self.selection_fg = StyleValue::Inherit;
    }

    /// Reset the selected-text foreground to the document/default style.
    pub fn unset_selection_fg(&mut self) {
        self.selection_fg = StyleValue::Unset;
    }

    // -- Cursor Shape ---------------------------------------------------

    /// Set the input cursor shape.
    pub fn cursor_shape(&mut self, value: CursorShape) {
        self.cursor_shape = StyleValue::Set(value);
    }

    /// Explicitly inherit input cursor shape from the parent node.
    pub fn inherit_cursor_shape(&mut self) {
        self.cursor_shape = StyleValue::Inherit;
    }

    /// Reset input cursor shape to the document/default style.
    pub fn unset_cursor_shape(&mut self) {
        self.cursor_shape = StyleValue::Unset;
    }

    // -- Color Variables -----------------------------------------------

    /// Declare a color variable, in scope for this node and its descendants.
    ///
    /// Variables cascade: a node sees the ones its ancestors declared, and the document's beneath
    /// those. Redeclaring a name shadows it for this subtree. Reference one from any color
    /// property with [`Color::var`].
    ///
    /// A variable's value resolves against the *parent's* scope, so it can derive from the name it
    /// shadows — `Color::var("--accent").darken(0.1)` means the inherited `--accent`, darkened.
    ///
    /// Declaring variables in a pseudo-state style has no effect: the node's other colors have
    /// already resolved against its scope by the time a pseudo-state style merges on top.
    pub fn color_var(&mut self, name: impl Into<String>, value: Color) {
        self.color_vars.insert(name.into(), value);
    }

    /// Get a color variable declared on this style.
    pub fn get_color_var(&self, name: &str) -> Option<&Color> {
        self.color_vars.get(name)
    }

    /// Remove a color variable declared on this style.
    ///
    /// The node goes back to inheriting the name from its ancestors, if they declare it.
    pub fn remove_color_var(&mut self, name: &str) {
        self.color_vars.remove(name);
    }

    // -- Custom Properties ---------------------------------------------

    /// Set a raw custom style property.
    ///
    /// Custom properties are inline metadata only. They do not inherit, do not
    /// appear on resolved styles, and do not affect layout or rendering.
    pub fn set_custom(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.custom.insert(name.into(), value.into());
    }

    /// Get a raw custom style property.
    pub fn get_custom(&self, name: &str) -> Option<&str> {
        self.custom.get(name).map(String::as_str)
    }

    /// Remove a raw custom style property.
    pub fn remove_custom(&mut self, name: &str) {
        self.custom.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_chain() {
        let mut style = Style::new();
        style.width(Length::Percent(100.0));
        style.height(Length::Pixels(20));
        style.padding(EdgeInsets::symmetric(2, 1));
        style.margin(EdgeInsets::new(1, 2, 3, 4));
        style.color(Color::white());
        style.background(Color::blue());
        style.opacity(0.5);
        style.flex_direction(FlexDirection::Column);
        style.flex_basis(Length::Pixels(3));
        style.flex_grow(1.0);
        style.flex_shrink(0.5);
        style.flex_wrap(FlexWrap::Wrap);
        style.gap(FlexGap::new(1, 2));
        style.align_self(AlignSelf::Center);
        style.align_content(AlignContent::Center);
        style.position(Position::Absolute { x: 4, y: -2 });

        style.z_index(10);
        style.stacking_context(true);
        style.selection_boundary(true);
        style.cursor_shape(CursorShape::Bar);
        style.set_custom("--role", "panel");

        assert_eq!(style.width, StyleValue::Set(Length::Percent(100.0)));
        assert_eq!(style.height, StyleValue::Set(Length::Pixels(20)));
        assert_eq!(style.padding, StyleValue::Set(EdgeInsets::symmetric(2, 1)));
        assert_eq!(style.margin, StyleValue::Set(EdgeInsets::new(1, 2, 3, 4)));
        assert_eq!(style.opacity, StyleValue::Set(0.5));
        assert_eq!(style.flex_direction, StyleValue::Set(FlexDirection::Column));
        assert_eq!(style.flex_basis, StyleValue::Set(Length::Pixels(3)));
        assert_eq!(style.flex_grow, StyleValue::Set(1.0));
        assert_eq!(style.flex_shrink, StyleValue::Set(0.5));
        assert_eq!(style.flex_wrap, StyleValue::Set(FlexWrap::Wrap));
        assert_eq!(style.gap, StyleValue::Set(FlexGap::new(1, 2)));
        assert_eq!(style.align_self, StyleValue::Set(AlignSelf::Center));
        assert_eq!(style.align_content, StyleValue::Set(AlignContent::Center));
        assert_eq!(
            style.position,
            StyleValue::Set(Position::Absolute { x: 4, y: -2 })
        );
        assert_eq!(style.z_index, StyleValue::Set(10));
        assert_eq!(style.stacking_context, StyleValue::Set(true));
        assert_eq!(style.selection_boundary, StyleValue::Set(true));
        assert_eq!(style.cursor_shape, StyleValue::Set(CursorShape::Bar));
        assert_eq!(style.get_custom("--role"), Some("panel"));
    }

    #[test]
    fn default_is_all_unset() {
        let style = Style::new();
        assert_eq!(style.width, StyleValue::Unset);
        assert_eq!(style.padding, StyleValue::Unset);
        assert_eq!(style.margin, StyleValue::Unset);
        assert_eq!(style.opacity, StyleValue::Unset);
        assert_eq!(style.color, StyleValue::Unset);
        assert_eq!(style.flex_direction, StyleValue::Unset);
        assert_eq!(style.flex_basis, StyleValue::Unset);
        assert_eq!(style.flex_grow, StyleValue::Unset);
        assert_eq!(style.flex_shrink, StyleValue::Unset);
        assert_eq!(style.flex_wrap, StyleValue::Unset);
        assert_eq!(style.gap, StyleValue::Unset);
        assert_eq!(style.align_self, StyleValue::Unset);
        assert_eq!(style.align_content, StyleValue::Unset);
        assert_eq!(style.position, StyleValue::Unset);
        assert_eq!(style.z_index, StyleValue::Unset);
        assert_eq!(style.stacking_context, StyleValue::Unset);
        assert_eq!(style.selection_boundary, StyleValue::Unset);
        assert_eq!(style.cursor_shape, StyleValue::Unset);
        assert_eq!(style.get_custom("--role"), None);
    }

    #[test]
    fn custom_properties_are_raw_inline_metadata() {
        let mut style = Style::new();
        style.set_custom(String::from("--role"), String::from("panel"));
        assert_eq!(style.get_custom("--role"), Some("panel"));

        let cloned = style.clone();
        assert_eq!(cloned.get_custom("--role"), Some("panel"));

        style.remove_custom("--role");
        assert_eq!(style.get_custom("--role"), None);
    }

    #[test]
    fn inheritance_is_explicit() {
        let mut style = Style::new();

        style.inherit_width();
        style.inherit_padding();
        style.inherit_margin();
        style.inherit_opacity();
        style.inherit_color();
        style.inherit_flex_direction();
        style.inherit_flex_basis();
        style.inherit_flex_grow();
        style.inherit_flex_shrink();
        style.inherit_flex_wrap();
        style.inherit_gap();
        style.inherit_align_self();
        style.inherit_align_content();
        style.inherit_position();
        style.inherit_z_index();
        style.inherit_stacking_context();
        style.inherit_selection_boundary();
        style.inherit_cursor_shape();

        assert_eq!(style.width, StyleValue::Inherit);
        assert_eq!(style.padding, StyleValue::Inherit);
        assert_eq!(style.margin, StyleValue::Inherit);
        assert_eq!(style.opacity, StyleValue::Inherit);
        assert_eq!(style.color, StyleValue::Inherit);
        assert_eq!(style.flex_direction, StyleValue::Inherit);
        assert_eq!(style.flex_basis, StyleValue::Inherit);
        assert_eq!(style.flex_grow, StyleValue::Inherit);
        assert_eq!(style.flex_shrink, StyleValue::Inherit);
        assert_eq!(style.flex_wrap, StyleValue::Inherit);
        assert_eq!(style.gap, StyleValue::Inherit);
        assert_eq!(style.align_self, StyleValue::Inherit);
        assert_eq!(style.align_content, StyleValue::Inherit);
        assert_eq!(style.position, StyleValue::Inherit);
        assert_eq!(style.z_index, StyleValue::Inherit);
        assert_eq!(style.stacking_context, StyleValue::Inherit);
        assert_eq!(style.selection_boundary, StyleValue::Inherit);
        assert_eq!(style.cursor_shape, StyleValue::Inherit);
    }
}

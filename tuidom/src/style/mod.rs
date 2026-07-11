/// Color types and OKLCH → RGB conversion.
pub mod color;
/// Resolved style computation and caching.
pub(crate) mod resolution;

use std::collections::HashMap;

pub use color::Color;

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
}

/// Whether flex children remain on one line or wrap onto multiple lines.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    /// Keep flex children on a single line.
    #[default]
    NoWrap,
    /// Allow flex children to wrap onto additional lines.
    Wrap,
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
    /// Display mode.
    pub(crate) display: StyleValue<Display>,
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
    /// Input cursor shape.
    pub(crate) cursor_shape: StyleValue<CursorShape>,
    /// Raw custom style properties.
    pub(crate) custom: HashMap<String, String>,
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
            display: StyleValue::Unset,
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
            cursor_shape: StyleValue::Unset,
            custom: HashMap::new(),
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
        assert_eq!(style.cursor_shape, StyleValue::Inherit);
    }
}

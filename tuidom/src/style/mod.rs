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
// Style
// ---------------------------------------------------------------------------

/// Inline style for a node.
///
/// Known properties are typed; unknown properties fall back to a string map.
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
    /// Display mode.
    pub(crate) display: StyleValue<Display>,
    /// Opacity (0–1).
    pub(crate) opacity: StyleValue<f64>,
    /// Foreground text color.
    pub(crate) color: StyleValue<Color>,
    /// Background color.
    pub(crate) background: StyleValue<Color>,
    /// Cross-axis alignment (flex container).
    pub(crate) align_items: StyleValue<AlignItems>,
    /// Main-axis alignment (flex container).
    pub(crate) justify_content: StyleValue<JustifyContent>,
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
            display: StyleValue::Unset,
            opacity: StyleValue::Unset,
            color: StyleValue::Unset,
            background: StyleValue::Unset,
            align_items: StyleValue::Unset,
            justify_content: StyleValue::Unset,
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
        style.color(Color::white());
        style.background(Color::blue());
        style.opacity(0.5);

        style.z_index(10);
        style.stacking_context(true);
        style.cursor_shape(CursorShape::Bar);
        style.set_custom("--role", "panel");

        assert_eq!(style.width, StyleValue::Set(Length::Percent(100.0)));
        assert_eq!(style.height, StyleValue::Set(Length::Pixels(20)));
        assert_eq!(style.opacity, StyleValue::Set(0.5));
        assert_eq!(style.z_index, StyleValue::Set(10));
        assert_eq!(style.stacking_context, StyleValue::Set(true));
        assert_eq!(style.cursor_shape, StyleValue::Set(CursorShape::Bar));
        assert_eq!(style.get_custom("--role"), Some("panel"));
    }

    #[test]
    fn default_is_all_unset() {
        let style = Style::new();
        assert_eq!(style.width, StyleValue::Unset);
        assert_eq!(style.opacity, StyleValue::Unset);
        assert_eq!(style.color, StyleValue::Unset);
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
        style.inherit_opacity();
        style.inherit_color();
        style.inherit_z_index();
        style.inherit_stacking_context();
        style.inherit_cursor_shape();

        assert_eq!(style.width, StyleValue::Inherit);
        assert_eq!(style.opacity, StyleValue::Inherit);
        assert_eq!(style.color, StyleValue::Inherit);
        assert_eq!(style.z_index, StyleValue::Inherit);
        assert_eq!(style.stacking_context, StyleValue::Inherit);
        assert_eq!(style.cursor_shape, StyleValue::Inherit);
    }
}

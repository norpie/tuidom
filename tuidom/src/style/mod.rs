/// Color types and OKLCH → RGB conversion.
pub mod color;
/// Resolved style computation and caching.
pub(crate) mod resolution;

use std::collections::HashMap;

pub use color::Color;

// ---------------------------------------------------------------------------
// StyleValue — explicit inheritance control
// ---------------------------------------------------------------------------

/// Wraps a style property value. Either explicitly `Set` or `Inherit` from parent.
///
/// By default, nothing inherits — you must explicitly use `Inherit` to opt in.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleValue<T> {
    /// An explicitly set value.
    Set(T),
    /// Inherit the resolved value from the parent node.
    Inherit,
}

impl<T> Default for StyleValue<T> {
    fn default() -> Self {
        Self::Inherit
    }
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
/// });
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
    /// Catch-all for unknown / future properties.
    pub(crate) extra: HashMap<String, String>,
}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

impl Style {
    /// Create a new [`Style`] with all properties set to [`StyleValue::Inherit`].
    pub fn new() -> Self {
        Self {
            width: StyleValue::Inherit,
            height: StyleValue::Inherit,
            display: StyleValue::Inherit,
            opacity: StyleValue::Inherit,
            color: StyleValue::Inherit,
            background: StyleValue::Inherit,
            align_items: StyleValue::Inherit,
            justify_content: StyleValue::Inherit,
            extra: HashMap::new(),
        }
    }

    // -- Width ----------------------------------------------------------

    /// Set the width.
    pub fn width(&mut self, value: Length) -> &mut Self {
        self.width = StyleValue::Set(value);
        self
    }

    // -- Height ---------------------------------------------------------

    /// Set the height.
    pub fn height(&mut self, value: Length) -> &mut Self {
        self.height = StyleValue::Set(value);
        self
    }

    // -- Display --------------------------------------------------------

    /// Set the display mode.
    pub fn display(&mut self, value: Display) -> &mut Self {
        self.display = StyleValue::Set(value);
        self
    }

    // -- Opacity --------------------------------------------------------

    /// Set the opacity (0–1).
    pub fn opacity(&mut self, value: f64) -> &mut Self {
        self.opacity = StyleValue::Set(value);
        self
    }

    // -- Color ----------------------------------------------------------

    /// Set the foreground text color.
    pub fn color(&mut self, value: Color) -> &mut Self {
        self.color = StyleValue::Set(value);
        self
    }

    // -- Background -----------------------------------------------------

    /// Set the background color.
    pub fn background(&mut self, value: Color) -> &mut Self {
        self.background = StyleValue::Set(value);
        self
    }

    // -- Align Items ----------------------------------------------------

    /// Set the cross-axis alignment.
    pub fn align_items(&mut self, value: AlignItems) -> &mut Self {
        self.align_items = StyleValue::Set(value);
        self
    }

    // -- Justify Content -------------------------------------------------

    /// Set the main-axis alignment.
    pub fn justify_content(&mut self, value: JustifyContent) -> &mut Self {
        self.justify_content = StyleValue::Set(value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_chain() {
        let mut style = Style::new();
        style
            .width(Length::Percent(100.0))
            .height(Length::Pixels(20))
            .color(Color::white())
            .background(Color::blue())
            .opacity(0.5);

        assert_eq!(style.width, StyleValue::Set(Length::Percent(100.0)));
        assert_eq!(style.height, StyleValue::Set(Length::Pixels(20)));
        assert_eq!(style.opacity, StyleValue::Set(0.5));
    }

    #[test]
    fn default_is_all_inherit() {
        let style = Style::new();
        assert_eq!(style.width, StyleValue::Inherit);
        assert_eq!(style.opacity, StyleValue::Inherit);
        assert_eq!(style.color, StyleValue::Inherit);
    }
}

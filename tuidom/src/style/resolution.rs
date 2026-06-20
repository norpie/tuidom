//! Resolved style computation and caching.
//!
//! `ResolvedStyle` is the fully concrete style — all `Inherit` values have
//! been resolved by walking the parent chain. It is computed once on change
//! and cached per-node.

use crate::node::NodeData;
use crate::style::{AlignItems, Color, Display, JustifyContent, Length, StyleValue};

/// Fully resolved style — no [`StyleValue::Inherit`] placeholders remain.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedStyle {
    /// Resolved width.
    pub width: Length,
    /// Resolved height.
    pub height: Length,
    /// Resolved display mode.
    pub display: Display,
    /// Resolved opacity (0–1).
    pub opacity: f64,
    /// Resolved foreground text color.
    pub color: Color,
    /// Resolved background color.
    pub background: Color,
    /// Resolved cross-axis alignment.
    pub align_items: AlignItems,
    /// Resolved main-axis alignment.
    pub justify_content: JustifyContent,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            display: Display::Flex,
            opacity: 1.0,
            color: Color::white(),
            background: Color::black(),
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::FlexStart,
        }
    }
}

impl ResolvedStyle {
    /// Compute the resolved style for a node given its parent's resolved style.
    ///
    /// For each property, uses the node's value if `Set`, falls back to the
    /// parent's resolved value if `Inherit`, and falls back to [`Default`]
    /// if the node is the root.
    pub fn compute(data: &NodeData, parent: Option<&ResolvedStyle>) -> Self {
        let defaults = ResolvedStyle::default();

        Self {
            width: resolve(&data.style.width, parent.map(|p| &p.width), &defaults.width),
            height: resolve(&data.style.height, parent.map(|p| &p.height), &defaults.height),
            display: resolve(&data.style.display, parent.map(|p| &p.display), &defaults.display),
            opacity: resolve(
                &data.style.opacity,
                parent.map(|p| &p.opacity),
                &defaults.opacity,
            ),
            color: resolve(&data.style.color, parent.map(|p| &p.color), &defaults.color),
            background: resolve(
                &data.style.background,
                parent.map(|p| &p.background),
                &defaults.background,
            ),
            align_items: resolve(
                &data.style.align_items,
                parent.map(|p| &p.align_items),
                &defaults.align_items,
            ),
            justify_content: resolve(
                &data.style.justify_content,
                parent.map(|p| &p.justify_content),
                &defaults.justify_content,
            ),
        }
    }
}

/// Resolve a single [`StyleValue`] given the parent's resolved value and a
/// root default.
fn resolve<T: Clone>(value: &StyleValue<T>, parent: Option<&T>, default: &T) -> T {
    match value {
        StyleValue::Set(v) => v.clone(),
        StyleValue::Inherit => parent.cloned().unwrap_or_else(|| default.clone()),
    }
}

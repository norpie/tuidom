//! Resolved style computation and caching.
//!
//! `ResolvedStyle` is the fully concrete style — all unresolved style values have
//! been resolved from explicit values, explicit parent inheritance, or document defaults.

use crate::node::NodeData;
use crate::style::{AlignItems, Color, Display, JustifyContent, Length, Style, StyleValue};

/// Fully resolved style — no [`StyleValue`] placeholders remain.
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
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
    /// Resolved background color. `None` means transparent (terminal default shows through).
    pub background: Option<Color>,
    /// Resolved cross-axis alignment.
    pub align_items: AlignItems,
    /// Resolved main-axis alignment.
    pub justify_content: JustifyContent,
    /// Resolved paint order within the current stacking context.
    pub z_index: i32,
    /// Whether this node creates an isolated stacking context.
    pub stacking_context: bool,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        StyleDefaults::default().to_resolved_style()
    }
}

/// Default style values used for unset properties.
#[derive(Debug, Clone)]
pub(crate) struct StyleDefaults {
    width: Length,
    height: Length,
    display: Display,
    opacity: f64,
    color: Color,
    background: Option<Color>,
    align_items: AlignItems,
    justify_content: JustifyContent,
    z_index: i32,
    stacking_context: bool,
}

impl Default for StyleDefaults {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            display: Display::Flex,
            opacity: 1.0,
            color: Color::white(),
            background: None,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::FlexStart,
            z_index: 0,
            stacking_context: false,
        }
    }
}

impl StyleDefaults {
    /// Defaults used by the permanent document root when a property is unset.
    pub(crate) fn root() -> Self {
        Self {
            width: Length::Percent(100.0),
            height: Length::Percent(100.0),
            ..Self::default()
        }
    }

    fn to_resolved_style(&self) -> ResolvedStyle {
        ResolvedStyle {
            width: self.width,
            height: self.height,
            display: self.display,
            opacity: self.opacity,
            color: self.color,
            background: self.background,
            align_items: self.align_items,
            justify_content: self.justify_content,
            z_index: self.z_index,
            stacking_context: self.stacking_context,
        }
    }
}

impl ResolvedStyle {
    /// Compute the resolved style for a node given its parent's resolved style.
    ///
    /// For each property, uses the node's value if `Set`, uses the parent's
    /// resolved value if `Inherit`, and uses the document/default style if `Unset`.
    pub(crate) fn compute(data: &NodeData, parent: Option<&ResolvedStyle>) -> Self {
        Self::compute_with_defaults(data, parent, &StyleDefaults::default())
    }

    pub(crate) fn compute_with_defaults(
        data: &NodeData,
        parent: Option<&ResolvedStyle>,
        defaults: &StyleDefaults,
    ) -> Self {
        Self {
            width: resolve(&data.style.width, parent.map(|p| &p.width), &defaults.width),
            height: resolve(
                &data.style.height,
                parent.map(|p| &p.height),
                &defaults.height,
            ),
            display: resolve(
                &data.style.display,
                parent.map(|p| &p.display),
                &defaults.display,
            ),
            opacity: resolve(
                &data.style.opacity,
                parent.map(|p| &p.opacity),
                &defaults.opacity,
            ),
            color: resolve(&data.style.color, parent.map(|p| &p.color), &defaults.color),
            background: resolve_opt(
                &data.style.background,
                parent.and_then(|p| p.background),
                defaults.background,
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
            z_index: resolve(
                &data.style.z_index,
                parent.map(|p| &p.z_index),
                &defaults.z_index,
            ),
            stacking_context: resolve(
                &data.style.stacking_context,
                parent.map(|p| &p.stacking_context),
                &defaults.stacking_context,
            ),
        }
    }

    pub(crate) fn apply_overrides(
        &mut self,
        style: &Style,
        parent: Option<&ResolvedStyle>,
        defaults: &StyleDefaults,
    ) {
        apply_override(
            &mut self.width,
            &style.width,
            parent.map(|p| &p.width),
            &defaults.width,
        );
        apply_override(
            &mut self.height,
            &style.height,
            parent.map(|p| &p.height),
            &defaults.height,
        );
        apply_override(
            &mut self.display,
            &style.display,
            parent.map(|p| &p.display),
            &defaults.display,
        );
        apply_override(
            &mut self.opacity,
            &style.opacity,
            parent.map(|p| &p.opacity),
            &defaults.opacity,
        );
        apply_override(
            &mut self.color,
            &style.color,
            parent.map(|p| &p.color),
            &defaults.color,
        );
        apply_opt_override(
            &mut self.background,
            &style.background,
            parent.and_then(|p| p.background),
            defaults.background,
        );
        apply_override(
            &mut self.align_items,
            &style.align_items,
            parent.map(|p| &p.align_items),
            &defaults.align_items,
        );
        apply_override(
            &mut self.justify_content,
            &style.justify_content,
            parent.map(|p| &p.justify_content),
            &defaults.justify_content,
        );
        apply_override(
            &mut self.z_index,
            &style.z_index,
            parent.map(|p| &p.z_index),
            &defaults.z_index,
        );
        apply_override(
            &mut self.stacking_context,
            &style.stacking_context,
            parent.map(|p| &p.stacking_context),
            &defaults.stacking_context,
        );
    }
}

/// Resolve a single [`StyleValue`] given the parent's resolved value and a default.
fn resolve<T: Clone>(value: &StyleValue<T>, parent: Option<&T>, default: &T) -> T {
    match value {
        StyleValue::Unset => default.clone(),
        StyleValue::Inherit => parent.cloned().unwrap_or_else(|| default.clone()),
        StyleValue::Set(v) => v.clone(),
    }
}

/// Resolve a [`StyleValue<Color>`] to `Option<Color>`.
///
/// `Set(Color)` → `Some(Color)`. `Inherit` uses the parent's value or the
/// default if there is no parent. `Unset` always uses the default value.
fn resolve_opt(
    value: &StyleValue<Color>,
    parent: Option<Color>,
    default: Option<Color>,
) -> Option<Color> {
    match value {
        StyleValue::Unset => default,
        StyleValue::Inherit => parent.or(default),
        StyleValue::Set(v) => Some(*v),
    }
}

fn apply_override<T: Clone>(
    target: &mut T,
    value: &StyleValue<T>,
    parent: Option<&T>,
    default: &T,
) {
    match value {
        StyleValue::Unset => {}
        StyleValue::Inherit => *target = parent.cloned().unwrap_or_else(|| default.clone()),
        StyleValue::Set(v) => *target = v.clone(),
    }
}

fn apply_opt_override(
    target: &mut Option<Color>,
    value: &StyleValue<Color>,
    parent: Option<Color>,
    default: Option<Color>,
) {
    match value {
        StyleValue::Unset => {}
        StyleValue::Inherit => *target = parent.or(default),
        StyleValue::Set(v) => *target = Some(*v),
    }
}

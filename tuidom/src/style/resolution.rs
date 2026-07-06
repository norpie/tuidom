//! Resolved style computation and caching.
//!
//! `ResolvedStyle` is the fully concrete style — all unresolved style values have
//! been resolved from explicit values, explicit parent inheritance, or document defaults.

use crate::node::NodeData;
use crate::style::{
    AlignItems, AlignSelf, Color, CursorShape, Display, EdgeInsets, FlexDirection, FlexGap,
    FlexWrap, JustifyContent, Length, Style, StyleValue,
};

/// Fully resolved style — no [`StyleValue`] placeholders remain.
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
    /// Resolved width.
    pub width: Length,
    /// Resolved height.
    pub height: Length,
    /// Resolved inner spacing.
    pub padding: EdgeInsets,
    /// Resolved outer spacing.
    pub margin: EdgeInsets,
    /// Resolved display mode.
    pub display: Display,
    /// Resolved opacity (0–1).
    pub opacity: f64,
    /// Resolved foreground text color.
    pub color: Color,
    /// Resolved background color. `None` means transparent (terminal default shows through).
    pub background: Option<Color>,
    /// Resolved main-axis direction.
    pub flex_direction: FlexDirection,
    /// Resolved initial main-axis size for flex items.
    pub flex_basis: Length,
    /// Resolved relative grow factor for flex items.
    pub flex_grow: f32,
    /// Resolved relative shrink factor for flex items.
    pub flex_shrink: f32,
    /// Resolved flex wrapping behavior.
    pub flex_wrap: FlexWrap,
    /// Resolved spacing between flex children and flex lines.
    pub gap: FlexGap,
    /// Resolved cross-axis alignment override for this flex item.
    pub align_self: Option<AlignSelf>,
    /// Resolved cross-axis alignment.
    pub align_items: AlignItems,
    /// Resolved main-axis alignment.
    pub justify_content: JustifyContent,
    /// Resolved paint order within the current stacking context.
    pub z_index: i32,
    /// Whether this node creates an isolated stacking context.
    pub stacking_context: bool,
    /// Resolved input cursor shape.
    pub cursor_shape: CursorShape,
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
    padding: EdgeInsets,
    margin: EdgeInsets,
    display: Display,
    opacity: f64,
    color: Color,
    background: Option<Color>,
    flex_direction: FlexDirection,
    flex_basis: Length,
    flex_grow: f32,
    flex_shrink: f32,
    flex_wrap: FlexWrap,
    gap: FlexGap,
    align_self: Option<AlignSelf>,
    align_items: AlignItems,
    justify_content: JustifyContent,
    z_index: i32,
    stacking_context: bool,
    cursor_shape: CursorShape,
}

impl Default for StyleDefaults {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            padding: EdgeInsets::ZERO,
            margin: EdgeInsets::ZERO,
            display: Display::Flex,
            opacity: 1.0,
            color: Color::white(),
            background: None,
            flex_direction: FlexDirection::Row,
            flex_basis: Length::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_wrap: FlexWrap::NoWrap,
            gap: FlexGap::ZERO,
            align_self: None,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::FlexStart,
            z_index: 0,
            stacking_context: false,
            cursor_shape: CursorShape::Block,
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
            padding: self.padding,
            margin: self.margin,
            display: self.display,
            opacity: self.opacity,
            color: self.color,
            background: self.background,
            flex_direction: self.flex_direction,
            flex_basis: self.flex_basis,
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            flex_wrap: self.flex_wrap,
            gap: self.gap,
            align_self: self.align_self,
            align_items: self.align_items,
            justify_content: self.justify_content,
            z_index: self.z_index,
            stacking_context: self.stacking_context,
            cursor_shape: self.cursor_shape,
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
            padding: resolve(
                &data.style.padding,
                parent.map(|p| &p.padding),
                &defaults.padding,
            ),
            margin: resolve(
                &data.style.margin,
                parent.map(|p| &p.margin),
                &defaults.margin,
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
            flex_direction: resolve(
                &data.style.flex_direction,
                parent.map(|p| &p.flex_direction),
                &defaults.flex_direction,
            ),
            flex_basis: resolve(
                &data.style.flex_basis,
                parent.map(|p| &p.flex_basis),
                &defaults.flex_basis,
            ),
            flex_grow: resolve(
                &data.style.flex_grow,
                parent.map(|p| &p.flex_grow),
                &defaults.flex_grow,
            ),
            flex_shrink: resolve(
                &data.style.flex_shrink,
                parent.map(|p| &p.flex_shrink),
                &defaults.flex_shrink,
            ),
            flex_wrap: resolve(
                &data.style.flex_wrap,
                parent.map(|p| &p.flex_wrap),
                &defaults.flex_wrap,
            ),
            gap: resolve(&data.style.gap, parent.map(|p| &p.gap), &defaults.gap),
            align_self: resolve_optional(
                &data.style.align_self,
                parent.map(|p| p.align_self),
                defaults.align_self,
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
            cursor_shape: resolve(
                &data.style.cursor_shape,
                parent.map(|p| &p.cursor_shape),
                &defaults.cursor_shape,
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
            &mut self.padding,
            &style.padding,
            parent.map(|p| &p.padding),
            &defaults.padding,
        );
        apply_override(
            &mut self.margin,
            &style.margin,
            parent.map(|p| &p.margin),
            &defaults.margin,
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
            &mut self.flex_direction,
            &style.flex_direction,
            parent.map(|p| &p.flex_direction),
            &defaults.flex_direction,
        );
        apply_override(
            &mut self.flex_basis,
            &style.flex_basis,
            parent.map(|p| &p.flex_basis),
            &defaults.flex_basis,
        );
        apply_override(
            &mut self.flex_grow,
            &style.flex_grow,
            parent.map(|p| &p.flex_grow),
            &defaults.flex_grow,
        );
        apply_override(
            &mut self.flex_shrink,
            &style.flex_shrink,
            parent.map(|p| &p.flex_shrink),
            &defaults.flex_shrink,
        );
        apply_override(
            &mut self.flex_wrap,
            &style.flex_wrap,
            parent.map(|p| &p.flex_wrap),
            &defaults.flex_wrap,
        );
        apply_override(
            &mut self.gap,
            &style.gap,
            parent.map(|p| &p.gap),
            &defaults.gap,
        );
        apply_optional_override(
            &mut self.align_self,
            &style.align_self,
            parent.map(|p| p.align_self),
            defaults.align_self,
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
        apply_override(
            &mut self.cursor_shape,
            &style.cursor_shape,
            parent.map(|p| &p.cursor_shape),
            &defaults.cursor_shape,
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

fn resolve_optional<T: Clone>(
    value: &StyleValue<T>,
    parent: Option<Option<T>>,
    default: Option<T>,
) -> Option<T> {
    match value {
        StyleValue::Unset => default,
        StyleValue::Inherit => parent.unwrap_or(default),
        StyleValue::Set(v) => Some(v.clone()),
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

fn apply_optional_override<T: Clone>(
    target: &mut Option<T>,
    value: &StyleValue<T>,
    parent: Option<Option<T>>,
    default: Option<T>,
) {
    match value {
        StyleValue::Unset => {}
        StyleValue::Inherit => *target = parent.unwrap_or(default),
        StyleValue::Set(v) => *target = Some(v.clone()),
    }
}

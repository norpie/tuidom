//! Resolved style computation and caching.
//!
//! `ResolvedStyle` is the fully concrete style — all unresolved style values have
//! been resolved from explicit values, explicit parent inheritance, or document defaults.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::node::NodeData;
use crate::style::color::ColorContext;
use crate::style::{
    AlignContent, AlignItems, AlignSelf, Border, Color, CursorShape, Display, EdgeInsets,
    FlexDirection, FlexGap, FlexWrap, JustifyContent, Length, Overflow, Position, ResolvedColor,
    ScrollbarCharset, ScrollbarShow, Sides, Style, StyleValue,
};

/// A node's color variables, inherited from its ancestors and the document.
pub(crate) type ColorScope = Arc<HashMap<String, ResolvedColor>>;

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
    /// Resolved border charset and drawn sides.
    pub border: Border,
    /// Resolved border color. `None` means the border follows this node's resolved `color`.
    pub border_color: Option<ResolvedColor>,
    /// Resolved sides drawn as a half-block edge.
    pub half_block_edges: Sides,
    /// Resolved half-block inner color. `None` means it follows this node's `background`.
    pub half_block_inner_color: Option<ResolvedColor>,
    /// Resolved half-block outer color. `None` means the outer half keeps what is painted there.
    pub half_block_outer_color: Option<ResolvedColor>,
    /// Resolved display mode.
    pub display: Display,
    /// Resolved horizontal overflow behavior.
    pub overflow_x: Overflow,
    /// Resolved vertical overflow behavior.
    pub overflow_y: Overflow,
    /// Resolved scrollbar visibility.
    pub scrollbar_show: ScrollbarShow,
    /// Resolved hold time before a [`ScrollbarShow::WhenScrolling`] bar starts fading.
    pub scrollbar_hide_delay: Duration,
    /// Resolved fade-out time of a [`ScrollbarShow::WhenScrolling`] bar after its delay.
    pub scrollbar_fade_duration: Duration,
    /// Resolved scrollbar drawing characters.
    pub scrollbar_charset: ScrollbarCharset,
    /// Resolved scrollbar track color. `None` follows this node's resolved `color`.
    pub scrollbar_track_color: Option<ResolvedColor>,
    /// Resolved scrollbar thumb color. `None` follows this node's resolved `color`.
    pub scrollbar_thumb_color: Option<ResolvedColor>,
    /// Resolved opacity (0–1).
    pub opacity: f64,
    /// Resolved foreground text color.
    pub color: ResolvedColor,
    /// Resolved background color. `None` means transparent (terminal default shows through).
    pub background: Option<ResolvedColor>,
    /// The background this node visually sits on: its own if it has one, otherwise the nearest
    /// ancestor's, falling back to the document's declared terminal background.
    ///
    /// This is what [`Color::CurrentBg`] resolves to. It is never `None` — a node deriving a color
    /// from the background it sits on needs an answer even when nothing in its ancestry paints one.
    pub effective_background: ResolvedColor,
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
    /// Resolved cross-axis alignment for wrapped flex lines.
    pub align_content: AlignContent,
    /// Resolved main-axis alignment.
    pub justify_content: JustifyContent,
    /// Resolved positioning mode.
    pub position: Position,
    /// Resolved paint order within the current stacking context.
    pub z_index: i32,
    /// Whether this node creates an isolated stacking context.
    pub stacking_context: bool,
    /// Whether mouse drag selection is confined to this node's subtree.
    pub selection_boundary: bool,
    /// Resolved selected-text background. `None` means reverse video: selected glyphs
    /// swap their foreground and background.
    pub selection_bg: Option<ResolvedColor>,
    /// Resolved selected-text foreground. `None` means reverse video: selected glyphs
    /// swap their foreground and background.
    pub selection_fg: Option<ResolvedColor>,
    /// Whether text is drawn bold.
    pub bold: bool,
    /// Whether text is drawn italic.
    pub italic: bool,
    /// Whether text is drawn underlined.
    pub underline: bool,
    /// Resolved input cursor shape.
    pub cursor_shape: CursorShape,
    /// The color variables in scope on this node.
    ///
    /// Shared by pointer with the parent whenever a node declares none of its own, which is what
    /// keeps a `ResolvedStyle` cheap to clone for every node of every frame.
    pub(crate) color_vars: ColorScope,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        StyleDefaults::default().to_resolved_style()
    }
}

impl ResolvedStyle {
    /// The color filling the node's half of a half-block edge cell.
    ///
    /// An unset inner color follows the node's background, since the edge exists to make that
    /// background end half a cell early. `None` means there is no fill to take a half of, and
    /// the edge draws nothing at all.
    pub(crate) fn half_block_inner(&self) -> Option<ResolvedColor> {
        self.half_block_inner_color.or(self.background)
    }

    /// Whether this node draws any half-block edge.
    pub(crate) fn draws_half_block_edges(&self) -> bool {
        self.half_block_edges.any() && self.half_block_inner().is_some()
    }
}

/// Default style values used for unset properties.
#[derive(Debug, Clone)]
pub(crate) struct StyleDefaults {
    width: Length,
    height: Length,
    padding: EdgeInsets,
    margin: EdgeInsets,
    border: Border,
    border_color: Option<ResolvedColor>,
    half_block_edges: Sides,
    half_block_inner_color: Option<ResolvedColor>,
    half_block_outer_color: Option<ResolvedColor>,
    display: Display,
    overflow_x: Overflow,
    overflow_y: Overflow,
    scrollbar_show: ScrollbarShow,
    scrollbar_hide_delay: Duration,
    scrollbar_fade_duration: Duration,
    scrollbar_charset: ScrollbarCharset,
    scrollbar_track_color: Option<ResolvedColor>,
    scrollbar_thumb_color: Option<ResolvedColor>,
    opacity: f64,
    color: ResolvedColor,
    background: Option<ResolvedColor>,
    flex_direction: FlexDirection,
    flex_basis: Length,
    flex_grow: f32,
    flex_shrink: f32,
    flex_wrap: FlexWrap,
    gap: FlexGap,
    align_self: Option<AlignSelf>,
    align_items: AlignItems,
    align_content: AlignContent,
    justify_content: JustifyContent,
    position: Position,
    z_index: i32,
    stacking_context: bool,
    selection_boundary: bool,
    selection_bg: Option<ResolvedColor>,
    selection_fg: Option<ResolvedColor>,
    bold: bool,
    italic: bool,
    underline: bool,
    cursor_shape: CursorShape,
    /// The document's color variables. Only the root reads these — every other node inherits its
    /// scope from its parent.
    color_vars: ColorScope,
    /// The document's declared terminal background. Only the root reads it — every other node
    /// inherits an effective background from its parent.
    terminal_background: ResolvedColor,
}

impl Default for StyleDefaults {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            padding: EdgeInsets::ZERO,
            margin: EdgeInsets::ZERO,
            border: Border::none(),
            border_color: None,
            half_block_edges: Sides::NONE,
            half_block_inner_color: None,
            half_block_outer_color: None,
            display: Display::Flex,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            scrollbar_show: ScrollbarShow::Always,
            scrollbar_hide_delay: Duration::from_secs(1),
            scrollbar_fade_duration: Duration::from_millis(250),
            scrollbar_charset: ScrollbarCharset::block(),
            scrollbar_track_color: None,
            scrollbar_thumb_color: None,
            opacity: 1.0,
            color: ResolvedColor::white(),
            background: None,
            flex_direction: FlexDirection::Row,
            flex_basis: Length::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_wrap: FlexWrap::NoWrap,
            gap: FlexGap::ZERO,
            align_self: None,
            align_items: AlignItems::Stretch,
            align_content: AlignContent::Stretch,
            justify_content: JustifyContent::FlexStart,
            position: Position::Flow,
            z_index: 0,
            stacking_context: false,
            selection_boundary: false,
            selection_bg: None,
            selection_fg: None,
            bold: false,
            italic: false,
            underline: false,
            cursor_shape: CursorShape::Block,
            color_vars: ColorScope::default(),
            terminal_background: ResolvedColor::black(),
        }
    }
}

impl StyleDefaults {
    /// Defaults used by the permanent document root when a property is unset.
    ///
    /// The document's color variables are the root of the variable scope chain, and its declared
    /// terminal background is the root of the effective-background chain.
    pub(crate) fn root(color_vars: ColorScope, terminal_background: ResolvedColor) -> Self {
        Self {
            width: Length::Percent(100.0),
            height: Length::Percent(100.0),
            color_vars,
            terminal_background,
            ..Self::default()
        }
    }

    fn to_resolved_style(&self) -> ResolvedStyle {
        ResolvedStyle {
            width: self.width,
            height: self.height,
            padding: self.padding,
            margin: self.margin,
            border: self.border,
            border_color: self.border_color,
            half_block_edges: self.half_block_edges,
            half_block_inner_color: self.half_block_inner_color,
            half_block_outer_color: self.half_block_outer_color,
            display: self.display,
            overflow_x: self.overflow_x,
            overflow_y: self.overflow_y,
            scrollbar_show: self.scrollbar_show,
            scrollbar_hide_delay: self.scrollbar_hide_delay,
            scrollbar_fade_duration: self.scrollbar_fade_duration,
            scrollbar_charset: self.scrollbar_charset,
            scrollbar_track_color: self.scrollbar_track_color,
            scrollbar_thumb_color: self.scrollbar_thumb_color,
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
            align_content: self.align_content,
            justify_content: self.justify_content,
            position: self.position,
            z_index: self.z_index,
            stacking_context: self.stacking_context,
            selection_boundary: self.selection_boundary,
            selection_bg: self.selection_bg,
            selection_fg: self.selection_fg,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            cursor_shape: self.cursor_shape,
            color_vars: self.color_vars.clone(),
            effective_background: self.background.unwrap_or(self.terminal_background),
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
        // Colors resolve in a fixed order, because `CurrentBg` and `CurrentFg` are self-referential
        // in the two properties they are defined from. In `background`, and in a variable
        // declaration, they mean the *parent's* values — the only reading that is not circular.
        // From `color` on, they mean this node's own.
        let inherited_bg = parent.map_or(defaults.terminal_background, |p| p.effective_background);
        let inherited_fg = parent.map_or(defaults.color, |p| p.color);

        let color_vars = resolve_color_vars(data, parent, defaults, inherited_bg, inherited_fg);
        let parent_ctx = ColorContext {
            vars: &color_vars,
            current_bg: inherited_bg,
            current_fg: inherited_fg,
        };

        let background = resolve_opt(
            &data.style.background,
            parent.and_then(|p| p.background),
            defaults.background,
            &parent_ctx,
        );

        // What the node visually sits on: its own background, or whatever shows through.
        let effective_background = background.unwrap_or(inherited_bg);

        let color = resolve_color(
            &data.style.color,
            parent.map(|p| p.color),
            defaults.color,
            &ColorContext {
                current_bg: effective_background,
                ..parent_ctx
            },
        );

        // Every remaining color property sees this node's own resolved colors.
        let ctx = ColorContext {
            vars: &color_vars,
            current_bg: effective_background,
            current_fg: color,
        };

        Self {
            color_vars: color_vars.clone(),
            background,
            effective_background,
            color,
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
            border: resolve(
                &data.style.border,
                parent.map(|p| &p.border),
                &defaults.border,
            ),
            border_color: resolve_opt(
                &data.style.border_color,
                parent.and_then(|p| p.border_color),
                defaults.border_color,
                &ctx,
            ),
            half_block_edges: resolve(
                &data.style.half_block_edges,
                parent.map(|p| &p.half_block_edges),
                &defaults.half_block_edges,
            ),
            half_block_inner_color: resolve_opt(
                &data.style.half_block_inner_color,
                parent.and_then(|p| p.half_block_inner_color),
                defaults.half_block_inner_color,
                &ctx,
            ),
            half_block_outer_color: resolve_opt(
                &data.style.half_block_outer_color,
                parent.and_then(|p| p.half_block_outer_color),
                defaults.half_block_outer_color,
                &ctx,
            ),
            display: resolve(
                &data.style.display,
                parent.map(|p| &p.display),
                &defaults.display,
            ),
            overflow_x: resolve(
                &data.style.overflow_x,
                parent.map(|p| &p.overflow_x),
                &defaults.overflow_x,
            ),
            overflow_y: resolve(
                &data.style.overflow_y,
                parent.map(|p| &p.overflow_y),
                &defaults.overflow_y,
            ),
            scrollbar_show: resolve(
                &data.style.scrollbar_show,
                parent.map(|p| &p.scrollbar_show),
                &defaults.scrollbar_show,
            ),
            scrollbar_hide_delay: resolve(
                &data.style.scrollbar_hide_delay,
                parent.map(|p| &p.scrollbar_hide_delay),
                &defaults.scrollbar_hide_delay,
            ),
            scrollbar_fade_duration: resolve(
                &data.style.scrollbar_fade_duration,
                parent.map(|p| &p.scrollbar_fade_duration),
                &defaults.scrollbar_fade_duration,
            ),
            scrollbar_charset: resolve(
                &data.style.scrollbar_charset,
                parent.map(|p| &p.scrollbar_charset),
                &defaults.scrollbar_charset,
            ),
            scrollbar_track_color: resolve_opt(
                &data.style.scrollbar_track_color,
                parent.and_then(|p| p.scrollbar_track_color),
                defaults.scrollbar_track_color,
                &ctx,
            ),
            scrollbar_thumb_color: resolve_opt(
                &data.style.scrollbar_thumb_color,
                parent.and_then(|p| p.scrollbar_thumb_color),
                defaults.scrollbar_thumb_color,
                &ctx,
            ),
            opacity: resolve(
                &data.style.opacity,
                parent.map(|p| &p.opacity),
                &defaults.opacity,
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
            align_content: resolve(
                &data.style.align_content,
                parent.map(|p| &p.align_content),
                &defaults.align_content,
            ),
            justify_content: resolve(
                &data.style.justify_content,
                parent.map(|p| &p.justify_content),
                &defaults.justify_content,
            ),
            position: resolve(
                &data.style.position,
                parent.map(|p| &p.position),
                &defaults.position,
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
            selection_boundary: resolve(
                &data.style.selection_boundary,
                parent.map(|p| &p.selection_boundary),
                &defaults.selection_boundary,
            ),
            selection_bg: resolve_opt(
                &data.style.selection_bg,
                parent.and_then(|p| p.selection_bg),
                defaults.selection_bg,
                &ctx,
            ),
            selection_fg: resolve_opt(
                &data.style.selection_fg,
                parent.and_then(|p| p.selection_fg),
                defaults.selection_fg,
                &ctx,
            ),
            bold: resolve(&data.style.bold, parent.map(|p| &p.bold), &defaults.bold),
            italic: resolve(
                &data.style.italic,
                parent.map(|p| &p.italic),
                &defaults.italic,
            ),
            underline: resolve(
                &data.style.underline,
                parent.map(|p| &p.underline),
                &defaults.underline,
            ),
            cursor_shape: resolve(
                &data.style.cursor_shape,
                parent.map(|p| &p.cursor_shape),
                &defaults.cursor_shape,
            ),
        }
    }

    /// Merge a pseudo-state style on top of this resolved style.
    ///
    /// Colors in the override resolve against the node's own variable scope. A pseudo-state style
    /// cannot *declare* variables: doing so would change the scope every color on the node already
    /// resolved against, so it would mean re-resolving the whole node rather than merging onto it.
    pub(crate) fn apply_overrides(
        &mut self,
        style: &Style,
        parent: Option<&ResolvedStyle>,
        defaults: &StyleDefaults,
    ) {
        let color_vars = self.color_vars.clone();
        let inherited_bg = parent.map_or(defaults.terminal_background, |p| p.effective_background);
        let inherited_fg = parent.map_or(defaults.color, |p| p.color);

        // Background, then color, then the rest — the order a fresh resolve uses. An override that
        // changes the background changes what `CurrentBg` means for every color after it, so
        // merging in field order would resolve some of them against colors this style replaced.
        apply_opt_override(
            &mut self.background,
            &style.background,
            parent.and_then(|p| p.background),
            defaults.background,
            &ColorContext {
                vars: &color_vars,
                current_bg: inherited_bg,
                current_fg: inherited_fg,
            },
        );
        self.effective_background = self.background.unwrap_or(inherited_bg);

        apply_color_override(
            &mut self.color,
            &style.color,
            parent.map(|p| p.color),
            defaults.color,
            &ColorContext {
                vars: &color_vars,
                current_bg: self.effective_background,
                current_fg: inherited_fg,
            },
        );

        let ctx = ColorContext {
            vars: &color_vars,
            current_bg: self.effective_background,
            current_fg: self.color,
        };

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
            &mut self.border,
            &style.border,
            parent.map(|p| &p.border),
            &defaults.border,
        );
        apply_opt_override(
            &mut self.border_color,
            &style.border_color,
            parent.and_then(|p| p.border_color),
            defaults.border_color,
            &ctx,
        );
        apply_override(
            &mut self.half_block_edges,
            &style.half_block_edges,
            parent.map(|p| &p.half_block_edges),
            &defaults.half_block_edges,
        );
        apply_opt_override(
            &mut self.half_block_inner_color,
            &style.half_block_inner_color,
            parent.and_then(|p| p.half_block_inner_color),
            defaults.half_block_inner_color,
            &ctx,
        );
        apply_opt_override(
            &mut self.half_block_outer_color,
            &style.half_block_outer_color,
            parent.and_then(|p| p.half_block_outer_color),
            defaults.half_block_outer_color,
            &ctx,
        );
        apply_override(
            &mut self.display,
            &style.display,
            parent.map(|p| &p.display),
            &defaults.display,
        );
        apply_override(
            &mut self.overflow_x,
            &style.overflow_x,
            parent.map(|p| &p.overflow_x),
            &defaults.overflow_x,
        );
        apply_override(
            &mut self.overflow_y,
            &style.overflow_y,
            parent.map(|p| &p.overflow_y),
            &defaults.overflow_y,
        );
        apply_override(
            &mut self.scrollbar_show,
            &style.scrollbar_show,
            parent.map(|p| &p.scrollbar_show),
            &defaults.scrollbar_show,
        );
        apply_override(
            &mut self.scrollbar_hide_delay,
            &style.scrollbar_hide_delay,
            parent.map(|p| &p.scrollbar_hide_delay),
            &defaults.scrollbar_hide_delay,
        );
        apply_override(
            &mut self.scrollbar_fade_duration,
            &style.scrollbar_fade_duration,
            parent.map(|p| &p.scrollbar_fade_duration),
            &defaults.scrollbar_fade_duration,
        );
        apply_override(
            &mut self.scrollbar_charset,
            &style.scrollbar_charset,
            parent.map(|p| &p.scrollbar_charset),
            &defaults.scrollbar_charset,
        );
        apply_opt_override(
            &mut self.scrollbar_track_color,
            &style.scrollbar_track_color,
            parent.and_then(|p| p.scrollbar_track_color),
            defaults.scrollbar_track_color,
            &ctx,
        );
        apply_opt_override(
            &mut self.scrollbar_thumb_color,
            &style.scrollbar_thumb_color,
            parent.and_then(|p| p.scrollbar_thumb_color),
            defaults.scrollbar_thumb_color,
            &ctx,
        );
        apply_override(
            &mut self.opacity,
            &style.opacity,
            parent.map(|p| &p.opacity),
            &defaults.opacity,
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
            &mut self.align_content,
            &style.align_content,
            parent.map(|p| &p.align_content),
            &defaults.align_content,
        );
        apply_override(
            &mut self.justify_content,
            &style.justify_content,
            parent.map(|p| &p.justify_content),
            &defaults.justify_content,
        );
        apply_override(
            &mut self.position,
            &style.position,
            parent.map(|p| &p.position),
            &defaults.position,
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
            &mut self.selection_boundary,
            &style.selection_boundary,
            parent.map(|p| &p.selection_boundary),
            &defaults.selection_boundary,
        );
        apply_opt_override(
            &mut self.selection_bg,
            &style.selection_bg,
            parent.and_then(|p| p.selection_bg),
            defaults.selection_bg,
            &ctx,
        );
        apply_opt_override(
            &mut self.selection_fg,
            &style.selection_fg,
            parent.and_then(|p| p.selection_fg),
            defaults.selection_fg,
            &ctx,
        );
        apply_override(
            &mut self.bold,
            &style.bold,
            parent.map(|p| &p.bold),
            &defaults.bold,
        );
        apply_override(
            &mut self.italic,
            &style.italic,
            parent.map(|p| &p.italic),
            &defaults.italic,
        );
        apply_override(
            &mut self.underline,
            &style.underline,
            parent.map(|p| &p.underline),
            &defaults.underline,
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

/// Resolve an optional [`StyleValue<Color>`] to a concrete color.
///
/// `Set(expr)` evaluates the expression; an expression that names a variable nothing defines is
/// unresolvable and falls back to the default, so a typo'd name fails visibly rather than
/// half-applying a derivation. `Inherit` uses the parent's value or the default if there is no
/// parent. `Unset` always uses the default value.
fn resolve_opt(
    value: &StyleValue<Color>,
    parent: Option<ResolvedColor>,
    default: Option<ResolvedColor>,
    ctx: &ColorContext,
) -> Option<ResolvedColor> {
    match value {
        StyleValue::Unset => default,
        StyleValue::Inherit => parent.or(default),
        StyleValue::Set(v) => v.eval(ctx).or(default),
    }
}

/// Resolve a required [`StyleValue<Color>`] to a concrete color.
fn resolve_color(
    value: &StyleValue<Color>,
    parent: Option<ResolvedColor>,
    default: ResolvedColor,
    ctx: &ColorContext,
) -> ResolvedColor {
    match value {
        StyleValue::Unset => default,
        StyleValue::Inherit => parent.unwrap_or(default),
        StyleValue::Set(v) => v.eval(ctx).unwrap_or(default),
    }
}

/// Build a node's color variable scope.
///
/// A node's own declarations resolve against its *parent's* scope, never against each other. A
/// `HashMap` has no declaration order, so resolving them against themselves would be
/// nondeterministic — and resolving them against an already-concrete scope makes reference cycles
/// impossible to write. `--a: Var("--a").darken(0.1)` therefore means "the inherited `--a`,
/// darkened", which terminates.
fn resolve_color_vars(
    data: &NodeData,
    parent: Option<&ResolvedStyle>,
    defaults: &StyleDefaults,
    inherited_bg: ResolvedColor,
    inherited_fg: ResolvedColor,
) -> ColorScope {
    // The document's variables are the root of the chain; every other node inherits its parent's.
    let inherited = match parent {
        Some(parent) => &parent.color_vars,
        None => &defaults.color_vars,
    };

    if data.style.color_vars.is_empty() {
        return inherited.clone();
    }

    let parent_ctx = ColorContext {
        vars: inherited,
        current_bg: inherited_bg,
        current_fg: inherited_fg,
    };
    let mut scope = (**inherited).clone();
    for (name, expr) in &data.style.color_vars {
        match expr.eval(&parent_ctx) {
            Some(color) => scope.insert(name.clone(), color),
            // A declaration that does not resolve leaves the name undefined here rather than
            // falling through to the inherited value, which would hide the broken declaration
            // behind an ancestor's color.
            None => scope.remove(name),
        };
    }

    Arc::new(scope)
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
    target: &mut Option<ResolvedColor>,
    value: &StyleValue<Color>,
    parent: Option<ResolvedColor>,
    default: Option<ResolvedColor>,
    ctx: &ColorContext,
) {
    match value {
        StyleValue::Unset => {}
        StyleValue::Inherit => *target = parent.or(default),
        StyleValue::Set(v) => *target = v.eval(ctx).or(default),
    }
}

fn apply_color_override(
    target: &mut ResolvedColor,
    value: &StyleValue<Color>,
    parent: Option<ResolvedColor>,
    default: ResolvedColor,
    ctx: &ColorContext,
) {
    match value {
        StyleValue::Unset => {}
        StyleValue::Inherit => *target = parent.unwrap_or(default),
        StyleValue::Set(v) => *target = v.eval(ctx).unwrap_or(default),
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

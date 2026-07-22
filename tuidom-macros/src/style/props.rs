//! The property table — the whole vocabulary of `style!`.
//!
//! Every entry names a real setter on `tuidom::style::Style`, and a name absent from this
//! table is rejected at compile time. Adding a `Style` field means adding a line here;
//! adding a *variant* to an existing property's type means nothing at all, because variant
//! sugar resolves through the property's type rather than through a keyword list.

/// What kind of value a property takes, and so which sugar applies to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropType {
    /// `Length`: `auto`, cells, or a percentage.
    Length,
    /// `EdgeInsets`: one to four cell counts.
    EdgeInsets,
    /// `FlexGap`: one or two cell counts.
    FlexGap,
    /// `Color`: a color expression.
    Color,
    /// `Sides`: a set of edge names.
    Sides,
    /// `Border`: a charset, optionally limited to some sides.
    Border,
    /// `ScrollbarCharset`: a named charset.
    ScrollbarCharset,
    /// `Position`: `flow`, or `absolute(x, y)`.
    Position,
    /// `Duration`: a time literal.
    Duration,
    /// A fieldless enum, named here so a bare ident can sugar to one of its variants.
    Enum(&'static str),
    /// A plain `bool`.
    Bool,
    /// A plain numeric field, named by the type a bare literal should take.
    Number(&'static str),
}

/// One row of the property table.
pub(crate) struct Prop {
    /// The name written in `style!` source.
    pub name: &'static str,
    /// The `Style` method a set value calls.
    pub setter: &'static str,
    /// Base names of the `inherit_*` / `unset_*` methods. More than one when the property
    /// is a shorthand over several fields.
    pub states: &'static [&'static str],
    /// Which sugar applies to this property's values.
    pub ty: PropType,
}

macro_rules! prop {
    ($name:ident, $ty:expr) => {
        Prop {
            name: stringify!($name),
            setter: stringify!($name),
            states: &[stringify!($name)],
            ty: $ty,
        }
    };
    ($name:ident, $ty:expr, states: [$($state:ident),+]) => {
        Prop {
            name: stringify!($name),
            setter: stringify!($name),
            states: &[$(stringify!($state)),+],
            ty: $ty,
        }
    };
}

/// Every property `style!` accepts.
pub(crate) const PROPS: &[Prop] = &[
    prop!(width, PropType::Length),
    prop!(height, PropType::Length),
    prop!(padding, PropType::EdgeInsets),
    prop!(margin, PropType::EdgeInsets),
    prop!(border, PropType::Border),
    prop!(border_color, PropType::Color),
    prop!(half_block_edges, PropType::Sides),
    prop!(half_block_inner_color, PropType::Color),
    prop!(half_block_outer_color, PropType::Color),
    prop!(bold, PropType::Bool),
    prop!(italic, PropType::Bool),
    prop!(underline, PropType::Bool),
    prop!(display, PropType::Enum("Display")),
    prop!(overflow_x, PropType::Enum("Overflow")),
    prop!(overflow_y, PropType::Enum("Overflow")),
    // The one shorthand the engine itself provides: `overflow` sets both axes, so its
    // `inherit` and `unset` forms have to reach both.
    prop!(overflow, PropType::Enum("Overflow"), states: [overflow_x, overflow_y]),
    prop!(scrollbar_show, PropType::Enum("ScrollbarShow")),
    prop!(scrollbar_hide_delay, PropType::Duration),
    prop!(scrollbar_fade_duration, PropType::Duration),
    prop!(scrollbar_charset, PropType::ScrollbarCharset),
    prop!(scrollbar_track_color, PropType::Color),
    prop!(scrollbar_thumb_color, PropType::Color),
    prop!(opacity, PropType::Number("f64")),
    prop!(color, PropType::Color),
    prop!(background, PropType::Color),
    prop!(flex_direction, PropType::Enum("FlexDirection")),
    prop!(flex_basis, PropType::Length),
    prop!(flex_grow, PropType::Number("f32")),
    prop!(flex_shrink, PropType::Number("f32")),
    prop!(flex_wrap, PropType::Enum("FlexWrap")),
    prop!(gap, PropType::FlexGap),
    prop!(align_self, PropType::Enum("AlignSelf")),
    prop!(align_items, PropType::Enum("AlignItems")),
    prop!(align_content, PropType::Enum("AlignContent")),
    prop!(justify_content, PropType::Enum("JustifyContent")),
    prop!(position, PropType::Position),
    prop!(z_index, PropType::Number("i32")),
    prop!(stacking_context, PropType::Bool),
    prop!(selection_boundary, PropType::Bool),
    prop!(selection_bg, PropType::Color),
    prop!(selection_fg, PropType::Color),
    prop!(cursor_shape, PropType::Enum("CursorShape")),
];

/// Look a property up by the name written in source.
pub(crate) fn lookup(name: &str) -> Option<&'static Prop> {
    PROPS.iter().find(|prop| prop.name == name)
}

/// The known property closest to a misspelling, if one is close enough to be worth naming.
pub(crate) fn nearest(name: &str) -> Option<&'static str> {
    let limit = (name.len() / 2).max(2);
    PROPS
        .iter()
        .map(|prop| (distance(prop.name, name), prop.name))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name)
}

/// Levenshtein distance, over bytes — every property name is ASCII.
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != right);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

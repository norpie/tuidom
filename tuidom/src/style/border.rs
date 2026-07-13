use crate::style::EdgeInsets;

/// The eight characters that draw a box.
///
/// The charset is the primitive: the named presets ([`single`](BorderCharset::single),
/// [`double`](BorderCharset::double), and friends) are ordinary constructors, so a custom
/// look is built the same way a shipped one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderCharset {
    /// Character drawn along the top edge.
    pub top: char,
    /// Character drawn along the right edge.
    pub right: char,
    /// Character drawn along the bottom edge.
    pub bottom: char,
    /// Character drawn along the left edge.
    pub left: char,
    /// Character drawn in the top-left corner.
    pub top_left: char,
    /// Character drawn in the top-right corner.
    pub top_right: char,
    /// Character drawn in the bottom-left corner.
    pub bottom_left: char,
    /// Character drawn in the bottom-right corner.
    pub bottom_right: char,
}

impl BorderCharset {
    /// Thin single lines: `┌─┐`.
    pub const fn single() -> Self {
        Self {
            top: '─',
            right: '│',
            bottom: '─',
            left: '│',
            top_left: '┌',
            top_right: '┐',
            bottom_left: '└',
            bottom_right: '┘',
        }
    }

    /// Double lines: `╔═╗`.
    pub const fn double() -> Self {
        Self {
            top: '═',
            right: '║',
            bottom: '═',
            left: '║',
            top_left: '╔',
            top_right: '╗',
            bottom_left: '╚',
            bottom_right: '╝',
        }
    }

    /// Single lines with rounded corners: `╭─╮`.
    pub const fn rounded() -> Self {
        Self {
            top_left: '╭',
            top_right: '╮',
            bottom_left: '╰',
            bottom_right: '╯',
            ..Self::single()
        }
    }

    /// Heavy lines: `┏━┓`.
    pub const fn thick() -> Self {
        Self {
            top: '━',
            right: '┃',
            bottom: '━',
            left: '┃',
            top_left: '┏',
            top_right: '┓',
            bottom_left: '┗',
            bottom_right: '┛',
        }
    }

    /// Plain ASCII, for terminals without box-drawing characters: `+-+`.
    pub const fn ascii() -> Self {
        Self {
            top: '-',
            right: '|',
            bottom: '-',
            left: '|',
            top_left: '+',
            top_right: '+',
            bottom_left: '+',
            bottom_right: '+',
        }
    }
}

/// Which sides of a node an edge treatment is drawn on.
///
/// Presence, not width: every edge treatment tuidom draws — a border, a half-block edge — is
/// either on a side or not, so there is nothing else to say about a side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sides {
    /// Whether the top edge is drawn.
    pub top: bool,
    /// Whether the right edge is drawn.
    pub right: bool,
    /// Whether the bottom edge is drawn.
    pub bottom: bool,
    /// Whether the left edge is drawn.
    pub left: bool,
}

impl Sides {
    /// Every side drawn.
    pub const ALL: Self = Self {
        top: true,
        right: true,
        bottom: true,
        left: true,
    };

    /// No side drawn.
    pub const NONE: Self = Self {
        top: false,
        right: false,
        bottom: false,
        left: false,
    };

    /// Choose each side explicitly.
    pub const fn new(top: bool, right: bool, bottom: bool, left: bool) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Whether any side is drawn.
    pub const fn any(self) -> bool {
        self.top || self.right || self.bottom || self.left
    }

    /// One cell on each drawn side.
    ///
    /// Plain arithmetic, not a statement about space: what a side costs is the edge treatment's
    /// business, and a half-block edge costs nothing.
    pub(crate) const fn one_cell_insets(self) -> EdgeInsets {
        EdgeInsets::new(
            self.top as u16,
            self.right as u16,
            self.bottom as u16,
            self.left as u16,
        )
    }
}

/// A node's border: one charset, plus which sides it is drawn on.
///
/// One charset per node rather than one per side, because corners are drawn from the charset
/// and a double-top/single-left corner has no coherent character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Border {
    /// Characters used to draw the border.
    pub charset: BorderCharset,
    /// Which sides are drawn.
    pub sides: Sides,
}

impl Default for Border {
    fn default() -> Self {
        Self::none()
    }
}

impl Border {
    /// A closed box drawn with `charset`.
    pub const fn new(charset: BorderCharset) -> Self {
        Self {
            charset,
            sides: Sides::ALL,
        }
    }

    /// No border.
    ///
    /// This is a value, not the absence of one: a focus or disabled style needs it to
    /// positively remove a border, since an unset style property means "do not override".
    pub const fn none() -> Self {
        Self {
            charset: BorderCharset::single(),
            sides: Sides::NONE,
        }
    }

    /// Draw only the given sides.
    pub const fn with_sides(mut self, sides: Sides) -> Self {
        self.sides = sides;
        self
    }

    /// The space the drawn sides take out of the node: one cell each.
    ///
    /// A border is never thicker than a cell, so presence is the only degree of freedom. This
    /// belongs to the border rather than to [`Sides`], because a half-block edge is drawn on
    /// sides too and takes no space at all.
    pub(crate) const fn insets(self) -> EdgeInsets {
        self.sides.one_cell_insets()
    }
}

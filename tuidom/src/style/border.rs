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

/// Which sides of a border are drawn.
///
/// A terminal border is always exactly one cell thick, so per-side control means presence,
/// not width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderSides {
    /// Whether the top edge is drawn.
    pub top: bool,
    /// Whether the right edge is drawn.
    pub right: bool,
    /// Whether the bottom edge is drawn.
    pub bottom: bool,
    /// Whether the left edge is drawn.
    pub left: bool,
}

impl BorderSides {
    /// Every side drawn — a closed box.
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
    pub sides: BorderSides,
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
            sides: BorderSides::ALL,
        }
    }

    /// No border.
    ///
    /// This is a value, not the absence of one: a focus or disabled style needs it to
    /// positively remove a border, since an unset style property means "do not override".
    pub const fn none() -> Self {
        Self {
            charset: BorderCharset::single(),
            sides: BorderSides::NONE,
        }
    }

    /// Draw only the given sides.
    pub const fn with_sides(mut self, sides: BorderSides) -> Self {
        self.sides = sides;
        self
    }
}

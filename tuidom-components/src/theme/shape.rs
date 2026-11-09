use tuidom::style::BorderCharset;

/// The non-color half of a theme: how components frame, space, and mark themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Characters every bordered component draws with. The terminal stand-in for a corner
    /// radius.
    pub border: BorderCharset,
    /// How buttons are framed.
    pub button: Surface,
    /// How fields are framed: the text input, the select trigger, and the list a select or menu
    /// opens.
    pub field: Surface,
    /// How containers are framed: panels, modals, and toasts.
    pub container: Surface,
    /// How much padding sits inside controls and containers.
    pub density: Density,
    /// How a focused component shows it.
    pub focus: FocusMark,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            border: BorderCharset::rounded(),
            button: Surface::Bordered,
            field: Surface::Bordered,
            container: Surface::Bordered,
            density: Density::Comfortable,
            focus: FocusMark::Ring,
        }
    }
}

/// How a surface is framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Surface {
    /// A border in the theme's charset, which takes a cell on each side.
    #[default]
    Bordered,
    /// Half-block edges, which frame the fill without taking layout space.
    HalfBlock,
    /// The fill alone.
    Flat,
}

/// How much padding sits inside controls and containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    /// The least padding that keeps content readable.
    Compact,
    /// A cell more on the long axis.
    #[default]
    Comfortable,
}

/// How a focused component shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusMark {
    /// Its border or edge takes `--ring`.
    #[default]
    Ring,
    /// Its label is underlined.
    Underline,
    /// Its colors swap.
    Reverse,
}

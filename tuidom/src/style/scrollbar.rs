//! Scrollbar visibility and drawing characters.

/// When a scroll container draws its scrollbar.
///
/// Every mode is gated on the axis actually being scrollable: a container whose content
/// fits draws no bar regardless.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarShow {
    /// Show the bar whenever the axis is scrollable.
    #[default]
    Always,
    /// Show the bar while the container or one of its descendants holds focus.
    ///
    /// Hover focuses under the hover-as-focus policy, so this is also "when hovered".
    WhenFocused,
    /// Never show a bar.
    Never,
}

/// The characters a scrollbar is drawn with, per orientation.
///
/// The charset is the primitive — [`block`](Self::block) and
/// [`half_block`](Self::half_block) are named constructors, not special cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarCharset {
    /// Track character of a vertical bar.
    pub vertical_track: char,
    /// Thumb character of a vertical bar.
    pub vertical_thumb: char,
    /// Track character of a horizontal bar.
    pub horizontal_track: char,
    /// Thumb character of a horizontal bar.
    pub horizontal_thumb: char,
}

impl ScrollbarCharset {
    /// Full-block thumb on a shaded track.
    pub fn block() -> Self {
        Self {
            vertical_track: '░',
            vertical_thumb: '█',
            horizontal_track: '░',
            horizontal_thumb: '█',
        }
    }

    /// Half-block thumb on a line track, for a thinner look.
    pub fn half_block() -> Self {
        Self {
            vertical_track: '│',
            vertical_thumb: '▐',
            horizontal_track: '─',
            horizontal_thumb: '▄',
        }
    }
}

impl Default for ScrollbarCharset {
    fn default() -> Self {
        Self::block()
    }
}

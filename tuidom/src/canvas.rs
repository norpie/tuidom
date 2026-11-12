//! Cells an app draws itself, shown by a canvas node.
//!
//! The app fills a [`CanvasBuffer`] on its own task and hands it to
//! [`Document::set_canvas`](crate::Document::set_canvas). Painting copies the stored buffer, so
//! no app code runs while a frame is drawn.

use std::fmt;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::render::grid::{CellContent, Glyph};
use crate::style::ResolvedColor;

/// How a canvas cell is drawn. An unset color shows whatever is behind the canvas: the node's
/// own background, or what it sits on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CanvasStyle {
    /// Glyph color.
    pub fg: Option<ResolvedColor>,
    /// Cell background.
    pub bg: Option<ResolvedColor>,
    /// Bold glyph.
    pub bold: bool,
    /// Italic glyph.
    pub italic: bool,
    /// Underlined glyph.
    pub underline: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CanvasCell {
    pub content: CellContent,
    pub style: CanvasStyle,
}

impl CanvasCell {
    const EMPTY: Self = Self {
        content: CellContent::Empty,
        style: CanvasStyle {
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
        },
    };
}

/// A grid of cells to show in a canvas node, in terminal cells.
///
/// A wide grapheme takes two cells. One that would not fit before the right edge is not
/// written, and writing over either half of one empties the other half.
#[derive(Clone, Default, PartialEq)]
pub struct CanvasBuffer {
    width: u16,
    height: u16,
    cells: Vec<CanvasCell>,
}

impl fmt::Debug for CanvasBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl CanvasBuffer {
    /// An empty buffer of `width` by `height` cells.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![CanvasCell::EMPTY; usize::from(width) * usize::from(height)],
        }
    }

    /// Width in cells.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Write the first grapheme of `symbol` at `x`, `y`.
    ///
    /// Nothing is written outside the buffer, for an empty symbol, or for one with no width,
    /// such as a control character.
    pub fn set(&mut self, x: u16, y: u16, symbol: &str, style: CanvasStyle) {
        if let Some(grapheme) = symbol.graphemes(true).next() {
            self.put(x, y, grapheme, style);
        }
    }

    /// Write `text` from `x`, `y` rightward, one grapheme per cell or two for a wide one,
    /// stopping at the right edge. Never wraps; graphemes with no width are skipped.
    ///
    /// Returns the column after the last grapheme written, or `x` if none was.
    pub fn text(&mut self, x: u16, y: u16, text: &str, style: CanvasStyle) -> u16 {
        if y >= self.height {
            return x;
        }
        let mut column = x;
        for grapheme in text.graphemes(true) {
            let width = grapheme_width(grapheme);
            if width == 0 {
                continue;
            }
            if u32::from(column) + u32::from(width) > u32::from(self.width) {
                break;
            }
            self.put(column, y, grapheme, style);
            column += u16::from(width);
        }
        column
    }

    /// The grapheme and style at `x`, `y`, or `None` outside the buffer.
    ///
    /// The grapheme is empty for a cell nothing was written to and for the second half of a
    /// wide grapheme.
    pub fn get(&self, x: u16, y: u16) -> Option<(&str, CanvasStyle)> {
        let cell = self.cells.get(self.index(x, y)?)?;
        let symbol = match &cell.content {
            CellContent::Glyph { text, .. } => text.as_str(),
            CellContent::Empty | CellContent::WideContinuation => "",
        };
        Some((symbol, cell.style))
    }

    /// Empty every cell.
    pub fn clear(&mut self) {
        self.cells.fill(CanvasCell::EMPTY);
    }

    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn cell(&self, x: u16, y: u16) -> Option<&CanvasCell> {
        self.cells.get(self.index(x, y)?)
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    fn put(&mut self, x: u16, y: u16, grapheme: &str, style: CanvasStyle) {
        let width = grapheme_width(grapheme);
        if width == 0 || y >= self.height || u32::from(x) + u32::from(width) > u32::from(self.width)
        {
            return;
        }
        for column in x..x + u16::from(width) {
            self.break_wide_at(column, y);
        }
        let Some(head) = self.index(x, y) else {
            return;
        };
        self.cells[head] = CanvasCell {
            content: CellContent::Glyph {
                text: Glyph::new(grapheme),
                width,
            },
            style,
        };
        if width == 2 {
            self.cells[head + 1] = CanvasCell {
                content: CellContent::WideContinuation,
                style,
            };
        }
    }

    /// Empty the other half of a wide grapheme that `x`, `y` is part of, so writing over one
    /// half never leaves the other half behind.
    fn break_wide_at(&mut self, x: u16, y: u16) {
        let Some(index) = self.index(x, y) else {
            return;
        };
        let other = match self.cells[index].content {
            CellContent::WideContinuation => x.checked_sub(1),
            CellContent::Glyph { width: 2, .. } => x.checked_add(1),
            _ => None,
        };
        if let Some(other) = other.and_then(|other| self.index(other, y)) {
            self.cells[other].content = CellContent::Empty;
        }
    }
}

fn grapheme_width(grapheme: &str) -> u8 {
    if grapheme.chars().any(char::is_control) {
        return 0;
    }
    UnicodeWidthStr::width(grapheme).min(2) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buffer: &CanvasBuffer, y: u16) -> Vec<&str> {
        (0..buffer.width())
            .map(|x| buffer.get(x, y).unwrap().0)
            .collect()
    }

    fn red() -> CanvasStyle {
        CanvasStyle {
            fg: Some(ResolvedColor::from_srgb(255, 0, 0)),
            ..CanvasStyle::default()
        }
    }

    #[test]
    fn set_writes_the_first_grapheme_with_its_style() {
        let mut buffer = CanvasBuffer::new(3, 1);
        buffer.set(1, 0, "e\u{301}x", red());
        assert_eq!(buffer.get(1, 0), Some(("e\u{301}", red())));
        assert_eq!(row(&buffer, 0), ["", "e\u{301}", ""]);
    }

    #[test]
    fn writes_outside_the_buffer_or_without_width_are_ignored() {
        let mut buffer = CanvasBuffer::new(2, 1);
        buffer.set(2, 0, "a", red());
        buffer.set(0, 1, "a", red());
        buffer.set(0, 0, "\n", red());
        buffer.set(0, 0, "", red());
        assert_eq!(buffer, CanvasBuffer::new(2, 1));
        assert_eq!(buffer.get(2, 0), None);
    }

    #[test]
    fn a_wide_grapheme_takes_two_cells_and_is_dropped_at_the_edge() {
        let mut buffer = CanvasBuffer::new(3, 1);
        buffer.set(0, 0, "日", red());
        buffer.set(2, 0, "本", red());
        assert_eq!(row(&buffer, 0), ["日", "", ""]);
        assert_eq!(buffer.get(1, 0), Some(("", red())));
    }

    #[test]
    fn writing_over_half_a_wide_grapheme_empties_the_other_half() {
        let mut buffer = CanvasBuffer::new(4, 1);
        buffer.set(0, 0, "日", red());
        buffer.set(1, 0, "a", CanvasStyle::default());
        assert_eq!(row(&buffer, 0), ["", "a", "", ""]);

        buffer.set(2, 0, "日", red());
        buffer.set(2, 0, "b", CanvasStyle::default());
        assert_eq!(row(&buffer, 0), ["", "a", "b", ""]);

        buffer.set(2, 0, "日", red());
        buffer.set(1, 0, "本", red());
        assert_eq!(row(&buffer, 0), ["", "本", "", ""]);
        assert!(matches!(
            buffer.cell(3, 0).unwrap().content,
            CellContent::Empty
        ));
    }

    #[test]
    fn text_stops_at_the_edge_and_returns_the_next_column() {
        let mut buffer = CanvasBuffer::new(5, 2);
        assert_eq!(buffer.text(1, 0, "a\tb日本", red()), 5);
        assert_eq!(row(&buffer, 0), ["", "a", "b", "日", ""]);
        assert_eq!(buffer.text(0, 1, "", red()), 0);
        assert_eq!(buffer.text(0, 2, "a", red()), 0);
    }

    #[test]
    fn clear_empties_every_cell() {
        let mut buffer = CanvasBuffer::new(2, 2);
        buffer.text(0, 0, "ab", red());
        buffer.clear();
        assert_eq!(buffer, CanvasBuffer::new(2, 2));
    }
}

//! Cell buffer — the 2D grid representing the virtual screen.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::style::color::Rgb;
use crate::style::{Border, Sides};

/// One side of a cell's edge treatment, used to pick a half-block glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// Text content stored in a single terminal cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellContent {
    /// Empty cell content. Rendered as a space.
    Empty,
    /// A grapheme cluster starting at this cell.
    Glyph {
        /// Grapheme cluster text.
        text: String,
        /// Terminal cell width. Currently 1 or 2.
        width: u8,
    },
    /// Second cell occupied by a width-2 glyph.
    WideContinuation,
}

impl CellContent {
    fn terminal_text(&self) -> &str {
        match self {
            Self::Empty => " ",
            Self::Glyph { text, .. } => text,
            Self::WideContinuation => "",
        }
    }

    fn width(&self) -> u8 {
        match self {
            Self::Glyph { width, .. } => *width,
            _ => 1,
        }
    }
}

/// A rectangular region in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GridRect {
    /// Left edge in terminal cells. May be negative when the region starts offscreen.
    pub x: i32,
    /// Top edge in terminal cells. May be negative when the region starts offscreen.
    pub y: i32,
    /// Width in terminal cells.
    pub width: u16,
    /// Height in terminal cells.
    pub height: u16,
}

/// A paint clip region with independently bounded axes.
///
/// Scroll and clip containers bound their descendants' painting per axis — a node with
/// `overflow_y: Scroll` and `overflow_x: Visible` clips rows but lets columns spill — so
/// the identity element is "unbounded on both axes", not a grid-sized rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipRect {
    /// Leftmost paintable column (inclusive).
    pub left: i64,
    /// Topmost paintable row (inclusive).
    pub top: i64,
    /// Rightmost paintable column (exclusive).
    pub right: i64,
    /// Bottommost paintable row (exclusive).
    pub bottom: i64,
}

impl ClipRect {
    /// The identity clip: nothing is clipped.
    pub const UNBOUNDED: Self = Self {
        left: i64::MIN,
        top: i64::MIN,
        right: i64::MAX,
        bottom: i64::MAX,
    };

    /// Whether nothing can paint inside this clip.
    pub fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    /// Bound the horizontal axis to `[left, right)`, keeping the tighter of the two.
    pub fn bound_x(self, left: i64, right: i64) -> Self {
        Self {
            left: self.left.max(left),
            right: self.right.min(right),
            ..self
        }
    }

    /// Bound the vertical axis to `[top, bottom)`, keeping the tighter of the two.
    pub fn bound_y(self, top: i64, bottom: i64) -> Self {
        Self {
            top: self.top.max(top),
            bottom: self.bottom.min(bottom),
            ..self
        }
    }

    /// Whether a point lies inside the clip.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Whether a rectangle lies entirely outside the clip, so nothing of it can paint.
    pub fn excludes(&self, x: i32, y: i32, width: u16, height: u16) -> bool {
        let left = i64::from(x);
        let top = i64::from(y);
        let right = left + i64::from(width);
        let bottom = top + i64::from(height);
        right <= self.left || left >= self.right || bottom <= self.top || top >= self.bottom
    }

    /// Whether the clip covers an entire `width` × `height` grid.
    pub fn covers_grid(&self, width: u16, height: u16) -> bool {
        self.left <= 0
            && self.top <= 0
            && self.right >= i64::from(width)
            && self.bottom >= i64::from(height)
    }
}

/// Terminal text attributes carried by a cell's glyph.
///
/// Packed here — unlike on `Style`, where they are three separate properties — because
/// nothing merges at the cell level: attributes belong to the glyph, so they are replaced
/// or cleared along with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CellAttrs {
    /// Bold / increased intensity.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
}

/// A single terminal character position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    /// Display content occupying this terminal cell.
    pub content: CellContent,
    /// Foreground color. `None` means terminal default.
    pub fg: Option<Rgb>,
    /// Background color. `None` means terminal default.
    pub bg: Option<Rgb>,
    /// Terminal attributes for this cell's glyph.
    pub attrs: CellAttrs,
}

impl Cell {
    /// Create an empty cell (space, no colors).
    pub fn empty() -> Self {
        Self {
            content: CellContent::Empty,
            fg: None,
            bg: None,
            attrs: CellAttrs::default(),
        }
    }

    /// Create an empty cell with a background color.
    pub fn empty_with_bg(bg: Rgb) -> Self {
        Self {
            content: CellContent::Empty,
            fg: None,
            bg: Some(bg),
            attrs: CellAttrs::default(),
        }
    }

    /// Text to print for this cell when flushing to the terminal.
    pub fn terminal_text(&self) -> &str {
        self.content.terminal_text()
    }

    /// Whether this cell is the continuation of a wide glyph.
    pub fn is_wide_continuation(&self) -> bool {
        matches!(self.content, CellContent::WideContinuation)
    }

    /// Width occupied by this cell's terminal text.
    pub fn content_width(&self) -> u8 {
        self.content.width()
    }
}

/// Blend `src` over `dst` using node opacity combined with source color alpha.
fn blend_cell(
    dst: &Cell,
    src: &Cell,
    opacity: f64,
    replace_content: bool,
    default_bg: Rgb,
) -> Cell {
    let bg = blend_color(dst.bg, src.bg, opacity, default_bg);
    Cell {
        content: if replace_content {
            src.content.clone()
        } else {
            dst.content.clone()
        },
        fg: blend_fg(dst.fg, src.fg, opacity, bg.or(dst.bg), default_bg),
        bg,
        // Attributes travel with the glyph: a translucent fill that preserves the destination
        // text preserves its attributes, and one that replaces the text takes the source's.
        attrs: if replace_content {
            src.attrs
        } else {
            dst.attrs
        },
    }
}

/// Blend a source foreground color over a destination foreground color.
///
/// When the destination is transparent (None), fades toward the cell's background color — or, if
/// the cell has none either, toward the terminal background the document declares.
fn blend_fg(
    dst: Option<Rgb>,
    src: Option<Rgb>,
    opacity: f64,
    cell_bg: Option<Rgb>,
    default_bg: Rgb,
) -> Option<Rgb> {
    match (dst, src) {
        (None, None) => None,
        (_, Some(s)) if effective_alpha(s, opacity) <= 0.0 => dst,
        (None, Some(s)) => {
            let alpha = effective_alpha(s, opacity);
            let target = cell_bg.unwrap_or(default_bg);
            Some(Rgb {
                r: lerp_u8(target.r, s.r, alpha),
                g: lerp_u8(target.g, s.g, alpha),
                b: lerp_u8(target.b, s.b, alpha),
                a: 255,
            })
        }
        (Some(d), None) => Some(d),
        (Some(d), Some(s)) => {
            let alpha = effective_alpha(s, opacity);
            Some(Rgb {
                r: lerp_u8(d.r, s.r, alpha),
                g: lerp_u8(d.g, s.g, alpha),
                b: lerp_u8(d.b, s.b, alpha),
                a: 255,
            })
        }
    }
}

/// Blend a source color over a destination color (for backgrounds).
///
/// An unpainted destination has no color of its own, so a translucent source fades toward the
/// terminal background the document declares.
fn blend_color(dst: Option<Rgb>, src: Option<Rgb>, opacity: f64, default_bg: Rgb) -> Option<Rgb> {
    match (dst, src) {
        (None, None) => None,
        (_, Some(s)) if effective_alpha(s, opacity) <= 0.0 => dst,
        (None, Some(s)) => {
            let alpha = effective_alpha(s, opacity);
            Some(Rgb {
                r: lerp_u8(default_bg.r, s.r, alpha),
                g: lerp_u8(default_bg.g, s.g, alpha),
                b: lerp_u8(default_bg.b, s.b, alpha),
                a: 255,
            })
        }
        (Some(d), None) => Some(d),
        (Some(d), Some(s)) => {
            let alpha = effective_alpha(s, opacity);
            Some(Rgb {
                r: lerp_u8(d.r, s.r, alpha),
                g: lerp_u8(d.g, s.g, alpha),
                b: lerp_u8(d.b, s.b, alpha),
                a: 255,
            })
        }
    }
}

fn effective_alpha(color: Rgb, opacity: f64) -> f64 {
    clamp_alpha(opacity) * (color.a as f64 / 255.0)
}

/// Interpolate between two channel values, rounding half away from zero.
///
/// Spelled out rather than `f64::round`, which is an out-of-line software float call
/// (it has to handle exponents and NaN this domain cannot produce) and runs once per
/// color channel of every blended cell — a large share of the cost of a translucent
/// fill. `t` is clamped to 0..=1 between two `u8`s, so the value is in 0..=255: the
/// cast truncates to the floor, and the remainder is exact, so comparing it against
/// 0.5 reproduces `round` bit for bit. Note that the shorter `(v + 0.5) as u8` does
/// *not*: one ULP below a .5 boundary that addition rounds up to the boundary itself.
fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let value = a as f64 + (b as f64 - a as f64) * t;
    let floored = value as u8;
    if value - floored as f64 >= 0.5 {
        floored.saturating_add(1)
    } else {
        floored
    }
}

fn clamp_alpha(alpha: f64) -> f64 {
    if alpha.is_nan() {
        0.0
    } else {
        alpha.clamp(0.0, 1.0)
    }
}

/// Horizontal cell span touched in one grid row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TouchedSpan {
    /// First touched cell, inclusive.
    pub start: usize,
    /// Last touched cell, exclusive.
    pub end: usize,
}

impl TouchedSpan {
    fn include(&mut self, start: usize, end: usize) {
        self.start = self.start.min(start);
        self.end = self.end.max(end);
    }
}

/// The terminal background a grid assumes until the document declares otherwise.
const BLACK: Rgb = Rgb {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

/// A 2D buffer of [`Cell`]s representing a single frame's screen state.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    /// Row-major cell storage: cells[row][col].
    pub cells: Vec<Vec<Cell>>,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
    /// Horizontal spans touched by paint operations after the frame base clear.
    touched_spans: Vec<Option<TouchedSpan>>,
    /// The terminal background color assumed when blending onto an unpainted cell.
    default_bg: Rgb,
    /// The active clip: writes outside it are dropped. Scroll and clip containers set it
    /// per painted node so clipping happens once, at the cell-write level, for every kind
    /// of paint — fills, text, borders, and half-block edges alike.
    clip: ClipRect,
}

impl Grid {
    /// Create a new grid filled with empty cells.
    pub fn new(width: u16, height: u16) -> Self {
        let cells = vec![vec![Cell::empty(); width as usize]; height as usize];
        Self {
            cells,
            width,
            height,
            touched_spans: vec![None; height as usize],
            default_bg: BLACK,
            clip: ClipRect::UNBOUNDED,
        }
    }

    /// Set the clip for subsequent writes. Painting outside it is dropped.
    pub fn set_clip(&mut self, clip: ClipRect) {
        self.clip = clip;
    }

    /// Reset the clip so writes are bounded only by the grid.
    pub fn clear_clip(&mut self) {
        self.clip = ClipRect::UNBOUNDED;
    }

    /// The writable cell bounds: the grid intersected with the active clip, as
    /// `(left, top, right, bottom)` with exclusive right/bottom.
    fn writable_bounds(&self) -> (i64, i64, i64, i64) {
        (
            self.clip.left.max(0),
            self.clip.top.max(0),
            self.clip.right.min(i64::from(self.width)),
            self.clip.bottom.min(i64::from(self.height)),
        )
    }

    /// Set the terminal background color assumed when blending.
    ///
    /// A translucent color painted onto a cell with no background has to fade toward *something*,
    /// and the terminal's real background is unknowable — so the document declares it. This only
    /// feeds the blending math: an unpainted cell still emits the terminal default, so the user's
    /// real background keeps showing through whatever is declared here.
    pub fn set_default_background(&mut self, bg: Rgb) {
        self.default_bg = bg;
    }

    /// Reset all cells to empty terminal-default state while preserving allocation.
    pub fn clear(&mut self) {
        self.touched_spans.fill(None);
        for row in &mut self.cells {
            for cell in row {
                cell.content = CellContent::Empty;
                cell.fg = None;
                cell.bg = None;
                cell.attrs = CellAttrs::default();
            }
        }
    }

    /// Reset all cells to empty content with a shared opaque background.
    pub fn clear_with_bg(&mut self, bg: Rgb) {
        self.touched_spans.fill(None);
        for row in &mut self.cells {
            for cell in row {
                cell.content = CellContent::Empty;
                cell.fg = None;
                cell.bg = Some(bg);
                cell.attrs = CellAttrs::default();
            }
        }
    }

    /// Horizontal spans touched by paint operations after the frame base clear.
    pub fn touched_spans(&self) -> &[Option<TouchedSpan>] {
        &self.touched_spans
    }

    /// Mark one row span as touched by direct grid mutation.
    pub fn touch_span(&mut self, row: usize, start: usize, end: usize) {
        if row >= self.touched_spans.len() || start >= end {
            return;
        }

        let span = TouchedSpan {
            start: start.min(self.width as usize),
            end: end.min(self.width as usize),
        };
        if span.start >= span.end {
            return;
        }

        match &mut self.touched_spans[row] {
            Some(existing) => existing.include(span.start, span.end),
            slot @ None => *slot = Some(span),
        }
    }

    /// Fill a rectangular region with a cell value, blending by `alpha`.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u16, h: u16, cell: Cell, alpha: f64) -> usize {
        let Some((x_start, y_start, x_end, y_end)) = self.clip_rect(x, y, w, h) else {
            return 0;
        };

        let alpha = clamp_alpha(alpha);
        if alpha <= 0.0 {
            return 0;
        }

        self.touch_rect(x_start, x_end, y_start, y_end);

        if alpha >= 1.0 && matches!(cell.content, CellContent::Empty) {
            if let Some(bg) = cell.bg {
                if bg.a == 255 {
                    return self.fill_opaque_empty_bg_rect(x_start, y_start, x_end, y_end, bg);
                }
            }
        }

        let replaces_content = !matches!(cell.content, CellContent::Empty)
            || cell.bg.is_some_and(|bg| effective_alpha(bg, alpha) >= 1.0);
        for row in y_start..y_end {
            for col in x_start..x_end {
                if replaces_content {
                    self.clear_text_span_at(row, col);
                }
                let dst = &self.cells[row][col];
                self.cells[row][col] =
                    blend_cell(dst, &cell, alpha, replaces_content, self.default_bg);
            }
        }

        (x_end - x_start) * (y_end - y_start)
    }

    fn touch_rect(&mut self, x_start: usize, x_end: usize, y_start: usize, y_end: usize) {
        let end = y_end.min(self.touched_spans.len());
        for row in y_start..end {
            self.touch_span(row, x_start, x_end);
        }
    }

    fn fill_opaque_empty_bg_rect(
        &mut self,
        x_start: usize,
        y_start: usize,
        x_end: usize,
        y_end: usize,
        bg: Rgb,
    ) -> usize {
        for row in y_start..y_end {
            for col in x_start..x_end {
                if !matches!(self.cells[row][col].content, CellContent::Empty) {
                    self.clear_text_span_at(row, col);
                }

                let cell = &mut self.cells[row][col];
                cell.content = CellContent::Empty;
                cell.fg = None;
                cell.bg = Some(bg);
                cell.attrs = CellAttrs::default();
            }
        }

        (x_end - x_start) * (y_end - y_start)
    }

    /// Write one line of text at a position, clipped to the screen width.
    /// Bg is left as-is (assumes the background was already filled by `fill_rect`).
    #[cfg(test)]
    pub fn write_text(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) -> usize {
        if y < 0 || y >= self.height as i32 {
            return 0;
        }

        let line = text.lines().next().unwrap_or("");
        let (bound_left, _, bound_right, _) = self.writable_bounds();
        self.write_text_line_clipped(
            x,
            y as usize,
            bound_left,
            bound_right,
            line,
            fg,
            alpha,
            attrs,
        )
    }

    /// Write multiline text clipped to a rectangular region.
    /// Bg is left as-is (assumes the background was already filled by `fill_rect`).
    pub fn write_text_clipped(
        &mut self,
        rect: GridRect,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) -> usize {
        if rect.width == 0 || rect.height == 0 {
            return 0;
        }

        let (bound_left, bound_top, bound_right, bound_bottom) = self.writable_bounds();

        let rect_top = rect.y as i64;
        let rect_bottom = rect_top + i64::from(rect.height);
        if rect_bottom <= bound_top || rect_top >= bound_bottom {
            return 0;
        }

        let clip_left = (rect.x as i64).max(bound_left);
        let clip_right = (rect.x as i64 + i64::from(rect.width)).min(bound_right);
        if clip_right <= clip_left {
            return 0;
        }

        let mut glyphs = 0;
        for (line_index, line) in text.lines().take(rect.height as usize).enumerate() {
            let y = i64::from(rect.y) + line_index as i64;
            if y < bound_top {
                continue;
            }
            if y >= bound_bottom {
                break;
            }
            glyphs += self.write_text_line_clipped(
                rect.x, y as usize, clip_left, clip_right, line, fg, alpha, attrs,
            );
        }
        glyphs
    }

    #[allow(clippy::too_many_arguments)]
    fn write_text_line_clipped(
        &mut self,
        x: i32,
        row: usize,
        clip_left: i64,
        clip_right: i64,
        text: &str,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) -> usize {
        let alpha = clamp_alpha(alpha);
        if alpha <= 0.0 || clip_right <= clip_left {
            return 0;
        }

        let mut col = i64::from(x);
        let mut glyphs = 0;

        for grapheme in text.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme).min(2) as i64;
            if width == 0 {
                continue;
            }

            let next_col = col + width;
            if next_col <= clip_left {
                col = next_col;
                continue;
            }
            if col < clip_left {
                col = next_col;
                continue;
            }
            if next_col > clip_right {
                break;
            }

            self.write_glyph(row, col as usize, grapheme, width as u8, fg, alpha, attrs);
            self.touch_span(row, col as usize, next_col as usize);
            glyphs += 1;
            col = next_col;
        }

        glyphs
    }

    /// Recolor a run of selected glyph cells on one row.
    ///
    /// With explicit selection colors, blends them over the run at the given alpha.
    /// With neither, swaps each cell's colors — reverse video — using the grid's
    /// default background where a cell has no color of its own. A wide glyph's
    /// continuation cell follows its head, so both halves highlight as one.
    pub fn apply_selection_colors(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        fg: Option<Rgb>,
        bg: Option<Rgb>,
        alpha: f64,
    ) {
        let alpha = clamp_alpha(alpha);
        if alpha <= 0.0 || width == 0 {
            return;
        }

        let (bound_left, bound_top, bound_right, bound_bottom) = self.writable_bounds();
        let row = i64::from(y);
        if row < bound_top || row >= bound_bottom {
            return;
        }
        let start = i64::from(x).max(bound_left);
        let end = (i64::from(x) + i64::from(width)).min(bound_right);
        if end <= start {
            return;
        }

        let default_bg = self.default_bg;
        let row_index = row as usize;
        let mut head_fg: Option<Rgb> = None;
        let mut head_bg: Option<Rgb> = None;
        for col in start..end {
            let cell = &mut self.cells[row_index][col as usize];
            let is_continuation = matches!(cell.content, CellContent::WideContinuation);
            if !is_continuation {
                head_fg = cell.fg;
                head_bg = cell.bg;
            }

            if fg.is_none() && bg.is_none() {
                // Reverse video: a rearrangement of colors already composited into the
                // cell, so alpha plays no part.
                if !is_continuation {
                    cell.fg = Some(head_bg.unwrap_or(default_bg));
                }
                cell.bg = Some(head_fg.unwrap_or(default_bg));
                continue;
            }

            if bg.is_some() {
                cell.bg = blend_color(cell.bg, bg, alpha, default_bg);
            }
            if fg.is_some() && !is_continuation {
                let cell_bg = cell.bg;
                cell.fg = blend_fg(cell.fg, fg, alpha, cell_bg, default_bg);
            }
        }
        self.touch_span(row_index, start as usize, end as usize);
    }

    /// Draw a border around the edge cells of `rect`, clipped to the grid.
    ///
    /// Bg is left as-is: the border sits on whatever background the node already filled.
    pub fn write_border(
        &mut self,
        rect: GridRect,
        border: Border,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) -> usize {
        let alpha = clamp_alpha(alpha);
        if rect.width == 0 || rect.height == 0 || !border.sides.any() || alpha <= 0.0 {
            return 0;
        }

        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        let right = left + i64::from(rect.width) - 1;
        let bottom = top + i64::from(rect.height) - 1;

        let mut glyphs = 0;
        for row in top..=bottom {
            if row == top || row == bottom {
                for col in left..=right {
                    glyphs += self.write_border_cell(
                        border, row, col, left, right, top, bottom, fg, alpha, attrs,
                    );
                }
            } else {
                glyphs += self.write_border_cell(
                    border, row, left, left, right, top, bottom, fg, alpha, attrs,
                );
                if right != left {
                    glyphs += self.write_border_cell(
                        border, row, right, left, right, top, bottom, fg, alpha, attrs,
                    );
                }
            }
        }
        glyphs
    }

    #[allow(clippy::too_many_arguments)]
    fn write_border_cell(
        &mut self,
        border: Border,
        row: i64,
        col: i64,
        left: i64,
        right: i64,
        top: i64,
        bottom: i64,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) -> usize {
        let sides = border.sides;
        let on_top = row == top && sides.top;
        let on_bottom = row == bottom && sides.bottom;
        let on_left = col == left && sides.left;
        let on_right = col == right && sides.right;

        let charset = border.charset;
        // A corner cell gets its corner character only when both adjacent sides are drawn.
        // Otherwise the one side that is present runs straight through it, so a top-only
        // border draws a clean rule rather than a rule with two stray corners.
        let glyph = match (on_top, on_bottom, on_left, on_right) {
            (true, _, true, _) => charset.top_left,
            (true, _, _, true) => charset.top_right,
            (_, true, true, _) => charset.bottom_left,
            (_, true, _, true) => charset.bottom_right,
            (true, ..) => charset.top,
            (_, true, ..) => charset.bottom,
            (_, _, true, _) => charset.left,
            (_, _, _, true) => charset.right,
            _ => return 0,
        };

        let (bound_left, bound_top, bound_right, bound_bottom) = self.writable_bounds();
        if row < bound_top || row >= bound_bottom || col < bound_left || col >= bound_right {
            return 0;
        }

        let (row, col) = (row as usize, col as usize);
        let mut text = [0u8; 4];
        self.write_glyph(row, col, glyph.encode_utf8(&mut text), 1, fg, alpha, attrs);
        self.touch_span(row, col, col + 1);
        1
    }

    /// Draw half-block edges on the outermost cells of `rect`, clipped to the grid.
    ///
    /// The node's fill (`inner`) covers only the half of each edge cell facing into the node,
    /// so its boundary lands mid-cell. `outer` is the other half; `None` leaves whatever is
    /// already painted there, so the edge fades into the color it sits on.
    pub fn write_half_block_edges(
        &mut self,
        rect: GridRect,
        sides: Sides,
        inner: Rgb,
        outer: Option<Rgb>,
        alpha: f64,
    ) -> usize {
        let alpha = clamp_alpha(alpha);
        if rect.width == 0 || rect.height == 0 || !sides.any() || alpha <= 0.0 {
            return 0;
        }

        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        let right = left + i64::from(rect.width) - 1;
        let bottom = top + i64::from(rect.height) - 1;

        let mut glyphs = 0;
        for row in top..=bottom {
            if row == top || row == bottom {
                for col in left..=right {
                    glyphs += self.write_half_block_cell(
                        sides, row, col, left, right, top, bottom, inner, outer, alpha,
                    );
                }
            } else {
                glyphs += self.write_half_block_cell(
                    sides, row, left, left, right, top, bottom, inner, outer, alpha,
                );
                if right != left {
                    glyphs += self.write_half_block_cell(
                        sides, row, right, left, right, top, bottom, inner, outer, alpha,
                    );
                }
            }
        }
        glyphs
    }

    #[allow(clippy::too_many_arguments)]
    fn write_half_block_cell(
        &mut self,
        sides: Sides,
        row: i64,
        col: i64,
        left: i64,
        right: i64,
        top: i64,
        bottom: i64,
        inner: Rgb,
        outer: Option<Rgb>,
        alpha: f64,
    ) -> usize {
        // Opposing sides can land on the same cell when the node is one cell wide or tall, and
        // no glyph leaves a strip of fill between two outer halves. The start side wins.
        let vertical = if row == top && sides.top {
            Some(Side::Top)
        } else if row == bottom && sides.bottom {
            Some(Side::Bottom)
        } else {
            None
        };
        let horizontal = if col == left && sides.left {
            Some(Side::Left)
        } else if col == right && sides.right {
            Some(Side::Right)
        } else {
            None
        };

        // Where two edges meet, the fill is left with a single quadrant of the cell.
        let glyph = match (vertical, horizontal) {
            (Some(Side::Top), Some(Side::Left)) => '▗',
            (Some(Side::Top), Some(Side::Right)) => '▖',
            (Some(Side::Bottom), Some(Side::Left)) => '▝',
            (Some(Side::Bottom), Some(Side::Right)) => '▘',
            (Some(Side::Top), None) => '▄',
            (Some(Side::Bottom), None) => '▀',
            (None, Some(Side::Left)) => '▐',
            (None, Some(Side::Right)) => '▌',
            _ => return 0,
        };

        let (bound_left, bound_top, bound_right, bound_bottom) = self.writable_bounds();
        if row < bound_top || row >= bound_bottom || col < bound_left || col >= bound_right {
            return 0;
        }

        let (row, col) = (row as usize, col as usize);
        self.clear_text_span_at(row, col);

        // The half-block replaces the cell's content, so both of its halves sit directly on the
        // background that was painted underneath — never on the fg of a glyph that is now gone.
        let under = self.cells[row][col].bg;
        let mut text = [0u8; 4];
        self.cells[row][col] = Cell {
            content: CellContent::Glyph {
                text: glyph.encode_utf8(&mut text).to_string(),
                width: 1,
            },
            fg: blend_fg(under, Some(inner), alpha, under, self.default_bg),
            bg: match outer {
                Some(outer) => blend_color(under, Some(outer), alpha, self.default_bg),
                None => under,
            },
            // No attributes: a half block is fill, not text. Bolding or italicizing it would
            // distort the shape the effect depends on.
            attrs: CellAttrs::default(),
        };
        self.touch_span(row, col, col + 1);
        1
    }

    fn clip_rect(
        &self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    ) -> Option<(usize, usize, usize, usize)> {
        if width == 0 || height == 0 {
            return None;
        }

        let left = i64::from(x);
        let top = i64::from(y);
        let right = left + i64::from(width);
        let bottom = top + i64::from(height);
        let (bound_left, bound_top, bound_right, bound_bottom) = self.writable_bounds();

        let clipped_left = left.max(bound_left);
        let clipped_top = top.max(bound_top);
        let clipped_right = right.min(bound_right);
        let clipped_bottom = bottom.min(bound_bottom);

        if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
            return None;
        }

        Some((
            clipped_left as usize,
            clipped_top as usize,
            clipped_right as usize,
            clipped_bottom as usize,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn write_glyph(
        &mut self,
        row: usize,
        col: usize,
        text: &str,
        width: u8,
        fg: Option<Rgb>,
        alpha: f64,
        attrs: CellAttrs,
    ) {
        self.clear_text_span_at(row, col);
        if width == 2 {
            self.clear_text_span_at(row, col + 1);
        }

        let dst = &self.cells[row][col];
        let glyph_cell = Cell {
            content: CellContent::Glyph {
                text: text.to_string(),
                width,
            },
            fg: blend_fg(dst.fg, fg, alpha, dst.bg, self.default_bg),
            bg: dst.bg,
            attrs,
        };
        self.cells[row][col] = glyph_cell;

        if width == 2 {
            self.cells[row][col + 1].content = CellContent::WideContinuation;
            self.cells[row][col + 1].fg = None;
            self.cells[row][col + 1].attrs = attrs;
        }
    }

    fn clear_text_span_at(&mut self, row: usize, col: usize) {
        if row >= self.height as usize || col >= self.width as usize {
            return;
        }

        match self.cells[row][col].content.clone() {
            CellContent::Glyph { width: 2, .. } => {
                self.clear_one_cell_text(row, col);
                if col + 1 < self.width as usize {
                    self.clear_one_cell_text(row, col + 1);
                }
            }
            CellContent::WideContinuation => {
                if col > 0 {
                    self.clear_one_cell_text(row, col - 1);
                }
                self.clear_one_cell_text(row, col);
            }
            _ => self.clear_one_cell_text(row, col),
        }
    }

    fn clear_one_cell_text(&mut self, row: usize, col: usize) {
        self.cells[row][col].content = CellContent::Empty;
        self.cells[row][col].fg = None;
        self.cells[row][col].attrs = CellAttrs::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b, a: 255 }
    }

    /// `lerp_u8` hand-rolls the rounding `f64::round` would do, to keep a software
    /// float call out of the per-cell blend. Pin it to `round` across the domain,
    /// including the alphas one ULP off a .5 boundary where the tempting shorthand
    /// `(value + 0.5) as u8` silently disagrees.
    #[test]
    fn lerp_u8_matches_float_round_over_its_domain() {
        let rounded = |a: u8, b: u8, t: f64| (a as f64 + (b as f64 - a as f64) * t).round() as u8;

        for a in 0..=255u8 {
            for b in 0..=255u8 {
                for step in 0..=16 {
                    let t = f64::from(step) / 16.0;
                    assert_eq!(lerp_u8(a, b, t), rounded(a, b, t), "a={a} b={b} t={t}");
                }
            }
        }

        // Alphas landing just below each half-way point between two channel values.
        for (a, b) in [(0u8, 1u8), (0, 255), (127, 128), (200, 201), (254, 255)] {
            let (low, high) = (f64::from(a), f64::from(b));
            for k in a..b {
                let boundary = (f64::from(k) + 0.5 - low) / (high - low);
                let mut t = boundary;
                for _ in 0..4 {
                    t = f64::from_bits(t.to_bits().wrapping_sub(1));
                }
                for _ in 0..8 {
                    if (0.0..=1.0).contains(&t) {
                        assert_eq!(lerp_u8(a, b, t), rounded(a, b, t), "a={a} b={b} t={t:?}");
                    }
                    t = f64::from_bits(t.to_bits().wrapping_add(1));
                }
            }
        }
    }

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.cells[row]
            .iter()
            .filter(|cell| !cell.is_wide_continuation())
            .map(Cell::terminal_text)
            .collect()
    }

    #[test]
    fn fill_rect_ignores_offscreen_and_handles_overflow() {
        let blue = rgb(0, 0, 255);
        let cell = Cell::empty_with_bg(blue);
        let mut grid = Grid::new(2, 2);

        grid.fill_rect(5, 0, 1, 1, cell.clone(), 1.0);
        assert_eq!(grid.cells, vec![vec![Cell::empty(); 2]; 2]);

        grid.fill_rect(1, 1, u16::MAX, u16::MAX, cell, 1.0);
        assert_eq!(grid.cells[1][1].bg, Some(blue));
        assert_eq!(grid.cells[0][0], Cell::empty());
    }

    #[test]
    fn fill_rect_clips_negative_position() {
        let blue = rgb(0, 0, 255);
        let cell = Cell::empty_with_bg(blue);
        let mut grid = Grid::new(3, 2);

        grid.fill_rect(-1, -1, 3, 2, cell, 1.0);

        assert_eq!(grid.cells[0][0].bg, Some(blue));
        assert_eq!(grid.cells[0][1].bg, Some(blue));
        assert_eq!(grid.cells[0][2].bg, None);
        assert_eq!(grid.cells[1][0].bg, None);
    }

    #[test]
    fn fill_rect_ignores_fully_negative_offscreen_position() {
        let blue = rgb(0, 0, 255);
        let cell = Cell::empty_with_bg(blue);
        let mut grid = Grid::new(2, 1);

        grid.fill_rect(-3, 0, 2, 1, cell, 1.0);

        assert_eq!(grid.cells, vec![vec![Cell::empty(); 2]]);
    }

    #[test]
    fn translucent_empty_fill_blends_background_without_erasing_text() {
        let white = rgb(255, 255, 255);
        let blue = rgb(0, 0, 255);
        let red = rgb(255, 0, 0);
        let mut grid = Grid::new(2, 1);

        grid.fill_rect(0, 0, 2, 1, Cell::empty_with_bg(blue), 1.0);
        grid.write_text(0, 0, "hi", Some(white), 1.0, CellAttrs::default());
        grid.fill_rect(0, 0, 2, 1, Cell::empty_with_bg(red), 0.5);

        assert_eq!(row_text(&grid, 0), "hi");
        assert_eq!(grid.cells[0][0].fg, Some(white));
        assert_eq!(grid.cells[0][0].bg, Some(rgb(128, 0, 128)));
        assert_eq!(grid.cells[0][1].fg, Some(white));
        assert_eq!(grid.cells[0][1].bg, Some(rgb(128, 0, 128)));
    }

    #[test]
    fn fill_rect_clamps_alpha() {
        let red = rgb(255, 0, 0);
        let cell = Cell::empty_with_bg(red);
        let mut grid = Grid::new(1, 1);

        grid.fill_rect(0, 0, 1, 1, cell.clone(), f64::NAN);
        assert_eq!(grid.cells[0][0], Cell::empty());

        grid.fill_rect(0, 0, 1, 1, cell, 2.0);
        assert_eq!(grid.cells[0][0].bg, Some(red));
    }

    #[test]
    fn write_text_ignores_offscreen_y() {
        let mut grid = Grid::new(3, 1);
        let before = grid.clone();

        grid.write_text(
            0,
            1,
            "abc",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(grid.cells, before.cells);

        grid.write_text(
            0,
            -1,
            "abc",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(grid.cells, before.cells);
    }

    #[test]
    fn write_text_clips_negative_x() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            -2,
            0,
            "abcd",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), "cd ");
    }

    #[test]
    fn write_text_skips_partial_wide_glyph_at_left_edge() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            -1,
            0,
            "界ab",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), " ab");
    }

    #[test]
    fn write_text_stops_at_newline() {
        let mut grid = Grid::new(5, 1);

        grid.write_text(
            0,
            0,
            "ab\ncd",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(row_text(&grid, 0), "ab   ");
    }

    #[test]
    fn write_text_clipped_respects_width() {
        let mut grid = Grid::new(5, 1);

        grid.write_text_clipped(
            GridRect {
                x: 1,
                y: 0,
                width: 2,
                height: 1,
            },
            "abcd",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(row_text(&grid, 0), " ab  ");
    }

    #[test]
    fn write_text_clipped_respects_height_and_newlines() {
        let mut grid = Grid::new(4, 3);

        grid.write_text_clipped(
            GridRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            "ab\ncd\nef",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(row_text(&grid, 0), "ab  ");
        assert_eq!(row_text(&grid, 1), "cd  ");
        assert_eq!(row_text(&grid, 2), "    ");
    }

    #[test]
    fn write_text_clipped_clips_negative_position() {
        let mut grid = Grid::new(4, 2);

        grid.write_text_clipped(
            GridRect {
                x: -1,
                y: -1,
                width: 4,
                height: 3,
            },
            "ab\ncd\nef",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), "d   ");
        assert_eq!(row_text(&grid, 1), "f   ");
    }

    #[test]
    fn ascii_glyphs_are_width_one() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            0,
            0,
            "abc",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][2].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn wide_glyph_occupies_two_cells() {
        let mut grid = Grid::new(4, 1);

        grid.write_text(
            0,
            0,
            "a界b",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), "a界b");
        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 2, .. }
        ));
        assert!(matches!(
            grid.cells[0][2].content,
            CellContent::WideContinuation
        ));
        assert!(matches!(
            grid.cells[0][3].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn wide_glyph_is_skipped_at_clip_boundary() {
        let mut grid = Grid::new(2, 1);

        grid.write_text_clipped(
            GridRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            "a界",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(row_text(&grid, 0), "a ");
    }

    #[test]
    fn combining_grapheme_occupies_one_cell() {
        let mut grid = Grid::new(2, 1);

        grid.write_text(
            0,
            0,
            "e\u{301}x",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        assert_eq!(row_text(&grid, 0), "e\u{301}x");
        assert!(matches!(
            grid.cells[0][0].content,
            CellContent::Glyph { width: 1, .. }
        ));
        assert!(matches!(
            grid.cells[0][1].content,
            CellContent::Glyph { width: 1, .. }
        ));
    }

    #[test]
    fn overwriting_wide_head_clears_continuation() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            0,
            0,
            "界",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        grid.write_text(
            0,
            0,
            "a",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), "a  ");
        assert!(matches!(grid.cells[0][1].content, CellContent::Empty));
    }

    #[test]
    fn overwriting_wide_continuation_clears_head() {
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            0,
            0,
            "界",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        grid.write_text(
            1,
            0,
            "a",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );

        assert_eq!(row_text(&grid, 0), " a ");
        assert!(matches!(grid.cells[0][0].content, CellContent::Empty));
    }

    #[test]
    fn fill_rect_clears_wide_span_it_overlaps() {
        let blue = rgb(0, 0, 255);
        let mut grid = Grid::new(3, 1);

        grid.write_text(
            0,
            0,
            "界",
            Some(rgb(255, 255, 255)),
            1.0,
            CellAttrs::default(),
        );
        grid.fill_rect(1, 0, 1, 1, Cell::empty_with_bg(blue), 1.0);

        assert_eq!(row_text(&grid, 0), "   ");
        assert!(matches!(grid.cells[0][0].content, CellContent::Empty));
        assert_eq!(grid.cells[0][1].bg, Some(blue));
    }
}

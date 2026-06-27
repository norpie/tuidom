use crate::document::Document;
use crate::error::Result;
use crate::event::ResizeEvent;
use crate::render::grid::{Cell, Grid};
use crate::render::render_to_grid;
use crate::style::color::{Rgb, RgbCache};

/// A terminal-free runtime harness for deterministic layout, paint, and input tests.
pub struct HeadlessRuntime {
    doc: Document,
    width: u16,
    height: u16,
    grid: Option<Grid>,
    rgb_cache: RgbCache,
}

impl HeadlessRuntime {
    /// Create a headless runtime for `doc` with the given screen dimensions.
    pub fn new(doc: Document, width: u16, height: u16) -> Self {
        Self {
            doc,
            width,
            height,
            grid: None,
            rgb_cache: RgbCache::new(),
        }
    }

    /// Return the document driven by this runtime.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Return the current screen width in terminal cells.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Return the current screen height in terminal cells.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Update the screen dimensions and dispatch a document-level resize event.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.grid = None;
        self.doc.dispatch_resize(ResizeEvent { width, height });
    }

    /// Compute layout and paint the document into the inspectable screen buffer.
    pub fn render(&mut self) -> Result<()> {
        self.doc.compute_layout(self.width, self.height)?;
        let (grid, _) = render_to_grid(&self.doc, self.width, self.height, &mut self.rgb_cache);
        self.grid = Some(grid);
        Ok(())
    }

    /// Return the last rendered cell at the given screen coordinate.
    ///
    /// Returns `None` before the first render or when the coordinate is outside
    /// the current screen dimensions.
    pub fn get_cell(&self, x: i32, y: i32) -> Option<ScreenCell> {
        let grid = self.grid.as_ref()?;
        grid_cell(grid, x, y).map(ScreenCell::from_cell)
    }

    /// Return a row-major snapshot of a rectangular screen region.
    ///
    /// The returned region preserves the requested dimensions. Cells outside
    /// the current screen, or all cells before the first render, are returned as
    /// empty cells.
    pub fn get_screen_region(&self, x: i32, y: i32, width: u16, height: u16) -> ScreenRegion {
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for row in 0..height {
            for col in 0..width {
                let cell = self
                    .grid
                    .as_ref()
                    .and_then(|grid| grid_cell(grid, x + i32::from(col), y + i32::from(row)))
                    .map(ScreenCell::from_cell)
                    .unwrap_or_else(ScreenCell::empty);
                cells.push(cell);
            }
        }

        ScreenRegion {
            x,
            y,
            width,
            height,
            cells,
        }
    }
}

/// RGBA color captured from a rendered screen cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenColor {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

impl From<Rgb> for ScreenColor {
    fn from(value: Rgb) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }
    }
}

/// A rendered terminal cell snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    /// Display text for this cell.
    pub text: String,
    /// Foreground color, if one was rendered.
    pub fg: Option<ScreenColor>,
    /// Background color, if one was rendered.
    pub bg: Option<ScreenColor>,
    /// Terminal cell width occupied by this cell's content.
    pub width: u8,
    /// Whether this cell is the continuation cell of a wide glyph.
    pub is_wide_continuation: bool,
}

impl ScreenCell {
    fn empty() -> Self {
        Self::from_cell(&Cell::empty())
    }

    fn from_cell(cell: &Cell) -> Self {
        Self {
            text: cell.terminal_text().to_string(),
            fg: cell.fg.map(ScreenColor::from),
            bg: cell.bg.map(ScreenColor::from),
            width: cell.content_width(),
            is_wide_continuation: cell.is_wide_continuation(),
        }
    }
}

/// Row-major snapshot of a rectangular rendered screen region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRegion {
    /// Requested region left edge.
    pub x: i32,
    /// Requested region top edge.
    pub y: i32,
    /// Requested region width.
    pub width: u16,
    /// Requested region height.
    pub height: u16,
    /// Row-major cells for the requested region.
    pub cells: Vec<ScreenCell>,
}

fn grid_cell(grid: &Grid, x: i32, y: i32) -> Option<&Cell> {
    if x < 0 || y < 0 || x >= i32::from(grid.width) || y >= i32::from(grid.height) {
        return None;
    }

    grid.cells
        .get(y as usize)
        .and_then(|row| row.get(x as usize))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::style::{Color, Length, Style};

    #[test]
    fn render_exposes_cells_for_inspection() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("Hi").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();

        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "H");
        assert_eq!(runtime.get_cell(1, 0).unwrap().text, "i");
        assert_eq!(runtime.get_cell(10, 0), None);
    }

    #[test]
    fn screen_region_preserves_requested_bounds_and_pads_out_of_bounds() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("A").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 2, 1);
        runtime.render().unwrap();

        let region = runtime.get_screen_region(-1, 0, 3, 1);

        assert_eq!(region.x, -1);
        assert_eq!(region.y, 0);
        assert_eq!(region.width, 3);
        assert_eq!(region.height, 1);
        assert_eq!(region.cells.len(), 3);
        assert_eq!(region.cells[0].text, " ");
        assert_eq!(region.cells[1].text, "A");
        assert_eq!(region.cells[2].text, " ");
    }

    #[test]
    fn rendered_background_color_is_exposed() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        let mut style = Style::new();
        style.width(Length::Pixels(1));
        style.height(Length::Pixels(1));
        style.background(Color::red());
        doc.set_style(node, &style).unwrap();
        doc.append_child(doc.root(), node).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 3, 3);
        runtime.render().unwrap();

        let bg = runtime.get_cell(0, 0).unwrap().bg.unwrap();
        assert_eq!(bg.a, 255);
        assert!(bg.r > bg.g);
        assert!(bg.r > bg.b);
    }

    #[test]
    fn resize_updates_dimensions_and_dispatches_resize_event() {
        let doc = Document::new().unwrap();
        let seen = Arc::new(Mutex::new(None));
        let seen_for_handler = seen.clone();
        doc.on_resize(move |event| {
            *seen_for_handler.lock().unwrap() = Some((event.width, event.height));
        });

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.resize(20, 5);

        assert_eq!(runtime.width(), 20);
        assert_eq!(runtime.height(), 5);
        assert_eq!(*seen.lock().unwrap(), Some((20, 5)));
    }
}

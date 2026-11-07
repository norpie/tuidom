//! A headless harness for component tests: mount a view, drive it with simulated input, and
//! assert on the screen or on signals.
//!
//! [`Harness`] wires its root through [`App::mount_on`], so a test exercises the same session
//! wiring a real app uses, and each harness has its own session, so tests using it run in
//! parallel. It keeps the screen **settled**: every action settles afterwards and every read
//! settles first. Settling pumps queued flushes, then renders until two consecutive frames
//! match, since a frame can schedule work that only shows on the next one (a virtualized list
//! learning its viewport, for instance). A write from the test body, which runs outside any
//! event-task span and so defers like an off-thread write, is therefore visible on the next read
//! with nothing to call.
//!
//! [`document`](Harness::document) and [`runtime`](Harness::runtime) reach the underlying pieces
//! for tests that need to observe a state before it settles.
//!
//! # Panics
//!
//! Unlike the rest of the crate, the harness panics on failure instead of returning `Result`:
//! it only runs in tests, where a failed setup or render is a test failure like any assertion.
//! Messages say which harness step failed.
//!
//! Enabled by the `testing` feature.

use tuidom::Document;
use tuidom::NodeId;
use tuidom::event::{KeyCode, KeyModifiers};
use tuidom::headless::{HeadlessRuntime, ScreenCell, ScreenCursor, ScreenRegion};

use crate::app::{App, AppSession};
use crate::view::View;

/// Frames a settle may render before it gives up on the screen ever matching the last frame.
/// With the clock frozen, a settled UI stops changing within a frame or two; reaching this means
/// something re-renders on every frame.
const MAX_SETTLE_FRAMES: usize = 16;

/// A mounted app on a headless document, kept settled between steps of a test.
///
/// Dropping it tears the app down: the root scope is disposed and its session removed.
pub struct Harness {
    _session: AppSession,
    rt: HeadlessRuntime,
    doc: Document,
}

impl Harness {
    /// Mount `root` on a fresh headless document of `width` × `height` cells and settle.
    ///
    /// `root` is the same kind of closure [`App::new`] takes, run once as the root component.
    /// The document clock is frozen from the start; [`runtime`](Self::runtime) gives access to
    /// `advance_time`.
    pub fn new<F, V>(width: u16, height: u16, root: F) -> Harness
    where
        F: FnOnce() -> V + Send + Sync + 'static,
        V: Into<View>,
    {
        let doc =
            Document::new().unwrap_or_else(|err| panic!("harness: creating the document: {err}"));
        // The runtime freezes the clock, so it is created before anything mounts and could start
        // a transition against the wall clock.
        let rt = HeadlessRuntime::new(doc.clone(), width, height);
        let session = App::new(root)
            .mount_on(&doc)
            .unwrap_or_else(|err| panic!("harness: mounting the root: {err}"));
        let mut harness = Harness {
            _session: session,
            rt,
            doc,
        };
        harness.settle();
        harness
    }

    /// Press a key on the focused node.
    pub fn press(&mut self, code: KeyCode) {
        self.rt.simulate_key(code);
        self.settle();
    }

    /// Press a key with modifiers held.
    pub fn press_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.rt.simulate_key_with_modifiers(code, modifiers);
        self.settle();
    }

    /// Type `text` into the focused node, one key press per character.
    pub fn type_text(&mut self, text: &str) {
        self.rt.simulate_text(text);
        self.settle();
    }

    /// Click the cell at `(x, y)`.
    pub fn click(&mut self, x: i32, y: i32) {
        self.rt.simulate_click(x, y);
        self.settle();
    }

    /// Turn the mouse wheel over `(x, y)`; a positive `delta` scrolls down.
    pub fn scroll(&mut self, x: i32, y: i32, delta: i16) {
        self.rt.simulate_scroll(x, y, delta);
        self.settle();
    }

    /// Resize the screen.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.rt.resize(width, height);
        self.settle();
    }

    /// Move focus to `node`.
    ///
    /// # Panics
    ///
    /// If the node does not exist or cannot take focus.
    pub fn focus(&mut self, node: NodeId) {
        self.doc
            .focus(node)
            .unwrap_or_else(|err| panic!("harness: focusing {node:?}: {err}"));
        self.settle();
    }

    /// The text of row `y`, trailing blanks trimmed. Empty for a row off the screen.
    pub fn row(&mut self, y: u16) -> String {
        self.settle();
        self.row_text(y)
    }

    /// Every row of the screen, trailing blanks trimmed, joined with newlines.
    pub fn screen(&mut self) -> String {
        self.settle();
        (0..self.rt.height())
            .map(|y| self.row_text(y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The painted cell at `(x, y)`, or `None` off the screen.
    pub fn cell(&mut self, x: i32, y: i32) -> Option<ScreenCell> {
        self.settle();
        self.rt.get_cell(x, y)
    }

    /// Where `needle` first appears on screen, as the `(x, y)` of the cell it starts in.
    ///
    /// Rows are searched top to bottom and each left to right; a match never spans rows. `None`
    /// when the text is not on screen, or when `needle` is empty.
    pub fn find_text(&mut self, needle: &str) -> Option<(i32, i32)> {
        self.settle();
        if needle.is_empty() {
            return None;
        }
        (0..self.rt.height()).find_map(|y| {
            let (text, starts) = self.row_cells(y);
            let at = text.find(needle)?;
            // The cell whose text begins at or before the match's first byte.
            let cell = starts.partition_point(|&(offset, _)| offset <= at) - 1;
            Some((starts[cell].1, y as i32))
        })
    }

    /// Click the first cell of the first place `needle` appears on screen.
    ///
    /// # Panics
    ///
    /// If `needle` is not on screen.
    pub fn click_text(&mut self, needle: &str) {
        let Some((x, y)) = self.find_text(needle) else {
            panic!(
                "harness: no {needle:?} on screen to click:\n{}",
                self.screen()
            );
        };
        self.click(x, y);
    }

    /// The first node in document order whose attribute `key` is `value`, such as one tagged
    /// `test_id: "save"` in `view!`.
    pub fn find_attr(&mut self, key: &str, value: &str) -> Option<NodeId> {
        self.settle();
        self.doc
            .snapshot()
            .nodes
            .into_iter()
            .find(|node| node.attrs.get(key).is_some_and(|v| v == value))
            .map(|node| node.id)
    }

    /// The document the app is mounted on.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The headless runtime, for anything the harness does not wrap. Nothing it does is settled
    /// until the next harness action or read.
    pub fn runtime(&mut self) -> &mut HeadlessRuntime {
        &mut self.rt
    }

    /// Pump queued flushes and render until the screen stops changing.
    fn settle(&mut self) {
        let mut last = None;
        for _ in 0..MAX_SETTLE_FRAMES {
            self.rt.pump();
            if let Err(err) = self.rt.render() {
                panic!("harness: rendering a frame: {err}");
            }
            let frame = self.frame();
            if last.as_ref() == Some(&frame) {
                return;
            }
            last = Some(frame);
        }
        panic!(
            "harness: the screen was still changing after {MAX_SETTLE_FRAMES} frames; \
             something re-renders on every frame"
        );
    }

    fn frame(&self) -> (ScreenRegion, Option<ScreenCursor>) {
        let region = self
            .rt
            .get_screen_region(0, 0, self.rt.width(), self.rt.height());
        (region, self.rt.cursor())
    }

    fn row_text(&self, y: u16) -> String {
        self.row_cells(y).0.trim_end().to_string()
    }

    /// Row `y`'s joined cell text, plus the byte offset and column where each cell's text
    /// starts. A wide glyph's continuation cell contributes no text, so offsets and columns
    /// drift apart after one and the map is how a match finds its column.
    fn row_cells(&self, y: u16) -> (String, Vec<(usize, i32)>) {
        let mut text = String::new();
        let mut starts = Vec::new();
        for x in 0..self.rt.width() as i32 {
            let Some(cell) = self.rt.get_cell(x, y as i32) else {
                continue;
            };
            if cell.is_wide_continuation {
                continue;
            }
            starts.push((text.len(), x));
            text.push_str(&cell.text);
        }
        (text, starts)
    }
}

#[cfg(test)]
mod tests;

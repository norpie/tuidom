use std::time::{Duration, Instant};

use crate::document::Document;
use crate::error::Result;
use crate::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, ResizeEvent, WheelEvent};
use crate::performance::RenderMetrics;
use crate::render::grid::{Cell, Grid};
use crate::render::{RenderCursor, render_into_grid};
use crate::runtime_event::{RuntimeEvent, RuntimeEventState, process_runtime_event};
use crate::style::CursorShape;
use crate::style::color::{Rgb, RgbCache};

/// A terminal-free runtime harness for deterministic layout, paint, and input tests.
pub struct HeadlessRuntime {
    doc: Document,
    width: u16,
    height: u16,
    grid: Option<Grid>,
    cursor: Option<RenderCursor>,
    rgb_cache: RgbCache,
    event_state: RuntimeEventState,
}

impl HeadlessRuntime {
    /// Create a headless runtime for `doc` with the given screen dimensions.
    ///
    /// Freezes the document clock: animations only progress through
    /// [`advance_time`](Self::advance_time), so interpolated values in tests are
    /// exact instead of racing the wall clock.
    pub fn new(doc: Document, width: u16, height: u16) -> Self {
        doc.enable_manual_time();
        Self {
            doc,
            width,
            height,
            grid: None,
            cursor: None,
            rgb_cache: RgbCache::new(),
            event_state: RuntimeEventState::default(),
        }
    }

    /// Advance the frozen document clock.
    ///
    /// Active transitions and animations progress by exactly `delta`; finished
    /// ones settle and dispatch their end and iteration events through the
    /// shared runtime path. The next [`render`](Self::render) shows the frame
    /// as it would look at the advanced instant.
    pub fn advance_time(&mut self, delta: Duration) {
        self.doc.advance_manual_time(delta);
        for event in self.doc.run_animation_upkeep() {
            process_runtime_event(&self.doc, event, &mut self.event_state);
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
        self.cursor = None;
        self.doc.dispatch_resize(ResizeEvent { width, height });
    }

    /// Compute layout and paint the document into the inspectable screen buffer.
    ///
    /// Records frame metrics for the performance API and dispatches the post-frame
    /// event through the shared runtime path, so both are observable in tests.
    /// Diff and flush metrics stay zero — headless frames are painted, not flushed.
    pub fn render(&mut self) -> Result<()> {
        let frame_start = Instant::now();

        let layout_start = Instant::now();
        self.doc.compute_layout(self.width, self.height)?;
        let layout_time = layout_start.elapsed();

        // Painted into the frame the last render left behind, which the paint pass
        // clears first — the same buffer reuse the terminal renderer does with its two
        // grids. Allocating a fresh grid per frame is a large share of a headless
        // frame's cost, and it would be cost no real backend pays.
        let mut grid = match self.grid.take() {
            Some(grid) if grid.width == self.width && grid.height == self.height => grid,
            _ => Grid::new(self.width, self.height),
        };
        let output = render_into_grid(&self.doc, &mut grid, &mut self.rgb_cache);
        self.cursor = output.cursor;
        self.grid = Some(grid);

        let stats = RenderMetrics {
            grid_time: output.stats.grid_time,
            dom_collect_time: output.stats.dom_collect_time,
            dom_paint_time: output.stats.dom_paint_time,
            paint_profile: output.stats.paint_profile,
            ..RenderMetrics::default()
        };
        self.doc
            .record_frame_metrics(frame_start.elapsed(), layout_time, stats);

        if let Some(event) = self.doc.pending_post_frame_event() {
            process_runtime_event(
                &self.doc,
                RuntimeEvent::PostFrame(Box::new(event)),
                &mut self.event_state,
            );
        }
        Ok(())
    }

    /// Return cursor metadata from the last rendered frame.
    ///
    /// Returns `None` before the first render or when no focused input produced
    /// cursor metadata.
    pub fn cursor(&self) -> Option<ScreenCursor> {
        self.cursor.map(ScreenCursor::from_cursor)
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

    /// Dispatch a simulated key press through the shared runtime-event path.
    pub fn simulate_key(&mut self, code: KeyCode) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::KeyPress(KeyEvent::new(code)),
            &mut self.event_state,
        );
    }

    /// Dispatch a simulated mouse button press at a screen coordinate.
    ///
    /// Mouse targeting uses the latest committed layout snapshot. Call
    /// [`render`](Self::render) first when the DOM or styles have changed.
    pub fn simulate_mouse_down(&mut self, x: i32, y: i32, button: MouseButton) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::MouseDown(MouseEvent::new(x, y, button)),
            &mut self.event_state,
        );
    }

    /// Dispatch a simulated mouse pointer move at a screen coordinate.
    ///
    /// Moving over a focusable node focuses it through the shared runtime path.
    pub fn simulate_mouse_move(&mut self, x: i32, y: i32) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::MouseMove { x, y, held: None },
            &mut self.event_state,
        );
    }

    /// Dispatch a simulated drag movement: a pointer move with `button` held down.
    pub fn simulate_mouse_drag_move(&mut self, x: i32, y: i32, button: MouseButton) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::MouseMove {
                x,
                y,
                held: Some(button),
            },
            &mut self.event_state,
        );
    }

    /// Dispatch a simulated mouse button release at a screen coordinate.
    ///
    /// If it matches the previous simulated mouse down by target, cell, and
    /// button, this also synthesizes a click through the shared runtime path.
    pub fn simulate_mouse_up(&mut self, x: i32, y: i32, button: MouseButton) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::MouseUp(MouseEvent::new(x, y, button)),
            &mut self.event_state,
        );
    }

    /// Dispatch a left-button down/up pair at a screen coordinate.
    pub fn simulate_click(&mut self, x: i32, y: i32) {
        self.simulate_mouse_down(x, y, MouseButton::Left);
        self.simulate_mouse_up(x, y, MouseButton::Left);
    }

    /// Dispatch a simulated mouse wheel event at a screen coordinate.
    pub fn simulate_scroll(&mut self, x: i32, y: i32, delta: i16) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::Wheel(WheelEvent::new(x, y, delta)),
            &mut self.event_state,
        );
    }

    /// Dispatch a simulated horizontal mouse wheel event at a screen coordinate.
    pub fn simulate_horizontal_scroll(&mut self, x: i32, y: i32, delta: i16) {
        process_runtime_event(
            &self.doc,
            RuntimeEvent::Wheel(WheelEvent::horizontal(x, y, delta)),
            &mut self.event_state,
        );
    }

    /// Dispatch each character in `text` as a simulated key press.
    pub fn simulate_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.simulate_key(KeyCode::Char(ch));
        }
    }

    /// Dispatch a left-button press at `start`, a drag movement to `end`, and a
    /// release at `end` — the sequence a terminal reports for a pointer drag.
    pub fn simulate_mouse_drag(&mut self, start: (i32, i32), end: (i32, i32)) {
        self.simulate_mouse_down(start.0, start.1, MouseButton::Left);
        self.simulate_mouse_drag_move(end.0, end.1, MouseButton::Left);
        self.simulate_mouse_up(end.0, end.1, MouseButton::Left);
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

impl ScreenColor {
    /// Create an opaque RGB screen color.
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
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

/// Cursor metadata captured from a rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCursor {
    /// Screen x coordinate in terminal cells.
    pub x: i32,
    /// Screen y coordinate in terminal cells.
    pub y: i32,
    /// Cursor shape requested by style.
    pub shape: CursorShape,
    /// Cursor color derived from the focused node's resolved foreground color.
    pub color: ScreenColor,
    /// Whether this cursor should be visible after layout/input clipping.
    pub visible: bool,
}

impl ScreenCursor {
    fn from_cursor(cursor: RenderCursor) -> Self {
        Self {
            x: cursor.x,
            y: cursor.y,
            shape: cursor.shape,
            color: ScreenColor::from(cursor.color),
            visible: cursor.visible,
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
    /// Whether this cell's glyph is bold.
    pub bold: bool,
    /// Whether this cell's glyph is italic.
    pub italic: bool,
    /// Whether this cell's glyph is underlined.
    pub underline: bool,
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
            bold: cell.attrs.bold,
            italic: cell.attrs.italic,
            underline: cell.attrs.underline,
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
    use crate::animation::{
        AnimatableProperty, AnimationDirection, Easing, KeyframeAnimation, TransitionConfig,
        TransitionProperty,
    };
    use crate::document::SelectionPoint;
    use crate::event::EventPhase;
    use crate::lock;
    use crate::style::{Color, Length, Position, ResolvedColor, Style};

    #[test]
    fn advancing_frozen_time_interpolates_a_transition_exactly() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.opacity(0.0);
        })
        .unwrap();

        assert_eq!(doc.resolved_style(node).unwrap().opacity, 1.0);

        runtime.advance_time(Duration::from_millis(250));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.75);

        runtime.advance_time(Duration::from_millis(250));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.5);
    }

    #[test]
    fn advancing_past_the_end_settles_on_the_target_and_goes_idle() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.opacity(0.25);
        })
        .unwrap();
        assert!(lock::mutex(&doc.inner.animation).has_active());

        runtime.advance_time(Duration::from_secs(2));

        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.25);
        assert!(!lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn a_background_transition_interpolates_in_oklch() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let mut style = Style::new();
        style.background(Color::red());
        doc.set_style(node, &style).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::new(
                TransitionProperty::Background,
                Duration::from_secs(1),
                Easing::Linear,
            ),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.background(Color::blue());
        })
        .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        let expected = ResolvedColor::red().mix(ResolvedColor::blue(), 0.5);
        assert_eq!(doc.resolved_style(node).unwrap().background, Some(expected));

        runtime.advance_time(Duration::from_secs(1));
        assert_eq!(
            doc.resolved_style(node).unwrap().background,
            Some(ResolvedColor::blue())
        );
    }

    #[test]
    fn a_width_transition_reflows_siblings_per_frame() {
        let doc = Document::new().unwrap();
        let a = doc.create_box().unwrap();
        let b = doc.create_box().unwrap();
        doc.append_child(doc.root(), a).unwrap();
        doc.append_child(doc.root(), b).unwrap();
        let mut a_style = Style::new();
        a_style.width(Length::Pixels(2));
        a_style.height(Length::Pixels(1));
        a_style.flex_shrink(0.0);
        doc.set_style(a, &a_style).unwrap();
        let mut b_style = Style::new();
        b_style.width(Length::Pixels(1));
        b_style.height(Length::Pixels(1));
        doc.set_style(b, &b_style).unwrap();
        doc.set_transition(
            a,
            TransitionConfig::new(
                TransitionProperty::Width,
                Duration::from_secs(1),
                Easing::Linear,
            ),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        let doc = runtime.document().clone();
        assert_eq!(doc.get_node(b).unwrap().layout.unwrap().rect.x, 2);

        doc.update_style(a, |style| {
            style.width(Length::Pixels(10));
        })
        .unwrap();

        runtime.advance_time(Duration::from_millis(250));
        runtime.render().unwrap();
        assert_eq!(doc.get_node(a).unwrap().layout.unwrap().rect.width, 4);
        assert_eq!(doc.get_node(b).unwrap().layout.unwrap().rect.x, 4);

        // 300ms in, the interpolated width is 4.4 cells — it stays 4 on screen
        // until the fraction crosses the rounding boundary.
        runtime.advance_time(Duration::from_millis(50));
        runtime.render().unwrap();
        assert_eq!(doc.get_node(a).unwrap().layout.unwrap().rect.width, 4);

        runtime.advance_time(Duration::from_secs(1));
        runtime.render().unwrap();
        assert_eq!(doc.get_node(a).unwrap().layout.unwrap().rect.width, 10);
        assert_eq!(doc.get_node(b).unwrap().layout.unwrap().rect.x, 10);
        assert!(!lock::mutex(&doc.inner.animation).has_active());
        assert!(
            lock::mutex(&doc.inner.animation)
                .layout_animating_nodes()
                .is_empty()
        );
    }

    #[test]
    fn an_absolute_position_transition_glides_across_cells() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let mut style = Style::new();
        style.width(Length::Pixels(1));
        style.height(Length::Pixels(1));
        style.background(Color::red());
        style.position(Position::Absolute { x: 0, y: 0 });
        doc.set_style(node, &style).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::new(
                TransitionProperty::Position,
                Duration::from_secs(1),
                Easing::Linear,
            ),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 12, 3);
        runtime.render().unwrap();
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.position(Position::Absolute { x: 8, y: 0 });
        })
        .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        runtime.render().unwrap();
        let midway = runtime.get_cell(4, 0).unwrap().bg.unwrap();
        assert!(midway.r > midway.g && midway.r > midway.b);
        assert!(runtime.get_cell(0, 0).unwrap().bg.is_none());

        runtime.advance_time(Duration::from_secs(1));
        runtime.render().unwrap();
        let settled = runtime.get_cell(8, 0).unwrap().bg.unwrap();
        assert!(settled.r > settled.g && settled.r > settled.b);
        assert!(runtime.get_cell(4, 0).unwrap().bg.is_none());
    }

    #[test]
    fn focus_change_transitions_the_focus_style() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_focusable(node, true).unwrap();
        let mut focus_style = Style::new();
        focus_style.opacity(0.5);
        doc.set_focus_style(node, &focus_style).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();

        doc.focus(node).unwrap();
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 1.0);

        runtime.advance_time(Duration::from_millis(500));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.75);

        // Blurring reverses from the displayed value — and as a pure reversal it
        // covers the half already traveled in half the duration.
        doc.blur();
        runtime.advance_time(Duration::from_millis(250));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.875);

        runtime.advance_time(Duration::from_millis(250));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 1.0);
        assert!(!lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn disabling_a_container_transitions_a_configured_descendant() {
        let doc = Document::new().unwrap();
        let container = doc.create_box().unwrap();
        let child = doc.create_box().unwrap();
        doc.append_child(doc.root(), container).unwrap();
        doc.append_child(container, child).unwrap();
        let mut disabled_style = Style::new();
        disabled_style.opacity(0.3);
        doc.set_disabled_style(child, &disabled_style).unwrap();
        doc.set_transition(
            child,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();

        doc.set_disabled(container, true).unwrap();
        runtime.advance_time(Duration::from_millis(500));
        assert_eq!(doc.resolved_style(child).unwrap().opacity, 0.65);

        runtime.advance_time(Duration::from_secs(1));
        assert_eq!(doc.resolved_style(child).unwrap().opacity, 0.3);
    }

    #[test]
    fn pressing_a_node_transitions_the_active_style() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let mut active_style = Style::new();
        active_style.opacity(0.6);
        doc.set_active_style(node, &active_style).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();

        doc.set_active(node, true).unwrap();
        runtime.advance_time(Duration::from_millis(500));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 0.8);
    }

    #[test]
    fn a_pseudo_style_not_touching_the_property_starts_no_transition() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_focusable(node, true).unwrap();
        let mut focus_style = Style::new();
        focus_style.bold(true);
        doc.set_focus_style(node, &focus_style).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let _runtime = HeadlessRuntime::new(doc.clone(), 10, 3);
        doc.focus(node).unwrap();

        assert!(!lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn a_keyframe_animation_samples_between_keyframes() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.animate(
            node,
            KeyframeAnimation::new(Duration::from_secs(1))
                .keyframe(0.0, [AnimatableProperty::Opacity(1.0)])
                .keyframe(50.0, [AnimatableProperty::Opacity(0.2)])
                .keyframe(100.0, [AnimatableProperty::Opacity(1.0)]),
        )
        .unwrap();

        runtime.advance_time(Duration::from_millis(250));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.6).abs() < 1e-9);

        runtime.advance_time(Duration::from_millis(250));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.2).abs() < 1e-9);

        // After the last iteration the animation is removed and the node
        // returns to its underlying style.
        runtime.advance_time(Duration::from_secs(1));
        assert_eq!(doc.resolved_style(node).unwrap().opacity, 1.0);
        assert!(!lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn alternate_direction_reverses_odd_iterations() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.animate(
            node,
            KeyframeAnimation::from_to(
                Duration::from_secs(1),
                [AnimatableProperty::Opacity(0.0)],
                [AnimatableProperty::Opacity(1.0)],
            )
            .iterations(4)
            .direction(AnimationDirection::Alternate),
        )
        .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.5).abs() < 1e-9);

        // 1.25s in: the second iteration plays backward, so 25% in reads 0.75.
        runtime.advance_time(Duration::from_millis(750));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.75).abs() < 1e-9);
    }

    #[test]
    fn an_infinite_animation_stays_active_and_coalesces_iterations() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let iterations = Arc::new(Mutex::new(Vec::new()));
        let seen = iterations.clone();
        doc.on_animation_iteration(node, move |event| {
            seen.lock().unwrap().push(event.iteration);
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.animate(
            node,
            KeyframeAnimation::from_to(
                Duration::from_secs(1),
                [AnimatableProperty::Opacity(0.0)],
                [AnimatableProperty::Opacity(1.0)],
            )
            .infinite(),
        )
        .unwrap();

        // Ten iterations pass within one upkeep: boundaries coalesce into a
        // single event carrying the latest count, and the animation stays active.
        runtime.advance_time(Duration::from_millis(10_500));
        assert_eq!(*iterations.lock().unwrap(), vec![10]);
        assert!(lock::mutex(&doc.inner.animation).has_active());
    }

    #[test]
    fn pausing_freezes_values_and_resuming_continues() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let handle = doc
            .animate(
                node,
                KeyframeAnimation::from_to(
                    Duration::from_secs(1),
                    [AnimatableProperty::Opacity(0.0)],
                    [AnimatableProperty::Opacity(1.0)],
                ),
            )
            .unwrap();

        runtime.advance_time(Duration::from_millis(250));
        assert!(doc.pause_animation(handle));

        // A paused animation holds its value and drives no frames.
        runtime.advance_time(Duration::from_secs(5));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.25).abs() < 1e-9);
        assert!(!lock::mutex(&doc.inner.animation).has_active());

        assert!(doc.resume_animation(handle));
        runtime.advance_time(Duration::from_millis(250));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cancelling_restores_the_underlying_style_without_an_end_event() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let ends = Arc::new(Mutex::new(0));
        let seen = ends.clone();
        doc.on_animation_end(doc.root(), move |_| {
            *seen.lock().unwrap() += 1;
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let handle = doc
            .animate(
                node,
                KeyframeAnimation::from_to(
                    Duration::from_secs(1),
                    [AnimatableProperty::Opacity(0.0)],
                    [AnimatableProperty::Opacity(1.0)],
                ),
            )
            .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        assert!(doc.cancel_animation(handle));
        assert!(!doc.cancel_animation(handle));

        assert_eq!(doc.resolved_style(node).unwrap().opacity, 1.0);
        runtime.advance_time(Duration::from_secs(2));
        assert_eq!(*ends.lock().unwrap(), 0);
    }

    #[test]
    fn animation_end_fires_once_and_bubbles_with_the_handle() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let ends = Arc::new(Mutex::new(Vec::new()));
        let seen = ends.clone();
        doc.on_animation_end(doc.root(), move |event| {
            seen.lock()
                .unwrap()
                .push((event.target(), event.phase(), event.handle));
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let handle = doc
            .animate(
                node,
                KeyframeAnimation::from_to(
                    Duration::from_secs(1),
                    [AnimatableProperty::Opacity(0.0)],
                    [AnimatableProperty::Opacity(1.0)],
                )
                .iterations(2),
            )
            .unwrap();

        runtime.advance_time(Duration::from_millis(2_100));
        assert_eq!(
            *ends.lock().unwrap(),
            vec![(node, EventPhase::Bubble, handle)]
        );

        runtime.advance_time(Duration::from_secs(1));
        assert_eq!(ends.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_lone_keyframe_uses_the_underlying_value_as_implicit_endpoints() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.animate(
            node,
            KeyframeAnimation::new(Duration::from_secs(1))
                .keyframe(50.0, [AnimatableProperty::Opacity(0.0)]),
        )
        .unwrap();

        // Base opacity is 1.0: the track dips to 0 at 50% and returns.
        runtime.advance_time(Duration::from_millis(250));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.5).abs() < 1e-9);
        runtime.advance_time(Duration::from_millis(500));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.5).abs() < 1e-9);
    }

    #[test]
    fn animations_override_transitions_on_conflict() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.opacity(0.0);
        })
        .unwrap();
        doc.animate(
            node,
            KeyframeAnimation::from_to(
                Duration::from_secs(1),
                [AnimatableProperty::Opacity(0.9)],
                [AnimatableProperty::Opacity(0.7)],
            ),
        )
        .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        assert!((doc.resolved_style(node).unwrap().opacity - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_keyframe_width_animation_drives_layout() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        let mut style = Style::new();
        style.width(Length::Pixels(4));
        style.height(Length::Pixels(1));
        doc.set_style(node, &style).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        let doc = runtime.document().clone();
        doc.animate(
            node,
            KeyframeAnimation::from_to(
                Duration::from_secs(1),
                [AnimatableProperty::Width(Length::Pixels(2))],
                [AnimatableProperty::Width(Length::Pixels(10))],
            ),
        )
        .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        runtime.render().unwrap();
        assert_eq!(doc.get_node(node).unwrap().layout.unwrap().rect.width, 6);

        // Ending settles layout back on the underlying width.
        runtime.advance_time(Duration::from_secs(1));
        runtime.render().unwrap();
        assert_eq!(doc.get_node(node).unwrap().layout.unwrap().rect.width, 4);
    }

    #[test]
    fn a_frames_node_advances_exactly_on_interval_boundaries() {
        let doc = Document::new().unwrap();
        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let spinner = doc
            .create_frames(["A", "B", "C"], Duration::from_millis(100))
            .unwrap();
        doc.append_child(doc.root(), spinner).unwrap();

        runtime.render().unwrap();
        assert_eq!(doc.current_frame(spinner).unwrap(), 0);
        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "A");

        // One nanosecond short of the boundary still shows the first frame.
        runtime.advance_time(Duration::from_millis(100) - Duration::from_nanos(1));
        runtime.render().unwrap();
        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "A");

        runtime.advance_time(Duration::from_nanos(1));
        runtime.render().unwrap();
        assert_eq!(doc.current_frame(spinner).unwrap(), 1);
        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "B");

        // The cycle wraps: 350ms in, the fourth flip returns to the first frame.
        runtime.advance_time(Duration::from_millis(250));
        runtime.render().unwrap();
        assert_eq!(doc.current_frame(spinner).unwrap(), 0);
        assert_eq!(runtime.get_cell(0, 0).unwrap().text, "A");
    }

    #[test]
    fn a_frames_node_is_measured_on_its_largest_frame() {
        let doc = Document::new().unwrap();
        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let spinner = doc
            .create_frames([".", "..."], Duration::from_millis(100))
            .unwrap();
        doc.append_child(doc.root(), spinner).unwrap();

        runtime.render().unwrap();
        assert_eq!(doc.get_node(spinner).unwrap().layout.unwrap().rect.width, 3);

        // Replacing the frames re-measures on the new largest frame.
        doc.set_frames(spinner, ["....."]).unwrap();
        runtime.render().unwrap();
        assert_eq!(doc.get_node(spinner).unwrap().layout.unwrap().rect.width, 5);
    }

    #[test]
    fn a_lone_frames_node_paces_by_its_interval_not_the_tick() {
        let doc = Document::new().unwrap();
        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        let spinner = doc
            .create_frames(["A", "B"], Duration::from_millis(100))
            .unwrap();
        doc.append_child(doc.root(), spinner).unwrap();

        {
            let driver = lock::mutex(&doc.inner.animation);
            assert!(driver.has_active());
            assert!(!driver.has_smooth_active());
            let flip = driver.next_frames_flip(doc.now()).unwrap();
            assert_eq!(flip.duration_since(doc.now()), Duration::from_millis(100));
        }

        // Mid-interval, the next flip is the remainder — not a fixed tick.
        runtime.advance_time(Duration::from_millis(30));
        let driver = lock::mutex(&doc.inner.animation);
        let flip = driver.next_frames_flip(doc.now()).unwrap();
        assert_eq!(flip.duration_since(doc.now()), Duration::from_millis(70));
    }

    #[test]
    fn a_single_frame_or_zero_interval_drives_no_rendering() {
        let doc = Document::new().unwrap();
        let runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();

        let still = doc
            .create_frames(["only"], Duration::from_millis(100))
            .unwrap();
        doc.append_child(doc.root(), still).unwrap();
        assert!(!lock::mutex(&doc.inner.animation).has_active());

        let frozen = doc.create_frames(["a", "b"], Duration::ZERO).unwrap();
        doc.append_child(doc.root(), frozen).unwrap();
        assert!(!lock::mutex(&doc.inner.animation).has_active());
        assert_eq!(doc.current_frame(frozen).unwrap(), 0);
    }

    #[test]
    fn frames_apis_reject_non_frames_nodes() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("plain").unwrap();

        assert!(matches!(
            doc.current_frame(text),
            Err(crate::TuidomError::NodeNotFrames { .. })
        ));
        assert!(matches!(
            doc.set_frames(text, ["a"]),
            Err(crate::TuidomError::NodeNotFrames { .. })
        ));
    }

    #[test]
    fn transition_end_fires_once_at_completion_and_bubbles() {
        let doc = Document::new().unwrap();
        let container = doc.create_box().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), container).unwrap();
        doc.append_child(container, node).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let fired = Arc::new(Mutex::new(Vec::new()));
        let on_node = fired.clone();
        doc.on_transition_end(node, move |event| {
            on_node.lock().unwrap().push(("node", event.phase()));
        })
        .unwrap();
        let on_container = fired.clone();
        doc.on_transition_end(container, move |event| {
            assert_eq!(event.target(), node);
            assert_eq!(event.current_target(), container);
            on_container
                .lock()
                .unwrap()
                .push(("container", event.phase()));
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime
            .document()
            .update_style(node, |style| {
                style.opacity(0.0);
            })
            .unwrap();

        runtime.advance_time(Duration::from_millis(500));
        assert!(fired.lock().unwrap().is_empty());

        runtime.advance_time(Duration::from_millis(600));
        assert_eq!(
            *fired.lock().unwrap(),
            vec![
                ("node", EventPhase::Target),
                ("container", EventPhase::Bubble)
            ]
        );

        runtime.advance_time(Duration::from_secs(1));
        assert_eq!(fired.lock().unwrap().len(), 2);
    }

    #[test]
    fn removing_a_node_mid_transition_fires_no_end_event() {
        let doc = Document::new().unwrap();
        let node = doc.create_box().unwrap();
        doc.append_child(doc.root(), node).unwrap();
        doc.set_transition(
            node,
            TransitionConfig::opacity(Duration::from_secs(1), Easing::Linear),
        )
        .unwrap();

        let fired = Arc::new(Mutex::new(0));
        let fired_in_handler = fired.clone();
        doc.on_transition_end(doc.root(), move |_| {
            *fired_in_handler.lock().unwrap() += 1;
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        let doc = runtime.document().clone();
        doc.update_style(node, |style| {
            style.opacity(0.0);
        })
        .unwrap();

        doc.remove_child(doc.root(), node).unwrap();
        runtime.advance_time(Duration::from_secs(2));

        assert_eq!(*fired.lock().unwrap(), 0);
    }

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

    #[test]
    fn render_dispatches_post_frame_with_frame_metrics() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("Hi").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_handler = frames.clone();
        doc.on_post_frame(move |event| {
            frames_for_handler
                .lock()
                .unwrap()
                .push(event.metrics.frame_time);
        });

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        assert!(frames.lock().unwrap().is_empty());

        runtime.render().unwrap();
        runtime.render().unwrap();

        assert_eq!(frames.lock().unwrap().len(), 2);
        assert_eq!(runtime.document().performance_snapshot().frame_count, 2);
    }

    #[test]
    fn removed_post_frame_listener_stops_firing() {
        let doc = Document::new().unwrap();
        let calls = Arc::new(Mutex::new(0));
        let calls_for_handler = calls.clone();
        let handle = doc.on_post_frame(move |_| {
            *calls_for_handler.lock().unwrap() += 1;
        });

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        assert!(runtime.document().remove_listener(handle));
        runtime.render().unwrap();

        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn simulated_mouse_move_focuses_focusable_node_under_pointer() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("A").unwrap();
        doc.append_child(doc.root(), text).unwrap();
        doc.set_focusable(text, true).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_move(0, 0);

        assert_eq!(runtime.document().focused(), Some(text));
    }

    #[test]
    fn simulated_mouse_move_focuses_focusable_ancestor_of_hit_node() {
        let doc = Document::new().unwrap();
        let parent = doc.create_box().unwrap();
        let text = doc.create_text("A").unwrap();
        doc.append_child(doc.root(), parent).unwrap();
        doc.append_child(parent, text).unwrap();
        doc.set_focusable(parent, true).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_move(0, 0);

        assert_eq!(runtime.document().focused(), Some(parent));
    }

    #[test]
    fn drag_movement_does_not_move_hover_focus() {
        let doc = Document::new().unwrap();
        let first = doc.create_text("A").unwrap();
        let second = doc.create_text("B").unwrap();
        doc.append_child(doc.root(), first).unwrap();
        doc.append_child(doc.root(), second).unwrap();
        doc.set_focusable(first, true).unwrap();
        doc.set_focusable(second, true).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_move(0, 0);
        assert_eq!(runtime.document().focused(), Some(first));

        runtime.simulate_mouse_down(0, 0, MouseButton::Left);
        runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
        assert_eq!(runtime.document().focused(), Some(first));
    }

    #[test]
    fn simulated_click_targets_rendered_node() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("A").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let seen = Arc::new(Mutex::new(None));
        let seen_for_handler = seen.clone();
        doc.on_click(text, move |event| {
            *seen_for_handler.lock().unwrap() = Some(event.target());
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_click(0, 0);

        assert_eq!(*seen.lock().unwrap(), Some(text));
    }

    #[test]
    fn simulated_click_uses_stop_propagation() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("A").unwrap();
        doc.append_child(root, text).unwrap();

        let calls = Arc::new(Mutex::new(Vec::new()));
        let child_calls = calls.clone();
        doc.on_click(text, move |event| {
            child_calls.lock().unwrap().push("child");
            event.stop_propagation();
        })
        .unwrap();

        let root_calls = calls.clone();
        doc.on_click(root, move |_| {
            root_calls.lock().unwrap().push("root");
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_click(0, 0);

        assert_eq!(*calls.lock().unwrap(), vec!["child"]);
    }

    #[test]
    fn simulated_mouse_drag_does_not_synthesize_click_when_release_differs() {
        let doc = Document::new().unwrap();
        let calls = Arc::new(Mutex::new(0));
        let calls_for_handler = calls.clone();
        doc.on_click(doc.root(), move |_| {
            *calls_for_handler.lock().unwrap() += 1;
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (1, 0));

        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[test]
    fn drag_selects_across_sibling_text_nodes() {
        let doc = Document::new().unwrap();
        let hello = doc.create_text("hello").unwrap();
        let world = doc.create_text("world").unwrap();
        doc.append_child(doc.root(), hello).unwrap();
        doc.append_child(doc.root(), world).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((1, 0), (7, 0));

        let (start, end) = runtime.document().selection().unwrap();
        assert_eq!(
            start,
            SelectionPoint {
                node: hello,
                offset: 1
            }
        );
        assert_eq!(
            end,
            SelectionPoint {
                node: world,
                offset: 3
            }
        );
    }

    #[test]
    fn reverse_drag_produces_the_same_ordered_range() {
        let doc = Document::new().unwrap();
        let hello = doc.create_text("hello").unwrap();
        let world = doc.create_text("world").unwrap();
        doc.append_child(doc.root(), hello).unwrap();
        doc.append_child(doc.root(), world).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((7, 0), (1, 0));

        let (start, end) = runtime.document().selection().unwrap();
        // Both endpoint cells are included regardless of drag direction: the range
        // starts at the earlier cell and ends past the glyph under the later one.
        assert_eq!(
            start,
            SelectionPoint {
                node: hello,
                offset: 1
            }
        );
        assert_eq!(
            end,
            SelectionPoint {
                node: world,
                offset: 3
            }
        );
    }

    #[test]
    fn drag_is_confined_to_its_selection_boundary() {
        let doc = Document::new().unwrap();
        let mut boundary_style = Style::new();
        boundary_style.selection_boundary(true);

        let sidebar = doc.create_box().unwrap();
        doc.set_style(sidebar, &boundary_style).unwrap();
        let main = doc.create_box().unwrap();
        doc.set_style(main, &boundary_style).unwrap();
        let side_text = doc.create_text("side").unwrap();
        let main_text = doc.create_text("main content").unwrap();
        doc.append_child(doc.root(), sidebar).unwrap();
        doc.append_child(doc.root(), main).unwrap();
        doc.append_child(sidebar, side_text).unwrap();
        doc.append_child(main, main_text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 30, 3);
        runtime.render().unwrap();
        // "side" occupies x 0..4, "main content" starts at x 4. Drag well into main.
        runtime.simulate_mouse_drag((1, 0), (8, 0));

        let (start, end) = runtime.document().selection().unwrap();
        assert_eq!(start.node, side_text);
        assert_eq!(end.node, side_text);
        // The focus snapped to the end of the boundary's text, not into `main`.
        assert_eq!(end.offset, 4);
    }

    #[test]
    fn two_columns_in_a_shared_boundary_select_the_dom_order_range() {
        let doc = Document::new().unwrap();
        let left = doc.create_box().unwrap();
        let right = doc.create_box().unwrap();
        let one = doc.create_text("one\ntwo\nthree").unwrap();
        let four = doc.create_text("four\nfive\nsix").unwrap();
        doc.append_child(doc.root(), left).unwrap();
        doc.append_child(doc.root(), right).unwrap();
        doc.append_child(left, one).unwrap();
        doc.append_child(right, four).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 4);
        runtime.render().unwrap();
        // Anchor after "tw" in the left column, focus after "fi" in the right one.
        // Left column is 5 wide ("three"), so the right column starts at x 5.
        runtime.simulate_mouse_drag((2, 1), (6, 1));

        let (start, end) = runtime.document().selection().unwrap();
        // Everything between the two points in DOM order: the tail of the left
        // column and the head of the right one, including "three" untouched by the
        // pointer — browser semantics.
        assert_eq!(
            start,
            SelectionPoint {
                node: one,
                offset: 6
            }
        );
        assert_eq!(
            end,
            SelectionPoint {
                node: four,
                offset: 7
            }
        );
    }

    #[test]
    fn drag_from_empty_space_snaps_to_nearest_text() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 5);
        runtime.render().unwrap();
        // Anchor below the text and past its end, focus on the first glyph.
        runtime.simulate_mouse_drag((8, 2), (0, 0));

        let (start, end) = runtime.document().selection().unwrap();
        assert_eq!(
            start,
            SelectionPoint {
                node: text,
                offset: 0
            }
        );
        // The anchor snapped to the line's end offset.
        assert_eq!(
            end,
            SelectionPoint {
                node: text,
                offset: 5
            }
        );
    }

    #[test]
    fn drag_over_wide_glyph_continuation_selects_the_glyph() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("日本").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        // x=1 is the continuation cell of 日 (width 2). Drag within the one glyph.
        runtime.simulate_mouse_down(0, 0, MouseButton::Left);
        runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
        runtime.simulate_mouse_up(1, 0, MouseButton::Left);

        let (start, end) = runtime.document().selection().unwrap();
        assert_eq!(
            start,
            SelectionPoint {
                node: text,
                offset: 0
            }
        );
        assert_eq!(
            end,
            SelectionPoint {
                node: text,
                offset: "日".len()
            }
        );
    }

    #[test]
    fn selected_text_renders_reverse_video_by_default() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        let mut style = Style::new();
        style.color(Color::white());
        style.background(Color::black());
        doc.set_style(text, &style).unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (2, 0));
        runtime.render().unwrap();

        let selected = runtime.get_cell(1, 0).unwrap();
        assert_eq!(selected.fg, Some(ScreenColor::from_rgb(0, 0, 0)));
        assert_eq!(selected.bg, Some(ScreenColor::from_rgb(255, 255, 255)));

        let unselected = runtime.get_cell(4, 0).unwrap();
        assert_eq!(unselected.fg, Some(ScreenColor::from_rgb(255, 255, 255)));
        assert_eq!(unselected.bg, Some(ScreenColor::from_rgb(0, 0, 0)));
    }

    #[test]
    fn selected_text_uses_explicit_selection_colors() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        let mut style = Style::new();
        style.color(Color::black());
        style.background(Color::white());
        style.selection_bg(Color::black());
        style.selection_fg(Color::white());
        doc.set_style(text, &style).unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (2, 0));
        runtime.render().unwrap();

        let selected = runtime.get_cell(1, 0).unwrap();
        assert_eq!(selected.fg, Some(ScreenColor::from_rgb(255, 255, 255)));
        assert_eq!(selected.bg, Some(ScreenColor::from_rgb(0, 0, 0)));

        let unselected = runtime.get_cell(4, 0).unwrap();
        assert_eq!(unselected.fg, Some(ScreenColor::from_rgb(0, 0, 0)));
        assert_eq!(unselected.bg, Some(ScreenColor::from_rgb(255, 255, 255)));
    }

    #[test]
    fn wide_glyph_selection_highlights_both_cells() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("日本").unwrap();
        let mut style = Style::new();
        style.color(Color::white());
        style.background(Color::black());
        doc.set_style(text, &style).unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        // Select only 日 (cells 0 and 1).
        runtime.simulate_mouse_down(0, 0, MouseButton::Left);
        runtime.simulate_mouse_drag_move(1, 0, MouseButton::Left);
        runtime.simulate_mouse_up(1, 0, MouseButton::Left);
        runtime.render().unwrap();

        // Head and continuation cell both carry the swapped background.
        let head = runtime.get_cell(0, 0).unwrap();
        let continuation = runtime.get_cell(1, 0).unwrap();
        assert_eq!(head.bg, Some(ScreenColor::from_rgb(255, 255, 255)));
        assert_eq!(continuation.bg, Some(ScreenColor::from_rgb(255, 255, 255)));

        // 本 stays unswapped.
        let outside = runtime.get_cell(2, 0).unwrap();
        assert_eq!(outside.bg, Some(ScreenColor::from_rgb(0, 0, 0)));
    }

    #[test]
    fn drag_inside_input_drives_input_selection() {
        let doc = Document::new().unwrap();
        let input = doc.create_input("hello world").unwrap();
        doc.append_child(doc.root(), input).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (4, 0));

        let doc = runtime.document();
        // Terminal-inclusive endpoints: cells 0..=4 select bytes 0..5.
        assert_eq!(doc.input_selection(input).unwrap(), Some(0..5));
        // The cursor follows the drag's focus end.
        assert_eq!(doc.input_cursor(input).unwrap(), 4);
        // The input is its own boundary: no document selection exists.
        assert_eq!(doc.selection(), None);
        assert_eq!(doc.get_selection(), None);
    }

    #[test]
    fn click_positions_input_cursor() {
        let doc = Document::new().unwrap();
        let input = doc.create_input("hello").unwrap();
        doc.append_child(doc.root(), input).unwrap();
        doc.set_input_selection(input, 0..4).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_click(3, 0);

        let doc = runtime.document();
        assert_eq!(doc.input_cursor(input).unwrap(), 3);
        assert_eq!(doc.input_selection(input).unwrap(), None);
    }

    #[test]
    fn document_drag_does_not_cross_into_an_input() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("label").unwrap();
        let input = doc.create_input("value").unwrap();
        doc.append_child(doc.root(), text).unwrap();
        doc.append_child(doc.root(), input).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        // "label" is cells 0..5, the input starts at cell 5. Drag deep into it.
        runtime.simulate_mouse_drag((1, 0), (8, 0));

        let doc = runtime.document();
        let (start, end) = doc.selection().unwrap();
        assert_eq!(start.node, text);
        assert_eq!(end.node, text);
        assert_eq!(end.offset, 5);
        // The input's own selection is untouched by a document drag.
        assert_eq!(doc.input_selection(input).unwrap(), None);
    }

    #[test]
    fn focused_masked_input_renders_selection_over_mask_glyphs() {
        let doc = Document::new().unwrap();
        let input = doc.create_input("secret").unwrap();
        doc.set_input_mask(input, Some('*')).unwrap();
        let mut style = Style::new();
        style.color(Color::white());
        style.background(Color::black());
        doc.set_style(input, &style).unwrap();
        doc.append_child(doc.root(), input).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.document().focus(input).unwrap();
        runtime.simulate_mouse_drag((0, 0), (2, 0));
        runtime.render().unwrap();

        // Selected mask glyphs render reverse video; the value stays masked.
        let selected = runtime.get_cell(1, 0).unwrap();
        assert_eq!(selected.text, "*");
        assert_eq!(selected.bg, Some(ScreenColor::from_rgb(255, 255, 255)));

        let unselected = runtime.get_cell(4, 0).unwrap();
        assert_eq!(unselected.text, "*");
        assert_eq!(unselected.bg, Some(ScreenColor::from_rgb(0, 0, 0)));
    }

    #[test]
    fn drag_in_a_scrolled_container_selects_the_scrolled_content() {
        let doc = Document::new().unwrap();
        let container = doc.create_box().unwrap();
        let mut style = Style::new();
        style.flex_direction(crate::style::FlexDirection::Column);
        style.overflow_y(crate::style::Overflow::Scroll);
        style.scrollbar_show(crate::style::ScrollbarShow::Never);
        doc.set_style(container, &style).unwrap();
        doc.append_child(doc.root(), container).unwrap();

        let mut lines = Vec::new();
        for i in 0..8 {
            let text = doc.create_text(format!("line{i}")).unwrap();
            doc.append_child(container, text).unwrap();
            lines.push(text);
        }

        let mut runtime = HeadlessRuntime::new(doc, 10, 4);
        runtime.render().unwrap();
        runtime.document().scroll_to(container, 0, 2).unwrap();
        runtime.render().unwrap();
        assert_eq!(runtime.get_cell(4, 0).unwrap().text, "2");

        // Screen row 0 now shows "line2": the mapping works on scroll-translated
        // paint geometry, so the drag lands in the node that scrolled into view.
        runtime.simulate_mouse_drag((0, 0), (3, 0));

        let (start, end) = runtime.document().selection().unwrap();
        assert_eq!(
            start,
            SelectionPoint {
                node: lines[2],
                offset: 0
            }
        );
        assert_eq!(
            end,
            SelectionPoint {
                node: lines[2],
                offset: 4
            }
        );
    }

    #[test]
    fn removing_a_selected_node_clears_the_selection() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_handler = events.clone();
        doc.on_selection_change(move |event| {
            events_for_handler
                .lock()
                .unwrap()
                .push(event.selection.is_some());
        });

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (3, 0));
        assert!(runtime.document().selection().is_some());

        let doc = runtime.document();
        doc.remove_child(doc.root(), text).unwrap();
        assert_eq!(doc.selection(), None);
        assert_eq!(*events.lock().unwrap(), vec![true, false]);
    }

    #[test]
    fn shrinking_selected_text_clamps_or_clears_the_selection() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello world").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((2, 0), (8, 0));
        let doc = runtime.document();
        assert_eq!(doc.selection().unwrap().1.offset, 9);

        // Shrink but keep part of the range: offsets clamp to the new content.
        doc.set_text_content(text, "hell").unwrap();
        let (start, end) = doc.selection().unwrap();
        assert_eq!(start.offset, 2);
        assert_eq!(end.offset, 4);

        // Shrink past the whole range: the collapsed selection is cleared.
        doc.set_text_content(text, "h").unwrap();
        assert_eq!(doc.selection(), None);
    }

    #[test]
    fn get_selection_joins_rows_with_newlines() {
        let doc = Document::new().unwrap();
        let mut column = Style::new();
        column.flex_direction(crate::style::FlexDirection::Column);
        doc.set_style(doc.root(), &column).unwrap();

        let first = doc.create_text("first line").unwrap();
        let second = doc.create_text("second").unwrap();
        doc.append_child(doc.root(), first).unwrap();
        doc.append_child(doc.root(), second).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 4);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((6, 0), (2, 1));

        assert_eq!(
            runtime.document().get_selection().as_deref(),
            Some("line\nsec")
        );
    }

    #[test]
    fn get_selection_concatenates_nodes_sharing_a_row() {
        let doc = Document::new().unwrap();
        let hello = doc.create_text("hello ").unwrap();
        let world = doc.create_text("world").unwrap();
        doc.append_child(doc.root(), hello).unwrap();
        doc.append_child(doc.root(), world).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (10, 0));

        assert_eq!(
            runtime.document().get_selection().as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn selection_change_event_fires_on_change_and_clear_only() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_handler = events.clone();
        doc.on_selection_change(move |event| {
            events_for_handler
                .lock()
                .unwrap()
                .push(event.selection.is_some());
        });

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();

        // A click with nothing selected clears nothing: no event.
        runtime.simulate_click(0, 0);
        assert_eq!(events.lock().unwrap().len(), 0);

        // One drag move → one change event; the release changes nothing.
        runtime.simulate_mouse_drag((0, 0), (3, 0));
        assert_eq!(*events.lock().unwrap(), vec![true]);

        // Clearing a non-empty selection fires with `None`.
        runtime.document().clear_selection();
        assert_eq!(*events.lock().unwrap(), vec![true, false]);

        // Clearing again is a no-op.
        runtime.document().clear_selection();
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[test]
    fn click_clears_the_selection() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        doc.append_child(doc.root(), text).unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (3, 0));
        assert!(runtime.document().selection().is_some());

        runtime.simulate_click(2, 0);
        assert!(runtime.document().selection().is_none());
    }

    #[test]
    fn prevent_default_on_mouse_down_keeps_the_selection() {
        let doc = Document::new().unwrap();
        let text = doc.create_text("hello").unwrap();
        let button = doc.create_text("[ok]").unwrap();
        doc.append_child(doc.root(), text).unwrap();
        doc.append_child(doc.root(), button).unwrap();
        doc.on_mouse_down(button, |event| event.prevent_default())
            .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 20, 3);
        runtime.render().unwrap();
        runtime.simulate_mouse_drag((0, 0), (3, 0));
        let selected = runtime.document().selection();
        assert!(selected.is_some());

        // A press whose default is prevented neither clears nor restarts selection.
        runtime.simulate_click(6, 0);
        assert_eq!(runtime.document().selection(), selected);
    }

    #[test]
    fn simulated_scroll_targets_and_bubbles() {
        let doc = Document::new().unwrap();
        let root = doc.root();
        let text = doc.create_text("A").unwrap();
        doc.append_child(root, text).unwrap();

        let seen = Arc::new(Mutex::new(None));
        let seen_for_handler = seen.clone();
        doc.on_wheel(root, move |event| {
            *seen_for_handler.lock().unwrap() =
                Some((event.target(), event.current_target(), event.delta));
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.render().unwrap();
        runtime.simulate_scroll(0, 0, -2);

        assert_eq!(*seen.lock().unwrap(), Some((text, root, -2)));
    }

    #[test]
    fn simulated_key_and_text_dispatch_key_presses() {
        let doc = Document::new().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let seen_for_handler = seen.clone();
        doc.on_key_press(doc.root(), move |event| {
            if let KeyCode::Char(ch) = event.code {
                seen_for_handler.lock().unwrap().push(ch);
            }
        })
        .unwrap();

        let mut runtime = HeadlessRuntime::new(doc, 10, 3);
        runtime.simulate_key(KeyCode::Char('a'));
        runtime.simulate_text("bc");

        assert_eq!(&*seen.lock().unwrap(), "abc");
    }
}

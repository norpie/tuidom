//! Debug overlay — renders performance stats on top of the screen.
//!
//! Toggled via `Document::toggle_debug_overlay()` (F1 by convention).

use std::time::Duration;

use crate::render::grid::Grid;
use crate::style::color::Rgb;

/// Performance metrics collected each frame.
#[derive(Debug, Clone)]
pub(crate) struct DebugOverlay {
    /// Whether the overlay is visible.
    pub enabled: bool,

    // Current frame metrics
    pub fps: f64,
    pub frame_time: Duration,
    pub layout_time: Duration,
    pub render_time: Duration,
    pub cells_changed: usize,

    // Running averages
    avg_frame_time: Duration,
    avg_layout_time: Duration,
    avg_render_time: Duration,
    avg_cells_changed: f64,

    // Internal tracking
    frame_count: u64,
    total_frame: Duration,
    total_layout: Duration,
    total_render: Duration,
    total_cells: usize,
    last_fps_update: std::time::Instant,
    frames_since_fps: u64,
}

impl DebugOverlay {
    /// Create a new overlay (initially hidden).
    pub fn new() -> Self {
        Self {
            enabled: false,
            fps: 0.0,
            frame_time: Duration::ZERO,
            layout_time: Duration::ZERO,
            render_time: Duration::ZERO,
            cells_changed: 0,

            avg_frame_time: Duration::ZERO,
            avg_layout_time: Duration::ZERO,
            avg_render_time: Duration::ZERO,
            avg_cells_changed: 0.0,

            frame_count: 0,
            total_frame: Duration::ZERO,
            total_layout: Duration::ZERO,
            total_render: Duration::ZERO,
            total_cells: 0,
            last_fps_update: std::time::Instant::now(),
            frames_since_fps: 0,
        }
    }

    /// Record metrics for a completed frame.
    pub fn record(&mut self, frame: Duration, layout: Duration, render: Duration, cells: usize) {
        self.frame_time = frame;
        self.layout_time = layout;
        self.render_time = render;
        self.cells_changed = cells;

        // Running totals
        self.frame_count += 1;
        self.total_frame += frame;
        self.total_layout += layout;
        self.total_render += render;
        self.total_cells += cells;

        // Averages
        let n = self.frame_count as f64;
        self.avg_frame_time = Duration::from_secs_f64(self.total_frame.as_secs_f64() / n);
        self.avg_layout_time = Duration::from_secs_f64(self.total_layout.as_secs_f64() / n);
        self.avg_render_time = Duration::from_secs_f64(self.total_render.as_secs_f64() / n);
        self.avg_cells_changed = self.total_cells as f64 / n;

        // FPS (updated every ~500ms)
        self.frames_since_fps += 1;
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_fps_update);
        if elapsed >= Duration::from_millis(500) {
            self.fps = self.frames_since_fps as f64 / elapsed.as_secs_f64();
            self.frames_since_fps = 0;
            self.last_fps_update = now;
        }
    }

    /// Paint the overlay text onto the grid (top-right corner).
    pub fn render(&self, grid: &mut Grid) {
        if !self.enabled {
            return;
        }

        let lines = self.format_lines();
        let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(0);

        // Position: right-aligned, starting from row 0
        let x = grid.width.saturating_sub(max_width as u16 + 1).max(0);
        let fg = Some(Rgb { r: 255, g: 255, b: 255, a: 255 });
        let bg = Some(Rgb { r: 30, g: 30, b: 30, a: 200 });

        // Background strip
        for (i, line) in lines.iter().enumerate() {
            let y = i as u16;
            let cell = crate::render::grid::Cell { ch: ' ', fg: None, bg };
            grid.fill_rect(x, y, max_width as u16, 1, cell);
            grid.write_text(x, y, line, fg, bg);
        }
    }

    fn format_lines(&self) -> Vec<String> {
        vec![
            format!("FPS: {:.0}", self.fps),
            format!(
                "Frame: {:.3}ms (avg: {:.3}ms)",
                ms(self.frame_time),
                ms(self.avg_frame_time)
            ),
            format!(
                "Layout: {:.3}ms (avg: {:.3}ms)",
                ms(self.layout_time),
                ms(self.avg_layout_time)
            ),
            format!(
                "Render: {:.3}ms (avg: {:.3}ms)",
                ms(self.render_time),
                ms(self.avg_render_time)
            ),
            format!(
                "Cells: {} (avg: {:.0})",
                self.cells_changed, self.avg_cells_changed
            ),
        ]
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

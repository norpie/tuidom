//! Debug overlay — renders performance stats on top of the screen.
//!
//! Toggled via `Document::toggle_debug_overlay()` (F1 by convention).

use std::time::Duration;

use crate::render::RenderStats;
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
    pub stats: RenderStats,

    // Running averages
    avg_frame_time: Duration,
    avg_layout_time: Duration,
    avg_grid_time: Duration,
    avg_dom_collect_time: Duration,
    avg_dom_paint_time: Duration,
    avg_overlay_paint_time: Duration,
    avg_diff_time: Duration,
    avg_flush_time: Duration,
    avg_cells_changed: f64,

    // Internal tracking
    frame_count: u64,
    total_frame: Duration,
    total_layout: Duration,
    total_grid: Duration,
    total_dom_collect: Duration,
    total_dom_paint: Duration,
    total_overlay_paint: Duration,
    total_diff: Duration,
    total_flush: Duration,
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
            stats: RenderStats::default(),

            avg_frame_time: Duration::ZERO,
            avg_layout_time: Duration::ZERO,
            avg_grid_time: Duration::ZERO,
            avg_dom_collect_time: Duration::ZERO,
            avg_dom_paint_time: Duration::ZERO,
            avg_overlay_paint_time: Duration::ZERO,
            avg_diff_time: Duration::ZERO,
            avg_flush_time: Duration::ZERO,
            avg_cells_changed: 0.0,

            frame_count: 0,
            total_frame: Duration::ZERO,
            total_layout: Duration::ZERO,
            total_grid: Duration::ZERO,
            total_dom_collect: Duration::ZERO,
            total_dom_paint: Duration::ZERO,
            total_overlay_paint: Duration::ZERO,
            total_diff: Duration::ZERO,
            total_flush: Duration::ZERO,
            total_cells: 0,
            last_fps_update: std::time::Instant::now(),
            frames_since_fps: 0,
        }
    }

    /// Record metrics for a completed frame.
    pub fn record(&mut self, frame: Duration, layout: Duration, stats: RenderStats) {
        self.frame_time = frame;
        self.layout_time = layout;
        self.stats = stats;

        // Running totals
        self.frame_count += 1;
        self.total_frame += frame;
        self.total_layout += layout;
        self.total_grid += stats.grid_time;
        self.total_dom_collect += stats.dom_collect_time;
        self.total_dom_paint += stats.dom_paint_time;
        self.total_overlay_paint += stats.overlay_paint_time;
        self.total_diff += stats.diff_time;
        self.total_flush += stats.flush_time;
        self.total_cells += stats.cells_changed;

        // Averages
        let n = self.frame_count as f64;
        self.avg_frame_time = avg(self.total_frame, n);
        self.avg_layout_time = avg(self.total_layout, n);
        self.avg_grid_time = avg(self.total_grid, n);
        self.avg_dom_collect_time = avg(self.total_dom_collect, n);
        self.avg_dom_paint_time = avg(self.total_dom_paint, n);
        self.avg_overlay_paint_time = avg(self.total_overlay_paint, n);
        self.avg_diff_time = avg(self.total_diff, n);
        self.avg_flush_time = avg(self.total_flush, n);
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
        let height = lines.len() as u16;

        let x = i32::from(grid.width.saturating_sub(max_width as u16 + 1));
        let fg = Some(Rgb {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        });
        let bg = Rgb {
            r: 30,
            g: 30,
            b: 30,
            a: 200,
        };

        // Background strip
        for y in 0..height {
            let cell = crate::render::grid::Cell::empty_with_bg(bg);
            grid.fill_rect(x, i32::from(y), max_width as u16, 1, cell, 1.0);
        }
        for (i, line) in lines.iter().enumerate() {
            grid.write_text(x, i as i32, line, fg, 1.0);
        }
    }

    fn format_lines(&self) -> Vec<String> {
        let avg_render_time = self.avg_grid_time
            + self.avg_dom_collect_time
            + self.avg_dom_paint_time
            + self.avg_overlay_paint_time
            + self.avg_diff_time
            + self.avg_flush_time;
        let dom_time = self.stats.dom_collect_time + self.stats.dom_paint_time;
        let avg_dom_time = self.avg_dom_collect_time + self.avg_dom_paint_time;
        let cells_label = if self.stats.full_redraw {
            "full"
        } else {
            "changed"
        };

        let mut lines = vec![
            format!("FPS:        {:.0}", self.fps),
            format!(
                "Frame:      {:.3}ms (avg: {:.3}ms)",
                ms(self.frame_time),
                ms(self.avg_frame_time)
            ),
            format!(
                "  Layout:   {:.3}ms (avg: {:.3}ms)",
                ms(self.layout_time),
                ms(self.avg_layout_time)
            ),
            format!(
                "  Render:   {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.render_time()),
                ms(avg_render_time),
            ),
            format!(
                "    Grid:   {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.grid_time),
                ms(self.avg_grid_time)
            ),
            format!(
                "    DOM:    {:.3}ms (avg: {:.3}ms)",
                ms(dom_time),
                ms(avg_dom_time)
            ),
            format!(
                "      Collect: {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.dom_collect_time),
                ms(self.avg_dom_collect_time)
            ),
            format!(
                "      Paint:   {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.dom_paint_time),
                ms(self.avg_dom_paint_time)
            ),
        ];

        let paint_profile = self.stats.paint_profile;
        if paint_profile.enabled {
            lines.extend([
                format!(
                    "        Fill:  {:.3}ms / {} cells / {} calls",
                    ms(paint_profile.background_fill_time),
                    paint_profile.filled_cells,
                    paint_profile.background_fill_calls
                ),
                format!(
                    "        Area:  {} requested / {} opaque",
                    paint_profile.requested_fill_cells, paint_profile.opaque_filled_cells
                ),
            ]);
            if let Some(largest) = paint_profile.largest_fill {
                lines.push(format!(
                    "        Max:   {}#{} {}x{}@{},{} / {}/{} cells",
                    largest.node_kind,
                    largest.node_id.index,
                    largest.width,
                    largest.height,
                    largest.x,
                    largest.y,
                    largest.clipped_cells,
                    largest.requested_cells
                ));
            }
            lines.extend([
                format!(
                    "        Text:  {:.3}ms / {} glyphs",
                    ms(paint_profile.text_write_time),
                    paint_profile.glyphs_written
                ),
                format!(
                    "        RGB:   {:.3}ms / {} resolves",
                    ms(paint_profile.rgb_resolve_time),
                    paint_profile.rgb_resolves
                ),
                format!(
                    "        Input: {:.3}ms",
                    ms(paint_profile.input_format_time)
                ),
            ]);
        }

        lines.extend([
            format!(
                "    Debug:  {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.overlay_paint_time),
                ms(self.avg_overlay_paint_time)
            ),
            format!(
                "    Diff:   {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.diff_time),
                ms(self.avg_diff_time)
            ),
        ]);

        let diff_profile = self.stats.diff_profile;
        if diff_profile.enabled {
            lines.extend([
                format!(
                    "      Rows: {} same / {} changed / {} total",
                    diff_profile.unchanged_rows, diff_profile.changed_rows, diff_profile.rows
                ),
                format!(
                    "      Check: {:.3}ms row / {:.3}ms cell / {} cells",
                    ms(diff_profile.row_equality_time),
                    ms(diff_profile.cell_scan_time),
                    diff_profile.cells_compared
                ),
                format!(
                    "      Emit:  {:.3}ms / {} dirty",
                    ms(diff_profile.emit_time),
                    diff_profile.dirty_cells
                ),
            ]);
        }

        lines.extend([
            format!(
                "    Flush:  {:.3}ms (avg: {:.3}ms)",
                ms(self.stats.flush_time),
                ms(self.avg_flush_time)
            ),
            format!(
                "    Cells:  {} {} (avg: {:.0})",
                self.stats.cells_changed, cells_label, self.avg_cells_changed
            ),
        ]);
        lines
    }
}

fn avg(d: Duration, n: f64) -> Duration {
    Duration::from_secs_f64(d.as_secs_f64() / n)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

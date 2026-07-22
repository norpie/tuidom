//! Runtime performance metrics.
//!
//! Metrics are collected as data so applications can decide whether to log,
//! graph, or render a small subset of them in their own UI.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::id::NodeId;

/// Amount of detailed instrumentation collected while rendering.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceDetail {
    /// Collect inexpensive frame, phase, diff, and flush timings.
    #[default]
    Basic,
    /// Also collect detailed paint and diff counters and sub-timings.
    Detailed,
}

impl PerformanceDetail {
    /// Return whether detailed instrumentation should be collected.
    pub fn is_detailed(self) -> bool {
        matches!(self, Self::Detailed)
    }
}

/// Snapshot of currently collected performance metrics.
#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    /// Current instrumentation detail level.
    pub detail: PerformanceDetail,
    /// Most recently calculated frames per second.
    pub fps: f64,
    /// Number of terminal frames recorded so far.
    pub frame_count: u64,
    /// Latest completed frame metrics, if a frame has been rendered.
    pub latest: Option<FrameMetrics>,
    /// Running averages across all recorded frames.
    pub averages: PerformanceAverages,
}

/// Metrics for one completed terminal frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameMetrics {
    /// Total wall-clock time spent on layout, render, diff, and flush.
    pub frame_time: Duration,
    /// Time spent computing layout.
    pub layout_time: Duration,
    /// Renderer metrics for this frame.
    pub render: RenderMetrics,
    /// Style resolutions requested since the previous frame.
    ///
    /// Counted rather than traced. Style resolution runs about twice per node per frame —
    /// the paint-order walk once and the visible-tree walk once — so a span here would
    /// cost more than the work it measured once anything was listening, and thousands of
    /// indistinguishable spans a frame answer no question anyone has.
    ///
    /// "Since the previous frame" rather than "during it": a resolution triggered by a
    /// style change between frames is counted against the frame that follows it.
    pub style_resolves: u64,
    /// Style resolutions that missed the cache and recomputed, of [`Self::style_resolves`].
    ///
    /// The load-bearing number. In a steady state this is **zero** — the cache holds every
    /// resolved style until something invalidates it — so a frame-after-frame nonzero value
    /// means something is invalidating every frame, which is the one style-resolution
    /// pathology worth catching.
    pub style_cache_misses: u64,
}

/// Lock-free counters for style resolution.
///
/// Deliberately not inside [`PerformanceState`]'s mutex: these are incremented thousands
/// of times per frame, and taking a lock at that rate would cost far more than the work
/// being measured. Relaxed ordering throughout is correct because nothing branches on
/// these — they are diagnostic totals, read once per frame, and a count that lands one
/// frame late is not a correctness problem.
#[derive(Debug, Default)]
pub(crate) struct StyleCounters {
    resolves: AtomicU64,
    misses: AtomicU64,
}

impl StyleCounters {
    /// Record one style resolution request, hit or miss.
    pub(crate) fn record_resolve(&self) {
        self.resolves.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one resolution that missed the cache and had to recompute.
    pub(crate) fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Read and reset both counters, returning `(resolves, misses)`.
    pub(crate) fn take(&self) -> (u64, u64) {
        (
            self.resolves.swap(0, Ordering::Relaxed),
            self.misses.swap(0, Ordering::Relaxed),
        )
    }
}

/// Running average performance metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformanceAverages {
    /// Average total frame time.
    pub frame_time: Duration,
    /// Average layout time.
    pub layout_time: Duration,
    /// Average renderer time.
    pub render_time: Duration,
    /// Average time spent clearing or preparing the frame grid.
    pub grid_time: Duration,
    /// Average time spent collecting visible DOM paint entries.
    pub dom_collect_time: Duration,
    /// Average time spent painting the DOM into the grid.
    pub dom_paint_time: Duration,
    /// Average time spent diffing old and new grids.
    pub diff_time: Duration,
    /// Average time spent flushing terminal output.
    pub flush_time: Duration,
    /// Average number of dirty cells found by diffing.
    pub diff_dirty_cells: f64,
    /// Average number of cells flushed to the terminal.
    pub cells_flushed: f64,
}

/// Renderer metrics for one frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMetrics {
    /// Number of dirty cells found by diffing before any full-redraw fallback.
    pub diff_dirty_cells: usize,
    /// Number of cells flushed to the terminal.
    pub cells_flushed: usize,
    /// Terminal flush strategy used for this frame.
    pub flush_mode: FlushMode,
    /// Time spent creating, clearing, or preparing the frame grid.
    pub grid_time: Duration,
    /// Time spent collecting the visible DOM tree into paint entries.
    pub dom_collect_time: Duration,
    /// Time spent painting DOM nodes into the grid.
    pub dom_paint_time: Duration,
    /// Detailed instrumentation for DOM painting.
    pub paint_profile: PaintProfile,
    /// Time spent diffing old and new grids.
    pub diff_time: Duration,
    /// Detailed instrumentation for diffing.
    pub diff_profile: DiffProfile,
    /// Time spent flushing terminal output.
    pub flush_time: Duration,
}

impl RenderMetrics {
    /// Total time spent inside the renderer for this frame.
    pub fn render_time(self) -> Duration {
        self.grid_time
            + self.dom_collect_time
            + self.dom_paint_time
            + self.diff_time
            + self.flush_time
    }
}

/// Terminal flush strategy used for a frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FlushMode {
    /// Only changed cells were flushed.
    #[default]
    Changes,
    /// The entire screen was redrawn.
    FullRedraw,
}

/// Largest background fill observed during a profiled paint pass.
#[derive(Debug, Clone, Copy)]
pub struct LargestFillProfile {
    /// Node id that requested the fill.
    pub node_id: NodeId,
    /// Node kind that requested the fill.
    pub node_kind: &'static str,
    /// Requested fill x coordinate before clipping.
    pub x: i32,
    /// Requested fill y coordinate before clipping.
    pub y: i32,
    /// Requested fill width before clipping.
    pub width: u16,
    /// Requested fill height before clipping.
    pub height: u16,
    /// Requested area before clipping.
    pub requested_cells: usize,
    /// Actual grid cells touched after clipping.
    pub clipped_cells: usize,
}

/// Detailed DOM paint instrumentation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaintProfile {
    /// Whether detailed paint instrumentation was enabled for this frame.
    pub enabled: bool,
    /// Number of resolved colors converted or read from the RGB cache.
    ///
    /// Deliberately a count with no matching duration. A resolve is a cache
    /// lookup costing about as much as reading the clock twice, so timing each
    /// one measured the instrumentation rather than the work — the duration this
    /// replaced reported 5.1µs for 150 resolves whose own clock reads accounted
    /// for 5.5µs. Time a region only when its work outweighs a clock pair.
    pub rgb_resolves: usize,
    /// Time spent filling node backgrounds into the grid.
    ///
    /// Timed per call around the fill itself, so a frame of many small fills
    /// carries proportionally more measurement overhead than one of few large
    /// ones — at ~36 calls this reads roughly 15% high, at ~250ns per fill.
    /// Compare against [`background_fill_calls`](Self::background_fill_calls)
    /// before drawing conclusions from a small value.
    pub background_fill_time: Duration,
    /// Number of background fill calls.
    pub background_fill_calls: usize,
    /// Number of grid cells touched by background fills.
    pub filled_cells: usize,
    /// Total requested background fill area before clipping.
    pub requested_fill_cells: usize,
    /// Number of fully opaque background fill calls.
    pub opaque_fill_calls: usize,
    /// Number of cells touched by fully opaque background fills.
    pub opaque_filled_cells: usize,
    /// Largest background fill in this frame.
    pub largest_fill: Option<LargestFillProfile>,
    /// Time spent writing glyphs — text and borders — into the grid.
    pub text_write_time: Duration,
    /// Number of glyph heads written into the grid, including border characters.
    pub glyphs_written: usize,
    /// Time spent formatting input display content before text paint.
    pub input_format_time: Duration,
}

/// Detailed frame diff instrumentation.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffProfile {
    /// Whether detailed diff instrumentation was enabled for this frame.
    pub enabled: bool,
    /// Number of rows in the new grid.
    pub rows: usize,
    /// Number of rows skipped by render-provided dirty hints.
    pub hint_skipped_rows: usize,
    /// Number of rows skipped by exact row equality.
    pub unchanged_rows: usize,
    /// Number of rows that required cell-level scanning.
    pub changed_rows: usize,
    /// Number of cells compared inside changed rows.
    pub cells_compared: usize,
    /// Number of dirty cells emitted.
    pub dirty_cells: usize,
    /// Time spent checking row equality.
    pub row_equality_time: Duration,
    /// Time spent scanning cells in changed rows.
    pub cell_scan_time: Duration,
    /// Time spent emitting dirty cells from dirty row buffers.
    pub emit_time: Duration,
}

/// Mutable performance metrics collector owned by a document.
#[derive(Debug, Clone)]
pub(crate) struct PerformanceState {
    detail: PerformanceDetail,
    fps: f64,
    latest: Option<FrameMetrics>,
    averages: PerformanceAverages,
    frame_count: u64,
    total_frame: Duration,
    total_layout: Duration,
    total_render: Duration,
    total_grid: Duration,
    total_dom_collect: Duration,
    total_dom_paint: Duration,
    total_diff: Duration,
    total_flush: Duration,
    total_diff_dirty_cells: usize,
    total_cells_flushed: usize,
    last_fps_update: Instant,
    frames_since_fps: u64,
}

impl PerformanceState {
    /// Create an empty collector.
    pub(crate) fn new() -> Self {
        Self {
            detail: PerformanceDetail::Basic,
            fps: 0.0,
            latest: None,
            averages: PerformanceAverages::default(),
            frame_count: 0,
            total_frame: Duration::ZERO,
            total_layout: Duration::ZERO,
            total_render: Duration::ZERO,
            total_grid: Duration::ZERO,
            total_dom_collect: Duration::ZERO,
            total_dom_paint: Duration::ZERO,
            total_diff: Duration::ZERO,
            total_flush: Duration::ZERO,
            total_diff_dirty_cells: 0,
            total_cells_flushed: 0,
            last_fps_update: Instant::now(),
            frames_since_fps: 0,
        }
    }

    /// Return the current instrumentation detail level.
    pub(crate) fn detail(&self) -> PerformanceDetail {
        self.detail
    }

    /// Set the instrumentation detail level.
    pub(crate) fn set_detail(&mut self, detail: PerformanceDetail) {
        self.detail = detail;
    }

    /// Return an immutable public snapshot.
    pub(crate) fn snapshot(&self) -> PerformanceSnapshot {
        PerformanceSnapshot {
            detail: self.detail,
            fps: self.fps,
            frame_count: self.frame_count,
            latest: self.latest,
            averages: self.averages,
        }
    }

    /// Record metrics for a completed terminal frame.
    pub(crate) fn record(
        &mut self,
        frame_time: Duration,
        layout_time: Duration,
        render: RenderMetrics,
        style: (u64, u64),
    ) {
        let (style_resolves, style_cache_misses) = style;
        let frame = FrameMetrics {
            frame_time,
            layout_time,
            render,
            style_resolves,
            style_cache_misses,
        };
        self.latest = Some(frame);

        self.frame_count += 1;
        self.total_frame += frame_time;
        self.total_layout += layout_time;
        self.total_render += render.render_time();
        self.total_grid += render.grid_time;
        self.total_dom_collect += render.dom_collect_time;
        self.total_dom_paint += render.dom_paint_time;
        self.total_diff += render.diff_time;
        self.total_flush += render.flush_time;
        self.total_diff_dirty_cells += render.diff_dirty_cells;
        self.total_cells_flushed += render.cells_flushed;

        let n = self.frame_count as f64;
        self.averages = PerformanceAverages {
            frame_time: avg(self.total_frame, n),
            layout_time: avg(self.total_layout, n),
            render_time: avg(self.total_render, n),
            grid_time: avg(self.total_grid, n),
            dom_collect_time: avg(self.total_dom_collect, n),
            dom_paint_time: avg(self.total_dom_paint, n),
            diff_time: avg(self.total_diff, n),
            flush_time: avg(self.total_flush, n),
            diff_dirty_cells: self.total_diff_dirty_cells as f64 / n,
            cells_flushed: self.total_cells_flushed as f64 / n,
        };

        self.frames_since_fps += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_fps_update);
        if elapsed >= Duration::from_millis(250) {
            self.fps = self.frames_since_fps as f64 / elapsed.as_secs_f64();
            self.frames_since_fps = 0;
            self.last_fps_update = now;
        }
    }
}

fn avg(d: Duration, n: f64) -> Duration {
    Duration::from_secs_f64(d.as_secs_f64() / n)
}

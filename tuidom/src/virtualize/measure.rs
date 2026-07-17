use std::collections::HashMap;

use super::Window;

/// Measured extents for a collection of variably sized items on one scroll axis.
///
/// Every item starts at an estimated extent; recording a real measurement replaces the
/// estimate for that item and shifts every later offset. The cache answers the same
/// window question as [`Uniform`](super::Uniform), plus offset queries against the mix
/// of measured and estimated extents.
///
/// A measurement that arrives above the current viewport shifts the content under the
/// user's eyes unless the scroll offset absorbs it — [`record`](Self::record) returns
/// the signed extent change so the caller can apply that anchoring compensation.
///
/// The estimate should be at least one cell: an unmeasured item estimated at zero
/// occupies nothing, so it can never scroll into view to *get* measured.
#[derive(Debug, Clone)]
pub struct MeasurementCache {
    estimate: u16,
    /// Recorded measurements by item index.
    measured: HashMap<usize, u16>,
    /// Fenwick tree over each item's signed difference from the estimate, so offset
    /// queries stay logarithmic no matter how much of the collection has been measured.
    deltas: Fenwick,
}

impl MeasurementCache {
    /// Create a cache of `count` items, each estimated at `estimate` cells.
    pub fn new(count: usize, estimate: u16) -> Self {
        Self {
            estimate,
            measured: HashMap::new(),
            deltas: Fenwick::new(count),
        }
    }

    /// Number of items in the collection.
    pub fn count(&self) -> usize {
        self.deltas.len()
    }

    /// The estimated extent unmeasured items are given.
    pub fn estimate(&self) -> u16 {
        self.estimate
    }

    /// Resize the collection, keeping measurements for items that remain.
    ///
    /// Items keep their indices: growing appends estimated items, shrinking drops
    /// measurements from the removed tail. An insertion or removal in the middle
    /// renumbers items, so invalidate the shifted range afterwards.
    pub fn set_count(&mut self, count: usize) {
        self.measured.retain(|&index, _| index < count);

        let mut deltas = Fenwick::new(count);
        for (&index, &extent) in &self.measured {
            deltas.add(index, i64::from(extent) - i64::from(self.estimate));
        }
        self.deltas = deltas;
    }

    /// Record an item's measured extent, replacing its estimate.
    ///
    /// Returns how many cells the item grew or shrank by — the anchoring compensation:
    /// when the item lies above the viewport, adding this to the scroll offset keeps
    /// the content on screen visually pinned. An index outside the collection records
    /// nothing and returns zero.
    pub fn record(&mut self, index: usize, extent: u16) -> i64 {
        if index >= self.count() {
            return 0;
        }

        let old = self.measured.insert(index, extent).unwrap_or(self.estimate);
        let delta = i64::from(extent) - i64::from(old);
        if delta != 0 {
            self.deltas.add(index, delta);
        }
        delta
    }

    /// Drop an item's measurement, reverting it to the estimate.
    pub fn invalidate(&mut self, index: usize) {
        if let Some(extent) = self.measured.remove(&index) {
            self.deltas
                .add(index, i64::from(self.estimate) - i64::from(extent));
        }
    }

    /// Drop the measurements of a range of items.
    pub fn invalidate_range(&mut self, range: std::ops::Range<usize>) {
        for index in range {
            self.invalidate(index);
        }
    }

    /// Drop every measurement, reverting the whole collection to estimates.
    pub fn invalidate_all(&mut self) {
        self.measured.clear();
        self.deltas = Fenwick::new(self.count());
    }

    /// An item's current extent: its measurement, or the estimate.
    pub fn extent_of(&self, index: usize) -> u16 {
        self.measured.get(&index).copied().unwrap_or(self.estimate)
    }

    /// The offset of an item's start: what to scroll to for bringing it to the start
    /// of the viewport. `offset_of(count)` is the total extent.
    pub fn offset_of(&self, index: usize) -> u64 {
        let index = index.min(self.count());
        let estimated = index as u64 * u64::from(self.estimate);
        // Extents are unsigned, so the running offset can never go negative even
        // though individual deltas can.
        estimated.saturating_add_signed(self.deltas.prefix_sum(index))
    }

    /// Total extent of the collection in cells — what the container's content
    /// measures with both spacers in place.
    pub fn total_extent(&self) -> u64 {
        self.offset_of(self.count())
    }

    /// The window covering a scrollport, padded by `overscan` items on each side.
    ///
    /// Same contract as [`Uniform::window`](super::Uniform::window): an item straddling
    /// either edge is included, and spacers cover everything outside the window.
    pub fn window(&self, offset: u16, viewport: u16, overscan: usize) -> Window {
        let count = self.count();
        let offset = u64::from(offset);
        let viewport_end = offset + u64::from(viewport);

        // First item whose end reaches past the offset, first item that starts at or
        // beyond the viewport's end: both boundaries are monotonic in the offsets.
        let first_visible = partition(count, |index| self.offset_of(index + 1) <= offset);
        let end_visible = partition(count, |index| self.offset_of(index) < viewport_end);

        let start = first_visible.saturating_sub(overscan).min(count);
        let end = end_visible.saturating_add(overscan).min(count);
        let end = end.max(start);

        Window {
            items: start..end,
            lead: self.offset_of(start),
            trail: self.total_extent() - self.offset_of(end),
        }
    }
}

/// The first index in `0..count` where `pred` turns false; `count` if it never does.
/// `pred` must be monotonic: once false, false for every later index.
fn partition(count: usize, pred: impl Fn(usize) -> bool) -> usize {
    let mut low = 0;
    let mut high = count;
    while low < high {
        let mid = low + (high - low) / 2;
        if pred(mid) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

/// Fenwick (binary indexed) tree over signed per-item values.
#[derive(Debug, Clone)]
struct Fenwick {
    /// One-indexed partial sums; entry 0 is unused.
    tree: Vec<i64>,
}

impl Fenwick {
    fn new(count: usize) -> Self {
        Self {
            tree: vec![0; count + 1],
        }
    }

    fn len(&self) -> usize {
        self.tree.len() - 1
    }

    fn add(&mut self, index: usize, delta: i64) {
        let mut position = index + 1;
        while position < self.tree.len() {
            self.tree[position] += delta;
            position += position & position.wrapping_neg();
        }
    }

    /// Sum of the first `index` values.
    fn prefix_sum(&self, index: usize) -> i64 {
        let mut position = index.min(self.len());
        let mut sum = 0;
        while position > 0 {
            sum += self.tree[position];
            position -= position & position.wrapping_neg();
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::super::Uniform;
    use super::*;

    #[test]
    fn an_unmeasured_cache_matches_the_uniform_math() {
        let cache = MeasurementCache::new(100, 2);
        let uniform = Uniform {
            count: 100,
            stride: 2,
        };

        assert_eq!(cache.total_extent(), uniform.total_extent());
        for offset in [0_u16, 3, 95, 190] {
            assert_eq!(cache.window(offset, 10, 2), uniform.window(offset, 10, 2));
        }
    }

    #[test]
    fn measurements_shift_offsets_and_the_total() {
        let mut cache = MeasurementCache::new(10, 2);
        assert_eq!(cache.total_extent(), 20);

        // Item 3 turns out three cells taller, item 7 one cell shorter.
        assert_eq!(cache.record(3, 5), 3);
        assert_eq!(cache.record(7, 1), -1);

        assert_eq!(cache.extent_of(3), 5);
        assert_eq!(cache.extent_of(4), 2);
        assert_eq!(cache.total_extent(), 22);

        // Items before the measurement are unmoved; items after shift by the delta.
        assert_eq!(cache.offset_of(3), 6);
        assert_eq!(cache.offset_of(4), 11);
        assert_eq!(cache.offset_of(8), 18);
    }

    #[test]
    fn re_recording_returns_the_adjustment_not_the_original_delta() {
        let mut cache = MeasurementCache::new(10, 2);
        assert_eq!(cache.record(3, 5), 3);
        assert_eq!(cache.record(3, 4), -1);
        assert_eq!(cache.record(3, 4), 0);
        assert_eq!(cache.total_extent(), 22);
    }

    #[test]
    fn invalidation_reverts_to_the_estimate() {
        let mut cache = MeasurementCache::new(10, 2);
        cache.record(3, 5);
        cache.record(4, 6);
        cache.record(5, 7);

        cache.invalidate(3);
        assert_eq!(cache.extent_of(3), 2);

        cache.invalidate_range(0..5);
        assert_eq!(cache.extent_of(4), 2);
        assert_eq!(cache.extent_of(5), 7);

        cache.invalidate_all();
        assert_eq!(cache.total_extent(), 20);
    }

    #[test]
    fn windows_respect_measured_extents() {
        let mut cache = MeasurementCache::new(10, 2);
        // Item 0 fills the whole 6-cell viewport once measured.
        cache.record(0, 6);

        let window = cache.window(0, 6, 0);
        assert_eq!(window.items, 0..1);
        assert_eq!((window.lead, window.trail), (0, 18));

        // Scrolled past item 0, the viewport covers items 1..4 (2 cells each).
        let window = cache.window(6, 6, 0);
        assert_eq!(window.items, 1..4);
        assert_eq!(window.lead, 6);
        assert_eq!(window.trail, 24 - 6 - 6);
    }

    #[test]
    fn anchoring_compensation_keeps_the_window_pinned() {
        let mut cache = MeasurementCache::new(100, 2);
        let offset: u16 = 50;
        let before = cache.window(offset, 10, 0);

        // An item above the viewport is measured 4 cells taller. Absorbing the returned
        // delta into the offset leaves the same items at the same visual positions.
        let delta = cache.record(3, 6);
        assert_eq!(delta, 4);
        let compensated = offset.saturating_add_signed(delta as i16);

        let after = cache.window(compensated, 10, 0);
        assert_eq!(after.items, before.items);
        assert_eq!(after.lead, before.lead + delta as u64);
    }

    #[test]
    fn set_count_keeps_surviving_measurements() {
        let mut cache = MeasurementCache::new(10, 2);
        cache.record(2, 5);
        cache.record(8, 5);

        cache.set_count(5);
        assert_eq!(cache.count(), 5);
        assert_eq!(cache.extent_of(2), 5);
        assert_eq!(cache.total_extent(), 13);

        cache.set_count(8);
        // The dropped measurement stays dropped; new items are estimates.
        assert_eq!(cache.total_extent(), 19);
    }

    #[test]
    fn out_of_range_records_are_ignored() {
        let mut cache = MeasurementCache::new(5, 2);
        assert_eq!(cache.record(5, 9), 0);
        assert_eq!(cache.total_extent(), 10);
    }

    #[test]
    fn offsets_stay_exact_over_large_measured_collections() {
        let mut cache = MeasurementCache::new(1_000_000, 3);
        for index in 0..1_000 {
            cache.record(index * 1_000, 5);
        }
        // A thousand items grew by two cells each.
        assert_eq!(cache.total_extent(), 3_000_000 + 2_000);
        assert_eq!(cache.offset_of(500_000), 1_500_000 + 2 * 500);
    }
}

use std::ops::Range;

use super::{MeasurementCache, Uniform, Window};

/// The extent source a [`Virtualizer`] windows over.
#[derive(Debug, Clone)]
enum Extents {
    Uniform(Uniform),
    Measured(MeasurementCache),
}

/// Stateful window diffing over a virtualized collection.
///
/// Owns the current materialized range and, on each [`update`](Self::update), reports
/// which items to create and which to remove to reach the new window. It never touches
/// a [`Document`](crate::Document): downstream owns every node and the spacers, this
/// owns only the arithmetic of what changed.
///
/// ```text
/// on_scroll → virtualizer.update(offset, viewport) → apply add/remove + spacer sizes
/// ```
#[derive(Debug, Clone)]
pub struct Virtualizer {
    extents: Extents,
    overscan: usize,
    materialized: Range<usize>,
    last: Option<Window>,
}

/// One [`Virtualizer::update`]'s difference between the old window and the new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowUpdate {
    /// The new window: items that should now exist, and both spacer extents.
    pub window: Window,
    /// Index ranges to materialize — items in the new window but not the old.
    pub add: Vec<Range<usize>>,
    /// Index ranges to remove — items in the old window but not the new.
    pub remove: Vec<Range<usize>>,
}

impl Virtualizer {
    /// Virtualize `count` uniformly sized items of `stride` cells each.
    pub fn uniform(count: usize, stride: u16, overscan: usize) -> Self {
        Self {
            extents: Extents::Uniform(Uniform { count, stride }),
            overscan,
            materialized: 0..0,
            last: None,
        }
    }

    /// Virtualize `count` variably sized items, each estimated at `estimate` cells
    /// until measured.
    pub fn measured(count: usize, estimate: u16, overscan: usize) -> Self {
        Self {
            extents: Extents::Measured(MeasurementCache::new(count, estimate)),
            overscan,
            materialized: 0..0,
            last: None,
        }
    }

    /// Number of items in the collection.
    pub fn count(&self) -> usize {
        match &self.extents {
            Extents::Uniform(uniform) => uniform.count,
            Extents::Measured(cache) => cache.count(),
        }
    }

    /// Resize the collection. Items removed from the tail leave the window on the
    /// next [`update`](Self::update).
    pub fn set_count(&mut self, count: usize) {
        match &mut self.extents {
            Extents::Uniform(uniform) => uniform.count = count,
            Extents::Measured(cache) => cache.set_count(count),
        }
    }

    /// Record a measured item extent, replacing its estimate.
    ///
    /// Returns the anchoring compensation, exactly as
    /// [`MeasurementCache::record`] does. On a uniform collection there is nothing to
    /// measure, so this records nothing and returns zero.
    pub fn record(&mut self, index: usize, extent: u16) -> i64 {
        match &mut self.extents {
            Extents::Uniform(_) => 0,
            Extents::Measured(cache) => cache.record(index, extent),
        }
    }

    /// The measurement cache, when this virtualizer measures.
    ///
    /// Mutating it through [`cache_mut`](Self::cache_mut) is safe at any time: the next
    /// [`update`](Self::update) recomputes the window from scratch and diffs against
    /// what is actually materialized.
    pub fn cache(&self) -> Option<&MeasurementCache> {
        match &self.extents {
            Extents::Uniform(_) => None,
            Extents::Measured(cache) => Some(cache),
        }
    }

    /// Mutable access to the measurement cache, when this virtualizer measures.
    pub fn cache_mut(&mut self) -> Option<&mut MeasurementCache> {
        match &mut self.extents {
            Extents::Uniform(_) => None,
            Extents::Measured(cache) => Some(cache),
        }
    }

    /// The offset of an item's start — what to scroll to for bringing it to the
    /// start of the viewport.
    pub fn offset_of(&self, index: usize) -> u64 {
        match &self.extents {
            Extents::Uniform(uniform) => uniform.offset_of(index),
            Extents::Measured(cache) => cache.offset_of(index),
        }
    }

    /// Total extent of the collection in cells.
    pub fn total_extent(&self) -> u64 {
        match &self.extents {
            Extents::Uniform(uniform) => uniform.total_extent(),
            Extents::Measured(cache) => cache.total_extent(),
        }
    }

    /// The currently materialized index range.
    pub fn materialized(&self) -> Range<usize> {
        self.materialized.clone()
    }

    /// Forget the materialized state, as after downstream rebuilt its subtree.
    /// The next [`update`](Self::update) reports the whole window as additions.
    pub fn reset(&mut self) {
        self.materialized = 0..0;
        self.last = None;
    }

    /// Recompute the window for a scroll position and diff it against what is
    /// materialized.
    ///
    /// Returns `None` when nothing changed — same items, same spacers — so a scroll
    /// within the overscan margin costs no DOM work. Otherwise the caller applies the
    /// additions and removals and resizes both spacers, and the virtualizer considers
    /// the new window materialized.
    pub fn update(&mut self, offset: u16, viewport: u16) -> Option<WindowUpdate> {
        let window = match &self.extents {
            Extents::Uniform(uniform) => uniform.window(offset, viewport, self.overscan),
            Extents::Measured(cache) => cache.window(offset, viewport, self.overscan),
        };
        if self.last.as_ref() == Some(&window) {
            return None;
        }

        let add = subtract(window.items.clone(), self.materialized.clone());
        let remove = subtract(self.materialized.clone(), window.items.clone());

        self.materialized = window.items.clone();
        self.last = Some(window.clone());
        Some(WindowUpdate {
            window,
            add,
            remove,
        })
    }
}

/// The parts of `a` not covered by `b`: zero, one, or two disjoint ranges, in order.
fn subtract(a: Range<usize>, b: Range<usize>) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let before = a.start..a.end.min(b.start);
    if !before.is_empty() {
        parts.push(before);
    }
    let after = a.start.max(b.end)..a.end;
    if !after.is_empty() {
        parts.push(after);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_update_materializes_the_whole_window() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);

        let update = virtualizer.update(0, 10).unwrap();
        assert_eq!(update.window.items, 0..7);
        assert_eq!(update.add, vec![0..7]);
        assert_eq!(update.remove, Vec::<Range<usize>>::new());
        assert_eq!((update.window.lead, update.window.trail), (0, 186));
        assert_eq!(virtualizer.materialized(), 0..7);
    }

    #[test]
    fn a_small_scroll_is_an_incremental_diff() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        virtualizer.update(0, 10).unwrap();

        // Four cells down: window slides from 0..7 to 0..9.
        let update = virtualizer.update(4, 10).unwrap();
        assert_eq!(update.window.items, 0..9);
        assert_eq!(update.add, vec![7..9]);
        assert_eq!(update.remove, Vec::<Range<usize>>::new());

        // Four more: 2..11 — items fall off the top for the first time.
        let update = virtualizer.update(8, 10).unwrap();
        assert_eq!(update.add, vec![9..11]);
        assert_eq!(update.remove, vec![0..2]);
    }

    #[test]
    fn a_jump_replaces_the_window_wholesale() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        virtualizer.update(0, 10).unwrap();

        let update = virtualizer.update(100, 10).unwrap();
        assert_eq!(update.window.items, 48..57);
        assert_eq!(update.add, vec![48..57]);
        assert_eq!(update.remove, vec![0..7]);
    }

    #[test]
    fn an_unchanged_window_is_a_no_op() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        virtualizer.update(0, 10).unwrap();
        assert_eq!(virtualizer.update(0, 10), None);
    }

    #[test]
    fn shrinking_the_count_removes_items_beyond_it() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        virtualizer.update(0, 10).unwrap();
        assert_eq!(virtualizer.materialized(), 0..7);

        virtualizer.set_count(4);
        let update = virtualizer.update(0, 10).unwrap();
        assert_eq!(update.window.items, 0..4);
        assert_eq!(update.add, Vec::<Range<usize>>::new());
        assert_eq!(update.remove, vec![4..7]);
        assert_eq!((update.window.lead, update.window.trail), (0, 0));
    }

    #[test]
    fn a_measurement_alone_changes_the_window() {
        let mut virtualizer = Virtualizer::measured(100, 2, 0);
        virtualizer.update(50, 10).unwrap();
        let before = virtualizer.materialized();

        // An item above the window grows; same items, but the lead spacer moved,
        // so the caller still gets an update to apply.
        let delta = virtualizer.record(3, 6);
        assert_eq!(delta, 4);

        let compensated = 50 + delta as u16;
        let update = virtualizer.update(compensated, 10).unwrap();
        assert_eq!(update.window.items, before);
        assert_eq!(update.add, Vec::<Range<usize>>::new());
        assert_eq!(update.remove, Vec::<Range<usize>>::new());
        assert_eq!(update.window.lead, 54);
    }

    #[test]
    fn reset_rematerializes_everything() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        virtualizer.update(0, 10).unwrap();

        virtualizer.reset();
        let update = virtualizer.update(0, 10).unwrap();
        assert_eq!(update.add, vec![0..7]);
        assert_eq!(update.remove, Vec::<Range<usize>>::new());
    }

    #[test]
    fn uniform_collections_have_nothing_to_record() {
        let mut virtualizer = Virtualizer::uniform(100, 2, 2);
        assert_eq!(virtualizer.record(3, 6), 0);
        assert!(virtualizer.cache().is_none());
    }
}

//! Virtualization primitives: range math for materializing only the visible window
//! of a large collection.
//!
//! tuidom does not virtualize on its own — like a browser under a virtualized list,
//! the engine provides the primitives and downstream owns the nodes. The mechanism is
//! the spacer pattern: a scroll container holds a leading spacer, the materialized
//! window, and a trailing spacer, so the container's measured content size is the true
//! total and scroll clamping, scrollbar geometry, and wheel routing stay correct with
//! nothing virtual about them.
//!
//! Everything here is one axis: a vertical list virtualizes its rows, a horizontal
//! strip its columns, and a 2D grid runs the same math once per axis.

use std::ops::Range;

/// A collection of uniformly sized items on one scroll axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uniform {
    /// Number of items in the collection.
    pub count: usize,
    /// Cells from one item's start to the next: the item's extent plus any flex gap
    /// between items. Items are assumed not to flex — a grown or shrunk item breaks
    /// the stride the math is built on.
    pub stride: u16,
}

/// The DOM window a virtualized collection should materialize.
///
/// Extents are `u64` so the math stays exact for any collection; the engine itself
/// scrolls at most `u16::MAX` cells on an axis, so a spacer applied to a style
/// saturates at [`Length::Pixels`](crate::style::Length::Pixels)'s range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The items that should exist in the DOM, visible plus overscan.
    pub items: Range<usize>,
    /// Leading spacer extent in cells: the space of every item before the window.
    pub lead: u64,
    /// Trailing spacer extent in cells: the space of every item after the window.
    pub trail: u64,
}

impl Uniform {
    /// The window covering a scrollport, padded by `overscan` items on each side.
    ///
    /// `offset` is the container's scroll offset on this axis and `viewport` its
    /// scrollport extent, both in cells. An item straddling either edge is included.
    /// A zero stride cannot be windowed by offset, so it materializes everything.
    pub fn window(&self, offset: u16, viewport: u16, overscan: usize) -> Window {
        if self.stride == 0 {
            return Window {
                items: 0..self.count,
                lead: 0,
                trail: 0,
            };
        }

        let stride = u64::from(self.stride);
        let first_visible = (u64::from(offset) / stride) as usize;
        let end_visible = (u64::from(offset) + u64::from(viewport)).div_ceil(stride) as usize;

        let start = first_visible.saturating_sub(overscan).min(self.count);
        let end = end_visible.saturating_add(overscan).min(self.count);

        Window {
            items: start..end,
            lead: start as u64 * stride,
            trail: (self.count - end) as u64 * stride,
        }
    }

    /// Total extent of the collection in cells — what the container's content
    /// measures with both spacers in place.
    pub fn total_extent(&self) -> u64 {
        self.count as u64 * u64::from(self.stride)
    }

    /// The offset of an item's start: what to scroll to for bringing it to the
    /// start of the viewport.
    pub fn offset_of(&self, index: usize) -> u64 {
        index as u64 * u64::from(self.stride)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(count: usize, stride: u16) -> Uniform {
        Uniform { count, stride }
    }

    #[test]
    fn window_at_the_start_has_no_lead() {
        let window = uniform(100, 2).window(0, 10, 0);
        assert_eq!(window.items, 0..5);
        assert_eq!(window.lead, 0);
        assert_eq!(window.trail, 190);
    }

    #[test]
    fn window_at_the_end_has_no_trail() {
        // 100 items × 2 cells = 200; a 10-cell viewport leaves max offset 190.
        let window = uniform(100, 2).window(190, 10, 0);
        assert_eq!(window.items, 95..100);
        assert_eq!(window.lead, 190);
        assert_eq!(window.trail, 0);
    }

    #[test]
    fn items_straddling_either_edge_are_included() {
        // Offset 3 cuts item 1 at the top; offset+viewport = 13 cuts item 6 at the bottom.
        let window = uniform(100, 2).window(3, 10, 0);
        assert_eq!(window.items, 1..7);
        assert_eq!(window.lead, 2);
        assert_eq!(window.trail, 186);
    }

    #[test]
    fn overscan_pads_both_sides_and_clamps_to_the_collection() {
        let window = uniform(100, 2).window(3, 10, 3);
        assert_eq!(window.items, 0..10);

        let window = uniform(8, 2).window(3, 10, 3);
        assert_eq!(window.items, 0..8);
        assert_eq!((window.lead, window.trail), (0, 0));
    }

    #[test]
    fn spacers_and_window_always_sum_to_the_total_extent() {
        let axis = uniform(1000, 3);
        for offset in [0_u16, 1, 500, 1499, 2970] {
            let window = axis.window(offset, 30, 2);
            let window_extent = (window.items.end - window.items.start) as u64 * 3;
            assert_eq!(
                window.lead + window_extent + window.trail,
                axis.total_extent()
            );
        }
    }

    #[test]
    fn empty_collection_yields_an_empty_window() {
        let window = uniform(0, 5).window(0, 10, 4);
        assert_eq!(window.items, 0..0);
        assert_eq!((window.lead, window.trail), (0, 0));
    }

    #[test]
    fn viewport_larger_than_content_materializes_everything() {
        let window = uniform(3, 2).window(0, 50, 0);
        assert_eq!(window.items, 0..3);
        assert_eq!((window.lead, window.trail), (0, 0));
    }

    #[test]
    fn zero_stride_materializes_everything() {
        let window = uniform(7, 0).window(20, 10, 1);
        assert_eq!(window.items, 0..7);
        assert_eq!((window.lead, window.trail), (0, 0));
    }

    #[test]
    fn extents_stay_exact_beyond_the_engines_cell_range() {
        // 10 million items × 3 cells is far past u16 cells; the math must not wrap.
        let axis = uniform(10_000_000, 3);
        assert_eq!(axis.total_extent(), 30_000_000);
        assert_eq!(axis.offset_of(9_999_999), 29_999_997);

        let window = axis.window(60_000, 30, 5);
        assert_eq!(window.items, 19_995..20_015);
        assert_eq!(window.lead, 59_985);
        assert_eq!(window.trail, 30_000_000 - 60_045);
    }
}

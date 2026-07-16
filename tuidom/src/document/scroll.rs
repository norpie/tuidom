use crate::document::Document;
use crate::error::Result;
use crate::id::NodeId;
use crate::lock;
use crate::node::ScrollOffset;
use crate::style::Overflow;

impl Document {
    /// Get a node's current scroll offset.
    ///
    /// Returns `(0, 0)` for anything that has never been scrolled.
    pub fn scroll_offset(&self, node: NodeId) -> ScrollOffset {
        lock::mutex(&self.inner.scroll_offsets)
            .get(&node)
            .copied()
            .unwrap_or_default()
    }

    /// Scroll a node to an absolute offset, clamped to its scrollable range.
    ///
    /// An axis is scrollable only when the node's overflow on it is [`Overflow::Scroll`]
    /// and the latest layout measured content beyond the box; on any other axis the
    /// offset clamps to zero. Returns an error if the node does not exist.
    pub fn scroll_to(&self, node: NodeId, x: u16, y: u16) -> Result<()> {
        let resolved = self.resolved_style(node)?;
        let (max_x, max_y) = {
            let snapshot = lock::rw_read(&self.inner.layout_snapshot);
            match snapshot.get(&node) {
                Some(layout) => (
                    scrollable_max(resolved.overflow_x, layout.max_scroll_x),
                    scrollable_max(resolved.overflow_y, layout.max_scroll_y),
                ),
                None => (0, 0),
            }
        };

        let clamped = ScrollOffset {
            x: x.min(max_x),
            y: y.min(max_y),
        };

        let mut offsets = lock::mutex(&self.inner.scroll_offsets);
        let current = offsets.get(&node).copied().unwrap_or_default();
        if clamped == current {
            return Ok(());
        }
        if clamped == ScrollOffset::default() {
            offsets.remove(&node);
        } else {
            offsets.insert(node, clamped);
        }
        drop(offsets);

        self.inner.notify.notify_one();
        Ok(())
    }

    /// Scroll a node by a relative amount, clamped to its scrollable range.
    ///
    /// Deltas are signed terminal cells; positive scrolls content further out of view
    /// past the top/left edge. Returns an error if the node does not exist.
    pub fn scroll_by(&self, node: NodeId, dx: i32, dy: i32) -> Result<()> {
        let current = self.scroll_offset(node);
        self.scroll_to(node, offset_by(current.x, dx), offset_by(current.y, dy))
    }
}

fn scrollable_max(overflow: Overflow, max_scroll: u16) -> u16 {
    match overflow {
        Overflow::Scroll => max_scroll,
        Overflow::Visible | Overflow::Clip => 0,
    }
}

fn offset_by(current: u16, delta: i32) -> u16 {
    i64::from(current)
        .saturating_add(i64::from(delta))
        .clamp(0, i64::from(u16::MAX)) as u16
}

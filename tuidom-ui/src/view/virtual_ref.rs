use std::sync::{Arc, Mutex};

use tuidom::{Document, NodeId};

use super::{DriverState, lock};
use crate::runtime::{Signal, state, untracked};

/// A handle to a mounted [`virtual_list`](crate::virtual_list) or
/// [`virtual_list_measured`](crate::virtual_list_measured), for the operations that need its
/// window math.
///
/// Create one with [`virtual_list_ref`] and bind it with
/// [`VirtualList::list_ref`](crate::VirtualList::list_ref). It holds the list while the list is
/// mounted and is empty otherwise; on an empty ref every operation does nothing.
#[derive(Clone, Copy)]
pub struct VirtualListRef {
    slot: Signal<Option<Bound>>,
}

/// A fresh, empty ref, for an optional prop the caller may leave out.
impl Default for VirtualListRef {
    fn default() -> Self {
        virtual_list_ref()
    }
}

#[derive(Clone)]
struct Bound {
    doc: Document,
    container: NodeId,
    state: Arc<Mutex<DriverState>>,
}

// A `NodeId` already names its document, so the container alone decides equality.
impl PartialEq for Bound {
    fn eq(&self, other: &Self) -> bool {
        self.container == other.container
    }
}

/// Create an empty [`VirtualListRef`], owned by the current scope.
pub fn virtual_list_ref() -> VirtualListRef {
    VirtualListRef { slot: state(None) }
}

impl VirtualListRef {
    /// The list's scroll container, or `None` while no list holding this ref is mounted.
    pub fn node(&self) -> Option<NodeId> {
        self.bound().map(|bound| bound.container)
    }

    /// Scroll the list the least distance that shows item `index` whole, or its top when it is
    /// taller than the viewport. Nothing moves when it is already in view.
    ///
    /// A measured list places an item it has not laid out yet at its estimate, so the scroll can
    /// land a little off until the item has been seen. Returns `false` when the ref is empty,
    /// the list has not been laid out yet, or `index` is past its end.
    pub fn scroll_into_view(&self, index: usize) -> bool {
        let Some(Bound {
            doc,
            container,
            state,
        }) = untracked(|| self.bound())
        else {
            return false;
        };
        let Some(viewport) = doc
            .get_node(container)
            .and_then(|node| node.layout)
            .map(|layout| layout.scrollport.height)
        else {
            return false;
        };
        let (start, end) = {
            let st = lock(&state);
            if index >= st.virt.count() {
                return false;
            }
            (st.virt.offset_of(index), st.virt.offset_of(index + 1))
        };
        let offset = doc.scroll_offset(container);
        let top = u64::from(offset.y);
        let bottom = top + u64::from(viewport);
        let target = if start < top || end - start > u64::from(viewport) {
            start
        } else if end > bottom {
            end - u64::from(viewport)
        } else {
            return true;
        };
        let y = u16::try_from(target).unwrap_or(u16::MAX);
        if let Err(err) = doc.scroll_to(container, offset.x, y) {
            tracing::warn!(?container, "scroll into view failed: {err}");
        }
        true
    }

    pub(super) fn fill(&self, doc: &Document, container: NodeId, state: Arc<Mutex<DriverState>>) {
        self.slot.set(Some(Bound {
            doc: doc.clone(),
            container,
            state,
        }));
    }

    pub(super) fn release(&self, container: NodeId) {
        if untracked(|| self.node()) == Some(container) {
            self.slot.set(None);
        }
    }

    fn bound(&self) -> Option<Bound> {
        self.slot.try_with(Option::clone).flatten()
    }
}

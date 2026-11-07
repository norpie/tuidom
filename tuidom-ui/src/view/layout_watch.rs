//! The post-frame watcher behind a ref's tracked layout reads.
//!
//! Layout and scroll are engine state, not signals, so a frame is the only point where they
//! can change. Each document with a watched node gets one post-frame listener, which rereads
//! every watched node and writes the ref's signals in one batch. Writes of an unchanged value
//! are no-ops, so a frame that moved nothing reruns nothing.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tuidom::event::ListenerHandle;
use tuidom::{Document, LayoutRect, LayoutView, NodeId};

use super::lock;
use crate::runtime::{Signal, batch, with_inline_flush};

/// The signals a watched node's reads subscribe to.
#[derive(Clone, Copy)]
pub(super) struct LayoutSignals {
    pub(super) layout: Signal<Option<LayoutView>>,
    pub(super) screen_rect: Signal<Option<LayoutRect>>,
}

impl LayoutSignals {
    fn refresh(&self, doc: &Document, node: NodeId) {
        self.layout.set(layout_of(doc, node));
        self.screen_rect.set(doc.screen_rect(node));
    }

    pub(super) fn clear(&self) {
        self.layout.set(None);
        self.screen_rect.set(None);
    }
}

pub(super) fn layout_of(doc: &Document, node: NodeId) -> Option<LayoutView> {
    doc.get_node(node).and_then(|view| view.layout)
}

struct Watcher {
    listener: ListenerHandle,
    nodes: HashMap<NodeId, LayoutSignals>,
}

/// Watchers by their document's root, which names the document. An element binds at most one
/// ref, so a node has at most one set of signals.
static WATCHERS: LazyLock<Mutex<HashMap<NodeId, Watcher>>> = LazyLock::new(Default::default);

/// Start watching `node`, filling `signals` from the engine. A node already watched is left
/// as it is.
pub(super) fn watch(doc: &Document, node: NodeId, signals: LayoutSignals) {
    let mut watchers = lock(&WATCHERS);
    let watcher = watchers.entry(doc.root()).or_insert_with(|| Watcher {
        listener: listen(doc),
        nodes: HashMap::new(),
    });
    if watcher.nodes.insert(node, signals).is_some() {
        return;
    }
    drop(watchers);
    signals.refresh(doc, node);
}

/// Stop watching `node`. The document's last watch removes its listener, which holds a clone
/// of the document.
pub(super) fn unwatch(doc: &Document, node: NodeId) {
    let mut watchers = lock(&WATCHERS);
    let Some(watcher) = watchers.get_mut(&doc.root()) else {
        return;
    };
    watcher.nodes.remove(&node);
    if watcher.nodes.is_empty()
        && let Some(watcher) = watchers.remove(&doc.root())
    {
        drop(watchers);
        doc.remove_listener(watcher.listener);
    }
}

fn listen(doc: &Document) -> ListenerHandle {
    let listening = doc.clone();
    doc.on_post_frame(move |_| {
        let doc = &listening;
        // Collected first: a write reruns effects, which may watch or unwatch.
        let watched: Vec<(NodeId, LayoutSignals)> = lock(&WATCHERS)
            .get(&doc.root())
            .map(|watcher| watcher.nodes.iter().map(|(n, s)| (*n, *s)).collect())
            .unwrap_or_default();
        with_inline_flush(|| {
            batch(|| {
                for (node, signals) in watched {
                    signals.refresh(doc, node);
                }
            })
        });
    })
}

#[cfg(test)]
pub(crate) fn watched_count(doc: &Document) -> usize {
    lock(&WATCHERS)
        .get(&doc.root())
        .map_or(0, |watcher| watcher.nodes.len())
}

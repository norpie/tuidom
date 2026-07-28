//! The reactive runtime: a process-wide arena of signal, derived, and effect slots, plus
//! the push-based dirty-marking graph that connects them.
//!
//! # Why one global lock
//!
//! Every slot lives in a single `RwLock`-guarded arena. A reactive graph is pointer-chasing
//! rather than shardable — an effect run touches whichever signals it happens to read — so a
//! per-slot lock would buy contention headroom the workload cannot use while making the
//! flush impossible to reason about. The invariant that keeps a single lock safe is the same
//! one the engine enforces around its `DashMap`: **user code never runs while the lock is
//! held.** Every operation that runs a closure — an effect body, a derived's compute, a
//! scope cleanup — takes what it needs under the lock, releases it, then runs the closure.
//!
//! # Handles never alias
//!
//! Slot ids come from a monotonic counter and are never reused, mirroring the engine's
//! `NodeId`. So a handle to a disposed slot is *inert*, not dangling: a write finds nothing
//! and is a silent no-op, a read logs and falls back. This is what makes an aborted task's
//! late write into a freed signal harmless.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, RwLock};

use tokio::task::JoinHandle;

use crate::lock::{rw_read, rw_write};

mod computed;
mod effects;
mod scope;
mod signal;

#[cfg(test)]
mod tests;

pub use computed::{Derived, derived};
pub use effects::effect;
pub use scope::{Scope, on_cleanup, provide_context, use_context};
pub use signal::{Signal, state};

/// A `Copy` handle identifying one slot in the runtime arena.
///
/// Allocated from a monotonic counter and never reused, so it cannot alias a later slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct SlotId(u64);

/// A `Copy` handle identifying an owner scope.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ScopeId(u64);

/// Type-erased signal value. `Send + Sync` so a signal is safe to read and write from any
/// task, which the engine's `Send + Sync` listeners require.
pub(crate) type BoxAny = Box<dyn Any + Send + Sync>;

/// A backstop against a runaway flush — an effect that writes a signal it depends on would
/// otherwise reschedule itself forever. When exceeded, the flush logs and bails rather than
/// hanging.
const MAX_EFFECT_RUNS_PER_FLUSH: usize = 100_000;

/// What a slot holds. Signals store a value; deriveds and effects store a closure and a
/// dirty bit.
pub(crate) enum Kind {
    /// A source of truth.
    Signal { value: BoxAny },
    /// A memoized computation, recomputed lazily on read when dirty.
    Derived {
        value: Option<BoxAny>,
        compute: Arc<dyn Fn() -> BoxAny + Send + Sync>,
        dirty: bool,
    },
    /// A side effect, rerun on the event of a dependency change.
    Effect {
        run: Arc<dyn Fn() + Send + Sync>,
        dirty: bool,
    },
}

/// One node in the reactive graph.
pub(crate) struct Node {
    pub kind: Kind,
    /// Nodes that depend on this one (empty for effects — nothing observes an effect).
    pub subscribers: HashSet<SlotId>,
    /// Nodes this one depends on, re-collected on every run (empty for signals).
    pub sources: HashSet<SlotId>,
    /// The owner scope, if this node was created inside one.
    pub scope: Option<ScopeId>,
}

/// State a scope owns and frees on dispose.
struct ScopeData {
    parent: Option<ScopeId>,
    children: Vec<ScopeId>,
    nodes: Vec<SlotId>,
    cleanups: Vec<Box<dyn FnOnce() + Send + Sync>>,
    tasks: Vec<JoinHandle<()>>,
    /// Values provided to this scope's descendants, keyed by their concrete type (D3). A
    /// descendant injects one by walking parent scopes to the nearest provider of that type.
    /// Dropped with the scope, so a provided value lives exactly as long as the subtree.
    contexts: HashMap<TypeId, BoxAny>,
}

/// The arena behind the runtime's lock.
pub(crate) struct Inner {
    nodes: HashMap<SlotId, Node>,
    scopes: HashMap<ScopeId, ScopeData>,
    next_slot: u64,
    next_scope: u64,
}

impl Inner {
    fn alloc(&mut self, kind: Kind) -> SlotId {
        let id = SlotId(self.next_slot);
        self.next_slot += 1;
        self.nodes.insert(
            id,
            Node {
                kind,
                subscribers: HashSet::new(),
                sources: HashSet::new(),
                scope: None,
            },
        );
        id
    }
}

/// The runtime — a lock over the arena. Held privately; all access goes through [`rt`].
pub(crate) struct Runtime {
    inner: RwLock<Inner>,
}

impl Runtime {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        rw_read(&self.inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        rw_write(&self.inner)
    }
}

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| Runtime {
    inner: RwLock::new(Inner {
        nodes: HashMap::new(),
        scopes: HashMap::new(),
        next_slot: 0,
        next_scope: 0,
    }),
});

/// The process-wide runtime.
fn rt() -> &'static Runtime {
    &RUNTIME
}

thread_local! {
    /// The reactive node currently running, whose reads become subscriptions.
    static OBSERVER: Cell<Option<SlotId>> = const { Cell::new(None) };
    /// The open scope stack, innermost last; a created node attaches to the top.
    static SCOPE: RefCell<Vec<ScopeId>> = const { RefCell::new(Vec::new()) };
    /// Batching depth. Above zero, writes accumulate in [`PENDING`] without flushing.
    static BATCH_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Effects dirtied since the last flush, awaiting a run.
    static PENDING: RefCell<Vec<SlotId>> = const { RefCell::new(Vec::new()) };
    /// True while a flush is in progress, so a write from inside an effect does not reenter.
    static FLUSHING: Cell<bool> = const { Cell::new(false) };
}

/// The scope a newly created node should attach to.
fn current_scope() -> Option<ScopeId> {
    SCOPE.with(|s| s.borrow().last().copied())
}

/// Create a node, attaching it to the current scope if one is open.
fn insert(kind: Kind) -> SlotId {
    let scope = current_scope();
    let mut w = rt().write();
    let id = w.alloc(kind);
    if let Some(sid) = scope {
        if let Some(n) = w.nodes.get_mut(&id) {
            n.scope = Some(sid);
        }
        if let Some(s) = w.scopes.get_mut(&sid) {
            s.nodes.push(id);
        }
    }
    id
}

/// Register the currently running observer, if any, as a subscriber of `source`.
///
/// The edge is bidirectional: `source` gains a subscriber and the observer gains a source,
/// so a later run can detach the observer from everything it read last time.
fn track(source: SlotId) {
    let Some(observer) = OBSERVER.with(|o| o.get()) else {
        return;
    };
    if observer == source {
        return;
    }
    let mut w = rt().write();
    if let Some(n) = w.nodes.get_mut(&source) {
        n.subscribers.insert(observer);
    }
    if let Some(n) = w.nodes.get_mut(&observer) {
        n.sources.insert(source);
    }
}

/// Mark every node reachable downstream of `roots` dirty, collecting the effects among them
/// into `pending`. Pure graph mutation — safe to run under the lock.
///
/// A node is pushed to `pending` exactly once per dirty cycle, because the walk stops at any
/// node already dirty.
fn mark_dirty(w: &mut Inner, roots: Vec<SlotId>, pending: &mut Vec<SlotId>) {
    let mut stack = roots;
    while let Some(id) = stack.pop() {
        let Some(node) = w.nodes.get_mut(&id) else {
            continue;
        };
        match &mut node.kind {
            Kind::Derived { dirty, .. } => {
                if *dirty {
                    continue;
                }
                *dirty = true;
            }
            Kind::Effect { dirty, .. } => {
                if *dirty {
                    continue;
                }
                *dirty = true;
                pending.push(id);
                continue; // nothing observes an effect
            }
            Kind::Signal { .. } => continue, // a signal is never a subscriber
        }
        // A newly dirtied derived propagates to whatever observes it.
        let subs: Vec<SlotId> = node.subscribers.iter().copied().collect();
        stack.extend(subs);
    }
}

/// Dirty the subscribers of a changed signal, then flush unless a batch is open.
fn schedule(roots: Vec<SlotId>) {
    {
        let mut w = rt().write();
        PENDING.with(|p| mark_dirty(&mut w, roots, &mut p.borrow_mut()));
    }
    if BATCH_DEPTH.with(|d| d.get()) == 0 {
        flush();
    }
}

/// Run every pending effect, at most once each per cycle, until the queue drains.
///
/// A single guard against reentrancy: a write from inside an effect adds to [`PENDING`] and
/// is picked up by the loop below rather than starting a nested flush.
fn flush() {
    if FLUSHING.with(|f| f.get()) {
        return;
    }
    FLUSHING.with(|f| f.set(true));

    // One span per flush covers every effect it reruns, including those a cascading write
    // schedules mid-flush — so the flush reads as a single unit of work correlated against
    // the write that opened it, rather than a flat stream of rerun events.
    let _span = tracing::debug_span!("flush").entered();

    let mut runs = 0usize;
    loop {
        let batch: Vec<SlotId> = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        if batch.is_empty() {
            break;
        }
        for id in batch {
            runs += 1;
            if runs > MAX_EFFECT_RUNS_PER_FLUSH {
                tracing::error!(
                    "reactive flush exceeded {MAX_EFFECT_RUNS_PER_FLUSH} effect runs; \
                     dropping the rest — a signal is almost certainly driving a cycle"
                );
                PENDING.with(|p| p.borrow_mut().clear());
                FLUSHING.with(|f| f.set(false));
                return;
            }
            tracing::trace!(effect = id.0, "rerun");
            effects::run_effect(id);
        }
    }

    FLUSHING.with(|f| f.set(false));
}

/// Group writes so their effects flush once, when the outermost batch closes.
///
/// Nesting is by depth: only the outermost `batch` triggers the flush. A panic inside the
/// closure still restores the depth (via the guard) but does not flush a half-finished
/// batch.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    struct DepthGuard;
    impl Drop for DepthGuard {
        fn drop(&mut self) {
            BATCH_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }

    BATCH_DEPTH.with(|d| d.set(d.get() + 1));
    let guard = DepthGuard;
    let result = f();
    drop(guard); // decrement before the flush so it sees depth zero

    if BATCH_DEPTH.with(|d| d.get()) == 0 {
        flush();
    }
    result
}

/// Detach `id` from every source it currently reads, so a rerun can re-collect a fresh set.
///
/// Returns with `id`'s `sources` emptied and removed from each former source's `subscribers`.
/// Must be called under the write lock.
fn detach_sources(w: &mut Inner, id: SlotId) {
    let sources: Vec<SlotId> = match w.nodes.get_mut(&id) {
        Some(n) => n.sources.drain().collect(),
        None => return,
    };
    for src in sources {
        if let Some(s) = w.nodes.get_mut(&src) {
            s.subscribers.remove(&id);
        }
    }
}

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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use tokio::task::JoinHandle;

use crate::lock::{self, rw_read, rw_write};

mod computed;
mod effects;
mod scope;
mod signal;

#[cfg(test)]
mod tests;

pub use computed::{Derived, derived};
pub use effects::effect;
pub use scope::{Scope, on_cleanup, provide_context, spawn, use_context};
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

/// The runtime — a lock over the arena, plus the cross-thread flush plumbing. Held privately;
/// all access goes through [`rt`].
///
/// The mailbox and waker exist for one job: a reactive write made *off* the event task (a
/// spawned task, a foreign thread) cannot run effects there, so it deposits the dirtied effects
/// in the mailbox and wakes the event task to drain them. Writes on the event task never touch
/// either — they take the proven inline path through the thread-local [`PENDING`].
pub(crate) struct Runtime {
    inner: RwLock<Inner>,
    /// Effects dirtied by off-event-task writes, awaiting a drain on the event task.
    mailbox: Mutex<Vec<SlotId>>,
    /// The event-task waker an off-task write fires — in practice `Document::request_runtime_flush`.
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// A lock-free mirror of `waker.is_some()`, so the write path can decide inline-vs-deferred
    /// without taking the waker lock.
    has_waker: AtomicBool,
    /// Coalescing gate: set when a drain is already requested, so a burst of off-task writes
    /// enqueues at most one flush. Cleared when the drain begins.
    flush_requested: AtomicBool,
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
    mailbox: Mutex::new(Vec::new()),
    waker: Mutex::new(None),
    has_waker: AtomicBool::new(false),
    flush_requested: AtomicBool::new(false),
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
    /// True while this thread is inside a trusted event-task span — a listener handler, the
    /// mount and `on_mount` drain, or the deferred-flush drain. A write here flushes inline;
    /// a write with the flag unset (a spawned task, a foreign thread) defers to the mailbox.
    /// Set only across a single synchronous span with no awaits, so task migration never
    /// observes it mid-span.
    static INLINE_FLUSH: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` inside an inline-flush span, restoring the previous flag after — even on panic, so a
/// caught listener panic cannot leave the flag stuck on for the next write on this thread.
pub(crate) fn with_inline_flush<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            INLINE_FLUSH.with(|f| f.set(self.0));
        }
    }

    let prev = INLINE_FLUSH.with(|f| f.replace(true));
    let _restore = Restore(prev);
    f()
}

/// Whether a write off the event task should defer: a waker is registered to drain it, and this
/// thread is not in a trusted inline span. Reads only atomics/thread-locals, no arena lock.
fn should_defer() -> bool {
    !INLINE_FLUSH.with(|f| f.get()) && rt().has_waker.load(Ordering::Acquire)
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

/// Dirty the subscribers of a changed signal, then either flush inline or defer to the mailbox.
///
/// On the event task (or with no waker registered — the standalone-runtime case), this is the
/// path F3 shipped: dirty into the thread-local `PENDING` and flush unless a batch is open.
/// Off the event task with a waker registered, the effects are instead deposited in the shared
/// mailbox and the event task is woken to drain them, so effects never run on the writing task.
fn schedule(roots: Vec<SlotId>) {
    if should_defer() {
        let mut dirtied = Vec::new();
        {
            let mut w = rt().write();
            mark_dirty(&mut w, roots, &mut dirtied);
        }
        if !dirtied.is_empty() {
            lock::mutex(&rt().mailbox).extend(dirtied);
            wake();
        }
        return;
    }

    {
        let mut w = rt().write();
        PENDING.with(|p| mark_dirty(&mut w, roots, &mut p.borrow_mut()));
    }
    if BATCH_DEPTH.with(|d| d.get()) == 0 {
        flush();
    }
}

/// Fire the event-task waker for a deferred write, at most once per drain cycle (D4).
///
/// The gate collapses a burst of off-task writes to a single queued flush: the first write since
/// the last drain wins the swap and wakes; the rest see the flag already set and return.
fn wake() {
    if rt().flush_requested.swap(true, Ordering::AcqRel) {
        return;
    }
    let waker = lock::mutex(&rt().waker).clone();
    match waker {
        Some(waker) => waker(),
        // Registration was cleared between `should_defer` and here. Un-arm so a later
        // registration plus write can wake, rather than leaving the gate stuck set.
        None => rt().flush_requested.store(false, Ordering::Release),
    }
}

/// Drain the mailbox and run the deferred effects on the event task.
///
/// Registered as the engine's flush hook (via `Document::set_runtime_flush_hook`) so it runs
/// between runtime events, serialized with input dispatch. The gate is cleared *before* the
/// drain so a write that races in after the drain re-arms the waker rather than being lost. The
/// drain runs under an inline span, so an effect's cascading writes stay inline instead of
/// bouncing back through the mailbox.
///
/// Framework plumbing: `App` wires this to the engine's flush hook. Applications built
/// on the framework do not call it directly.
pub fn flush_deferred() {
    rt().flush_requested.store(false, Ordering::Release);
    let batch: Vec<SlotId> = std::mem::take(&mut *lock::mutex(&rt().mailbox));
    if batch.is_empty() {
        return;
    }
    with_inline_flush(|| {
        // The effects are already marked dirty; running them clears the bits.
        PENDING.with(|p| p.borrow_mut().extend(batch));
        flush();
    });
}

/// Register the waker an off-event-task write fires to request a mailbox drain.
///
/// The framework's `App` registers `Document::request_runtime_flush` here at startup, pairing it
/// with [`flush_deferred`] on the engine's flush hook. Registering enables the deferred path;
/// [`clear_flush_waker`] disables it again at shutdown.
///
/// Framework plumbing: applications wire this through `App`, not directly.
pub fn set_flush_waker(waker: Arc<dyn Fn() + Send + Sync>) {
    *lock::mutex(&rt().waker) = Some(waker);
    rt().has_waker.store(true, Ordering::Release);
}

/// Unregister the waker and drop any undrained deferred work, so a torn-down app leaves no stale
/// mailbox entries or armed gate behind.
///
/// Framework plumbing: `App` calls this on shutdown.
pub fn clear_flush_waker() {
    // Disable the deferred path first, so any write racing this teardown takes the inline path
    // rather than enqueuing into a mailbox nothing will drain.
    rt().has_waker.store(false, Ordering::Release);
    *lock::mutex(&rt().waker) = None;
    rt().flush_requested.store(false, Ordering::Release);
    lock::mutex(&rt().mailbox).clear();
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

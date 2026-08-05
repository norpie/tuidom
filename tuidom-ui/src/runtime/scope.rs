//! `Scope` — an owner that frees the signals, effects, cleanups, and tasks created inside it.

use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;

use super::{BoxAny, Inner, SCOPE, ScopeData, ScopeId, SlotId, current_scope, rt};

/// An ownership boundary for reactive state.
///
/// Signals, deriveds, and effects created while a scope is entered ([`run`](Scope::run))
/// belong to it. Disposing the scope frees them, runs its cleanups in reverse registration
/// order, and aborts its spawned tasks — innermost child scopes first. A handle to a freed
/// signal is inert, not dangling, so a task that writes just after disposal is a silent
/// no-op rather than a fault.
pub struct Scope {
    id: ScopeId,
}

impl Scope {
    /// Open a root scope, owning nothing until code runs inside [`run`](Scope::run).
    pub fn new() -> Scope {
        Scope {
            id: create_scope(None),
        }
    }

    /// Open a child scope. Disposing this scope disposes the child; disposing the parent
    /// disposes it too.
    pub fn child(&self) -> Scope {
        Scope {
            id: create_scope(Some(self.id)),
        }
    }

    /// Run `f` with this scope entered, so reactive nodes it creates are owned here.
    pub fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        struct Exit;
        impl Drop for Exit {
            fn drop(&mut self) {
                SCOPE.with(|s| {
                    s.borrow_mut().pop();
                });
            }
        }

        SCOPE.with(|s| s.borrow_mut().push(self.id));
        let _exit = Exit;
        f()
    }

    /// Register a callback to run when this scope is disposed. Callbacks run in reverse
    /// order, after child scopes are disposed and before this scope's nodes are freed, so a
    /// cleanup can still read the signals it owns.
    pub fn on_cleanup(&self, f: impl FnOnce() + Send + Sync + 'static) {
        let mut w = rt().write();
        if let Some(data) = w.scopes.get_mut(&self.id) {
            data.cleanups.push(Box::new(f));
        }
    }

    /// Spawn a task the scope owns. Its `JoinHandle` is aborted on dispose, so an unmounted
    /// component stops working rather than burning CPU on stale writes.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        let mut w = rt().write();
        if let Some(data) = w.scopes.get_mut(&self.id) {
            data.tasks.push(handle);
        }
    }

    /// Dispose the scope: free its nodes, run its cleanups, and abort its tasks.
    pub fn dispose(self) {
        dispose_scope(self.id);
    }
}

impl Default for Scope {
    fn default() -> Self {
        Scope::new()
    }
}

/// Provide a value to the current scope's descendants, so any of them can [`use_context`] it
/// without a prop threaded through every layer in between.
///
/// The value is keyed by its concrete type `T` (D3) — you provide a `Theme` and inject a
/// `Theme` — so a scope holds at most one value per type. Providing the same type again in a
/// nested scope *shadows* the outer one for that subtree, standard context semantics. The value
/// is dropped when its scope is disposed, living exactly as long as the subtree that owns it.
///
/// Called with no scope open, this is a no-op: nothing could ever inject the value, so it is
/// logged and dropped rather than leaked to no owner.
pub fn provide_context<T: Send + Sync + 'static>(value: T) {
    let Some(sid) = current_scope() else {
        tracing::debug!("provide_context called with no scope open; value dropped");
        return;
    };
    let mut w = rt().write();
    if let Some(data) = w.scopes.get_mut(&sid) {
        data.contexts
            .insert(TypeId::of::<T>(), Box::new(value) as BoxAny);
    }
}

/// Inject the nearest `T` a surrounding scope [`provide_context`]-ed, cloning it out, or `None`
/// if no ancestor provides one.
///
/// Resolution walks from the current scope rootward through parent links and stops at the first
/// scope holding a `T`, so a nearer provider shadows a farther one. An absent context is
/// `None`, never a panic — the caller decides what a missing value means.
///
/// The value is cloned while the runtime read lock is held (like [`Signal::get`]), so its
/// `Clone` **must not read or write any signal** — reentering the lock would deadlock. The
/// common contexts (a `Copy` [`Signal`] handle, an `Arc`) clone trivially and satisfy this.
///
/// [`Signal`]: crate::Signal
/// [`Signal::get`]: crate::Signal::get
pub fn use_context<T: Clone + 'static>() -> Option<T> {
    let mut sid = current_scope()?;
    let key = TypeId::of::<T>();
    let w = rt().read();
    loop {
        let data = w.scopes.get(&sid)?;
        if let Some(boxed) = data.contexts.get(&key) {
            return boxed.downcast_ref::<T>().cloned();
        }
        sid = data.parent?;
    }
}

/// Register `f` to run when the current owner scope is disposed — the ambient-scope form of
/// [`Scope::on_cleanup`], for calling inside a component body where the scope is implicit (the
/// same way [`state`](super::state) and [`effect`](super::effect) find their scope).
///
/// Cleanups run in reverse registration order, after child scopes are disposed and before this
/// scope's nodes are freed. With no scope open this is a logged no-op: nothing would own the
/// callback, so it could never run.
pub fn on_cleanup(f: impl FnOnce() + Send + Sync + 'static) {
    let Some(sid) = current_scope() else {
        tracing::debug!("on_cleanup called with no scope open; callback dropped");
        return;
    };
    let mut w = rt().write();
    if let Some(data) = w.scopes.get_mut(&sid) {
        data.cleanups.push(Box::new(f));
    }
}

/// Spawn a task owned by the current scope — the ambient-scope form of [`Scope::spawn`], for
/// calling inside a component body where the scope is implicit (the same way [`on_cleanup`] and
/// [`state`](super::state) find their scope).
///
/// The task's `JoinHandle` is aborted when the scope is disposed, so an unmounted component's
/// background work stops rather than writing into freed signals. This is how the resource pattern
/// is written by hand — the framework ships no resource type; a task plus a tri-state signal is
/// the whole of it:
///
/// ```ignore
/// enum Resource { Loading, Ready(Data) }
///
/// let resource = state(Resource::Loading);
/// spawn(async move {
///     let data = fetch().await;          // the background work
///     resource.set(Resource::Ready(data)); // off-task write: reaches the DOM on the next flush
/// });
/// // render from `resource`; unmounting the component aborts the task mid-flight.
/// ```
///
/// With no scope open the task is still spawned so its work is not silently lost, but detached —
/// nothing will abort it — which is logged, since an un-owned task outliving its intended lifetime
/// is the surprising case. Prefer calling it where a scope is entered.
///
/// Requires a Tokio runtime, like [`Scope::spawn`]; an [`App`](crate::App) provides one.
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(future);
    let Some(sid) = current_scope() else {
        tracing::debug!("spawn called with no scope open; task detached and will not be aborted");
        return;
    };
    let mut w = rt().write();
    if let Some(data) = w.scopes.get_mut(&sid) {
        data.tasks.push(handle);
    }
}

fn create_scope(parent: Option<ScopeId>) -> ScopeId {
    let mut w = rt().write();
    let id = ScopeId(w.next_scope);
    w.next_scope += 1;
    w.scopes.insert(
        id,
        ScopeData {
            parent,
            children: Vec::new(),
            nodes: Vec::new(),
            cleanups: Vec::new(),
            tasks: Vec::new(),
            contexts: HashMap::new(),
        },
    );
    if let Some(p) = parent
        && let Some(pd) = w.scopes.get_mut(&p)
    {
        pd.children.push(id);
    }
    id
}

fn dispose_scope(id: ScopeId) {
    // Take everything the scope owns under one lock, then do the work — running cleanups and
    // aborting tasks — with the lock released, since a cleanup is user code.
    let (children, cleanups, tasks, nodes) = {
        let mut w = rt().write();
        let Some(data) = w.scopes.remove(&id) else {
            return;
        };
        if let Some(p) = data.parent
            && let Some(pd) = w.scopes.get_mut(&p)
        {
            pd.children.retain(|c| *c != id);
        }
        (data.children, data.cleanups, data.tasks, data.nodes)
    };

    for child in children {
        dispose_scope(child);
    }
    for task in tasks {
        task.abort();
    }
    for cleanup in cleanups.into_iter().rev() {
        cleanup();
    }

    let mut w = rt().write();
    for nid in nodes {
        detach_and_remove(&mut w, nid);
    }
}

/// Remove a node and cut every edge touching it, so no surviving node points at a freed slot.
fn detach_and_remove(w: &mut Inner, id: SlotId) {
    let Some(node) = w.nodes.remove(&id) else {
        return;
    };
    for src in node.sources {
        if let Some(s) = w.nodes.get_mut(&src) {
            s.subscribers.remove(&id);
        }
    }
    for sub in node.subscribers {
        if let Some(s) = w.nodes.get_mut(&sub) {
            s.sources.remove(&id);
        }
    }
}

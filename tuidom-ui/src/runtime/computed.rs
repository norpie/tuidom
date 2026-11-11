//! `Derived<T>` — a memoized computation over other reactive values.

use std::marker::PhantomData;
use std::sync::Arc;

use super::{BoxAny, Kind, OBSERVER, SlotId, detach_sources, insert, rt, track};

/// A `Copy`, `Send + Sync` handle to a memoized computation.
///
/// A derived recomputes lazily: it stays dirty after a dependency changes and recomputes on
/// the next read, not on the write. Reading it subscribes the running computation to the
/// derived itself — never to the derived's own dependencies — so a change confined to one
/// input propagates at the derived's granularity.
pub struct Derived<T> {
    id: SlotId,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Derived<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Derived<T> {}

/// Create a memoized derived value from a computation over other signals and deriveds.
///
/// The closure is not run now — it runs on the first read, and again after any dependency it
/// read has changed.
pub fn derived<T, F>(compute: F) -> Derived<T>
where
    T: Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    let compute: Arc<dyn Fn() -> BoxAny + Send + Sync> = Arc::new(move || Box::new(compute()));
    let id = insert(Kind::Derived {
        value: None,
        compute,
        dirty: true,
    });
    Derived {
        id,
        _marker: PhantomData,
    }
}

/// Recompute `id` if it is dirty or has never been computed. Runs the compute closure with
/// the lock released and with `id` installed as the observer, so its reads become its fresh
/// source set.
fn ensure_fresh(id: SlotId) {
    let compute = {
        let guard = rt().read();
        match guard.nodes.get(&id).map(|n| &n.kind) {
            Some(Kind::Derived {
                value,
                compute,
                dirty,
            }) => {
                if !*dirty && value.is_some() {
                    return;
                }
                compute.clone()
            }
            _ => return,
        }
    };

    // Detach old sources and clear the dirty bit before recomputing, so reads during the
    // compute rebuild the source set from scratch.
    {
        let mut w = rt().write();
        if let Some(node) = w.nodes.get_mut(&id)
            && let Kind::Derived { dirty, .. } = &mut node.kind
        {
            *dirty = false;
        }
        detach_sources(&mut w, id);
    }

    let prev = OBSERVER.with(|o| o.replace(Some(id)));
    let value = (compute)();
    OBSERVER.with(|o| o.set(prev));

    let mut w = rt().write();
    if let Some(node) = w.nodes.get_mut(&id)
        && let Kind::Derived { value: slot, .. } = &mut node.kind
    {
        *slot = Some(value);
    }
}

impl<T: 'static> Derived<T> {
    /// Read via a projection without cloning, returning `None` if the derived has been
    /// disposed.
    ///
    /// The projection runs while the read lock is held; like [`Signal::try_with`], it must
    /// not read or write any signal.
    ///
    /// [`Signal::try_with`]: super::Signal::try_with
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        track(self.id);
        ensure_fresh(self.id);
        let guard = rt().read();
        match guard.nodes.get(&self.id).map(|n| &n.kind) {
            Some(Kind::Derived {
                value: Some(value), ..
            }) => value.downcast_ref::<T>().map(f),
            _ => None,
        }
    }

    /// Clone the current value, or `None` if the derived has been disposed.
    pub fn try_get(&self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(T::clone)
    }
}

impl<T: Default + 'static> Derived<T> {
    /// Recompute if stale, then read via a projection without cloning the whole value,
    /// subscribing the running computation to this derived.
    ///
    /// If the derived has been disposed the closure sees `T::default()` and a warning is
    /// logged. The same lock discipline as [`try_with`](Self::try_with) applies: the closure
    /// must not touch any signal.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(self.id);
        ensure_fresh(self.id);
        let guard = rt().read();
        if let Some(Kind::Derived {
            value: Some(value), ..
        }) = guard.nodes.get(&self.id).map(|n| &n.kind)
            && let Some(v) = value.downcast_ref::<T>()
        {
            return f(v);
        }
        drop(guard);
        tracing::warn!("read of a disposed derived; falling back to Default");
        f(&T::default())
    }
}

impl<T: Clone + Default + 'static> Derived<T> {
    /// Recompute if stale, then clone the value, subscribing the running computation to this
    /// derived. Falls back to `T::default()` (and logs) if it has been disposed.
    pub fn get(&self) -> T {
        self.with(T::clone)
    }
}

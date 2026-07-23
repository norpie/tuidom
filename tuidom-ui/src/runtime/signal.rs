//! `Signal<T>` — a writable source of truth, and its read/write API.

use std::marker::PhantomData;

use super::{BoxAny, Kind, SlotId, insert, rt, schedule, track};

/// A `Copy`, `Send + Sync` handle to a reactive value.
///
/// The handle is an index, not the value — copying it is free, and it can be captured in as
/// many closures as needed without the `Arc::clone` prelude that dominates other Rust signal
/// libraries. The value lives in the runtime arena, keyed by this handle.
///
/// Reading inside a [`derived`](super::derived) or [`effect`](super::effect) subscribes that
/// computation to this signal automatically; writing reruns whatever subscribed.
pub struct Signal<T> {
    id: SlotId,
    // `fn() -> T` keeps the handle `Send + Sync + Copy` regardless of `T`, while still
    // carrying `T` for the typed API.
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

/// Create a writable signal holding `value`.
pub fn state<T: Send + Sync + 'static>(value: T) -> Signal<T> {
    let id = insert(Kind::Signal {
        value: Box::new(value),
    });
    Signal {
        id,
        _marker: PhantomData,
    }
}

impl<T: 'static> Signal<T> {
    /// Read via a projection without cloning the whole value, returning `None` if the signal
    /// has been disposed.
    ///
    /// The closure runs while the arena's read lock is held, so it **must not read or write
    /// any signal** — doing so reenters the single runtime lock and deadlocks. Keep it to a
    /// pure projection of the value.
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        track(self.id);
        let guard = rt().read();
        match guard.nodes.get(&self.id).map(|n| &n.kind) {
            Some(Kind::Signal { value }) => value.downcast_ref::<T>().map(f),
            _ => None,
        }
    }

    /// Clone the current value, or `None` if the signal has been disposed.
    pub fn try_get(&self) -> Option<T>
    where
        T: Clone,
    {
        self.try_with(T::clone)
    }
}

impl<T: Default + 'static> Signal<T> {
    /// Read via a projection without cloning the whole value.
    ///
    /// If the signal has been disposed the closure sees `T::default()` and a warning is
    /// logged. The same lock discipline as [`try_with`](Self::try_with) applies: the closure
    /// must not touch any signal.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(self.id);
        let guard = rt().read();
        if let Some(Kind::Signal { value }) = guard.nodes.get(&self.id).map(|n| &n.kind)
            && let Some(v) = value.downcast_ref::<T>()
        {
            return f(v);
        }
        drop(guard);
        tracing::warn!("read of a disposed signal; falling back to Default");
        f(&T::default())
    }
}

impl<T: Clone + Default + 'static> Signal<T> {
    /// Clone the current value, subscribing the running computation to this signal.
    ///
    /// Falls back to `T::default()` (and logs) if the signal has been disposed.
    pub fn get(&self) -> T {
        self.with(T::clone)
    }
}

impl<T: PartialEq + Send + Sync + 'static> Signal<T> {
    /// Replace the value, rerunning subscribers — unless the new value is equal to the old,
    /// in which case nothing happens at all (no write, no rerun), mirroring the engine's
    /// `set_text_content`.
    ///
    /// A `set` on a disposed signal is a silent no-op.
    pub fn set(&self, value: T) {
        let subs = {
            let mut w = rt().write();
            match w.nodes.get_mut(&self.id) {
                Some(node) => match &mut node.kind {
                    Kind::Signal { value: slot } => {
                        let unchanged = slot.downcast_ref::<T>().is_some_and(|cur| *cur == value);
                        if unchanged {
                            None
                        } else {
                            let boxed: BoxAny = Box::new(value);
                            *slot = boxed;
                            Some(node.subscribers.iter().copied().collect::<Vec<_>>())
                        }
                    }
                    _ => None,
                },
                None => None,
            }
        };
        if let Some(subs) = subs {
            schedule(subs);
        }
    }
}

impl<T: Send + Sync + 'static> Signal<T> {
    /// Mutate the value in place, then rerun subscribers.
    ///
    /// Unlike [`set`](Self::set) there is no equality skip — a mutation is assumed to change
    /// the value, since the old one is never cloned to compare. The closure runs with the
    /// lock released, so it may read other signals; a mutation on a disposed signal is a
    /// silent no-op.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        // Take the value out under the lock, mutate it unlocked, then put it back — so the
        // user closure never runs while the runtime lock is held.
        let taken: Option<Box<T>> = {
            let mut w = rt().write();
            match w.nodes.get_mut(&self.id) {
                Some(node) => match &mut node.kind {
                    Kind::Signal { value } => {
                        let placeholder: BoxAny = Box::new(());
                        let old = std::mem::replace(value, placeholder);
                        old.downcast::<T>().ok()
                    }
                    _ => None,
                },
                None => None,
            }
        };

        let Some(mut boxed) = taken else {
            tracing::warn!("update on a disposed signal; ignored");
            return;
        };
        f(&mut boxed);

        let subs = {
            let mut w = rt().write();
            match w.nodes.get_mut(&self.id) {
                Some(node) => match &mut node.kind {
                    Kind::Signal { value } => {
                        let restored: BoxAny = boxed;
                        *value = restored;
                        Some(node.subscribers.iter().copied().collect::<Vec<_>>())
                    }
                    _ => None,
                },
                None => None,
            }
        };
        if let Some(subs) = subs {
            schedule(subs);
        }
    }
}

macro_rules! op_assign {
    ($assign:ident, $method:ident, $op_trait:ident, $op:tt) => {
        impl<T> ::core::ops::$assign<T> for Signal<T>
        where
            T: ::core::ops::$op_trait<Output = T> + Clone + PartialEq + Send + Sync + 'static,
        {
            fn $method(&mut self, rhs: T) {
                if let Some(cur) = self.try_get() {
                    self.set(cur $op rhs);
                }
            }
        }
    };
}

op_assign!(AddAssign, add_assign, Add, +);
op_assign!(SubAssign, sub_assign, Sub, -);
op_assign!(MulAssign, mul_assign, Mul, *);
op_assign!(DivAssign, div_assign, Div, /);
op_assign!(RemAssign, rem_assign, Rem, %);
op_assign!(BitAndAssign, bitand_assign, BitAnd, &);
op_assign!(BitOrAssign, bitor_assign, BitOr, |);
op_assign!(BitXorAssign, bitxor_assign, BitXor, ^);

//! How `view!` turns a call-site prop expression into the prop a component stores.
//!
//! The call site cannot see a prop's type, so it hands every non-literal, non-closure value over as
//! `move || expr` and lets the setter's parameter type pick the conversion: a signal prop stays
//! synced to the expression, whether it produces a value or a signal holding one, a raw prop
//! takes its value once, and a bound prop takes the caller's signal itself.

use crate::{Callback, Children, Derived, NodeRef, Signal, View, effect, state};

/// A prop type buildable from a call-site expression, given as a closure producing its value.
pub trait FromPropExpr<F> {
    /// Build the prop from `f`.
    fn from_prop_expr(f: F) -> Self;
}

/// Build a prop of whatever type the setter receiving it expects.
pub fn prop_expr<P: FromPropExpr<F>, F>(f: F) -> P {
    P::from_prop_expr(f)
}

/// Seeded once, then kept in step with the expression by an effect owned by the caller. The
/// expression may produce the value or a signal or derived holding it.
impl<T, V, F> FromPropExpr<F> for Signal<T>
where
    T: PartialEq + Send + Sync + 'static,
    V: PropValue<T>,
    F: Fn() -> V + Send + Sync + 'static,
{
    fn from_prop_expr(f: F) -> Self {
        let prop = state(f().read());
        effect(move || prop.set(f().read()));
        prop
    }
}

/// What a signal prop's expression may produce: the value itself, or a reactive handle read on
/// each run of the effect keeping the prop in step.
pub trait PropValue<T> {
    /// The value, subscribing the running computation when it comes from a handle.
    fn read(self) -> T;
}

impl<T> PropValue<T> for T {
    fn read(self) -> T {
        self
    }
}

impl<T: Clone + Default + 'static> PropValue<T> for Signal<T> {
    fn read(self) -> T {
        self.get()
    }
}

impl<T: Clone + Default + 'static> PropValue<T> for Derived<T> {
    fn read(self) -> T {
        self.get()
    }
}

/// What the setter of a `Signal<T>` parameter takes: the caller's own signal, passed through.
///
/// A separate type because `Signal<T>` already converts from a closure producing `T`, and a second
/// conversion from a closure producing `Signal<T>` would overlap it.
pub struct Bound<T: 'static>(pub Signal<T>);

impl<T: 'static, F: FnOnce() -> Signal<T>> FromPropExpr<F> for Bound<T> {
    fn from_prop_expr(f: F) -> Self {
        Bound(f())
    }
}

impl<F: FnOnce() -> NodeRef> FromPropExpr<F> for NodeRef {
    fn from_prop_expr(f: F) -> Self {
        f()
    }
}

impl<A, R, F: FnOnce() -> Callback<A, R>> FromPropExpr<F> for Callback<A, R> {
    fn from_prop_expr(f: F) -> Self {
        f()
    }
}

/// A slot: the call-site expression becomes the thunk, so it is built where the component places
/// the slot, and rebuilt when a signal it read changes.
impl<V: Into<View>, F: Fn() -> V + Send + Sync + 'static> FromPropExpr<F> for Children {
    fn from_prop_expr(f: F) -> Self {
        Children::new(move || f().into())
    }
}

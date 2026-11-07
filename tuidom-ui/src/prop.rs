//! How `view!` turns a call-site prop expression into the prop a component stores.
//!
//! The call site cannot see a prop's type, so it hands every non-literal, non-closure value over as
//! `move || expr` and lets the setter's parameter type pick the conversion: a signal prop stays
//! synced to the expression, a raw prop takes its value once.

use crate::{Callback, NodeRef, Signal, effect, state};

/// A prop type buildable from a call-site expression, given as a closure producing its value.
pub trait FromPropExpr<F> {
    /// Build the prop from `f`.
    fn from_prop_expr(f: F) -> Self;
}

/// Build a prop of whatever type the setter receiving it expects.
pub fn prop_expr<P: FromPropExpr<F>, F>(f: F) -> P {
    P::from_prop_expr(f)
}

/// Seeded once, then kept in step with the expression by an effect owned by the caller.
impl<T, F> FromPropExpr<F> for Signal<T>
where
    T: PartialEq + Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    fn from_prop_expr(f: F) -> Self {
        let prop = state(f());
        effect(move || prop.set(f()));
        prop
    }
}

impl<F: FnOnce() -> NodeRef> FromPropExpr<F> for NodeRef {
    fn from_prop_expr(f: F) -> Self {
        f()
    }
}

impl<A, F: FnOnce() -> Callback<A>> FromPropExpr<F> for Callback<A> {
    fn from_prop_expr(f: F) -> Self {
        f()
    }
}

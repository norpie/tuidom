//! Component props: how `view!` turns a call-site expression into the prop a component stores,
//! and how a mounted component's props take newer values.
//!
//! The call site cannot see a prop's type, so it hands every non-literal, non-closure value over as
//! `move || expr` and lets the setter's parameter type pick the conversion: a signal prop stays
//! synced to the expression, whether it produces a value or a signal holding one, a raw prop
//! takes its value once, and a bound prop takes the caller's signal itself.

use crate::{Callback, Children, Derived, NodeRef, Signal, View, effect, state};

/// The props struct `#[component]` generates for a component, which it implements this for.
///
/// Props built by hand, through the generated builder, mount with [`into_view`]. A clone shares
/// their signals, so a clone kept from before mounting takes newer props through [`assign`], and
/// the mounted component updates without its body running again: its nodes, and with them their
/// scroll offsets, focus and running transitions, stay as they are.
///
/// [`into_view`]: ComponentProps::into_view
/// [`assign`]: ComponentProps::assign
pub trait ComponentProps: Clone + Send + Sync + 'static {
    /// Move each signal prop's value from `next` into these props, in one batch.
    ///
    /// A value equal to the current one, for a type that implements `PartialEq`, writes nothing,
    /// so nothing that reads it reruns. Props that are not signals of their own are left as they
    /// are: children and other slots, callbacks, node refs, and a `Signal<T>` parameter bound to
    /// the caller's signal. A prop kept in step with an expression stays in step with it, so the
    /// next change on either side wins.
    fn assign(&self, next: Self);

    /// The component these props build. Its body runs when the view mounts, under the
    /// component's own scope; the props' signals stay owned by the scope that built them.
    fn into_view(self) -> View;
}

/// The props of a component that takes a `children` parameter.
pub trait WithChildren: ComponentProps {
    /// Replace the `children` prop.
    fn set_children(&mut self, children: Children);
}

/// One signal prop's move from newer props, dispatched on whether its type can be compared.
///
/// `(&&Assign(current, next)).assign()` resolves to [`AssignEq`] when `T: PartialEq`, since that
/// impl matches the receiver without a deref, and to [`AssignAny`] otherwise.
pub struct Assign<T: 'static>(pub Signal<T>, pub Signal<T>);

/// Move a comparable value, skipping an equal one.
pub trait AssignEq {
    /// Move the value.
    fn assign(&self);
}

impl<T: PartialEq + 'static> AssignEq for &Assign<T> {
    fn assign(&self) {
        self.0.take_from(self.1, |current, next| current == next);
    }
}

/// Move a value that cannot be compared, always rerunning its readers.
pub trait AssignAny {
    /// Move the value.
    fn assign(&self);
}

impl<T: 'static> AssignAny for Assign<T> {
    fn assign(&self) {
        self.0.take_from(self.1, |_, _| false);
    }
}

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

/// What the builder setter of a `Signal<T>` parameter takes: the caller's own signal, passed
/// through, so the component writes back into it.
///
/// `view!` builds one from a signal named at the call site. Props built by hand pass
/// `Bound(signal)`, or `signal.into()`.
///
/// A separate type because a signal prop's `Signal<T>` already converts from a closure producing
/// `T`, and a second conversion from a closure producing `Signal<T>` would overlap it.
pub struct Bound<T: 'static>(pub Signal<T>);

impl<T: 'static> From<Signal<T>> for Bound<T> {
    fn from(signal: Signal<T>) -> Self {
        Bound(signal)
    }
}

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

//! [`Callback`] — a cheaply cloned, type-erased handler a component takes as a prop.

use std::sync::Arc;

/// A handler a component accepts as a prop, for talking back to its parent.
///
/// A component cannot reach into its parent's state, so child-to-parent communication runs the
/// other way round: the parent hands the child a `Callback`, and the child *calls* it. The
/// parent's closure captures whatever parent state it means to touch — a signal to write, another
/// callback to forward to — and the child stays ignorant of all of it, invoking `on_x.call(arg)`
/// at the right moment.
///
/// ```ignore
/// # use tuidom_ui::{component, view, Callback, View};
/// #[component]
/// fn Emitter(on_fire: Callback<i32>) -> View {
///     view! {
///         box {
///             focusable: true,
///             on_key_press: move |_| on_fire.call(7),
///         }
///     }
/// }
///
/// // The parent wires the callback to its own signal — `view!` lowers the closure to a
/// // `Callback::new(..)` for it:
/// view! { Emitter { on_fire: move |n| received.set(n) } };
/// ```
///
/// A callback can also return a value, `R`, which lets a parent hand a child a builder: a list
/// taking `render: Callback<usize, View>` asks its parent for each row as it needs one.
///
/// Unlike a [`Signal`], a `Callback` is `Clone` but not `Copy` — it holds an `Arc`. A body that
/// invokes it from more than one closure clones it per closure.
///
/// [`Signal`]: crate::Signal
pub struct Callback<A, R = ()>(Arc<dyn Fn(A) -> R + Send + Sync>);

impl<A, R> Callback<A, R> {
    /// Wrap a closure as a callback.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(A) -> R + Send + Sync + 'static,
    {
        Callback(Arc::new(f))
    }

    /// Invoke the callback with `arg`, returning what it returns.
    ///
    /// The framework never calls this — the child body does, at a moment of its choosing that is
    /// typically already inside a caught engine listener, so no panic guard is added here.
    pub fn call(&self, arg: A) -> R {
        (self.0)(arg)
    }
}

// Hand-written rather than derived: cloning an `Arc` is always cheap and never needs `A: Clone`,
// which `#[derive(Clone)]` would demand.
impl<A, R> Clone for Callback<A, R> {
    fn clone(&self) -> Self {
        Callback(self.0.clone())
    }
}

/// A no-op callback returning `R::default()` — the default a `#[prop(default)]` callback prop
/// falls back to when the call site supplies none, so an optional handler needs no `Option`
/// dance at the call.
impl<A, R: Default> Default for Callback<A, R> {
    fn default() -> Self {
        Callback(Arc::new(|_| R::default()))
    }
}

//! `tuidom-ui` — the reactive layer built on top of the tuidom engine.
//!
//! Two layers live here. The **signal runtime** is a self-contained reactive graph — `Copy`,
//! `Send + Sync` signal handles with automatic dependency tracking — that touches no
//! `Document`. The **view layer** binds that runtime to real engine nodes: a [`View`] mounts
//! under a parent, its signal reads become subscriptions, and a change reruns only the affected
//! binding.
//!
//! On top of both sits the [`view!`] macro, a Rust-like template that lowers to the view
//! layer's builder calls — see its own docs for the grammar and a worked counter.
//!
//! ```
//! use tuidom_ui::{state, derived, effect, batch};
//!
//! let count = state(0);
//! let doubled = derived(move || count.get() * 2);
//!
//! effect(move || {
//!     // reruns whenever `count` (and therefore `doubled`) changes
//!     let _ = doubled.get();
//! });
//!
//! batch(|| {
//!     count.set(1);
//!     count.set(2);
//! }); // one flush, not two
//! ```
#![warn(missing_docs)]

// The `view!` macro expands to absolute `::tuidom_ui::…` paths so it resolves the same way in
// downstream crates and here; this alias is what makes those paths resolve in-crate.
extern crate self as tuidom_ui;

mod app;
mod callback;
mod lock;
mod prop;
mod runtime;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod view;

pub use app::{App, AppSession};
pub use callback::Callback;
pub use prop::{Bound, ComponentProps, WithChildren};
pub use runtime::{
    Derived, Scope, Signal, batch, derived, effect, on_cleanup, provide_context, spawn, state,
    untracked, use_context,
};
/// Support for code `view!` and `#[component]` expand to. Not public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::prop::{Assign, AssignAny, AssignEq, FromPropExpr, PropValue, prop_expr};
}

// `component` names two things in two namespaces: the attribute macro (macro namespace) that
// rewrites a component function, and the `view::component` value (below) it lowers to. They
// coexist under one path — `#[component]` finds the macro, `component(..)` the function.
pub use tuidom_ui_macros::{component, view};
pub use view::{
    Children, Element, Frames, Mounted, NodeRef, Text, View, VirtualList, VirtualListRef, boxed,
    canvas, component, dyn_text, dynamic, fragment, frames, image, input, keyed, mount, node_ref,
    on_mount, portal, text, use_document, virtual_list, virtual_list_measured, virtual_list_ref,
};

/// A [`Derived`] computing `expr`: shorthand for `derived(move || expr)`.
#[macro_export]
macro_rules! derived {
    ($expr:expr $(,)?) => {
        $crate::derived(move || $expr)
    };
}

/// `handle` itself, unchanged: a [`Signal`] or [`Derived`] passed along rather than read.
///
/// It does nothing to its argument. In a `#[component(reactive)]` body, where a signal's name
/// stands for its value, it keeps the handle.
#[macro_export]
macro_rules! signal {
    ($handle:expr $(,)?) => {
        $handle
    };
}

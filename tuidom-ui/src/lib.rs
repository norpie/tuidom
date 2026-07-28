//! `tuidom-ui` — the reactive layer built on top of the tuidom engine.
//!
//! Two layers live here. The **signal runtime** (framework milestone F2) is a self-contained
//! reactive graph — `Copy`, `Send + Sync` signal handles with automatic dependency tracking —
//! that touches no `Document`. The **view layer** (F3) binds that runtime to real engine
//! nodes: a [`View`] mounts under a parent, its signal reads become subscriptions, and a
//! change reruns only the affected binding.
//!
//! On top of both sits the [`view!`] macro (F4), a Rust-like template that lowers to the view
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

mod callback;
mod lock;
mod runtime;
mod view;

pub use callback::Callback;
pub use runtime::{
    Derived, Scope, Signal, batch, derived, effect, provide_context, state, use_context,
};
// `component` names two things in two namespaces: the attribute macro (macro namespace) that
// rewrites a component function, and the `view::component` value (below) it lowers to. They
// coexist under one path — `#[component]` finds the macro, `component(..)` the function.
pub use tuidom_ui_macros::{component, view};
pub use view::{
    Children, Element, Mounted, View, boxed, component, dyn_text, dynamic, fragment, input, mount,
    text,
};

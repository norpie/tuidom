//! `tuidom-ui` — the reactive layer built on top of the tuidom engine.
//!
//! This crate currently delivers the **signal runtime** (framework milestone F2): `Copy`,
//! `Send + Sync` signal handles with automatic dependency tracking. Nothing here touches a
//! `Document` — the runtime is a self-contained reactive graph, and the view layer that
//! binds it to real nodes lands in a later milestone.
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

mod lock;
mod runtime;

pub use runtime::{Derived, Scope, Signal, batch, derived, effect, state};

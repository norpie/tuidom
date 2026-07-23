//! `tuidom-ui` — the reactive layer built on top of the tuidom engine.
//!
//! Two layers live here. The **signal runtime** (framework milestone F2) is a self-contained
//! reactive graph — `Copy`, `Send + Sync` signal handles with automatic dependency tracking —
//! that touches no `Document`. The **view layer** (F3) binds that runtime to real engine
//! nodes: a [`View`] mounts under a parent, its signal reads become subscriptions, and a
//! change reruns only the affected binding.
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
mod view;

pub use runtime::{Derived, Scope, Signal, batch, derived, effect, state};
pub use view::{Children, Element, Mounted, View, boxed, dyn_text, dynamic, fragment, mount, text};

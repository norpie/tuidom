//! `tuidom-router` — layout chains for `tuidom-ui`.
//!
//! There is no path matcher and no route table: an application's route is its own enum, and a
//! `match` over it builds a [`Chain`], the layouts a route shows plus its leaf. [`routed`] mounts
//! the chain a closure produces and follows it as the route changes.
//!
//! The crate builds on `tuidom-ui`'s public API alone.
#![warn(missing_docs)]

mod chain;
#[cfg(test)]
mod tests;

pub use chain::{Chain, Layouts, routed};

//! # tuidom
//!
//! A DOM-based terminal UI library for Rust.
//!
//! tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives
//! for building sophisticated TUI applications.

#![warn(missing_docs)]

/// Style system: [`Color`], [`Style`], [`StyleValue`], and supporting types.
pub mod style;

// Re-export the macro
pub use tuidom_derive::node;

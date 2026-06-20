//! # tuidom
//!
//! A DOM-based terminal UI library for Rust.
//!
//! tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives
//! for building sophisticated TUI applications.

#![warn(missing_docs)]

/// Debug overlay.
mod debug;
/// Node handle types.
mod id;
/// Internal document state.
mod inner;
/// Render + event loop.
mod event_loop;
/// Animation driver and types.
pub mod animation;
/// Event types and dispatch.
pub mod event;
/// Taffy-based flexbox layout.
pub(crate) mod layout;
/// Node data storage and views.
mod node;
/// Screen buffer and rendering.
pub(crate) mod render;

/// The [`Document`] type and public API.
pub mod document;
/// Style system: [`Color`](style::Color), [`Style`](style::Style), [`StyleValue`](style::StyleValue), and supporting types.
pub mod style;

pub use document::Document;
pub use id::NodeId;

// Re-export the macro
pub use tuidom_derive::node;

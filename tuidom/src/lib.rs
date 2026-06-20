//! # tuidom
//!
//! A DOM-based terminal UI library for Rust.
//!
//! tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives
//! for building sophisticated TUI applications.

#![warn(missing_docs)]

/// Animation driver and types.
pub mod animation;
/// Debug overlay.
mod debug;
/// Error types returned by tuidom APIs.
mod error;
/// Event types and dispatch.
pub mod event;
/// Render + event loop.
mod event_loop;
/// Node handle types.
mod id;
/// Internal document state.
mod inner;
/// Taffy-based flexbox layout.
pub(crate) mod layout;
mod lock;
/// Node data storage and views.
mod node;
/// Screen buffer and rendering.
pub(crate) mod render;

/// The [`Document`] type and public API.
pub mod document;
/// Style system: [`Color`](style::Color), [`Style`](style::Style), [`StyleValue`](style::StyleValue), and supporting types.
pub mod style;

pub use document::Document;
pub use error::{Result, TuidomError};
pub use id::NodeId;

// Re-export the macro
pub use tuidom_derive::node;

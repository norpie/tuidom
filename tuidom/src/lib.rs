//! # tuidom
//!
//! A DOM-based terminal UI library for Rust.
//!
//! tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives
//! for building sophisticated TUI applications.

#![warn(missing_docs)]

/// Animation driver and types.
pub mod animation;
/// Error types returned by tuidom APIs.
mod error;
/// Event types and dispatch.
pub mod event;
/// Render + event loop.
mod event_loop;
/// Centering and other geometry helpers.
pub mod geometry;
/// Headless runtime and screen inspection APIs.
pub mod headless;
/// Node handle types.
mod id;
/// Internal document state.
mod inner;
/// Taffy-based flexbox layout.
pub(crate) mod layout;
mod lock;
/// Node data storage and views.
mod node;
mod paint_order;
/// Runtime performance metrics.
pub mod performance;
/// Screen buffer and rendering.
pub(crate) mod render;
mod runtime_event;

/// The [`Document`] type and public API.
pub mod document;
/// Style system: [`Color`](style::Color), [`Style`](style::Style), [`StyleValue`](style::StyleValue), and supporting types.
pub mod style;

pub use document::Document;
pub use error::{Result, TuidomError};
pub use id::NodeId;
pub use node::{LayoutRect, LayoutView, NodeKindView, NodeView, ScrollOffset};

// Re-export the macro
pub use tuidom_derive::node;

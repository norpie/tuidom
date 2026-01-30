//! Procedural macros for tuidom.
//!
//! This crate provides the `node!` macro for declarative DOM construction.

use proc_macro::TokenStream;

/// Declarative DOM construction macro.
///
/// # Example
///
/// ```ignore
/// let root = node!(doc,
///     box id="container" {
///         text { "Hello World" }
///     }
/// );
/// ```
#[proc_macro]
pub fn node(_input: TokenStream) -> TokenStream {
    // TODO: Implement node! macro
    todo!("node! macro not yet implemented")
}

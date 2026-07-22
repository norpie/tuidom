//! Macros for the [tuidom](https://github.com/norpie/tuidom) engine.
//!
//! This crate holds only macros that work against the engine on its own. That is a
//! permanent boundary rather than a description of what is here today: a macro that needs
//! the framework layer belongs to the framework's macro crate, and keeping the two apart is
//! what stops an engine-level macro from quietly growing a framework dependency.
//!
//! Nothing here is named directly by downstream code — `tuidom` re-exports it.

#![warn(missing_docs)]

mod style;

use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Build a [`Style`] from a property list.
///
/// ```ignore
/// let heading = style! {
///     width: 100%,
///     padding: 1 2,
///     flex_direction: column,
///     background: --primary.darken(0.1),
///     color: inherit,
/// };
/// ```
///
/// The result is a plain `Style`, so everything that accepts one accepts this.
///
/// Values are sugared according to the property's own type, and any Rust expression is
/// accepted wherever the sugar does not apply. Two consequences worth knowing:
///
/// - A bare lowercase ident is a keyword or an enum variant, never a local variable.
///   Wrap it in parentheses — `(column)` — to mean the variable.
/// - `..base` starts from a *copy* of another style, so the base stays usable.
///
/// [`Style`]: ../tuidom/style/struct.Style.html
#[proc_macro]
pub fn style(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as style::StyleInput);
    match style::expand(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

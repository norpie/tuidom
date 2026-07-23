//! Framework-level macros for [`tuidom-ui`](https://github.com/norpie/tuidom).
//!
//! This mirrors the `tuidom-macros`/`style!` split one layer up: a macro that needs the
//! framework layer — not just the engine — lives here rather than in `tuidom-macros`. That
//! boundary is permanent, so an engine-level macro can never grow a framework dependency by
//! accident. Nothing here is named directly; `tuidom-ui` re-exports it.
//!
//! `view!` is a *non-rewriting* template macro (framework form A): it lowers a Rust-like
//! template to `tuidom-ui` builder calls without analyzing whether expressions read signals.
//! Reads stay explicit. The only thing it dispatches on is syntax.

#![warn(missing_docs)]

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, braced, parenthesized};

/// Build a [`View`] from a Rust-like template.
///
/// ```ignore
/// use tuidom_ui::view;
///
/// let ui = view! { text("hello") };
/// let empty = view! { box {} };
/// ```
///
/// The macro emits absolute `::tuidom_ui::…` paths, so the crate must be reachable under that
/// name — which it is for downstream users, and in-crate via `extern crate self as tuidom_ui`.
///
/// This is step 1: a lone `text(..)` or empty `box {}`. Children, attributes, events, and
/// control flow arrive in later F4 steps.
///
/// [`View`]: ../tuidom_ui/struct.View.html
#[proc_macro]
pub fn view(input: TokenStream) -> TokenStream {
    let node = syn::parse_macro_input!(input as Node);
    let expr = node.expand();
    quote! { ::tuidom_ui::View::from(#expr) }.into()
}

/// One node of a `view!` template.
enum Node {
    /// An element by name, e.g. `box`.
    Element(Ident),
    /// A text node: `text(expr)`.
    Text(Expr),
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Element names may be reserved words — `box` is one — so read them permissively
        // rather than as ordinary identifiers.
        let name = Ident::parse_any(input)?;
        if name == "text" {
            let args;
            parenthesized!(args in input);
            Ok(Node::Text(args.parse()?))
        } else {
            let body;
            braced!(body in input);
            if !body.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "element children are not supported yet (F4 step 2)",
                ));
            }
            Ok(Node::Element(name))
        }
    }
}

impl Node {
    fn expand(&self) -> proc_macro2::TokenStream {
        match self {
            Node::Text(expr) => quote! { ::tuidom_ui::text(#expr) },
            Node::Element(name) => element_ctor(name),
        }
    }
}

/// Map an element name to its `tuidom-ui` constructor.
fn element_ctor(name: &Ident) -> proc_macro2::TokenStream {
    match name.to_string().as_str() {
        "box" => quote! { ::tuidom_ui::boxed() },
        other => {
            let message = format!("unknown element `{other}`");
            quote_spanned! { name.span() => compile_error!(#message) }
        }
    }
}

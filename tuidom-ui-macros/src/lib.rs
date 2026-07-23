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
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token, braced, parenthesized};

/// Build a [`View`] from a Rust-like template.
///
/// ```ignore
/// use tuidom_ui::view;
///
/// let ui = view! {
///     box {
///         text("Count:"),
///         box { "nested" },
///     }
/// };
/// ```
///
/// The macro emits absolute `::tuidom_ui::…` paths, so the crate must be reachable under that
/// name — which it is for downstream users, and in-crate via `extern crate self as tuidom_ui`.
///
/// This is step 2: the static element skeleton — `box`/`input` elements, nesting, `text(..)`,
/// and bare string-literal children. Attributes, events, and control flow arrive in later F4
/// steps and are rejected here with an error pointing at the offending token.
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
    /// An element by name with its static children, e.g. `box { text("x") }`.
    Element { name: Ident, children: Vec<Node> },
    /// A text node: `text(expr)`, or a bare string-literal child.
    Text(Expr),
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // A bare string literal is a static text child: `box { "hi" }`.
        if input.peek(LitStr) {
            return Ok(Node::Text(input.parse()?));
        }

        // A leading brace is a reactive view hole — `{ expr }` — which lowers to a dynamic
        // block. That arrives with control flow; reject it here rather than mis-parsing.
        if input.peek(syn::token::Brace) {
            return Err(input.error("reactive view holes `{ expr }` arrive in F4 step 4"));
        }

        // Element names may be reserved words — `box` is one — so read them permissively
        // rather than as ordinary identifiers.
        let name = Ident::parse_any(input)?;
        match name.to_string().as_str() {
            "text" => {
                let args;
                parenthesized!(args in input);
                Ok(Node::Text(args.parse()?))
            }
            "if" | "else" | "match" => Err(syn::Error::new(
                name.span(),
                "control flow (`if` / `match`) arrives in F4 step 4",
            )),
            "each" => Err(syn::Error::new(name.span(), "`each` arrives in F4 step 5")),
            _ => {
                let body;
                braced!(body in input);
                let children = parse_children(&body)?;
                Ok(Node::Element { name, children })
            }
        }
    }
}

/// Parse the comma-separated entries inside an element's braces.
///
/// Every entry is a static child for now; an `ident: value` attribute is detected by the
/// colon and rejected with a step-3 pointer rather than mis-parsed as an element.
fn parse_children(body: ParseStream) -> syn::Result<Vec<Node>> {
    let mut children = Vec::new();
    while !body.is_empty() {
        // An attribute is `key: value`; a child never has a colon as its second token, so the
        // colon is what tells the two apart without caring whether `key` is a keyword.
        if body.peek2(Token![:]) {
            return Err(body.error("attributes and events arrive in F4 step 3"));
        }
        children.push(body.parse()?);
        if body.is_empty() {
            break;
        }
        body.parse::<Token![,]>()?;
    }
    Ok(children)
}

impl Node {
    fn expand(&self) -> TokenStream2 {
        match self {
            Node::Text(expr) => quote! { ::tuidom_ui::text(#expr) },
            Node::Element { name, children } => {
                let ctor = element_ctor(name);
                let children = children.iter().map(Node::expand);
                quote! { #ctor #( .child(#children) )* }
            }
        }
    }
}

/// Map an element name to its `tuidom-ui` constructor.
fn element_ctor(name: &Ident) -> TokenStream2 {
    match name.to_string().as_str() {
        "box" => quote! { ::tuidom_ui::boxed() },
        "input" => quote! { ::tuidom_ui::input() },
        other => {
            let message = format!("unknown element `{other}`");
            quote_spanned! { name.span() => compile_error!(#message) }
        }
    }
}

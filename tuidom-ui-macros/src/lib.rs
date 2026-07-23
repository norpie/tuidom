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
use syn::{Expr, ExprLit, Ident, Lit, LitStr, Token, braced, parenthesized};

/// Build a [`View`] from a Rust-like template.
///
/// ```ignore
/// use tuidom_ui::view;
///
/// let ui = view! {
///     box {
///         focusable: true,
///         style: style!{ padding: 1 },
///         on_click: move |_| count.update(|n| *n += 1),
///
///         text("Count:"),
///         text(count.get()),
///     }
/// };
/// ```
///
/// The macro emits absolute `::tuidom_ui::…` paths, so the crate must be reachable under that
/// name — which it is for downstream users, and in-crate via `extern crate self as tuidom_ui`.
///
/// The one dispatch rule is **literal-vs-expression**: a literal value is applied once and
/// statically, anything else binds reactively (a `dyn_*` setter or a `dyn_text` hole). Control
/// flow (`if` / `match` / `each`) and reactive view holes (`{ expr }`) are rejected here with a
/// span-pointing error naming the F4 step that introduces them.
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
    /// An element by name with its attributes and static children, e.g. `box { text("x") }`.
    Element {
        name: Ident,
        attrs: Vec<Attr>,
        children: Vec<Node>,
    },
    /// A text node: `text(expr)`, or a bare string-literal child.
    Text(Expr),
}

/// One `key: value` attribute inside an element's braces.
struct Attr {
    name: Ident,
    value: Expr,
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
                let (attrs, children) = parse_body(&body)?;
                Ok(Node::Element {
                    name,
                    attrs,
                    children,
                })
            }
        }
    }
}

/// Parse the comma-separated entries inside an element's braces into its attributes and its
/// children. An `ident: value` entry is an attribute — detected by the colon as its second
/// token, so the split holds whether or not the key is a keyword — and everything else is a
/// child node.
fn parse_body(body: ParseStream) -> syn::Result<(Vec<Attr>, Vec<Node>)> {
    let mut attrs = Vec::new();
    let mut children = Vec::new();
    while !body.is_empty() {
        if body.peek2(Token![:]) {
            let name = body.call(Ident::parse_any)?;
            body.parse::<Token![:]>()?;
            let value: Expr = body.parse()?;
            // A misspelled event is a closure handed to a string-attribute setter otherwise —
            // a confusing type error deep in expanded code. Catch it at the user's token.
            let key = name.to_string();
            if key.starts_with("on_") && !is_known_event(&key) {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "unknown event handler `{key}`; F3 exposes `on_click` and `on_key_press`"
                    ),
                ));
            }
            attrs.push(Attr { name, value });
        } else {
            children.push(body.parse()?);
        }
        if body.is_empty() {
            break;
        }
        body.parse::<Token![,]>()?;
    }
    Ok((attrs, children))
}

/// The event attributes F3's `Element` exposes. Widening this set is mechanical as the view
/// layer grows more `on_*` setters.
fn is_known_event(name: &str) -> bool {
    matches!(name, "on_click" | "on_key_press")
}

impl Node {
    fn expand(&self) -> TokenStream2 {
        match self {
            Node::Text(expr) => text_node(expr),
            Node::Element {
                name,
                attrs,
                children,
            } => {
                let ctor = element_ctor(name);
                let attrs = attrs.iter().map(Attr::expand);
                let children = children.iter().map(|child| {
                    let view = child.expand();
                    quote! { .child(#view) }
                });
                quote! { #ctor #(#attrs)* #(#children)* }
            }
        }
    }
}

/// Lower a text node. A string literal is a static [`text`] node; any other expression is a
/// reactive text hole, updated in place through `dyn_text` — stringified through `Display` so a
/// non-`String` value (an integer, a `&str`) drops in without ceremony.
fn text_node(expr: &Expr) -> TokenStream2 {
    if is_str_literal(expr) {
        quote! { ::tuidom_ui::text(#expr) }
    } else {
        quote! { ::tuidom_ui::dyn_text(move || ::std::string::ToString::to_string(&(#expr))) }
    }
}

impl Attr {
    fn expand(&self) -> TokenStream2 {
        let name = &self.name;
        let value = &self.value;
        let key = name.to_string();
        match key.as_str() {
            // A `Style` is never a literal, so `style` is always a reactive whole-`Style` swap —
            // the engine animates the diff. A signal-free value (an inline `style!`) just runs
            // its effect once, which is the accepted cost of the literal-only static split.
            "style" => quote! { .dyn_style(move || #value) },
            "focusable" => {
                if is_bool_literal(value) {
                    quote! { .focusable(#value) }
                } else {
                    quote! { .dyn_focusable(move || #value) }
                }
            }
            "on_click" | "on_key_press" => {
                let handler = event_handler(value);
                quote! { .#name(#handler) }
            }
            // Any other key is a raw string attribute: a string literal is static, anything else
            // binds reactively and is stringified through `Display`.
            other => {
                if is_str_literal(value) {
                    quote! { .attr(#other, #value) }
                } else {
                    quote! {
                        .dyn_attr(#other, move || ::std::string::ToString::to_string(&(#value)))
                    }
                }
            }
        }
    }
}

/// Rebuild an event handler, wrapping a zero-argument closure (`|| …`) so it takes the ignored
/// event argument the engine listener requires (`|_| …`). Any other expression — a one-argument
/// closure, a function path — passes through untouched.
fn event_handler(value: &Expr) -> TokenStream2 {
    if let Expr::Closure(closure) = value
        && closure.inputs.is_empty()
    {
        let mut closure = closure.clone();
        closure.inputs.push(syn::Pat::Wild(syn::PatWild {
            attrs: Vec::new(),
            underscore_token: Default::default(),
        }));
        return quote! { #closure };
    }
    quote! { #value }
}

fn is_str_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Lit(ExprLit {
            lit: Lit::Str(_),
            ..
        })
    )
}

fn is_bool_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Lit(ExprLit {
            lit: Lit::Bool(_),
            ..
        })
    )
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

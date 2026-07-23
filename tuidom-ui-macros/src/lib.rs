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
use syn::{Expr, ExprLit, Ident, Lit, LitStr, Pat, Token, braced, parenthesized};

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
///         if count.get() > 9 { text("!") },
///     }
/// };
/// ```
///
/// The macro emits absolute `::tuidom_ui::…` paths, so the crate must be reachable under that
/// name — which it is for downstream users, and in-crate via `extern crate self as tuidom_ui`.
///
/// The one dispatch rule is **literal-vs-expression**: a literal value is applied once and
/// statically, anything else binds reactively (a `dyn_*` setter or a `dyn_text` hole). Control
/// flow (`if` / `match` / `each`) and reactive view holes (`{ expr }`) each lower to a single
/// `dynamic` block, so a change to a condition or collection re-runs only that block and never
/// its siblings. `each i, value in collection { … }` rebuilds the whole list on change in F4;
/// keyed, identity-preserving reconciliation is F6, behind the same syntax.
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
    /// A reactive view hole: `{ expr }`, where `expr` is `Into<View>`.
    Hole(Expr),
    /// An `if` / `else if` / `else` chain. Boxed — a control-flow node dwarfs the others.
    If(Box<IfNode>),
    /// A `match` over a scrutinee.
    Match(Box<MatchNode>),
    /// An `each i, value in collection { … }` loop.
    Each(Box<EachNode>),
}

/// One `key: value` attribute inside an element's braces.
struct Attr {
    name: Ident,
    value: Expr,
}

/// An `if cond { … }` with an optional `else` tail.
struct IfNode {
    cond: Expr,
    then_branch: Vec<Node>,
    else_branch: Option<ElseBranch>,
}

/// The tail of an `if`: either a chained `else if` or a final `else` block.
enum ElseBranch {
    If(Box<IfNode>),
    Block(Vec<Node>),
}

/// A `match scrutinee { arm, … }`.
struct MatchNode {
    scrutinee: Expr,
    arms: Vec<MatchArm>,
}

/// One `pat [if guard] => body` arm. The body is a single child node — wrap several in a `box`
/// or a `{ … }` hole.
struct MatchArm {
    pat: Pat,
    guard: Option<Expr>,
    body: Node,
}

/// An `each i, value in collection [key = …] { … }` loop. `i` binds the enumerate index and
/// `value` the item; either may be `_`. `key` parses but is a no-op until F6's keyed
/// reconciliation, so it is not stored.
struct EachNode {
    index_pat: Pat,
    value_pat: Pat,
    collection: Expr,
    body: Vec<Node>,
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // A bare string literal is a static text child: `box { "hi" }`.
        if input.peek(LitStr) {
            return Ok(Node::Text(input.parse()?));
        }

        // A leading brace is a reactive view hole — `{ expr }` — lowered to a dynamic block.
        if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            return Ok(Node::Hole(content.parse()?));
        }

        if input.peek(Token![if]) {
            return Ok(Node::If(Box::new(parse_if(input)?)));
        }
        if input.peek(Token![match]) {
            return Ok(Node::Match(Box::new(parse_match(input)?)));
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
            "else" => Err(syn::Error::new(
                name.span(),
                "`else` here has no matching `if`",
            )),
            "each" => Ok(Node::Each(Box::new(parse_each(input)?))),
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

/// Parse `if cond { … } [else …]`. The condition is parsed without eager brace consumption, so
/// the block that follows is the body rather than a struct literal — matching Rust's own rule.
fn parse_if(input: ParseStream) -> syn::Result<IfNode> {
    input.parse::<Token![if]>()?;
    let cond = Expr::parse_without_eager_brace(input)?;
    let then_branch = parse_child_block(input)?;
    let else_branch = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        if input.peek(Token![if]) {
            Some(ElseBranch::If(Box::new(parse_if(input)?)))
        } else {
            Some(ElseBranch::Block(parse_child_block(input)?))
        }
    } else {
        None
    };
    Ok(IfNode {
        cond,
        then_branch,
        else_branch,
    })
}

/// Parse `match scrutinee { pat [if guard] => body, … }`. Commas between arms are optional, as
/// in Rust. Each arm body is a single child node.
fn parse_match(input: ParseStream) -> syn::Result<MatchNode> {
    input.parse::<Token![match]>()?;
    let scrutinee = Expr::parse_without_eager_brace(input)?;
    let arms_body;
    braced!(arms_body in input);
    let mut arms = Vec::new();
    while !arms_body.is_empty() {
        let pat = Pat::parse_multi_with_leading_vert(&arms_body)?;
        let guard = if arms_body.peek(Token![if]) {
            arms_body.parse::<Token![if]>()?;
            Some(arms_body.parse::<Expr>()?)
        } else {
            None
        };
        arms_body.parse::<Token![=>]>()?;
        let body = arms_body.parse::<Node>()?;
        arms.push(MatchArm { pat, guard, body });
        if arms_body.peek(Token![,]) {
            arms_body.parse::<Token![,]>()?;
        }
    }
    Ok(MatchNode { scrutinee, arms })
}

/// Parse `i, value in collection [key = expr] { … }`, positioned just after the `each`
/// identifier. Either pattern may be `_`. The `key` clause is parsed and dropped — it is a
/// documented no-op until F6 turns naive rebuild into keyed reconciliation.
fn parse_each(input: ParseStream) -> syn::Result<EachNode> {
    let index_pat = Pat::parse_single(input)?;
    input.parse::<Token![,]>()?;
    let value_pat = Pat::parse_single(input)?;
    input.parse::<Token![in]>()?;
    let collection = Expr::parse_without_eager_brace(input)?;

    // After the collection comes either the item block or a `key = …` clause. Anything else is
    // a mistake worth naming rather than letting `braced!` report a bare "expected `{`".
    if !input.peek(syn::token::Brace) {
        let kw = input.parse::<Ident>()?;
        if kw != "key" {
            return Err(syn::Error::new(
                kw.span(),
                "expected `key = …` or the item block `{ … }` after the `each` collection",
            ));
        }
        input.parse::<Token![=]>()?;
        // Dropped: keyed reconciliation is F6. Parsing it keeps the grammar stable so item
        // templates need not change when the key starts mattering.
        let _key = Expr::parse_without_eager_brace(input)?;
    }

    let body = parse_child_block(input)?;
    Ok(EachNode {
        index_pat,
        value_pat,
        collection,
        body,
    })
}

/// Parse a braced list of children — an `if`/`else` branch or an `each` item template.
/// Attributes have no element to attach to here, so they are rejected at the offending key.
fn parse_child_block(input: ParseStream) -> syn::Result<Vec<Node>> {
    let body;
    braced!(body in input);
    let (attrs, children) = parse_body(&body)?;
    if let Some(attr) = attrs.first() {
        return Err(syn::Error::new(
            attr.name.span(),
            "an attribute has no element to attach to inside a control-flow block",
        ));
    }
    Ok(children)
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
            Node::Hole(expr) => {
                quote! { ::tuidom_ui::dynamic(move || ::tuidom_ui::View::from(#expr)) }
            }
            Node::If(node) => {
                let body = expand_if(node);
                quote! { ::tuidom_ui::dynamic(move || #body) }
            }
            Node::Match(node) => {
                let body = expand_match(node);
                quote! { ::tuidom_ui::dynamic(move || #body) }
            }
            Node::Each(node) => {
                let index = &node.index_pat;
                let value = &node.value_pat;
                let collection = &node.collection;
                let item = child_list_to_view(&node.body);
                // Naive rebuild (F4): the whole list re-mounts when the collection changes. Item
                // templates keep their own reactive holes; identity-preserving reorder is F6.
                quote! {
                    ::tuidom_ui::dynamic(move || ::tuidom_ui::fragment(
                        ::std::iter::IntoIterator::into_iter(#collection)
                            .enumerate()
                            .map(move |(#index, #value)| #item)
                    ))
                }
            }
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

/// Lower an `if` chain to an `if`/`else` expression yielding a [`View`] on every path. A missing
/// `else` becomes an empty branch, so the expression always has a value — required since both
/// arms feed the enclosing `dynamic`.
fn expand_if(node: &IfNode) -> TokenStream2 {
    let cond = &node.cond;
    let then_view = child_list_to_view(&node.then_branch);
    let else_part = match &node.else_branch {
        None => {
            let empty = empty_view();
            quote! { { #empty } }
        }
        Some(ElseBranch::Block(children)) => {
            let view = child_list_to_view(children);
            quote! { { #view } }
        }
        Some(ElseBranch::If(inner)) => expand_if(inner),
    };
    quote! { if #cond { #then_view } else #else_part }
}

/// Lower a `match` to a `match` expression whose every arm yields a [`View`].
fn expand_match(node: &MatchNode) -> TokenStream2 {
    let scrutinee = &node.scrutinee;
    let arms = node.arms.iter().map(|arm| {
        let pat = &arm.pat;
        let guard = arm.guard.as_ref().map(|g| quote! { if #g });
        let body = arm.body.expand();
        quote! { #pat #guard => ::tuidom_ui::View::from(#body), }
    });
    quote! { match #scrutinee { #(#arms)* } }
}

/// Collapse a child list into a single [`View`]. Zero children is an empty fragment; otherwise a
/// fragment over the children, each coerced through `View::from`. A fragment adds no wrapper
/// node, so this costs no layout.
fn child_list_to_view(children: &[Node]) -> TokenStream2 {
    if children.is_empty() {
        empty_view()
    } else {
        let views = children.iter().map(|child| {
            let expr = child.expand();
            quote! { ::tuidom_ui::View::from(#expr) }
        });
        quote! { ::tuidom_ui::fragment([ #(#views),* ]) }
    }
}

/// An empty fragment — a branch that renders nothing.
fn empty_view() -> TokenStream2 {
    quote! { ::tuidom_ui::fragment(::std::vec::Vec::<::tuidom_ui::View>::new()) }
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

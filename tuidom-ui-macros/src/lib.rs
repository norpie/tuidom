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
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Expr, ExprLit, FnArg, Ident, ItemFn, Lit, LitStr, Pat, PatType, Token, Type,
    Visibility, braced, parenthesized,
};

/// Build a [`View`] from a Rust-like template.
///
/// The canonical counter — a focusable box, a key handler that mutates a signal, and a reactive
/// text hole reflecting it:
///
/// ```ignore
/// use tuidom_ui::view;
/// use tuidom::event::KeyCode;
///
/// let count = state(0i64);
/// let ui = view! {
///     box {
///         focusable: true,
///         on_key_press: move |ev| match ev.code {
///             KeyCode::Char('+') => count.update(|n| *n += 1),
///             KeyCode::Char('-') => count.update(|n| *n -= 1),
///             _ => {}
///         },
///
///         text("count: "),
///         text(count.get()),
///     }
/// };
/// ```
///
/// # Grammar
///
/// - **Elements** — `box { … }` and `input { … }`. Braces hold a comma-separated mix of
///   attributes and children; a nested element or `text(..)` is a child.
/// - **Text** — `text("literal")` is a static node; `text(expr)` is a reactive hole updated in
///   place (`expr` is stringified through `Display`). A bare `"literal"` child is static text.
/// - **Attributes** — `key: value`. `style:` binds the whole `Style`; `focusable:` takes a
///   bool; `on_click:` / `on_key_press:` take a `|ev| …` or `|| …` closure; any other key is a
///   string attribute. A literal value is applied once; anything else binds reactively.
/// - **View hole** — `{ expr }`, where `expr` is `Into<View>` (splat an iterable with
///   `{ fragment(iter) }`).
/// - **Control flow** — `if` / `else if` / `else`, `match scrutinee { pat => body, … }`, and
///   `each i, value in collection [key = …] { … }` (`_` for an unwanted binding).
///
/// # How it lowers
///
/// The macro emits absolute `::tuidom_ui::…` paths, so the crate must be reachable under that
/// name — which it is for downstream users, and in-crate via `extern crate self as tuidom_ui`.
///
/// The one dispatch rule is **literal-vs-expression**: a literal value is applied once and
/// statically, anything else binds reactively (a `dyn_*` setter or a `dyn_text` hole). Control
/// flow and reactive view holes each lower to a single `dynamic` block, so a change to a
/// condition or collection re-runs only that block and never its siblings. `each` rebuilds the
/// whole list on change in F4; keyed, identity-preserving reconciliation is F6, behind the same
/// syntax, and `key` already parses today as a no-op.
///
/// [`View`]: ../tuidom_ui/struct.View.html
#[proc_macro]
pub fn view(input: TokenStream) -> TokenStream {
    let node = syn::parse_macro_input!(input as Node);
    let expr = node.expand();
    quote! { ::tuidom_ui::View::from(#expr) }.into()
}

/// Turn a function into a reusable component.
///
/// A component is a `fn Name(props…) -> View` whose body builds a subtree. `#[component]`
/// rewrites it into a props struct plus a constructor the [`view!`] macro instantiates when it
/// sees the component by name — PascalCase dispatches to a component, lowercase (`box`, `input`,
/// `text`) stays an engine element.
///
/// ```ignore
/// # use tuidom_ui::{component, view, state};
/// #[component]
/// fn Counter(start: i32) -> View {
///     // Each parameter arrives as a `Signal`: `start` is `Signal<i32>` inside the body.
///     let count = state(start.get());
///     view! {
///         box {
///             focusable: true,
///             text(count.get()),
///         }
///     }
/// }
///
/// // Instantiated in another template by name:
/// let ui = view! { Counter { start: 0 } };
/// ```
///
/// # Props are signals
///
/// Every parameter surfaces inside the body as a [`Signal`] of its declared type, not the plain
/// value. This is deliberate: a later milestone keeps a component's layout mounted across prop
/// changes rather than re-running its body, so a prop holding a plain value would render the one
/// it was first given forever. As a signal, a prop change reruns only the bindings that read it.
///
/// [`view!`]: macro.view.html
/// [`Signal`]: ../tuidom_ui/struct.Signal.html
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = syn::parse_macro_input!(item as ItemFn);
    match expand_component(func) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// One component parameter, classified for props-struct and builder generation.
struct PropSpec {
    /// The parameter name — the prop's field name and builder setter name.
    name: Ident,
    /// The declared value type `T`; the field is `Signal<T>`.
    ty: Type,
    /// `None` for a required prop; `Some(expr)` for an optional one, holding the default *value*
    /// expression (of type `T`) used when the call site omits it.
    default: Option<TokenStream2>,
}

/// Rewrite a component function into its props struct, a typestate builder, and a constructor
/// taking the props.
///
/// Each parameter `name: T` becomes a `Signal<T>` field on `NameProps` and a binding of the same
/// name destructured at the top of the body, so the body reads props as signals. The body is
/// otherwise untouched — a pure source transform, no reactivity analysis. Optionality comes from
/// `#[prop(default)]`, `#[prop(default = expr)]`, or an `Option<T>` type; everything else is a
/// required prop the call site must supply.
fn expand_component(func: ItemFn) -> syn::Result<TokenStream2> {
    let sig = &func.sig;
    if let Some(param) = sig.generics.params.first() {
        return Err(syn::Error::new_spanned(
            param,
            "generic components are not supported yet",
        ));
    }

    let attrs = &func.attrs;
    let vis = &func.vis;
    let name = &sig.ident;
    let output = &sig.output;
    let block = &func.block;
    let props_name = format_ident!("{}Props", name);

    let mut props = Vec::new();
    for input in &sig.inputs {
        let PatType {
            attrs: param_attrs,
            pat,
            ty,
            ..
        } = match input {
            FnArg::Typed(typed) => typed,
            FnArg::Receiver(recv) => {
                return Err(syn::Error::new_spanned(
                    recv,
                    "a component cannot take `self`",
                ));
            }
        };
        let Pat::Ident(pat_ident) = pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                pat,
                "a component parameter must be a plain identifier",
            ));
        };
        let default = parse_prop_default(param_attrs, ty)?;
        props.push(PropSpec {
            name: pat_ident.ident.clone(),
            ty: (**ty).clone(),
            default,
        });
    }

    let props_api = expand_props_builder(vis, &props_name, &props);
    let binds = props.iter().map(|p| &p.name);

    Ok(quote! {
        #props_api

        #(#attrs)*
        #[allow(non_snake_case)]
        #vis fn #name(props: #props_name) #output {
            let #props_name { #(#binds),* } = props;
            #block
        }
    })
}

/// Classify one parameter's optionality from its `#[prop(...)]` attribute and type.
///
/// `#[prop(default)]` defaults to `Default::default()`, `#[prop(default = expr)]` to `expr`, and a
/// bare `Option<T>` parameter to `None`. Anything else is required (`Ok(None)`).
fn parse_prop_default(attrs: &[Attribute], ty: &Type) -> syn::Result<Option<TokenStream2>> {
    for attr in attrs {
        if !attr.path().is_ident("prop") {
            continue;
        }
        let mut default = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                if meta.input.peek(Token![=]) {
                    let expr: Expr = meta.value()?.parse()?;
                    default = Some(quote! { #expr });
                } else {
                    default = Some(quote! { ::core::default::Default::default() });
                }
                Ok(())
            } else {
                Err(meta.error("unknown `prop` option; expected `default` or `default = <expr>`"))
            }
        })?;
        return Ok(default);
    }
    if is_option_type(ty) {
        return Ok(Some(quote! { ::core::option::Option::None }));
    }
    Ok(None)
}

/// Whether a type is spelled `Option<..>` (by its last path segment). This is the "optional prop,
/// defaults to `None`" test — a conservative syntactic check, not a resolve.
fn is_option_type(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

/// Generate `NameProps`, its typestate builder, and the constructor `NameProps::builder()`.
///
/// The builder carries one type parameter per **required** prop, each either an unset marker
/// (`NamePropsMissing_field`) or set (`(Signal<T>,)`). A required setter transitions its own
/// parameter from unset to set while leaving the others generic, so props may be supplied in any
/// order; `build` exists only when every required parameter is set — so omitting one is a compile
/// error naming the field (through the marker type), never a runtime panic. Optional props are
/// stored as `Option` and filled with their default at `build`.
fn expand_props_builder(vis: &Visibility, props_name: &Ident, props: &[PropSpec]) -> TokenStream2 {
    let builder_name = format_ident!("{}Builder", props_name);

    let struct_fields = props.iter().map(|p| {
        let n = &p.name;
        let ty = &p.ty;
        quote! { #n: ::tuidom_ui::Signal<#ty> }
    });

    let required: Vec<&PropSpec> = props.iter().filter(|p| p.default.is_none()).collect();
    let optional: Vec<&PropSpec> = props.iter().filter(|p| p.default.is_some()).collect();

    // Per-required-field: a generic param, an unset marker type, and the set marker `(Signal<T>,)`.
    let gens: Vec<Ident> = (0..required.len())
        .map(|i| format_ident!("__M{i}"))
        .collect();
    let missing: Vec<Ident> = required
        .iter()
        .map(|p| format_ident!("{}Missing_{}", props_name, p.name))
        .collect();
    let set_markers: Vec<TokenStream2> = required
        .iter()
        .map(|p| {
            let ty = &p.ty;
            quote! { (::tuidom_ui::Signal<#ty>,) }
        })
        .collect();

    // Wrap a list of generic arguments in `<...>`, or nothing when there are no required props.
    let angle = |items: &[TokenStream2]| -> TokenStream2 {
        if items.is_empty() {
            quote! {}
        } else {
            quote! { <#(#items),*> }
        }
    };
    let gen_args: Vec<TokenStream2> = gens.iter().map(|g| quote! { #g }).collect();
    let missing_args: Vec<TokenStream2> = missing.iter().map(|m| quote! { #m }).collect();
    let impl_generics = angle(&gen_args);
    let builder_generic = angle(&gen_args);
    let builder_missing = angle(&missing_args);
    let builder_set = angle(&set_markers);

    let req_fields = required.iter().zip(&gens).map(|(p, g)| {
        let n = &p.name;
        quote! { #n: #g }
    });
    let opt_fields = optional.iter().map(|p| {
        let n = &p.name;
        let ty = &p.ty;
        quote! { #n: ::core::option::Option<::tuidom_ui::Signal<#ty>> }
    });

    let req_init = required.iter().zip(&missing).map(|(p, m)| {
        let n = &p.name;
        quote! { #n: #m }
    });
    let opt_init = optional.iter().map(|p| {
        let n = &p.name;
        quote! { #n: ::core::option::Option::None }
    });

    let opt_setters = optional.iter().map(|p| {
        let n = &p.name;
        let ty = &p.ty;
        quote! {
            #vis fn #n(mut self, value: ::tuidom_ui::Signal<#ty>) -> Self {
                self.#n = ::core::option::Option::Some(value);
                self
            }
        }
    });

    // One impl per required field, transitioning only that field's marker while the rest stay
    // generic — this is what lets the call site set required props in any order.
    let req_setters = (0..required.len()).map(|j| {
        let p = required[j];
        let n = &p.name;
        let ty = &p.ty;

        let impl_gen = angle(
            &gens
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != j)
                .map(|(_, g)| quote! { #g })
                .collect::<Vec<_>>(),
        );
        let self_args: Vec<TokenStream2> = (0..required.len())
            .map(|i| {
                if i == j {
                    let m = &missing[j];
                    quote! { #m }
                } else {
                    let g = &gens[i];
                    quote! { #g }
                }
            })
            .collect();
        let ret_args: Vec<TokenStream2> = (0..required.len())
            .map(|i| {
                if i == j {
                    set_markers[j].clone()
                } else {
                    let g = &gens[i];
                    quote! { #g }
                }
            })
            .collect();
        let self_ty = angle(&self_args);
        let ret_ty = angle(&ret_args);

        let carry_req = required
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != j)
            .map(|(_, q)| {
                let qn = &q.name;
                quote! { #qn: self.#qn }
            });
        let carry_opt = optional.iter().map(|q| {
            let qn = &q.name;
            quote! { #qn: self.#qn }
        });

        quote! {
            impl #impl_gen #builder_name #self_ty {
                #vis fn #n(self, value: ::tuidom_ui::Signal<#ty>) -> #builder_name #ret_ty {
                    #builder_name {
                        #n: (value,),
                        #(#carry_req,)*
                        #(#carry_opt,)*
                    }
                }
            }
        }
    });

    let build_req = required.iter().map(|p| {
        let n = &p.name;
        quote! { #n: self.#n.0 }
    });
    let build_opt = optional.iter().map(|p| {
        let n = &p.name;
        let default = p.default.as_ref().expect("optional prop has a default");
        quote! { #n: self.#n.unwrap_or_else(|| ::tuidom_ui::state(#default)) }
    });

    let marker_defs = missing.iter().map(|m| {
        quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            #vis struct #m;
        }
    });

    quote! {
        #vis struct #props_name {
            #(#struct_fields),*
        }

        #(#marker_defs)*

        #vis struct #builder_name #impl_generics {
            #(#req_fields,)*
            #(#opt_fields,)*
        }

        impl #props_name {
            #vis fn builder() -> #builder_name #builder_missing {
                #builder_name {
                    #(#req_init,)*
                    #(#opt_init,)*
                }
            }
        }

        impl #impl_generics #builder_name #builder_generic {
            #(#opt_setters)*
        }

        #(#req_setters)*

        impl #builder_name #builder_set {
            #vis fn build(self) -> #props_name {
                #props_name {
                    #(#build_req,)*
                    #(#build_opt,)*
                }
            }
        }
    }
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
    /// A component instance, e.g. `Counter { start: 0 }`. Dispatched from an element by the
    /// name's case — PascalCase is a component. `props` reuse the `name: value` attribute
    /// grammar; each becomes a field on the generated `NameProps` struct.
    Component { name: Ident, props: Vec<Attr> },
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
            // PascalCase is a component instance; lowercase is an engine element. The case split
            // is the Rust convention and needs no new sigil to tell `Counter {…}` from `box {…}`.
            _ if is_pascal_case(&name) => {
                let body;
                braced!(body in input);
                // Component props are not engine events, so `on_*` names are not validated here —
                // a callback prop like `on_press` is legitimate (F5 step 5).
                let (props, children) = parse_body(&body, false)?;
                if !children.is_empty() {
                    return Err(syn::Error::new(
                        name.span(),
                        format!(
                            "component `{name}` cannot take children yet; the `children` prop lands in F5 step 4"
                        ),
                    ));
                }
                Ok(Node::Component { name, props })
            }
            _ => {
                let body;
                braced!(body in input);
                let (attrs, children) = parse_body(&body, true)?;
                Ok(Node::Element {
                    name,
                    attrs,
                    children,
                })
            }
        }
    }
}

/// Whether a name is PascalCase — its first character an ASCII uppercase letter. This is how
/// `view!` tells a component instance from an engine element.
fn is_pascal_case(name: &Ident) -> bool {
    name.to_string()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
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
    let (attrs, children) = parse_body(&body, true)?;
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
fn parse_body(body: ParseStream, validate_events: bool) -> syn::Result<(Vec<Attr>, Vec<Node>)> {
    let mut attrs = Vec::new();
    let mut children = Vec::new();
    while !body.is_empty() {
        if body.peek2(Token![:]) {
            let name = body.call(Ident::parse_any)?;
            body.parse::<Token![:]>()?;
            let value: Expr = body.parse()?;
            // A misspelled event is a closure handed to a string-attribute setter otherwise —
            // a confusing type error deep in expanded code. Catch it at the user's token. Only
            // for elements: a component's `on_*` is a callback prop, not an engine event.
            let key = name.to_string();
            if validate_events && key.starts_with("on_") && !is_known_event(&key) {
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
            Node::Component { name, props } => {
                let props_name = format_ident!("{}Props", name);
                // Each supplied prop becomes a builder setter call; `build` fills omitted optional
                // props and refuses (at compile time) a missing required one. An unknown prop is a
                // call to a setter that does not exist — a compile error naming it at the call site.
                let setters = props.iter().map(|attr| {
                    let field = &attr.name;
                    let init = prop_init(attr);
                    quote! { .#field(#init) }
                });
                // `component` defers the body to mount time under the component's own scope; the
                // builder — every prop signal and any syncing effect — runs there too, so it all
                // belongs to that scope and is freed when the component unmounts.
                quote! {
                    ::tuidom_ui::component(move || ::tuidom_ui::View::from(
                        #name(#props_name::builder() #(#setters)* .build())
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

/// Lower one component prop's value to the `Signal<T>` a builder setter receives, splitting on
/// the same literal-vs-expression rule `view!` uses everywhere.
///
/// A **literal** seeds a plain signal once (`state(value)`) — it never changes, so it needs no
/// effect. **Anything else** is a *reactive* prop: it may read a parent signal, so the prop stays
/// synced to the expression through an effect. The expression is captured in a single re-runnable
/// closure, evaluated once to seed the signal and again on every dependency change — so it appears
/// once in the source (no double-move) and picks up parent-signal changes without re-running the
/// component body. A reactive prop's type must be `PartialEq` (for the equality-skipping `set`);
/// a literal prop escapes that, since it is never set.
fn prop_init(attr: &Attr) -> TokenStream2 {
    let value = &attr.value;
    if is_literal(value) {
        quote! { ::tuidom_ui::state(#value) }
    } else {
        quote! {{
            let __prop_sync = move || #value;
            let __prop = ::tuidom_ui::state(__prop_sync());
            ::tuidom_ui::effect(move || __prop.set(__prop_sync()));
            __prop
        }}
    }
}

/// Whether an expression is a literal — an integer, float, string, bool, or char literal. This
/// is the static-prop test; a negative literal like `-1` is a unary expression, not a literal, so
/// it takes the (harmless, run-once) reactive path.
fn is_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(_))
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

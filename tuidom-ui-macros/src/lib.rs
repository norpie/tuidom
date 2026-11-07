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
/// - **Attributes** — `key: value`, dispatched by key:
///   - `style:`, `focus_style:`, `active_style:`, `disabled_style:` — a whole `Style`, always
///     bound reactively.
///   - `focusable:`, `disabled:`, and on `input` `multiline:` — a bool.
///   - An event key (`on_click:`, `on_focus:`, `on_input:`, … — one per `Element` listener
///     method) — a `|ev| …` or `|| …` closure.
///   - On `input` only: `value:` (stringified through `ToString`), `bind_value:` (a
///     `Signal<String>`, bound both ways), and `mask:` (an `Option<char>`). These and
///     `multiline:` are compile errors on `box`.
///   - `ref:` — a `NodeRef`, filled with the element's node while it is mounted.
///   - `transitions:` — an iterable of `TransitionConfig`, evaluated once when the element is
///     built. A signal read there does not update the transitions, and inside an `if` or
///     `match` it would rebuild the branch.
///   - Any other key — a string attribute, stringified through `ToString`. A misspelled typed
///     key lands here too, silently; only `on_*` names are checked.
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
/// Values dispatch on **literal-vs-expression**: a literal is applied once and statically,
/// anything else binds reactively (a `dyn_*` setter or a `dyn_text` hole). The exceptions follow
/// the key's type: a `Style` is never a literal, so the style keys always bind reactively; events
/// take their closure as the handler; `bind_value:` and `ref:` pass their handle through; and
/// `transitions:` is applied once whatever it is. An `if`,
/// `match`, or view hole lowers to a single `dynamic` block, so a change to a condition re-runs
/// only that block and never its siblings. An `each` lowers to a keyed differ: a `key = …` clause
/// preserves item identity (and thus scroll/focus/transition state) across reorders, while an
/// unkeyed `each` keys by position — reusing nodes on append, but showing stale content if a value
/// at a stable index changes without a key. An item body is built once per key and updates only
/// through the signals it reads.
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
/// At the call site `view!` decides how each prop value flows in by its **syntax**: a literal
/// (`start: 0`) seeds the signal once; any other expression (`start: base.get()`) stays synced to
/// a parent signal through an effect owned by the caller; and a **closure** is a callback prop
/// (below), never a signal.
///
/// # Optional props
///
/// A parameter is required unless it opts out. `#[prop(default)]` falls back to
/// `Default::default()` when the call site omits it, `#[prop(default = expr)]` to `expr`, and a
/// bare `Option<T>` parameter defaults to `None`. Omitting a *required* prop is a compile error
/// naming the field — the generated builder simply has no `build` until every required prop is
/// set.
///
/// # Children
///
/// A `children: Children` parameter receives the instantiation's body nodes as a deferred thunk,
/// which the body places with `{ children }`. The container node keeps its identity — scroll,
/// focus, running transitions — while the children swap beneath it. A component with no
/// `children` parameter rejects body nodes at compile time.
///
/// ```ignore
/// #[component]
/// fn Panel(title: String, children: Children) -> View {
///     view! { box { text(title.get()), { children } } }
/// }
/// // view! { Panel { title: heading.get(), Counter { start: 0 } } }
/// ```
///
/// # Callback props
///
/// A [`Callback<A>`] parameter is how a child talks back to its parent: the parent passes a
/// closure, the child invokes `on_x.call(arg)`. It is stored raw (not a signal), and `view!`
/// lowers a closure-valued prop to it automatically. A callback is required unless
/// `#[prop(default)]` gives it the no-op default.
///
/// # Context and lifecycle
///
/// A component body runs inside its own owner scope, so the ambient runtime functions work with
/// nothing threaded: [`provide_context`]/[`use_context`] pass a value down a subtree without
/// prop-drilling, and [`on_mount`]/[`on_cleanup`] tie a side effect to the component's mount and
/// unmount. Context resolves up the scope chain from where content is mounted, so it reaches
/// content inside `if`, `match`, and `each`, and `{ children }` content sees the context of the
/// component that places it.
///
/// [`view!`]: macro.view.html
/// [`Signal`]: ../tuidom_ui/struct.Signal.html
/// [`Callback<A>`]: ../tuidom_ui/struct.Callback.html
/// [`provide_context`]: ../tuidom_ui/fn.provide_context.html
/// [`use_context`]: ../tuidom_ui/fn.use_context.html
/// [`on_mount`]: ../tuidom_ui/fn.on_mount.html
/// [`on_cleanup`]: ../tuidom_ui/fn.on_cleanup.html
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
    /// The full field type the prop is stored and set as: `Signal<T>` for an ordinary prop, or the
    /// raw declared type for a special prop like `children: Children`.
    field_ty: TokenStream2,
    /// `None` for a required prop; `Some(value)` for an optional one, holding the full default of
    /// type `field_ty` used when the call site omits it (already `state`-wrapped for signal props).
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
    let mut saw_children = false;
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
        let is_children = pat_ident.ident == "children";
        if is_children {
            // Two `children` parameters would emit a struct with a duplicate field — an error
            // pointing into generated code. Name it at the offending parameter instead.
            if saw_children {
                return Err(syn::Error::new_spanned(
                    pat_ident,
                    "a component may declare only one `children` parameter",
                ));
            }
            saw_children = true;
        }
        // Two parameter kinds are stored raw as their declared type rather than signal-wrapped,
        // because they carry structure or behavior, not a reactive value:
        //   - `children` — the deferred-child slot, always optional (empty when none is given).
        //   - a `Callback<A>` prop — a handler; required unless `#[prop(default)]` opts into the
        //     no-op default. Detected syntactically by the type's last path segment (like
        //     `Option`), so a type aliased to `Callback` would slip through — accepted.
        // Every other parameter is a signal prop, wrapped so the body reads it as `Signal<T>`.
        let (field_ty, default) = if is_children {
            (
                quote! { #ty },
                Some(quote! { ::core::default::Default::default() }),
            )
        } else if is_callback_type(ty) {
            (quote! { #ty }, parse_prop_default(param_attrs, ty)?)
        } else {
            let value_default = parse_prop_default(param_attrs, ty)?;
            (
                quote! { ::tuidom_ui::Signal<#ty> },
                value_default.map(|v| quote! { ::tuidom_ui::state(#v) }),
            )
        };
        props.push(PropSpec {
            name: pat_ident.ident.clone(),
            field_ty,
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

/// Whether a type is spelled `Callback<..>` (by its last path segment). A callback prop is stored
/// raw rather than signal-wrapped; the check mirrors [`is_option_type`] — syntactic, so an alias
/// of `Callback` would not be recognized.
fn is_callback_type(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "Callback"))
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
        let ty = &p.field_ty;
        quote! { #n: #ty }
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
            let ty = &p.field_ty;
            quote! { (#ty,) }
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
        let ty = &p.field_ty;
        quote! { #n: ::core::option::Option<#ty> }
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
        let ty = &p.field_ty;
        quote! {
            #vis fn #n(mut self, value: #ty) -> Self {
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
        let ty = &p.field_ty;

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
                #vis fn #n(self, value: #ty) -> #builder_name #ret_ty {
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
        quote! { #n: self.#n.unwrap_or_else(|| #default) }
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
    /// grammar; each becomes a field on the generated `NameProps` struct. `children` are the body
    /// nodes, passed to the component's `children: Children` prop as a deferred thunk.
    Component {
        name: Ident,
        props: Vec<Attr>,
        children: Vec<Node>,
    },
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
/// `value` the item; either may be `_`. A `key = expr` clause makes the list keyed — items keep
/// their node identity across reorders; without it the enumerate index is the key, which reuses
/// nodes by position (correct for appends, stale on an unkeyed reorder).
struct EachNode {
    index_pat: Pat,
    value_pat: Pat,
    collection: Expr,
    key: Option<Expr>,
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
                // Children are passed as body nodes, which the lowering hands to the `children`
                // prop. A `children:` attribute would be the same thing spelled two ways, so reject
                // it rather than let both paths fight over the one slot.
                if let Some(attr) = props.iter().find(|a| a.name == "children") {
                    return Err(syn::Error::new(
                        attr.name.span(),
                        "pass children as body nodes (`Comp { child, … }`), not a `children:` attribute",
                    ));
                }
                Ok(Node::Component {
                    name,
                    props,
                    children,
                })
            }
            _ => {
                let body;
                braced!(body in input);
                let (attrs, children) = parse_body(&body, true)?;
                if name != "input"
                    && let Some(attr) = attrs.iter().find(|a| is_input_only(&a.name.to_string()))
                {
                    return Err(syn::Error::new(
                        attr.name.span(),
                        format!("`{}` applies only to `input`, not `{name}`", attr.name),
                    ));
                }
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
/// identifier. Either pattern may be `_`. A `key = expr` clause, when present, is what makes the
/// list keyed; its expression may read the index and value bindings.
fn parse_each(input: ParseStream) -> syn::Result<EachNode> {
    let index_pat = Pat::parse_single(input)?;
    input.parse::<Token![,]>()?;
    let value_pat = Pat::parse_single(input)?;
    input.parse::<Token![in]>()?;
    let collection = Expr::parse_without_eager_brace(input)?;

    // After the collection comes either the item block or a `key = …` clause. Anything else is
    // a mistake worth naming rather than letting `braced!` report a bare "expected `{`".
    let mut key = None;
    if !input.peek(syn::token::Brace) {
        let kw = input.parse::<Ident>()?;
        if kw != "key" {
            return Err(syn::Error::new(
                kw.span(),
                "expected `key = …` or the item block `{ … }` after the `each` collection",
            ));
        }
        input.parse::<Token![=]>()?;
        key = Some(Expr::parse_without_eager_brace(input)?);
    }

    let body = parse_child_block(input)?;
    Ok(EachNode {
        index_pat,
        value_pat,
        collection,
        key,
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
                        "unknown event handler `{key}`; elements take {}",
                        KNOWN_EVENTS
                            .iter()
                            .map(|event| format!("`{event}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
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

/// The `on_*` listener methods `Element` has. The view layer generates them from its own table,
/// which this crate cannot import, so the two lists are kept in step by hand and a `view!` test
/// that uses every name.
const KNOWN_EVENTS: &[&str] = &[
    "on_click",
    "on_key_press",
    "on_mouse_down",
    "on_mouse_up",
    "on_focus",
    "on_blur",
    "on_wheel",
    "on_scroll",
    "on_input",
    "on_transition_end",
    "on_animation_end",
    "on_animation_iteration",
];

fn is_known_event(name: &str) -> bool {
    KNOWN_EVENTS.contains(&name)
}

/// Keys that set a property only an `Input` node has.
fn is_input_only(name: &str) -> bool {
    matches!(name, "value" | "bind_value" | "multiline" | "mask")
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
                // Both forms lower to the same `keyed` differ, which reuses the node a surviving
                // key already owns instead of rebuilding. A `key = …` clause supplies the key and
                // the list preserves item identity across reorders; without one the enumerate index
                // is the key, reusing nodes by position — cheap and correct for appends, but stale
                // if items reorder or a value at a stable index changes without a key. Either way
                // an item body is built once per key and updates only through the signals it reads.
                match &node.key {
                    Some(key) => quote! {
                        ::tuidom_ui::keyed(move || {
                            ::std::iter::IntoIterator::into_iter(#collection)
                                .enumerate()
                                .map(move |(#index, #value)| {
                                    // The key is computed before `value` moves into the item body,
                                    // so `key = value.id` borrows it and the body still owns it.
                                    let __key = #key;
                                    (__key, move || #item)
                                })
                        })
                    },
                    None => quote! {
                        ::tuidom_ui::keyed(move || {
                            ::std::iter::IntoIterator::into_iter(#collection)
                                .enumerate()
                                .map(move |(__index, #value)| {
                                    // Key by position. The index is rebound inside the body so the
                                    // user's `i` binding is in scope even though the key owns it too
                                    // (a `usize` is `Copy`).
                                    (__index, move || {
                                        let #index = __index;
                                        #item
                                    })
                                })
                        })
                    },
                }
            }
            Node::Component {
                name,
                props,
                children,
            } => {
                // Span the generated props type at the component's name, so a PascalCase name that
                // is not a component — `Foo {}` with no `Foo` in scope — reports "cannot find type
                // `FooProps`" pointed at the user's `Foo`, not the whole macro invocation.
                let props_name = format_ident!("{}Props", name, span = name.span());
                // Each supplied prop becomes a builder setter call; `build` fills omitted optional
                // props and refuses (at compile time) a missing required one. An unknown prop is a
                // call to a setter that does not exist — a compile error naming it at the call site.
                let setters = props.iter().map(|attr| {
                    let field = &attr.name;
                    let init = prop_init(attr);
                    quote! { .#field(#init) }
                });
                // Body nodes feed the `children` prop as a deferred, re-runnable thunk. Absent
                // children leave the setter uncalled, so the component's `children` default (empty)
                // stands; children given to a component with no `children` prop are a `.children`
                // call to a setter that does not exist. Spanned at the component name so that "no
                // method named `children`" points at `Foo`, reading as "Foo takes no children".
                let children_setter = if children.is_empty() {
                    quote! {}
                } else {
                    let body = child_list_to_view(children);
                    quote_spanned! { name.span() =>
                        .children(::tuidom_ui::Children::new(move || #body))
                    }
                };
                // Span `build` at the component name too: a missing required prop makes `build`
                // resolve on an unfinished builder type whose name encodes the absent field
                // (`FooPropsMissing_bar`), and that error should land on `Foo`.
                let build = quote_spanned! { name.span() => .build() };
                // `component` defers the body to mount time under the component's own scope; the
                // builder — every prop signal and any syncing effect — runs there too, so it all
                // belongs to that scope and is freed when the component unmounts.
                quote! {
                    ::tuidom_ui::component(move || ::tuidom_ui::View::from(
                        #name(#props_name::builder() #(#setters)* #children_setter #build)
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

/// Lower one component prop's value to what its builder setter receives, splitting on syntax the
/// same way `view!` dispatches everywhere.
///
/// - A **closure** is a *callback* prop: the parent hands the child behavior, not a reactive
///   value, so it lowers to a raw [`Callback`] — never a signal, never a syncing effect. A
///   zero-argument closure is widened to ignore the callback's argument, exactly as an event
///   handler's `||` normalizes to `|_|`. (A closure is never a valid *data* prop, so this dispatch
///   never steals one — a `Signal<T>` prop's `T` could not be a closure.)
/// - A **literal** seeds a plain signal once (`state(value)`) — it never changes, so it needs no
///   effect.
/// - **Anything else** is a *reactive* prop: it may read a parent signal, so the prop stays synced
///   to the expression through an effect. The expression is captured in a single re-runnable
///   closure, evaluated once to seed the signal and again on every dependency change — so it
///   appears once in the source (no double-move) and picks up parent-signal changes without
///   re-running the component body. A reactive prop's type must be `PartialEq` (for the
///   equality-skipping `set`); a literal prop escapes that, since it is never set.
///
/// [`Callback`]: ../tuidom_ui/struct.Callback.html
fn prop_init(attr: &Attr) -> TokenStream2 {
    let value = &attr.value;
    if let Expr::Closure(_) = value {
        let handler = event_handler(value);
        quote! { ::tuidom_ui::Callback::new(#handler) }
    } else if is_literal(value) {
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
            "value" => {
                if is_str_literal(value) {
                    quote! { .value(#value) }
                } else {
                    quote! { .dyn_value(move || ::std::string::ToString::to_string(&(#value))) }
                }
            }
            "bind_value" => quote! { .bind_value(#value) },
            "ref" => quote! { .node_ref(#value) },
            // Evaluated once as the element is built, so a signal read here is not tracked by the
            // element. Inside an `if`/`match`, it would subscribe the enclosing dynamic block.
            "transitions" => quote! { .transitions(#value) },
            "mask" => quote! { .dyn_mask(move || #value) },
            "focusable" | "disabled" | "multiline" => {
                if is_bool_literal(value) {
                    quote! { .#name(#value) }
                } else {
                    let setter = format_ident!("dyn_{}", key, span = name.span());
                    quote! { .#setter(move || #value) }
                }
            }
            "focus_style" | "active_style" | "disabled_style" => {
                let setter = format_ident!("dyn_{}", key, span = name.span());
                quote! { .#setter(move || #value) }
            }
            event if is_known_event(event) => {
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

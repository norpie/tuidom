//! `style!` — parsing and expansion.
//!
//! The expansion is deliberately dull: a `Style::new()`, a setter call per entry, and the
//! value back out. Nothing here is a second source of truth about what a style is, so
//! anything the engine does with a hand-built `Style` it also does with this one.

mod props;
mod value;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Ident, LitInt, LitStr, Token};

use props::{Prop, lookup};

/// The whole macro body: an optional base to start from, then entries.
pub(crate) struct StyleInput {
    base: Option<Expr>,
    entries: Vec<Entry>,
}

enum Entry {
    /// A style property, looked up in the property table.
    Prop {
        prop: &'static Prop,
        /// The user's own ident, so setter calls carry the user's span and a type error
        /// points at the property they wrote.
        span: Span,
        value: Value,
    },
    /// `--name: <color>` — declares a color variable into the style's scope.
    ColorVar { name: String, value: TokenStream },
    /// `"name": <expr>` — a raw custom property.
    Custom { name: LitStr, value: TokenStream },
}

enum Value {
    Inherit,
    Unset,
    /// Tokens awaiting the type-driven value parser.
    Tokens(TokenStream),
}

impl Parse for StyleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut base = None;
        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;
            base = Some(input.parse::<Expr>()?);
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let mut entries = Vec::new();
        while !input.is_empty() {
            if input.peek(Token![..]) {
                let span = input.parse::<Token![..]>()?.span();
                return Err(syn::Error::new(
                    span,
                    "`..base` must come first, since later entries override it",
                ));
            }
            entries.push(parse_entry(input)?);
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Self { base, entries })
    }
}

fn parse_entry(input: ParseStream) -> syn::Result<Entry> {
    if input.peek(LitStr) {
        let name: LitStr = input.parse()?;
        input.parse::<Token![:]>()?;
        let value = value_tokens(input)?;
        return Ok(Entry::Custom { name, value });
    }

    if input.peek(Token![-]) {
        let name = parse_var_name(input)?;
        input.parse::<Token![:]>()?;
        let value = value_tokens(input)?;
        return Ok(Entry::ColorVar { name, value });
    }

    let ident: Ident = input.parse()?;
    let name = ident.to_string();
    let Some(prop) = lookup(&name) else {
        let message = match props::nearest(&name) {
            Some(nearest) => format!("unknown style property `{name}` — did you mean `{nearest}`?"),
            None => format!("unknown style property `{name}`"),
        };
        return Err(syn::Error::new(ident.span(), message));
    };
    input.parse::<Token![:]>()?;
    let tokens = value_tokens(input)?;
    let value = match keyword(&tokens).as_deref() {
        Some("inherit") => Value::Inherit,
        Some("unset") => Value::Unset,
        _ => Value::Tokens(tokens),
    };
    Ok(Entry::Prop {
        prop,
        span: ident.span(),
        value,
    })
}

/// A color variable name: `--primary`, `--surface-raised`, `--gray-100`.
///
/// Segments are joined with `-` because a Rust ident cannot hold one, so `--surface-raised`
/// arrives as three tokens and has to be put back together.
fn parse_var_name(input: ParseStream) -> syn::Result<String> {
    let first = input.parse::<Token![-]>()?;
    input.parse::<Token![-]>().map_err(|_| {
        syn::Error::new(
            first.span(),
            "a color variable name starts with `--`, as in `--primary`",
        )
    })?;

    let head: Ident = input.parse()?;
    let mut name = format!("--{head}");
    while input.peek(Token![-]) {
        input.parse::<Token![-]>()?;
        name.push('-');
        if input.peek(LitInt) {
            let segment: LitInt = input.parse()?;
            name.push_str(segment.base10_digits());
        } else {
            let segment: Ident = input.parse()?;
            name.push_str(&segment.to_string());
        }
    }
    Ok(name)
}

/// Collect a value's tokens up to the entry's terminating comma.
///
/// Nested commas are inside a `Group`, and a group is one token tree, so the only bare
/// comma this can meet is the one ending the entry.
fn value_tokens(input: ParseStream) -> syn::Result<TokenStream> {
    let mut tokens = TokenStream::new();
    while !input.is_empty() && !input.peek(Token![,]) {
        let tree: TokenTree = input.parse()?;
        tokens.extend(std::iter::once(tree));
    }
    if tokens.is_empty() {
        return Err(input.error("expected a value"));
    }
    Ok(tokens)
}

/// The single bare ident a value consists of, if that is all it is.
fn keyword(tokens: &TokenStream) -> Option<String> {
    let mut trees = tokens.clone().into_iter();
    match (trees.next(), trees.next()) {
        (Some(TokenTree::Ident(ident)), None) => Some(ident.to_string()),
        _ => None,
    }
}

/// Parse a value's tokens as a plain Rust expression.
///
/// The fallback under every sugar: whatever the macro cannot read as sugar has to be
/// something the user could have written by hand. Parse failures keep syn's own span, which
/// points at the offending token rather than at the whole value.
///
/// Outer parentheses are dropped because they are the escape hatch from ident sugar, and
/// passing them through would fire `unused_parens` at the user's own call site.
fn expr(tokens: TokenStream) -> syn::Result<Expr> {
    let mut expr = syn::parse2::<Expr>(tokens)?;
    while let Expr::Paren(paren) = expr {
        expr = *paren.expr;
    }
    Ok(expr)
}

/// The `inherit_*` / `unset_*` calls a state keyword expands to — more than one where the
/// property is a shorthand.
fn state_calls(prop: &Prop, span: Span, prefix: &str) -> Vec<TokenStream> {
    prop.states
        .iter()
        .map(|state| {
            let method = Ident::new(&format!("{prefix}_{state}"), span);
            quote!(__tuidom_style.#method();)
        })
        .collect()
}

pub(crate) fn expand(input: StyleInput) -> syn::Result<TokenStream> {
    // `..base` copies rather than moves. Struct-update syntax would move, which is wrong
    // for the pattern this replaces — a base style is derived from more than once.
    let init = match &input.base {
        Some(base) => quote!(::core::clone::Clone::clone(&#base)),
        None => quote!(::tuidom::style::Style::new()),
    };

    let mut stmts = Vec::new();
    for entry in input.entries {
        match entry {
            Entry::Prop { prop, span, value } => match value {
                Value::Inherit => stmts.extend(state_calls(prop, span, "inherit")),
                Value::Unset => stmts.extend(state_calls(prop, span, "unset")),
                Value::Tokens(tokens) => {
                    let setter = Ident::new(prop.setter, span);
                    let value = value::parse(prop.ty, tokens)?;
                    stmts.push(quote!(__tuidom_style.#setter(#value);));
                }
            },
            Entry::ColorVar { name, value } => {
                let value = value::parse(props::PropType::Color, value)?;
                stmts.push(quote!(__tuidom_style.color_var(#name, #value);));
            }
            Entry::Custom { name, value } => {
                let value = expr(value)?;
                stmts.push(quote!(__tuidom_style.set_custom(#name, #value);));
            }
        }
    }

    Ok(quote! {{
        let mut __tuidom_style: ::tuidom::style::Style = #init;
        #(#stmts)*
        __tuidom_style
    }})
}

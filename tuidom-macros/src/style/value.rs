//! Value sugar, dispatched on the property's own type.
//!
//! Every pattern here is something Rust's own literals cannot say — a percentage, a run of
//! edge counts, a duration. Anything that does not match falls through to a plain Rust
//! expression, so the sugar is never the only way to write a value.

use std::str::FromStr;

use proc_macro2::{Delimiter, Ident, Literal, Span, TokenStream, TokenTree};
use quote::quote;
use syn::Lit;

use super::props::PropType;

/// Turn a value's tokens into the expression its setter is called with.
pub(crate) fn parse(ty: PropType, tokens: TokenStream) -> syn::Result<TokenStream> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();

    // A parenthesized value is the escape hatch out of every sugar below.
    if !is_escaped(&trees) {
        // Caught here rather than left to the fallback: `--primary` is a valid Rust
        // expression — double negation — so the error would otherwise be about `Neg`.
        if ty != PropType::Color && starts_with_var(&trees) {
            return Err(syn::Error::new_spanned(
                &tokens,
                "`--name` is a color variable, and this property does not take a color",
            ));
        }
        if let Some(sugared) = sugar(ty, &trees, &tokens)? {
            return Ok(sugared);
        }
    }

    let expr = super::expr(tokens)?;
    Ok(quote!(#expr))
}

/// Whole-value diagnostics are spanned over `value` rather than its first token, so the
/// underline covers what the user actually wrote.
fn sugar(
    ty: PropType,
    trees: &[TokenTree],
    value: &TokenStream,
) -> syn::Result<Option<TokenStream>> {
    Ok(match ty {
        PropType::Length => length(trees, value)?,
        PropType::EdgeInsets => edge_insets(trees, value)?,
        PropType::FlexGap => flex_gap(trees, value)?,
        PropType::Duration => duration(trees)?,
        PropType::Number(target) => number(trees, target)?,
        PropType::Color => color(trees, value)?,
        PropType::Sides => sides(trees)?,
        PropType::Border => border(trees)?,
        PropType::ScrollbarCharset => charset("ScrollbarCharset", trees),
        PropType::Position => position(trees)?,
        PropType::Enum(ty) => match trees {
            [TokenTree::Ident(name)] => Some(variant(ty, name)),
            _ => None,
        },
        PropType::Bool => None,
    })
}

/// `all`, `none`, or a subset of edge names.
fn sides(trees: &[TokenTree]) -> syn::Result<Option<TokenStream>> {
    if trees.is_empty() {
        return Ok(None);
    }
    if let [TokenTree::Ident(name)] = trees {
        match name.to_string().as_str() {
            "all" => return Ok(Some(quote!(::tuidom::style::Sides::ALL))),
            "none" => return Ok(Some(quote!(::tuidom::style::Sides::NONE))),
            _ => {}
        }
    }

    let mut edges = [false; 4];
    for tree in trees {
        let TokenTree::Ident(name) = tree else {
            return Ok(None);
        };
        let index = match name.to_string().as_str() {
            "top" => 0,
            "right" => 1,
            "bottom" => 2,
            "left" => 3,
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "`{other}` is not an edge — expected `all`, `none`, or any of `top`, \
                         `right`, `bottom`, `left`"
                    ),
                ));
            }
        };
        if std::mem::replace(&mut edges[index], true) {
            return Err(syn::Error::new(
                name.span(),
                format!("`{name}` is repeated"),
            ));
        }
    }
    let [top, right, bottom, left] = edges;
    Ok(Some(
        quote!(::tuidom::style::Sides::new(#top, #right, #bottom, #left)),
    ))
}

/// `none`, a charset, or a charset limited to some sides.
fn border(trees: &[TokenTree]) -> syn::Result<Option<TokenStream>> {
    let [TokenTree::Ident(name), rest @ ..] = trees else {
        return Ok(None);
    };
    if *name == "none" && rest.is_empty() {
        return Ok(Some(quote!(::tuidom::style::Border::none())));
    }

    let charset = quote!(::tuidom::style::BorderCharset::#name());
    if rest.is_empty() {
        return Ok(Some(quote!(::tuidom::style::Border::new(#charset))));
    }
    let Some(sides) = sides(rest)? else {
        return Ok(None);
    };
    Ok(Some(
        quote!(::tuidom::style::Border::new(#charset).with_sides(#sides)),
    ))
}

/// A named charset constructor — `block`, `single`, and friends are functions, not variants.
fn charset(ty: &str, trees: &[TokenTree]) -> Option<TokenStream> {
    let [TokenTree::Ident(name)] = trees else {
        return None;
    };
    let ty = Ident::new(ty, name.span());
    Some(quote!(::tuidom::style::#ty::#name()))
}

/// `flow`, or `absolute(x, y)`.
fn position(trees: &[TokenTree]) -> syn::Result<Option<TokenStream>> {
    if let [TokenTree::Ident(name), TokenTree::Group(args)] = trees
        && *name == "absolute"
        && args.delimiter() == Delimiter::Parenthesis
    {
        let offsets = split_commas(args.stream());
        let [x, y] = offsets.as_slice() else {
            return Err(syn::Error::new(
                args.span(),
                "`absolute` takes a horizontal and a vertical offset",
            ));
        };
        let x = super::expr(x.clone())?;
        let y = super::expr(y.clone())?;
        return Ok(Some(
            quote!(::tuidom::style::Position::Absolute { x: #x, y: #y }),
        ));
    }
    if let [TokenTree::Ident(name)] = trees {
        return Ok(Some(variant("Position", name)));
    }
    Ok(None)
}

/// A color expression: a base, then any number of derivation calls.
///
/// ```text
/// --primary.darken(0.1)
/// current_bg.mix(white, 0.2)
/// oklch(0.6, 0.2, 250)
/// ```
fn color(trees: &[TokenTree], value: &TokenStream) -> syn::Result<Option<TokenStream>> {
    let Some((mut expr, mut rest)) = color_base(trees, value)? else {
        return Ok(None);
    };

    while let [
        TokenTree::Punct(dot),
        TokenTree::Ident(method),
        TokenTree::Group(args),
        tail @ ..,
    ] = rest
    {
        if dot.as_char() != '.' || args.delimiter() != Delimiter::Parenthesis {
            break;
        }
        let args = color_args(args.stream())?;
        expr = quote!(#expr.#method(#(#args),*));
        rest = tail;
    }

    // A trailing remainder means this was never a color expression — hand the whole value
    // back to the plain-expression fallback rather than guessing at half of it.
    Ok(rest.is_empty().then_some(expr))
}

/// The head of a color expression, and whatever follows it.
fn color_base<'a>(
    trees: &'a [TokenTree],
    value: &TokenStream,
) -> syn::Result<Option<(TokenStream, &'a [TokenTree])>> {
    if let [TokenTree::Punct(first), TokenTree::Punct(second), rest @ ..] = trees
        && first.as_char() == '-'
        && second.as_char() == '-'
    {
        let (name, rest) = var_name(rest, value)?;
        return Ok(Some((quote!(::tuidom::style::Color::var(#name)), rest)));
    }

    if let [TokenTree::Ident(name), TokenTree::Group(args), rest @ ..] = trees {
        let called = name.to_string();
        // Only the two OKLCH constructors sugar as calls. Sugaring every `name(..)` would
        // silently redirect a user's own function call into `Color::`.
        if matches!(called.as_str(), "oklch" | "oklcha")
            && args.delimiter() == Delimiter::Parenthesis
        {
            let args = color_args(args.stream())?;
            return Ok(Some((
                quote!(::tuidom::style::Color::#name(#(#args),*)),
                rest,
            )));
        }
    }

    if let [TokenTree::Ident(name), rest @ ..] = trees {
        // `CurrentBg` and `CurrentFg` are variants; every other named color is a
        // constructor. Both resolve through `Color` itself, so neither is a keyword list.
        let base = match name.to_string().as_str() {
            "current_bg" => quote!(::tuidom::style::Color::CurrentBg),
            "current_fg" => quote!(::tuidom::style::Color::CurrentFg),
            _ => quote!(::tuidom::style::Color::#name()),
        };
        return Ok(Some((base, rest)));
    }

    Ok(None)
}

/// A `--name` reference's segments, put back together.
///
/// A Rust ident cannot hold a `-`, so `--surface-raised` arrives as five tokens.
fn var_name<'a>(
    trees: &'a [TokenTree],
    value: &TokenStream,
) -> syn::Result<(String, &'a [TokenTree])> {
    let [TokenTree::Ident(head), tail @ ..] = trees else {
        return Err(syn::Error::new_spanned(value, "expected a name after `--`"));
    };
    let mut rest: &[TokenTree] = tail;
    let mut name = format!("--{head}");
    while let [TokenTree::Punct(dash), segment, tail @ ..] = rest {
        if dash.as_char() != '-' {
            break;
        }
        match segment {
            TokenTree::Ident(part) => name.push_str(&format!("-{part}")),
            TokenTree::Literal(part) => name.push_str(&format!("-{part}")),
            _ => break,
        }
        rest = tail;
    }
    Ok((name, rest))
}

/// The arguments of a color constructor or derivation, each sugared in turn.
///
/// Colors nest — `mix` takes one — and every numeric argument in the color API is an `f64`,
/// so a bare literal is retyped the same way a numeric property's is.
fn color_args(stream: TokenStream) -> syn::Result<Vec<TokenStream>> {
    let mut args = Vec::new();
    for arg in split_commas(stream) {
        let trees: Vec<TokenTree> = arg.clone().into_iter().collect();
        if !is_escaped(&trees) {
            if let Some(sugared) = color(&trees, &arg)? {
                args.push(sugared);
                continue;
            }
            if let Some((digits, span)) = single_number(&trees) {
                let literal = retyped(&digits, "f64", span)?;
                args.push(quote!(#literal));
                continue;
            }
        }
        let expr = super::expr(arg)?;
        args.push(quote!(#expr));
    }
    Ok(args)
}

fn split_commas(stream: TokenStream) -> Vec<TokenStream> {
    let mut args = Vec::new();
    let mut current = TokenStream::new();
    for tree in stream {
        match &tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                args.push(std::mem::take(&mut current));
            }
            _ => current.extend(std::iter::once(tree)),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn single_number(trees: &[TokenTree]) -> Option<(String, Span)> {
    match trees {
        [tree] => unsuffixed_number(tree).map(|digits| (digits, tree.span())),
        _ => None,
    }
}

/// `auto`, `12` cells, or `50%`.
fn length(trees: &[TokenTree], value: &TokenStream) -> syn::Result<Option<TokenStream>> {
    if let [TokenTree::Ident(name)] = trees {
        return Ok(Some(variant("Length", name)));
    }
    if let [tree] = trees
        && let Some(cells) = unsuffixed_int(tree)
    {
        return Ok(Some(quote!(::tuidom::style::Length::Cells(#cells))));
    }
    if let [tree, TokenTree::Punct(punct)] = trees
        && punct.as_char() == '%'
    {
        let Some(percent) = unsuffixed_number(tree) else {
            return Err(syn::Error::new_spanned(
                value,
                "a percentage needs a number before `%`",
            ));
        };
        let percent = retyped(&percent, "f64", tree.span())?;
        return Ok(Some(quote!(::tuidom::style::Length::Percent(#percent))));
    }
    Ok(None)
}

/// One to four cell counts, in CSS edge order.
fn edge_insets(trees: &[TokenTree], value: &TokenStream) -> syn::Result<Option<TokenStream>> {
    let Some(cells) = all_unsuffixed_ints(trees) else {
        return Ok(None);
    };
    let (top, right, bottom, left) = match cells.as_slice() {
        [all] => (all, all, all, all),
        [vertical, horizontal] => (vertical, horizontal, vertical, horizontal),
        [top, horizontal, bottom] => (top, horizontal, bottom, horizontal),
        [top, right, bottom, left] => (top, right, bottom, left),
        _ => {
            return Err(syn::Error::new_spanned(
                value,
                "edge spacing takes one to four cell counts: `1`, `1 2`, `1 2 3`, or `1 2 3 4`",
            ));
        }
    };
    Ok(Some(
        quote!(::tuidom::style::EdgeInsets::new(#top, #right, #bottom, #left)),
    ))
}

/// One cell count for both axes, or a row and a column count.
fn flex_gap(trees: &[TokenTree], value: &TokenStream) -> syn::Result<Option<TokenStream>> {
    let Some(cells) = all_unsuffixed_ints(trees) else {
        return Ok(None);
    };
    Ok(Some(match cells.as_slice() {
        [all] => quote!(::tuidom::style::FlexGap::all(#all)),
        [row, column] => quote!(::tuidom::style::FlexGap::new(#row, #column)),
        _ => {
            return Err(syn::Error::new_spanned(
                value,
                "a flex gap takes one cell count, or a row and a column count",
            ));
        }
    }))
}

/// `300ms`, `2s`, `1.5s`.
///
/// Rust lexes a suffixed literal as one token, so the unit rides along on the number rather
/// than being a token of its own.
fn duration(trees: &[TokenTree]) -> syn::Result<Option<TokenStream>> {
    let [TokenTree::Literal(literal)] = trees else {
        return Ok(None);
    };
    let span = literal.span();
    Ok(match Lit::new(literal.clone()) {
        Lit::Int(value) => {
            let digits = retyped(value.base10_digits(), "u64", span)?;
            match value.suffix() {
                "ms" => Some(quote!(::core::time::Duration::from_millis(#digits))),
                "s" => Some(quote!(::core::time::Duration::from_secs(#digits))),
                _ => None,
            }
        }
        Lit::Float(value) => {
            let digits = retyped(value.base10_digits(), "f64", span)?;
            match value.suffix() {
                "ms" => Some(quote!(::core::time::Duration::from_secs_f64(#digits / 1000.0))),
                "s" => Some(quote!(::core::time::Duration::from_secs_f64(#digits))),
                _ => None,
            }
        }
        _ => None,
    })
}

/// Give a bare numeric literal the type its property wants.
///
/// `opacity: 1` should mean what it looks like rather than an integer-into-`f64` error.
fn number(trees: &[TokenTree], target: &str) -> syn::Result<Option<TokenStream>> {
    let [tree] = trees else {
        return Ok(None);
    };
    let Some(digits) = unsuffixed_number(tree) else {
        return Ok(None);
    };
    let literal = retyped(&digits, target, tree.span())?;
    Ok(Some(quote!(#literal)))
}

/// `Type::PascalCase` for a snake_case ident, spanned at the user's token.
///
/// Variant sugar resolves through the property's type rather than a table of known names,
/// so a variant the engine gains needs no change here, and one that does not exist is a
/// plain rustc error pointing at what the user wrote.
pub(crate) fn variant(ty: &str, name: &Ident) -> TokenStream {
    let span = name.span();
    let ty = Ident::new(ty, span);
    let variant = Ident::new(&pascal(&name.to_string()), span);
    quote!(::tuidom::style::#ty::#variant)
}

fn pascal(snake: &str) -> String {
    snake
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// Whether a value opens with a `--name` color variable reference.
fn starts_with_var(trees: &[TokenTree]) -> bool {
    matches!(
        trees,
        [TokenTree::Punct(first), TokenTree::Punct(second), TokenTree::Ident(_), ..]
            if first.as_char() == '-' && second.as_char() == '-'
    )
}

/// Whether the whole value is one parenthesized group.
fn is_escaped(trees: &[TokenTree]) -> bool {
    matches!(trees, [TokenTree::Group(group)] if group.delimiter() == Delimiter::Parenthesis)
}

fn all_unsuffixed_ints(trees: &[TokenTree]) -> Option<Vec<Literal>> {
    if trees.is_empty() {
        return None;
    }
    trees.iter().map(unsuffixed_int).collect()
}

fn unsuffixed_int(tree: &TokenTree) -> Option<Literal> {
    let TokenTree::Literal(literal) = tree else {
        return None;
    };
    match Lit::new(literal.clone()) {
        Lit::Int(value) if value.suffix().is_empty() => Some(literal.clone()),
        _ => None,
    }
}

/// The digits of an unsuffixed integer or float literal.
fn unsuffixed_number(tree: &TokenTree) -> Option<String> {
    let TokenTree::Literal(literal) = tree else {
        return None;
    };
    match Lit::new(literal.clone()) {
        Lit::Int(value) if value.suffix().is_empty() => Some(value.base10_digits().to_string()),
        Lit::Float(value) if value.suffix().is_empty() => Some(value.base10_digits().to_string()),
        _ => None,
    }
}

/// A numeric literal with an explicit type suffix, so inference cannot land elsewhere.
fn retyped(digits: &str, target: &str, span: Span) -> syn::Result<Literal> {
    let mut literal = Literal::from_str(&format!("{digits}{target}"))
        .map_err(|_| syn::Error::new(span, format!("`{digits}` is not a valid {target}")))?;
    literal.set_span(span);
    Ok(literal)
}

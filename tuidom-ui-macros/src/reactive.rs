//! The body rewrite behind `#[component(reactive)]`: signal names used as values become reads.
//!
//! Names are resolved lexically, the way the compiler would: a binding of the same name in an
//! inner scope hides a signal until that scope ends. Nested items are skipped. `view!` and
//! `derived!` are rewritten and lowered here; other macro calls are skipped, so `signal!(x)`
//! passes its argument through untouched.
//!
//! A `with` or `update` closure runs under the runtime lock, where reading another signal can
//! deadlock. So everything a rewritten chain or write evaluates is moved into a local ahead of
//! it, and a closure passed along a chain may not read a signal at all.

use std::collections::HashMap;
use std::mem;

use proc_macro2::{Delimiter, Group, Span, TokenStream, TokenTree};
use quote::ToTokens;
use quote::{format_ident, quote_spanned};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{
    BinOp, Block, Expr, ExprLit, ExprMacro, ExprMethodCall, ExprPath, FieldValue, Ident, Item, Lit,
    LitStr, Local, Macro, Pat, Stmt, Token, Type, token,
};

use crate::format_string;

use crate::{ElseBranch, IfNode, Node};

/// Rewrite `block`, the body of a component whose props named `signals` are signals.
pub(crate) fn rewrite_body(
    block: &mut Block,
    signals: impl IntoIterator<Item = Ident>,
) -> syn::Result<()> {
    let props = signals.into_iter().map(|i| (i.to_string(), true)).collect();
    let mut rewriter = Rewriter {
        scopes: vec![props],
        uses: Vec::new(),
        error: None,
    };
    rewriter.visit_block_mut(block);
    rewriter.error.map_or(Ok(()), Err)
}

struct Rewriter {
    /// Innermost last. Each maps a bound name to whether it is a signal.
    scopes: Vec<HashMap<String, bool>>,
    /// Every use of a signal name met so far, in order.
    uses: Vec<Ident>,
    error: Option<syn::Error>,
}

/// Locals evaluated ahead of a rewritten read or write, in evaluation order.
type Hoisted = Vec<(Ident, Expr)>;

impl Rewriter {
    fn is_signal(&self, ident: &Ident) -> bool {
        let name = ident.to_string();
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
            .unwrap_or(false)
    }

    /// The signal a path names, when it is a bare in-scope signal name.
    fn signal_ident<'a>(&self, path: &'a ExprPath) -> Option<&'a Ident> {
        let ident = path.path.get_ident().filter(|_| path.qself.is_none())?;
        self.is_signal(ident).then_some(ident)
    }

    /// The signal at the root of a chain of method calls, field accesses and indexing.
    fn chain_root(&self, expr: &Expr) -> Option<Ident> {
        match expr {
            Expr::MethodCall(call) => self.chain_root(&call.receiver),
            Expr::Field(field) => self.chain_root(&field.base),
            Expr::Index(index) => self.chain_root(&index.expr),
            Expr::Path(path) => self.signal_ident(path).cloned(),
            _ => None,
        }
    }

    /// The signal a write targets: a bare signal name, or field accesses and indexing on one.
    fn write_root(&self, left: &Expr) -> Option<Ident> {
        match left {
            Expr::Field(field) => self.write_root(&field.base),
            Expr::Index(index) => self.write_root(&index.expr),
            Expr::Path(path) => self.signal_ident(path).cloned(),
            _ => None,
        }
    }

    fn fail(&mut self, error: syn::Error) {
        match &mut self.error {
            Some(existing) => existing.combine(error),
            None => self.error = Some(error),
        }
    }

    fn replace(&mut self, expr: &mut Expr, tokens: TokenStream) {
        match syn::parse2(tokens) {
            Ok(rewritten) => *expr = rewritten,
            Err(error) => self.fail(error),
        }
    }

    /// A rewritten and lowered `view!` or `derived!` call, a standard expression macro with its
    /// arguments rewritten, or `None` to leave the macro as is.
    fn lower_macro(&mut self, mac: &Macro) -> Option<Expr> {
        let name = mac.path.segments.last()?.ident.to_string();
        let lowered = match name.as_str() {
            "view" => {
                let mut node: Node = self.parse(mac.parse_body())?;
                self.visit_node(&mut node);
                return self.parse(syn::parse2(node.lower()));
            }
            "derived" => {
                let mut expr = self.parse(parse_derived.parse2(mac.tokens.clone()))?;
                self.visit_expr_mut(&mut expr);
                let lowered =
                    quote_spanned! {mac.path.span()=> ::tuidom_ui::derived(move || #expr) };
                return self.parse(syn::parse2(lowered));
            }
            "format" | "print" | "println" | "eprint" | "eprintln" | "panic" => {
                return self.rewrite_format_macro(mac, Some(0));
            }
            "write" | "writeln" | "assert" | "debug_assert" => {
                return self.rewrite_format_macro(mac, Some(1));
            }
            "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => {
                return self.rewrite_format_macro(mac, Some(2));
            }
            "dbg" => return self.rewrite_format_macro(mac, None),
            "vec" => {
                let mut args = self.parse(mac.parse_body_with(parse_vec_args))?;
                match &mut args {
                    VecArgs::Repeat(value, len) => {
                        self.visit_expr_mut(value);
                        self.visit_expr_mut(len);
                        quote_spanned! {mac.path.span()=> #value; #len }
                    }
                    VecArgs::List(items) => {
                        items.iter_mut().for_each(|item| self.visit_expr_mut(item));
                        quote_spanned! {mac.path.span()=> #items }
                    }
                }
            }
            "matches" => {
                let mut args = self.parse(mac.parse_body_with(parse_matches_args))?;
                self.visit_expr_mut(&mut args.expr);
                let pat = &args.pat;
                let guard = args.guard.as_mut().map(|guard| {
                    self.in_scope(|this| {
                        this.bind(pat, false);
                        this.visit_expr_mut(guard);
                    });
                    quote_spanned! {mac.path.span()=> if #guard }
                });
                let expr = &args.expr;
                quote_spanned! {mac.path.span()=> #expr, #pat #guard }
            }
            _ => return None,
        };
        let mut rewritten = mac.clone();
        rewritten.tokens = lowered;
        Some(Expr::Macro(ExprMacro {
            attrs: Vec::new(),
            mac: rewritten,
        }))
    }

    /// A macro taking expressions, with a format string at `format_at` if it takes one. Inline
    /// captures of signal names become named arguments holding the read, and a named argument
    /// the caller wrote has only its value rewritten, since `name = value` is not a write.
    fn rewrite_format_macro(&mut self, mac: &Macro, format_at: Option<usize>) -> Option<Expr> {
        let mut args =
            self.parse(mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated))?;
        let mut named = Vec::new();
        for arg in args.iter_mut() {
            match arg {
                Expr::Assign(assign) if format_at.is_some() => {
                    if let Expr::Path(path) = assign.left.as_ref()
                        && let Some(ident) = path.path.get_ident()
                    {
                        named.push(ident.to_string());
                    }
                    self.visit_expr_mut(&mut assign.right);
                }
                arg => self.visit_expr_mut(arg),
            }
        }
        if let Some(Expr::Lit(ExprLit {
            lit: Lit::Str(lit), ..
        })) = format_at.and_then(|at| args.iter().nth(at))
        {
            let lit = lit.clone();
            for name in format_string::captures(&lit.value()) {
                let ident = Ident::new(&name, lit.span());
                if named.contains(&name) || !self.is_signal(&ident) {
                    continue;
                }
                let mut value = Expr::Path(path_of(ident.clone()));
                self.visit_expr_mut(&mut value);
                let arg =
                    self.parse(syn::parse2(quote_spanned! {lit.span()=> #ident = #value }))?;
                args.push(arg);
            }
        }
        let mut rewritten = mac.clone();
        rewritten.tokens = quote_spanned! {mac.path.span()=> #args };
        Some(Expr::Macro(ExprMacro {
            attrs: Vec::new(),
            mac: rewritten,
        }))
    }

    fn parse<T>(&mut self, parsed: syn::Result<T>) -> Option<T> {
        parsed.map_err(|error| self.fail(error)).ok()
    }

    fn visit_node(&mut self, node: &mut Node) {
        match node {
            Node::Text { content, style } => {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(lit), ..
                }) = content
                {
                    if let Some(interpolated) = self.interpolate(lit) {
                        *content = interpolated;
                    }
                } else {
                    self.visit_expr_mut(content);
                }
                if let Some(style) = style {
                    self.visit_expr_mut(style);
                }
            }
            Node::Frames {
                list,
                interval,
                style,
            } => {
                self.visit_expr_mut(list);
                self.visit_expr_mut(interval);
                if let Some(style) = style {
                    self.visit_expr_mut(style);
                }
            }
            Node::Hole(expr) => self.visit_expr_mut(expr),
            Node::If(node) => self.visit_if_node(node),
            Node::Match(node) => {
                self.visit_expr_mut(&mut node.scrutinee);
                for arm in &mut node.arms {
                    self.in_scope(|this| {
                        this.bind(&arm.pat, false);
                        if let Some(guard) = &mut arm.guard {
                            this.visit_expr_mut(guard);
                        }
                        this.visit_node(&mut arm.body);
                    });
                }
            }
            Node::Each(node) => {
                self.visit_expr_mut(&mut node.collection);
                self.in_scope(|this| {
                    this.bind(&node.index_pat, false);
                    this.bind(&node.value_pat, false);
                    if let Some(key) = &mut node.key {
                        this.visit_expr_mut(key);
                    }
                    this.visit_nodes(&mut node.body);
                });
            }
            Node::Portal(children) => self.visit_nodes(children),
            Node::Component {
                props, children, ..
            } => {
                for prop in props {
                    self.visit_handle_or_value(&mut prop.value);
                }
                self.visit_nodes(children);
            }
            Node::Element {
                attrs, children, ..
            } => {
                for attr in attrs {
                    if attr.name == "bind_value" {
                        self.visit_handle_or_value(&mut attr.value);
                    } else {
                        self.visit_expr_mut(&mut attr.value);
                    }
                }
                self.visit_nodes(children);
            }
        }
    }

    /// A text literal's holes as a `format!` with a named argument per name, each read through
    /// the rewrite. A literal without holes stays a literal, its escapes resolved.
    fn interpolate(&mut self, lit: &LitStr) -> Option<Expr> {
        let scanned = match format_string::scan(&lit.value()) {
            Ok(scanned) => scanned,
            Err(message) => {
                self.fail(syn::Error::new(lit.span(), message));
                return None;
            }
        };
        if scanned.names.is_empty() {
            let text = LitStr::new(&scanned.unescaped, lit.span());
            return Some(Expr::Lit(ExprLit {
                attrs: Vec::new(),
                lit: Lit::Str(text),
            }));
        }
        let args = scanned.names.iter().map(|name| {
            let ident = Ident::new(name, lit.span());
            let mut value = Expr::Path(path_of(ident.clone()));
            self.visit_expr_mut(&mut value);
            quote_spanned! {lit.span()=> #ident = #value }
        });
        let tokens = quote_spanned! {lit.span()=> ::std::format!(#lit, #(#args),*) };
        self.parse(syn::parse2(tokens))
    }

    fn visit_nodes(&mut self, nodes: &mut [Node]) {
        for node in nodes {
            self.visit_node(node);
        }
    }

    fn visit_if_node(&mut self, node: &mut IfNode) {
        self.in_scope(|this| {
            this.visit_condition(&mut node.cond);
            this.visit_nodes(&mut node.then_branch);
        });
        match &mut node.else_branch {
            Some(ElseBranch::If(inner)) => self.visit_if_node(inner),
            Some(ElseBranch::Block(children)) => self.visit_nodes(children),
            None => {}
        }
    }

    /// Where a template takes a signal, a bare signal name passes the handle rather than a read.
    fn visit_handle_or_value(&mut self, value: &mut Expr) {
        if let Expr::Path(path) = value
            && let Some(ident) = self.signal_ident(path).cloned()
        {
            self.uses.push(ident);
        } else {
            self.visit_expr_mut(value);
        }
    }

    /// A chain whose first step is one of Signal's own methods, as in `x.get().len()`: the
    /// signal is left alone and only the arguments are visited.
    fn visit_own_method_chain(&mut self, expr: &mut Expr) {
        match expr {
            Expr::MethodCall(call) => {
                if let Expr::Path(path) = call.receiver.as_ref()
                    && let Some(ident) = self.signal_ident(path).cloned()
                {
                    self.uses.push(ident);
                } else {
                    self.visit_own_method_chain(&mut call.receiver);
                }
                for arg in &mut call.args {
                    self.visit_expr_mut(arg);
                }
            }
            Expr::Field(field) => self.visit_own_method_chain(&mut field.base),
            Expr::Index(index) => {
                self.visit_own_method_chain(&mut index.expr);
                self.visit_expr_mut(&mut index.index);
            }
            _ => {}
        }
    }

    /// `root.with(|root| chain)`, with the chain's arguments evaluated ahead of it.
    fn read_chain(&mut self, expr: &mut Expr, root: Ident) {
        self.uses.push(root.clone());
        let mut hoisted = Vec::new();
        self.hoist_chain(expr, &root, &mut hoisted);
        // The closing parenthesis takes the chain's end, so rustc sees the read as spanning the
        // whole chain and a type error underlines all of it rather than the signal's name.
        let mut call = Group::new(
            Delimiter::Parenthesis,
            quote_spanned! {root.span()=> |#root| #expr },
        );
        call.set_span(last_span(expr).unwrap_or(root.span()));
        let read = quote_spanned! {root.span()=> #root.with #call };
        let tokens = if hoisted.is_empty() {
            read
        } else {
            let lets = hoisted_lets(&hoisted, &root);
            quote_spanned! {root.span()=> ({ #lets #read }) }
        };
        self.replace(expr, tokens);
    }

    /// Move the arguments and indices along a chain into locals.
    fn hoist_chain(&mut self, expr: &mut Expr, root: &Ident, hoisted: &mut Hoisted) {
        match expr {
            Expr::MethodCall(call) => {
                self.hoist_chain(&mut call.receiver, root, hoisted);
                for arg in &mut call.args {
                    self.hoist(arg, root, hoisted);
                }
            }
            Expr::Field(field) => self.hoist_chain(&mut field.base, root, hoisted),
            Expr::Index(index) => {
                self.hoist_chain(&mut index.expr, root, hoisted);
                self.hoist(&mut index.index, root, hoisted);
            }
            _ => {}
        }
    }

    /// Rewrite `arg` and move it into a local. Literals stay in place, and so do closures,
    /// which would lose their parameter types as locals and so must not read signals.
    fn hoist(&mut self, arg: &mut Expr, root: &Ident, hoisted: &mut Hoisted) {
        let before = self.uses.len();
        self.visit_expr_mut(arg);
        if matches!(arg, Expr::Closure(_)) {
            if let Some(used) = self.uses.get(before) {
                let message = format!(
                    "this closure runs inside the read of `{root}`, where reading `{used}` can \
                     deadlock; read `{used}` into a local before this line"
                );
                self.fail(syn::Error::new(used.span(), message));
            }
            return;
        }
        if matches!(arg, Expr::Lit(_)) {
            return;
        }
        let local = format_ident!("arg{}", hoisted.len(), span = Span::mixed_site());
        let value = mem::replace(arg, Expr::Path(path_of(local.clone())));
        hoisted.push((local, value));
    }

    /// `root.set(v)`, or `root.update(|root| ..)` holding the assignment, with the value and
    /// any indices evaluated ahead of it.
    fn write(&mut self, expr: &mut Expr, root: Ident) {
        self.uses.push(root.clone());
        let plain = matches!(expr, Expr::Assign(_));
        let (left, op, right) = match expr {
            Expr::Assign(assign) => (
                assign.left.as_mut(),
                quote_spanned! {assign.eq_token.span=> = },
                assign.right.as_mut(),
            ),
            Expr::Binary(binary) => {
                let op = &binary.op;
                (
                    binary.left.as_mut(),
                    quote_spanned! {op.span()=> #op },
                    binary.right.as_mut(),
                )
            }
            _ => return,
        };
        self.visit_expr_mut(right);
        let bare = matches!(left, Expr::Path(_));
        let tokens = if bare && plain {
            quote_spanned! {root.span()=> #root.set(#right) }
        } else {
            let mut hoisted = Vec::new();
            if !matches!(right, Expr::Lit(_)) {
                let local = format_ident!("arg{}", hoisted.len(), span = Span::mixed_site());
                let value = mem::replace(right, Expr::Path(path_of(local.clone())));
                hoisted.push((local, value));
            }
            self.hoist_chain(left, &root, &mut hoisted);
            let target = if bare {
                quote_spanned! {root.span()=> *#root }
            } else {
                quote_spanned! {root.span()=> #left }
            };
            let update = quote_spanned! {root.span()=> #root.update(|#root| #target #op #right) };
            if hoisted.is_empty() {
                update
            } else {
                let lets = hoisted_lets(&hoisted, &root);
                quote_spanned! {root.span()=> { #lets #update } }
            }
        };
        self.replace(expr, tokens);
    }

    fn bind(&mut self, pat: &Pat, is_signal: bool) {
        let mut names = Vec::new();
        pat_bindings(pat, &mut names);
        if let Some(scope) = self.scopes.last_mut() {
            for name in names {
                scope.insert(name.to_string(), is_signal);
            }
        }
    }

    fn in_scope(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        f(self);
        self.scopes.pop();
    }

    fn visit_local(&mut self, local: &mut Local) {
        if let Some(init) = &mut local.init {
            self.visit_expr_mut(&mut init.expr);
            if let Some((_, diverge)) = &mut init.diverge {
                self.visit_expr_mut(diverge);
            }
        }
        let is_signal = declares_signal(local);
        self.bind(&local.pat, is_signal);
    }

    /// An `if` or `while` condition, whose `let` bindings stay in scope for the rest of the
    /// chain and the body that follows.
    fn visit_condition(&mut self, cond: &mut Expr) {
        match cond {
            Expr::Let(expr) => {
                self.visit_expr_mut(&mut expr.expr);
                self.bind(&expr.pat, false);
            }
            Expr::Binary(expr) if matches!(expr.op, BinOp::And(_)) => {
                self.visit_condition(&mut expr.left);
                self.visit_condition(&mut expr.right);
            }
            _ => self.visit_expr_mut(cond),
        }
    }
}

impl VisitMut for Rewriter {
    fn visit_block_mut(&mut self, block: &mut Block) {
        self.in_scope(|this| {
            // Items are visible across their whole block, ahead of the statements.
            for stmt in &block.stmts {
                if let Stmt::Item(item) = stmt
                    && let Some(ident) = value_item_ident(item)
                    && let Some(scope) = this.scopes.last_mut()
                {
                    scope.insert(ident.to_string(), false);
                }
            }
            for stmt in &mut block.stmts {
                match stmt {
                    Stmt::Local(local) => this.visit_local(local),
                    Stmt::Expr(expr, _) => this.visit_expr_mut(expr),
                    Stmt::Macro(stmt_macro) => {
                        if let Some(expr) = this.lower_macro(&stmt_macro.mac) {
                            *stmt = Stmt::Expr(expr, stmt_macro.semi_token);
                        }
                    }
                    Stmt::Item(_) => {}
                }
            }
        });
    }

    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Path(path) => {
                if let Some(ident) = self.signal_ident(path).cloned() {
                    *expr = read(path.clone(), ident.span());
                    self.uses.push(ident);
                }
            }
            Expr::MethodCall(_) | Expr::Field(_) | Expr::Index(_) => match self.chain_root(expr) {
                Some(_) if starts_with_own_method(expr) => self.visit_own_method_chain(expr),
                Some(root) => self.read_chain(expr, root),
                None => visit_mut::visit_expr_mut(self, expr),
            },
            Expr::Assign(assign) => match self.write_root(&assign.left) {
                Some(root) => self.write(expr, root),
                None => visit_mut::visit_expr_mut(self, expr),
            },
            Expr::Binary(binary) if is_assign_op(&binary.op) => {
                match self.write_root(&binary.left) {
                    Some(root) => self.write(expr, root),
                    None => visit_mut::visit_expr_mut(self, expr),
                }
            }
            Expr::Closure(closure) => self.in_scope(|this| {
                for input in &closure.inputs {
                    this.bind(input, false);
                }
                this.visit_expr_mut(&mut closure.body);
            }),
            Expr::ForLoop(for_loop) => {
                self.visit_expr_mut(&mut for_loop.expr);
                self.in_scope(|this| {
                    this.bind(&for_loop.pat, false);
                    this.visit_block_mut(&mut for_loop.body);
                });
            }
            Expr::Match(expr_match) => {
                self.visit_expr_mut(&mut expr_match.expr);
                for arm in &mut expr_match.arms {
                    self.in_scope(|this| {
                        this.bind(&arm.pat, false);
                        if let Some((_, guard)) = &mut arm.guard {
                            this.visit_expr_mut(guard);
                        }
                        this.visit_expr_mut(&mut arm.body);
                    });
                }
            }
            Expr::If(expr_if) => {
                self.in_scope(|this| {
                    this.visit_condition(&mut expr_if.cond);
                    this.visit_block_mut(&mut expr_if.then_branch);
                });
                if let Some((_, otherwise)) = &mut expr_if.else_branch {
                    self.visit_expr_mut(otherwise);
                }
            }
            Expr::While(expr_while) => self.in_scope(|this| {
                this.visit_condition(&mut expr_while.cond);
                this.visit_block_mut(&mut expr_while.body);
            }),
            Expr::Let(expr_let) => self.visit_expr_mut(&mut expr_let.expr),
            Expr::Macro(expr_macro) => {
                if let Some(lowered) = self.lower_macro(&expr_macro.mac) {
                    *expr = lowered;
                }
            }
            _ => visit_mut::visit_expr_mut(self, expr),
        }
    }

    fn visit_field_value_mut(&mut self, field: &mut FieldValue) {
        let shorthand = field.colon_token.is_none();
        self.visit_expr_mut(&mut field.expr);
        // `Foo { x }` prints only the member while the colon is absent.
        if shorthand && !matches!(field.expr, Expr::Path(_)) {
            field.colon_token = Some(Token![:](field.member.span()));
        }
    }

    fn visit_pat_mut(&mut self, _: &mut Pat) {}

    fn visit_type_mut(&mut self, _: &mut Type) {}

    fn visit_item_mut(&mut self, _: &mut Item) {}
}

/// Whether the first step of a chain is one of Signal's own methods with its own argument
/// count, so the chain is already Form A.
fn starts_with_own_method(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall(call) if matches!(call.receiver.as_ref(), Expr::Path(_)) => {
            let arity = match call.method.to_string().as_str() {
                "get" | "try_get" => 0,
                "set" | "update" | "with" | "try_with" => 1,
                _ => return false,
            };
            call.args.len() == arity
        }
        Expr::MethodCall(call) => starts_with_own_method(&call.receiver),
        Expr::Field(field) => starts_with_own_method(&field.base),
        Expr::Index(index) => starts_with_own_method(&index.expr),
        _ => false,
    }
}

enum VecArgs {
    Repeat(Box<Expr>, Box<Expr>),
    List(Punctuated<Expr, Token![,]>),
}

fn parse_vec_args(input: ParseStream) -> syn::Result<VecArgs> {
    if input.is_empty() {
        return Ok(VecArgs::List(Punctuated::new()));
    }
    let first: Expr = input.parse()?;
    if input.peek(Token![;]) {
        input.parse::<Token![;]>()?;
        return Ok(VecArgs::Repeat(Box::new(first), input.parse()?));
    }
    let mut items = Punctuated::new();
    items.push_value(first);
    if input.peek(Token![,]) {
        items.push_punct(input.parse()?);
        items.extend(Punctuated::<Expr, Token![,]>::parse_terminated(input)?.into_pairs());
    }
    Ok(VecArgs::List(items))
}

struct MatchesArgs {
    expr: Expr,
    pat: Pat,
    guard: Option<Expr>,
}

fn parse_matches_args(input: ParseStream) -> syn::Result<MatchesArgs> {
    let expr = input.parse()?;
    input.parse::<Token![,]>()?;
    let pat = Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.peek(Token![if]) {
        input.parse::<Token![if]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok(MatchesArgs { expr, pat, guard })
}

/// `derived!`'s argument: one expression and an optional trailing comma.
fn parse_derived(input: ParseStream) -> syn::Result<Expr> {
    let expr = input.parse()?;
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok(expr)
}

/// The span of an expression's last token, or of its closing delimiter.
fn last_span(expr: &Expr) -> Option<Span> {
    match expr.to_token_stream().into_iter().last()? {
        TokenTree::Group(group) => Some(group.span_close()),
        token => Some(token.span()),
    }
}

fn hoisted_lets(hoisted: &Hoisted, root: &Ident) -> TokenStream {
    let (locals, values): (Vec<_>, Vec<_>) = hoisted.iter().cloned().unzip();
    quote_spanned! {root.span()=> #(let #locals = #values;)* }
}

fn path_of(ident: Ident) -> ExprPath {
    ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: ident.into(),
    }
}

/// `path.get()`, spanned at the name it reads.
fn read(path: ExprPath, span: Span) -> Expr {
    Expr::MethodCall(ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(Expr::Path(path)),
        dot_token: Token![.](span),
        method: Ident::new("get", span),
        turbofish: None,
        paren_token: token::Paren(span),
        args: Punctuated::new(),
    })
}

fn is_assign_op(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

/// Whether a `let` binds one name to a signal: through a written `Signal<T>` or `Derived<T>`
/// type, or an initializer calling `state`, `derived` or `derived!`.
fn declares_signal(local: &Local) -> bool {
    let (pat, ty) = match &local.pat {
        Pat::Type(typed) => (typed.pat.as_ref(), Some(typed.ty.as_ref())),
        pat => (pat, None),
    };
    if !matches!(pat, Pat::Ident(_)) {
        return false;
    }
    if ty.is_some_and(|ty| last_segment_is(ty, &["Signal", "Derived"])) {
        return true;
    }
    let Some(init) = &local.init else {
        return false;
    };
    match init.expr.as_ref() {
        Expr::Call(call) => matches!(
            call.func.as_ref(),
            Expr::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "state" || s.ident == "derived")
        ),
        Expr::Macro(mac) => mac
            .mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "derived"),
        _ => false,
    }
}

fn last_segment_is(ty: &Type, names: &[&str]) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    tp.path
        .segments
        .last()
        .is_some_and(|s| names.iter().any(|name| s.ident == name))
}

/// The value-namespace name a block-level item declares.
fn value_item_ident(item: &Item) -> Option<&Ident> {
    match item {
        Item::Fn(item) => Some(&item.sig.ident),
        Item::Const(item) => Some(&item.ident),
        Item::Static(item) => Some(&item.ident),
        _ => None,
    }
}

fn pat_bindings(pat: &Pat, out: &mut Vec<Ident>) {
    match pat {
        Pat::Ident(p) => {
            out.push(p.ident.clone());
            if let Some((_, sub)) = &p.subpat {
                pat_bindings(sub, out);
            }
        }
        Pat::Or(p) => p.cases.iter().for_each(|case| pat_bindings(case, out)),
        Pat::Paren(p) => pat_bindings(&p.pat, out),
        Pat::Reference(p) => pat_bindings(&p.pat, out),
        Pat::Slice(p) => p.elems.iter().for_each(|elem| pat_bindings(elem, out)),
        Pat::Struct(p) => p
            .fields
            .iter()
            .for_each(|field| pat_bindings(&field.pat, out)),
        Pat::Tuple(p) => p.elems.iter().for_each(|elem| pat_bindings(elem, out)),
        Pat::TupleStruct(p) => p.elems.iter().for_each(|elem| pat_bindings(elem, out)),
        Pat::Type(p) => pat_bindings(&p.pat, out),
        _ => {}
    }
}

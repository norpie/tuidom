//! The body rewrite behind `#[component(reactive)]`: signal names used as values become reads.
//!
//! Names are resolved lexically, the way the compiler would: a binding of the same name in an
//! inner scope hides a signal until that scope ends. Macro calls and nested items are skipped,
//! so `signal!(x)` passes its argument through untouched.

use std::collections::HashMap;

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{
    BinOp, Block, Expr, ExprMethodCall, ExprPath, FieldValue, Ident, Item, Local, Pat, Stmt, Token,
    Type, token,
};

/// Rewrite `block`, the body of a component whose props named `signals` are signals.
pub(crate) fn rewrite_body(block: &mut Block, signals: impl IntoIterator<Item = Ident>) {
    let props = signals.into_iter().map(|i| (i.to_string(), true)).collect();
    let mut rewriter = Rewriter {
        scopes: vec![props],
    };
    rewriter.visit_block_mut(block);
}

struct Rewriter {
    /// Innermost last. Each maps a bound name to whether it is a signal.
    scopes: Vec<HashMap<String, bool>>,
}

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

    fn names_signal(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Path(path) if self.signal_ident(path).is_some())
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
                    Stmt::Item(_) | Stmt::Macro(_) => {}
                }
            }
        });
    }

    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Path(path) => {
                if let Some(span) = self.signal_ident(path).map(Ident::span) {
                    *expr = read(path.clone(), span);
                }
            }
            // Calls and field access on a signal are left as written.
            Expr::MethodCall(call) if self.names_signal(&call.receiver) => {
                for arg in &mut call.args {
                    self.visit_expr_mut(arg);
                }
            }
            Expr::Field(field) if self.names_signal(&field.base) => {}
            Expr::Assign(assign) if self.names_signal(&assign.left) => {
                self.visit_expr_mut(&mut assign.right);
            }
            Expr::Binary(binary) if is_assign_op(&binary.op) && self.names_signal(&binary.left) => {
                self.visit_expr_mut(&mut binary.right);
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
            Expr::Macro(_) => {}
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

//! The abstract syntax tree: the shape of a parsed lux program.
//!
//! A program is a list of statements. Statements declare names, assign to
//! them, branch, loop, or evaluate an expression for its effect (like
//! `print`). Expressions produce values. Every node carries a `Span` so the
//! interpreter can blame the right place when something goes wrong.

use crate::diagnostic::Span;

/// A written type annotation, like the `int` in `var count: int`.
#[derive(Debug, Clone)]
pub struct TypeAnn {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let name = value` — an immutable binding.
    Let {
        name: String,
        ty: Option<TypeAnn>,
        value: Expr,
        span: Span,
    },
    /// `var name = value` or `var name: type` — a mutable binding.
    Var {
        name: String,
        ty: Option<TypeAnn>,
        value: Option<Expr>,
        span: Span,
    },
    /// `name = value`, `name += value`, `name -= value`.
    Assign {
        name: String,
        name_span: Span,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    /// `if cond { ... } else { ... }`. An `else if` is represented as an
    /// `else` body holding a single nested `If`.
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    /// `while cond { ... }`.
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// A bare expression run for its effect, like `print("hi")`.
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Ident(String, Span),
    Unary {
        op: UnOp,
        rhs: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// A function call. In v0.1 these are all built-ins: print, string, int,
    /// float. User-defined functions arrive in the next milestone.
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    /// The source span covering this whole expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Str(_, s)
            | Expr::Bool(_, s)
            | Expr::Ident(_, s) => *s,
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

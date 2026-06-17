//! The abstract syntax tree: the shape of a parsed lux program.
//!
//! A program is a list of statements. Statements declare names, assign to
//! them, branch, loop, define functions, return, or evaluate an expression for
//! its effect (like `print`). Expressions produce values. Every node carries a
//! `Span` so the interpreter can blame the right place when something goes
//! wrong.

use crate::diagnostic::Span;

/// A written type annotation, like the `int` in `var count: int` or the
/// `[int]` in `let primes: [int]`. Types nest, so this is recursive.
#[derive(Debug, Clone)]
pub struct TypeAnn {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    /// A plain named type: `int`, `float`, `string`, `bool`.
    Named(String),
    /// An array type: `[int]`, `[[string]]`.
    Array(Box<TypeAnn>),
}

/// One parameter in a function signature, like the `x: int` in `func f(x: int)`.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeAnn,
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
    /// `func name(params) -> ret { body }`. A missing `-> ret` means the
    /// function returns nothing.
    Func {
        name: String,
        params: Vec<Param>,
        ret: Option<TypeAnn>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `return` or `return value`.
    Return {
        value: Option<Expr>,
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
    /// `for var in iter { ... }`, where `iter` is an array or a range.
    For {
        var: String,
        iter: Expr,
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
    /// An array literal: `[2, 3, 5]`.
    Array(Vec<Expr>, Span),
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
    /// Reading an element by position: `xs[0]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// A half-open range: `0..5` means 0, 1, 2, 3, 4.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        span: Span,
    },
    /// A function call. Built-ins (print, string, int, float, length) and
    /// user-defined functions share this node.
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
            | Expr::Ident(_, s)
            | Expr::Array(_, s) => *s,
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Index { span, .. }
            | Expr::Range { span, .. }
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

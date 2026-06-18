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

/// One field in a struct, or one labelled value carried by an enum case, like
/// the `x: int` in `struct Point { x: int }` or the `radius: float` in
/// `circle(radius: float)`. Same shape as a parameter, but it names data rather
/// than an argument, so it gets its own type for clarity.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub ty: TypeAnn,
    pub span: Span,
}

/// One case of an enum, like `circle(radius: float)` or the payload-less `dot`.
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
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
    /// `struct Name { field: type ... }` — declares a record type.
    Struct {
        name: String,
        fields: Vec<FieldDef>,
        span: Span,
    },
    /// `enum Name { case ... }` — declares a type that is exactly one of a
    /// fixed set of cases, each of which may carry its own values.
    Enum {
        name: String,
        variants: Vec<VariantDef>,
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
    /// Building a struct by naming its fields: `Point(x: 0, y: 0)`.
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// Building an enum case: `Shape.circle(radius: 2.0)`. A payload-less case
    /// like `Shape.dot` parses as a `Field` and is resolved at run time.
    EnumLit {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// Reading a struct field with a dot: `origin.x`.
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    /// `match scrutinee { pattern => expr ... }`. An expression: it evaluates to
    /// the body of the one arm that matches.
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
}

/// One arm of a `match`: a pattern and the expression to evaluate when it fits.
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// What a `match` arm tests for.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_` — matches anything, binds nothing.
    Wildcard(Span),
    /// A literal value: `0`, `"hi"`, `true`.
    Int(i64, Span),
    Str(String, Span),
    Bool(bool, Span),
    /// An enum case, optionally capturing its values: `dot`, `circle(let r)`.
    Variant {
        name: String,
        bindings: Vec<String>,
        span: Span,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(s)
            | Pattern::Int(_, s)
            | Pattern::Str(_, s)
            | Pattern::Bool(_, s) => *s,
            Pattern::Variant { span, .. } => *span,
        }
    }
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
            | Expr::Call { span, .. }
            | Expr::StructLit { span, .. }
            | Expr::EnumLit { span, .. }
            | Expr::Field { span, .. }
            | Expr::Match { span, .. } => *span,
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

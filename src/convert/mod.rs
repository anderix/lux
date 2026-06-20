//! Translate a parsed lux program into a target language's source.
//!
//! Each backend — `rust`, `swift`, `go` — walks the same ast the interpreter
//! runs and emits idiomatic source for its target: `func` becomes `fn` / `func`,
//! lux's enums become Rust variants, Swift cases, or a Go interface, and the
//! top-level statements are wrapped in a `main`. The point is for a learner to
//! watch their own program turn into the language they're growing toward, so the
//! output is meant to be read.
//!
//! lux has no separate type checker yet, so to decide the handful of places
//! where the same lux syntax must emit different code — string `+` versus
//! numeric `+`, `length` on a string versus an array, how a value prints — the
//! shared `Types` below carries a small `type_of` that infers an expression's
//! type on demand from the declared signatures. It assumes a well-formed
//! program; the target compiler is the backstop for anything it can't see.

mod go;
mod rust;
mod swift;

pub use go::to_go;
pub use rust::to_rust;
pub use swift::to_swift;

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::Span;

/// A lux type, inferred during translation. `User` covers both structs and
/// enums (each backend emits them by name); `Unknown` is the honest answer when
/// a value doesn't pin its own type, like a bare `none`, and lets the target's
/// own inference take over.
#[derive(Clone, PartialEq)]
pub(crate) enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Array(Box<Ty>),
    User(String),
    Option(Box<Ty>),
    Result(Box<Ty>, Box<Ty>),
    Range,
    Unit,
    Unknown,
}

impl Ty {
    fn has_unknown(&self) -> bool {
        match self {
            Ty::Unknown => true,
            Ty::Array(t) | Ty::Option(t) => t.has_unknown(),
            Ty::Result(a, b) => a.has_unknown() || b.has_unknown(),
            _ => false,
        }
    }

    /// Does this type involve `int`? lux's `int` is 64-bit, but a bare Rust
    /// integer literal defaults to `i32`, so the Rust backend annotates any
    /// binding whose type involves an int to keep the two from drifting apart.
    fn has_int(&self) -> bool {
        match self {
            Ty::Int => true,
            Ty::Array(t) | Ty::Option(t) => t.has_int(),
            Ty::Result(a, b) => a.has_int() || b.has_int(),
            _ => false,
        }
    }

    /// Scalars print plainly; compound values need a debug-style format.
    fn is_scalar(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float | Ty::Str | Ty::Bool)
    }
}

/// Turn a written annotation into an inferred type. Struct and enum names both
/// land as `User`; the built-in generics are recognised by name.
fn ty_from_ann(a: &TypeAnn) -> Ty {
    match &a.kind {
        TypeKind::Named(n) => match n.as_str() {
            "int" => Ty::Int,
            "float" => Ty::Float,
            "string" => Ty::Str,
            "bool" => Ty::Bool,
            "Unit" => Ty::Unit,
            _ => Ty::User(n.clone()),
        },
        TypeKind::Array(inner) => Ty::Array(Box::new(ty_from_ann(inner))),
        TypeKind::Generic(name, args) => match (name.as_str(), args.as_slice()) {
            ("Option", [t]) => Ty::Option(Box::new(ty_from_ann(t))),
            ("Result", [a, b]) => Ty::Result(Box::new(ty_from_ann(a)), Box::new(ty_from_ann(b))),
            _ => Ty::Unknown,
        },
    }
}

/// What the translator knows about the program's declared names, gathered in
/// one pass up front so a call or field access can be typed wherever it appears.
pub(crate) struct Env {
    structs: HashMap<String, Vec<FieldDef>>,
    enums: HashMap<String, Vec<VariantDef>>,
    funcs: HashMap<String, (Vec<Param>, Option<TypeAnn>)>,
}

/// The declared names plus the running scope stack. Every backend shares this:
/// it tracks what's in scope as emission walks the tree, so `type_of` can answer
/// the type of any expression on demand. It does no emitting of its own.
pub(crate) struct Types {
    env: Env,
    scopes: Vec<HashMap<String, Ty>>,
}

impl Types {
    fn new(program: &[Stmt]) -> Self {
        let mut env = Env {
            structs: HashMap::new(),
            enums: HashMap::new(),
            funcs: HashMap::new(),
        };
        for stmt in program {
            match stmt {
                Stmt::Struct { name, fields, .. } => {
                    env.structs.insert(name.clone(), fields.clone());
                }
                Stmt::Enum { name, variants, .. } => {
                    env.enums.insert(name.clone(), variants.clone());
                }
                Stmt::Func {
                    name, params, ret, ..
                } => {
                    env.funcs
                        .insert(name.clone(), (params.clone(), ret.clone()));
                }
                _ => {}
            }
        }
        // `Output` is the built-in struct `run` returns. Registering its fields
        // here lets a field access like `result.status` type correctly in every
        // backend, the same as a struct the program declared itself.
        env.structs.insert("Output".to_string(), output_fields());
        Types {
            env,
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: String, ty: Ty) {
        self.scopes.last_mut().unwrap().insert(name, ty);
    }

    fn lookup(&self, name: &str) -> Ty {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return t.clone();
            }
        }
        Ty::Unknown
    }

    fn type_of(&self, e: &Expr) -> Ty {
        match e {
            Expr::Int(..) => Ty::Int,
            Expr::Float(..) => Ty::Float,
            Expr::Str(..) => Ty::Str,
            Expr::Bool(..) => Ty::Bool,
            Expr::Ident(name, _) => {
                if name == "none" {
                    Ty::Option(Box::new(Ty::Unknown))
                } else {
                    self.lookup(name)
                }
            }
            Expr::Array(els, _) => match els.first() {
                Some(first) => Ty::Array(Box::new(self.type_of(first))),
                None => Ty::Array(Box::new(Ty::Unknown)),
            },
            Expr::Unary { op, rhs, .. } => match op {
                UnOp::Neg => self.type_of(rhs),
                UnOp::Not => Ty::Bool,
            },
            Expr::Binary { op, lhs, .. } => match op {
                BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::Le
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or => Ty::Bool,
                _ => self.type_of(lhs),
            },
            Expr::Index { base, .. } => match self.type_of(base) {
                Ty::Array(t) => *t,
                other => other,
            },
            Expr::Range { .. } => Ty::Range,
            Expr::Call { name, args, .. } => self.call_type(name, args),
            Expr::StructLit { name, .. } => Ty::User(name.clone()),
            Expr::EnumLit { enum_name, .. } => Ty::User(enum_name.clone()),
            Expr::Field { base, field, .. } => self.field_type(base, field),
            Expr::Match { arms, .. } => arms
                .first()
                .map(|a| self.type_of(&a.body))
                .unwrap_or(Ty::Unknown),
        }
    }

    fn call_type(&self, name: &str, args: &[Expr]) -> Ty {
        match name {
            "print" => Ty::Unit,
            "string" => Ty::Str,
            "int" | "length" => Ty::Int,
            "float" => Ty::Float,
            "some" => Ty::Option(Box::new(self.type_of(&args[0]))),
            "ok" => Ty::Result(Box::new(self.type_of(&args[0])), Box::new(Ty::Unknown)),
            "err" => Ty::Result(Box::new(Ty::Unknown), Box::new(self.type_of(&args[0]))),
            // The outside world: each fallible call hands its failure back as a
            // value, so its type is what `match` reads to pick the right arms.
            "readFile" => Ty::Result(Box::new(Ty::Str), Box::new(Ty::Str)),
            "writeFile" => Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Str)),
            "args" => Ty::Array(Box::new(Ty::Str)),
            "readLine" => Ty::Option(Box::new(Ty::Str)),
            // Parsing text into a number can fail, so it answers with an Option.
            "parseInt" => Ty::Option(Box::new(Ty::Int)),
            "parseFloat" => Ty::Option(Box::new(Ty::Float)),
            "eprint" => Ty::Unit,
            // `run` is the one built-in that succeeds with a struct: the captured
            // status and streams, or a string reason it could not launch.
            "run" => Ty::Result(Box::new(Ty::User("Output".into())), Box::new(Ty::Str)),
            _ => match self.env.funcs.get(name) {
                Some((_, Some(ret))) => ty_from_ann(ret),
                Some((_, None)) => Ty::Unit,
                None => Ty::Unknown,
            },
        }
    }

    fn field_type(&self, base: &Expr, field: &str) -> Ty {
        if let Expr::Ident(n, _) = base
            && let Some(variants) = self.env.enums.get(n)
            && variants.iter().any(|v| v.name == *field)
        {
            return Ty::User(n.clone());
        }
        match self.type_of(base) {
            Ty::User(s) => self
                .env
                .structs
                .get(&s)
                .and_then(|fields| fields.iter().find(|f| f.name == *field))
                .map(|f| ty_from_ann(&f.ty))
                .unwrap_or(Ty::Unknown),
            _ => Ty::Unknown,
        }
    }
}

/// The fields of the built-in `Output` struct, in declared order, so every
/// backend types and emits `run`'s result identically.
fn output_fields() -> Vec<FieldDef> {
    let field = |name: &str, ty: &str| FieldDef {
        name: name.to_string(),
        ty: TypeAnn {
            kind: TypeKind::Named(ty.to_string()),
            span: Span::new(0, 0),
        },
        span: Span::new(0, 0),
    };
    vec![
        field("status", "int"),
        field("stdout", "string"),
        field("stderr", "string"),
    ]
}

// --- helpers shared across backends ----------------------------------------

/// Binding strength of a binary operator, loosest (`||`) to tightest (`*`).
/// Used to decide which operands actually need parentheses. The three targets
/// share C's precedence, so they share this table.
fn bin_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 3,
        BinOp::Add | BinOp::Sub => 4,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 5,
    }
}

/// The operator's source spelling — identical in Rust, Swift, and Go.
fn op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn indent(n: usize) -> String {
    "    ".repeat(n)
}

/// `firstEven` becomes `first_even` — lux's camelCase identifiers become
/// snake_case for the Rust backend's functions, variables, and fields.
fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `circle` becomes `Circle` — used for Rust's PascalCase enum variants and for
/// Go's per-case struct names.
fn to_pascal(s: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Escape a string's contents for a double-quoted literal. The three targets
/// share C's escape conventions for the characters lux can hold.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Render a float so it always carries a decimal point, the way a float literal
/// must in all three targets: `2.0`, not `2`.
fn format_float(f: f64) -> String {
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{}.0", s)
    }
}

/// A lux identifier that collides with a reserved word in the target language
/// gets a trailing `_` so the generated program still compiles — `where`
/// becomes `where_`, `go` becomes `go_`. lux's own keywords never reach here
/// (they aren't legal lux identifiers either), so each list below holds only the
/// target words lux does *not* itself reserve.
///
/// This guards *value* names: functions, parameters, and locals. Type names,
/// struct fields, and enum cases are left as written — a type called `map` is a
/// documented rough edge (see learn-lux.md's scope notes), not a supported name.
fn reserve(name: &str, words: &[&str]) -> String {
    if words.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Go keywords, plus the predeclared names the generated code itself relies on
/// (`append`, `len`, `ptr`, …) where a user function of the same name would
/// silently shadow the one the emitter emits.
const GO_RESERVED: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "append",
    "cap",
    "copy",
    "delete",
    "len",
    "make",
    "new",
    "panic",
    "recover",
    "ptr",
    "any",
    "nil",
    "iota",
];

fn go_ident(name: &str) -> String {
    reserve(name, GO_RESERVED)
}

/// Rust's strict and reserved keywords (2021 edition).
const RUST_RESERVED: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "union",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "gen", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

fn rust_ident(name: &str) -> String {
    reserve(name, RUST_RESERVED)
}

/// Swift's keywords (declaration, statement, and expression).
const SWIFT_RESERVED: &[&str] = &[
    "associatedtype",
    "class",
    "deinit",
    "enum",
    "extension",
    "fileprivate",
    "func",
    "import",
    "init",
    "inout",
    "internal",
    "let",
    "open",
    "operator",
    "private",
    "protocol",
    "public",
    "rethrows",
    "static",
    "struct",
    "subscript",
    "typealias",
    "var",
    "actor",
    "break",
    "case",
    "continue",
    "default",
    "defer",
    "do",
    "else",
    "fallthrough",
    "for",
    "guard",
    "if",
    "in",
    "repeat",
    "return",
    "switch",
    "where",
    "while",
    "as",
    "Any",
    "catch",
    "false",
    "is",
    "nil",
    "super",
    "self",
    "Self",
    "throw",
    "throws",
    "true",
    "try",
    "await",
    "async",
    "some",
    "any",
];

fn swift_ident(name: &str) -> String {
    reserve(name, SWIFT_RESERVED)
}

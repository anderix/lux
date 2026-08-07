//! Translate a parsed lux program into a target language's source.
//!
//! Each backend — `rust`, `swift`, `go` — walks the same ast the interpreter
//! runs and emits idiomatic source for its target: `func` becomes `fn` / `func`,
//! lux's enums become Rust variants, Swift cases, or a Go interface, and the
//! top-level statements are wrapped in a `main`. The point is for a learner to
//! watch their own program turn into the language they're growing toward, so the
//! output is meant to be read.
//!
//! To decide the handful of places where the same lux syntax must emit different
//! code — string `+` versus numeric `+`, `length` on a string versus an array,
//! how a value prints — the shared `Types` below carries a small `type_of` that
//! infers an expression's type on demand from the declared signatures. Emission
//! assumes a well-formed program, which the `typeck` pass — built on the same
//! `Types` — guarantees by running first: it applies the interpreter's
//! concrete-type rules to every path before any backend sees the program, so a
//! type error is a lux error here, not a cryptic one from the target compiler.

mod go;
mod rust;
mod swift;
mod typeck;

pub use go::to_go;
pub use rust::to_rust;
pub use swift::to_swift;
pub use typeck::check as type_check;

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::Span;

/// A lux type, inferred during translation. `User` covers both structs and
/// enums (each backend emits them by name); `Unknown` is the right answer when
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
    pub(crate) fn new(program: &[Stmt]) -> Self {
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

    /// Is `name` bound in any enclosing scope? Used by a backend that picks a
    /// scratch name and must not shadow a value the surrounding code still reads.
    fn in_scope(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains_key(name))
    }

    /// Does enum `from` reach enum `target` by following its variants' by-value
    /// field types through the enum graph? A field of type `from` inside `target`
    /// is then part of a cycle — `target -> from -> ... -> target` — that Rust must
    /// break with a `Box` and Swift with `indirect`, or the type has no finite
    /// size. Reflexive, so a directly self-referential field (`node(left: Tree)`)
    /// is covered by the same test as mutual recursion (`Expr` holding a `Fn` that
    /// holds an `Expr`). Only a direct named-enum field is an edge: an array or
    /// `Option` of an enum already carries its own indirection, so it doesn't
    /// propagate the cycle — the same fields the by-value checks always skipped.
    fn enum_reaches(&self, from: &str, target: &str) -> bool {
        let mut stack = vec![from.to_string()];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
            if cur == target {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(variants) = self.env.enums.get(&cur) {
                for v in variants {
                    for f in &v.fields {
                        if let Ty::User(n) = ty_from_ann(&f.ty) {
                            stack.push(n);
                        }
                    }
                }
            }
        }
        false
    }

    pub(crate) fn type_of(&self, e: &Expr) -> Ty {
        match e {
            Expr::Int(..) => Ty::Int,
            Expr::Float(..) => Ty::Float,
            Expr::Str(..) => Ty::Str,
            Expr::Bool(..) => Ty::Bool,
            Expr::Ident(name, _) => {
                // `none` names the empty `Option` — unless the program bound it as
                // an ordinary variable, in which case the local wins, the same way
                // it does for every other built-in name (#19).
                if name == "none" && !self.in_scope("none") {
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
            // Every arm of a match yields the same type, so prefer the first arm
            // whose type is fully known: an arm like `some(let v) => some(v)` reads
            // as `Option<?>` because the binding isn't in scope for this pass, while
            // a sibling `none => findCoin(rest)` carries the concrete `Option<int>`.
            Expr::Match { arms, .. } => arms
                .iter()
                .map(|a| self.type_of(&a.body))
                .find(|t| !t.has_unknown())
                .or_else(|| arms.first().map(|a| self.type_of(&a.body)))
                .unwrap_or(Ty::Unknown),
        }
    }

    fn call_type(&self, name: &str, args: &[Expr]) -> Ty {
        match name {
            "print" => Ty::Unit,
            "string" => Ty::Str,
            "int" | "length" => Ty::Int,
            "float" => Ty::Float,
            // String operations, siblings of `length`. `split` is the first
            // built-in to hand back an array. A user function of the same name wins,
            // since these are plain verbs a learner readily names their own list or
            // tree helper — so their type comes from the declaration, not here.
            "contains" => Ty::Bool,
            "replace" => Ty::Str,
            "split" => Ty::Array(Box::new(Ty::Str)),
            "some" => Ty::Option(Box::new(self.type_of(&args[0]))),
            "ok" => Ty::Result(Box::new(self.type_of(&args[0])), Box::new(Ty::Unknown)),
            "err" => Ty::Result(Box::new(Ty::Unknown), Box::new(self.type_of(&args[0]))),
            // The outside world: each fallible call hands its failure back as a
            // value, so its type is what `match` reads to pick the right arms.
            "readFile" => Ty::Result(Box::new(Ty::Str), Box::new(Ty::Str)),
            "writeFile" => Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Str)),
            "args" => Ty::Array(Box::new(Ty::Str)),
            "readLine" => Ty::Option(Box::new(Ty::Str)),
            // `input` collapses the end-of-input case into an empty string, so it
            // hands back a plain string rather than the Option `readLine` gives.
            "input" => Ty::Str,
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

/// A generated type-level name — an injected trait or protocol — chosen not to
/// collide with a type the program declared. Start from `base` and append `_`
/// until it clears every struct and enum name, the way Go's scratch names dodge a
/// binding. Rust and Swift inject a `LuxShow` type for compound printing; a learner
/// who names a struct `LuxShow` should never have to know the prelude wanted that
/// name, so the prelude's copy steps aside instead (#37).
fn dodge_type_name(base: &str, program: &[Stmt]) -> String {
    let taken: std::collections::HashSet<&str> = program
        .iter()
        .filter_map(|s| match s {
            Stmt::Struct { name, .. } | Stmt::Enum { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut n = base.to_string();
    while taken.contains(n.as_str()) {
        n.push('_');
    }
    n
}

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
/// This guards the identifiers an author writes that the emitter reproduces:
/// functions, parameters, locals, struct fields, and enum payload labels (#77). A
/// field named `move` or `type` is escaped like any other name, while the label
/// lux prints stays the one the author wrote. Type names are left as written — a
/// type called `map` is a documented rough edge (see learn-lux.md's scope notes),
/// not a supported name, and PascalCasing keeps a type clear of the lowercase
/// keywords anyway. Enum case names are left as written too, except in Swift, which
/// alone emits the bare lowercase case name and so backtick-quotes a keyword
/// collision (see `swift_case`); Go and Rust PascalCase and qualify their cases,
/// which sidesteps the problem.
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

/// A Swift enum case name, backtick-quoted when it collides with a keyword so the
/// generated Swift still reads with the name lux gave it — `.`nil`` rather than a
/// mangled one. Swift is the only backend that needs this: Go and Rust PascalCase
/// and qualify a case (`TreeNil`, `Tree::Nil`), which can't collide with a
/// lowercase keyword, while Swift emits the bare `.nil`.
fn swift_case(name: &str) -> String {
    if SWIFT_RESERVED.contains(&name) {
        format!("`{name}`")
    } else {
        name.to_string()
    }
}

/// Names ever used as the root of an assignment target — a plain rebind, or a
/// write through a field or index — within one function's scope. A `var` whose
/// name never appears here is never mutated, so Rust and Swift can bind it
/// immutably (`let`) and stay warning-clean; Go doesn't care either way. Called
/// per function body (and once over the top-level statements for a generated
/// `main`), so nested function definitions are not descended into: a local named
/// `out` that one function mutates must not force another function's own `out` to
/// carry an unused `mut`. Shadowing within a scope is ignored, which is safe: a
/// name shared between a mutated and an unmutated binding at worst keeps the
/// mutable keyword and its warning, never the reverse.
fn mutated_roots(program: &[Stmt]) -> std::collections::HashSet<String> {
    fn walk(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
        for s in stmts {
            match s {
                Stmt::Assign { target, .. } => {
                    if let Some(root) = target.place_root() {
                        out.insert(root.to_string());
                    }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } => walk(body, out),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, out);
                    if let Some(e) = else_body {
                        walk(e, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(program, &mut out);
    out
}

/// Does `name` appear as an identifier anywhere in this expression? Used to decide
/// whether a match arm actually reads the payload it binds. Go rejects an unused
/// local outright; Rust and Swift only warn, but the backends' bar is warning-
/// clean output, so all three drop a binding the arm never reads. The check is
/// deliberately conservative: a name shadowed by an inner binding still counts as
/// "used", so at worst it keeps a binding that was safe to keep — it never drops
/// one the body relies on.
/// A "place" — a named value that could still be used after it's read, as opposed
/// to a fresh temporary like a literal or a call result. Rust clones a place to
/// avoid a move; Go deep-copies one whose type holds a slice, to keep lux's value
/// semantics. A fresh temporary already owns its storage, so neither touches it.
fn is_place(e: &Expr, t: &Types) -> bool {
    match e {
        // A bare `none` is the empty `Option` literal — a fresh temporary, not a
        // place — unless the program bound `none` as an ordinary variable, in which
        // case it is one and carries value semantics like any other name (#19).
        Expr::Ident(n, _) => n != "none" || t.in_scope("none"),
        Expr::Field { .. } | Expr::Index { .. } => true,
        _ => false,
    }
}

fn expr_mentions(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n, _) => n == name,
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) => false,
        Expr::Array(items, _) => items.iter().any(|x| expr_mentions(x, name)),
        Expr::Unary { rhs, .. } => expr_mentions(rhs, name),
        Expr::Binary { lhs, rhs, .. } => expr_mentions(lhs, name) || expr_mentions(rhs, name),
        Expr::Index { base, index, .. } => expr_mentions(base, name) || expr_mentions(index, name),
        Expr::Range { start, end, .. } => expr_mentions(start, name) || expr_mentions(end, name),
        Expr::Call { args, .. } => args.iter().any(|a| expr_mentions(a, name)),
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            fields.iter().any(|(_, v)| expr_mentions(v, name))
        }
        Expr::Field { base, .. } => expr_mentions(base, name),
        Expr::Match {
            scrutinee, arms, ..
        } => expr_mentions(scrutinee, name) || arms.iter().any(|a| expr_mentions(&a.body, name)),
    }
}

/// Does `name` appear anywhere in this block of statements? The `for`-body
/// analogue of `expr_mentions`, walking each statement's expressions and nested
/// bodies. A loop variable the body never reads is emitted as `_` so Rust and
/// Swift stay warning-clean; Go's counted loop already reads its variable in the
/// loop's own condition, so it never warns. Conservative in the same way — a
/// name shadowed by an inner binding still counts as used, so it never drops a
/// binding the body relies on.
fn stmts_mention(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| stmt_mentions(s, name))
}

fn stmt_mentions(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Let { value, .. } => expr_mentions(value, name),
        Stmt::Var { value, .. } => value.as_ref().is_some_and(|v| expr_mentions(v, name)),
        Stmt::Assign { target, value, .. } => {
            expr_mentions(target, name) || expr_mentions(value, name)
        }
        Stmt::Return { value, .. } => value.as_ref().is_some_and(|v| expr_mentions(v, name)),
        Stmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            expr_mentions(cond, name)
                || stmts_mention(then_body, name)
                || else_body.as_deref().is_some_and(|e| stmts_mention(e, name))
        }
        Stmt::While { cond, body, .. } => expr_mentions(cond, name) || stmts_mention(body, name),
        Stmt::For { iter, body, .. } => expr_mentions(iter, name) || stmts_mention(body, name),
        Stmt::Expr(e) => expr_mentions(e, name),
        Stmt::Func { .. } | Stmt::Struct { .. } | Stmt::Enum { .. } => false,
    }
}

//! Translate a parsed lux program into idiomatic Rust source.
//!
//! This is the first transpiler backend. It walks the same ast the interpreter
//! runs and emits real Rust: `func` becomes `fn`, lux's lowercase enum cases
//! become Rust's `PascalCase` variants, `Option`/`Result` map straight onto the
//! standard library, and the top-level statements are wrapped in a `fn main`.
//! The point is for a learner to watch their own program turn into the language
//! they're growing toward, so the output is meant to be read.
//!
//! lux has no separate type checker yet, so to decide the handful of places
//! where the same lux syntax must emit different Rust — string `+` versus
//! numeric `+`, `length` on a string versus an array, `print` formatting — this
//! module carries a small `type_of` that infers an expression's type on demand
//! from the declared signatures. It assumes a well-formed program; rustc is the
//! backstop for anything it can't see.

use std::collections::HashMap;

use crate::ast::*;

/// A lux type, inferred during translation. `User` covers both structs and
/// enums (they emit the same way — by name); `Unknown` is the honest answer
/// when a value doesn't pin its own type, like a bare `none`, and lets Rust's
/// own inference take over.
#[derive(Clone, PartialEq)]
enum Ty {
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

    /// Does this type involve `int`? lux's `int` is `i64`, but a bare Rust
    /// integer literal defaults to `i32`, so any binding whose type involves an
    /// int gets an explicit annotation to keep the two from drifting apart.
    fn has_int(&self) -> bool {
        match self {
            Ty::Int => true,
            Ty::Array(t) | Ty::Option(t) => t.has_int(),
            Ty::Result(a, b) => a.has_int() || b.has_int(),
            _ => false,
        }
    }

    /// Scalars print with `{}`; everything else needs `{:?}`.
    fn is_scalar(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float | Ty::Str | Ty::Bool)
    }

    fn rust(&self) -> String {
        match self {
            Ty::Int => "i64".into(),
            Ty::Float => "f64".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "bool".into(),
            Ty::Array(t) => format!("Vec<{}>", t.rust()),
            Ty::User(n) => n.clone(),
            Ty::Option(t) => format!("Option<{}>", t.rust()),
            Ty::Result(a, b) => format!("Result<{}, {}>", a.rust(), b.rust()),
            Ty::Range => "std::ops::Range<i64>".into(),
            Ty::Unit => "()".into(),
            Ty::Unknown => "_".into(),
        }
    }

    fn zero(&self) -> String {
        match self {
            Ty::Int => "0".into(),
            Ty::Float => "0.0".into(),
            Ty::Bool => "false".into(),
            Ty::Str => "String::new()".into(),
            Ty::Array(_) => "Vec::new()".into(),
            Ty::Option(_) => "None".into(),
            _ => "Default::default()".into(),
        }
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
struct Env {
    structs: HashMap<String, Vec<FieldDef>>,
    enums: HashMap<String, Vec<VariantDef>>,
    funcs: HashMap<String, (Vec<Param>, Option<TypeAnn>)>,
}

struct Gen {
    env: Env,
    scopes: Vec<HashMap<String, Ty>>,
    out: String,
    indent: usize,
}

/// Translate a whole program to Rust source text.
pub fn to_rust(program: &[Stmt]) -> String {
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
                env.funcs.insert(name.clone(), (params.clone(), ret.clone()));
            }
            _ => {}
        }
    }

    let mut g = Gen {
        env,
        scopes: vec![HashMap::new()],
        out: String::new(),
        indent: 0,
    };

    for stmt in program {
        if let Stmt::Struct { name, fields, .. } = stmt {
            g.emit_struct(name, fields);
        }
    }
    for stmt in program {
        if let Stmt::Enum { name, variants, .. } = stmt {
            g.emit_enum(name, variants);
        }
    }
    for stmt in program {
        if let Stmt::Func {
            name,
            params,
            ret,
            body,
            ..
        } = stmt
        {
            g.emit_func(name, params, ret.as_ref(), body);
        }
    }

    g.line("fn main() {".into());
    g.indent += 1;
    g.push_scope();
    for stmt in program {
        if !matches!(
            stmt,
            Stmt::Struct { .. } | Stmt::Enum { .. } | Stmt::Func { .. }
        ) {
            g.emit_stmt(stmt);
        }
    }
    g.pop_scope();
    g.indent -= 1;
    g.line("}".into());

    g.out
}

impl Gen {
    fn line(&mut self, s: String) {
        self.out.push_str(&indent(self.indent));
        self.out.push_str(&s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
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

    // --- declarations ------------------------------------------------------

    fn emit_struct(&mut self, name: &str, fields: &[FieldDef]) {
        self.line("#[derive(Debug, Clone, PartialEq)]".into());
        self.line(format!("struct {} {{", name));
        for f in fields {
            self.line(format!("    {}: {},", to_snake(&f.name), ty_from_ann(&f.ty).rust()));
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_enum(&mut self, name: &str, variants: &[VariantDef]) {
        self.line("#[derive(Debug, Clone, PartialEq)]".into());
        self.line(format!("enum {} {{", name));
        for v in variants {
            if v.fields.is_empty() {
                self.line(format!("    {},", to_pascal(&v.name)));
            } else {
                let tys: Vec<String> = v.fields.iter().map(|f| ty_from_ann(&f.ty).rust()).collect();
                self.line(format!("    {}({}),", to_pascal(&v.name), tys.join(", ")));
            }
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        let ps: Vec<String> = params
            .iter()
            .map(|p| format!("{}: {}", to_snake(&p.name), ty_from_ann(&p.ty).rust()))
            .collect();
        let r = ret
            .map(|t| format!(" -> {}", ty_from_ann(t).rust()))
            .unwrap_or_default();
        self.line(format!("fn {}({}){} {{", to_snake(name), ps.join(", "), r));
        self.indent += 1;
        self.push_scope();
        for p in params {
            self.declare(p.name.clone(), ty_from_ann(&p.ty));
        }
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.pop_scope();
        self.indent -= 1;
        self.line("}".into());
        self.blank();
    }

    // --- statements --------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value, .. } => self.emit_binding(name, ty.as_ref(), value, false),
            Stmt::Var {
                name,
                ty,
                value: Some(value),
                ..
            } => self.emit_binding(name, ty.as_ref(), value, true),
            Stmt::Var {
                name,
                ty: Some(ann),
                value: None,
                ..
            } => {
                let vty = ty_from_ann(ann);
                let zero = vty.zero();
                let snake = to_snake(name);
                self.declare(name.clone(), vty.clone());
                self.line(format!("let mut {}: {} = {};", snake, vty.rust(), zero));
            }
            Stmt::Var { value: None, .. } => {} // a var with neither type nor value can't occur
            Stmt::Assign { name, op, value, .. } => self.emit_assign(name, *op, value),
            Stmt::Return { value, .. } => match value {
                Some(v) => {
                    let e = self.emit_expr(v);
                    self.line(format!("return {};", e));
                }
                None => self.line("return;".into()),
            },
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => self.emit_if(cond, then_body, else_body.as_deref()),
            Stmt::While { cond, body, .. } => {
                let c = self.emit_expr(cond);
                self.line(format!("while {} {{", c));
                self.block(body);
                self.line("}".into());
            }
            Stmt::For { var, iter, body, .. } => self.emit_for(var, iter, body),
            Stmt::Expr(e) => {
                let s = self.emit_expr(e);
                self.line(format!("{};", s));
            }
            // Declarations are hoisted to module scope in the top-level pass.
            Stmt::Func { name, params, ret, body, .. } => {
                self.emit_func(name, params, ret.as_ref(), body)
            }
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }

    /// A block that owns its own scope, indented one level.
    fn block(&mut self, body: &[Stmt]) {
        self.indent += 1;
        self.push_scope();
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.pop_scope();
        self.indent -= 1;
    }

    fn emit_binding(&mut self, name: &str, ann: Option<&TypeAnn>, value: &Expr, mutable: bool) {
        let snake = to_snake(name);
        let vty = ann.map(ty_from_ann).unwrap_or_else(|| self.type_of(value));
        // A bare `none` (or anything that leaves its type open) carries no type
        // of its own, so when the source named one, write it down for Rust.
        let value_open = self.type_of(value).has_unknown();
        let annotate = !vty.has_unknown() && ((ann.is_some() && value_open) || vty.has_int());
        let kw = if mutable { "let mut" } else { "let" };
        let expr = self.emit_expr(value);
        if annotate {
            self.line(format!("{} {}: {} = {};", kw, snake, vty.rust(), expr));
        } else {
            self.line(format!("{} {} = {};", kw, snake, expr));
        }
        self.declare(name.to_string(), vty);
    }

    fn emit_assign(&mut self, name: &str, op: AssignOp, value: &Expr) {
        let snake = to_snake(name);
        let lty = self.lookup(name);
        match op {
            AssignOp::Set => {
                let e = self.emit_expr(value);
                self.line(format!("{} = {};", snake, e));
            }
            AssignOp::Add => match lty {
                Ty::Str => {
                    // lux `+=` on a string appends text.
                    if let Expr::Str(s, _) = value {
                        self.line(format!("{}.push_str(\"{}\");", snake, escape(s)));
                    } else {
                        let e = self.emit_expr(value);
                        self.line(format!("{}.push_str(&{});", snake, e));
                    }
                }
                Ty::Array(_) => {
                    // lux `+=` on an array appends one element.
                    let e = self.emit_expr(value);
                    self.line(format!("{}.push({});", snake, e));
                }
                _ => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} += {};", snake, e));
                }
            },
            AssignOp::Sub => {
                let e = self.emit_expr(value);
                self.line(format!("{} -= {};", snake, e));
            }
        }
    }

    fn emit_if(&mut self, cond: &Expr, then_body: &[Stmt], mut els: Option<&[Stmt]>) {
        let c = self.emit_expr(cond);
        self.line(format!("if {} {{", c));
        self.block(then_body);
        loop {
            match els {
                None => {
                    self.line("}".into());
                    break;
                }
                // A lone nested `if` is an `else if` — chain it on one line.
                Some(e) if e.len() == 1 && matches!(e[0], Stmt::If { .. }) => {
                    if let Stmt::If {
                        cond,
                        then_body,
                        else_body,
                        ..
                    } = &e[0]
                    {
                        let c = self.emit_expr(cond);
                        self.line(format!("}} else if {} {{", c));
                        self.block(then_body);
                        els = else_body.as_deref();
                    }
                }
                Some(e) => {
                    self.line("} else {".into());
                    self.block(e);
                    self.line("}".into());
                    break;
                }
            }
        }
    }

    fn emit_for(&mut self, var: &str, iter: &Expr, body: &[Stmt]) {
        let svar = to_snake(var);
        let (iter_str, elem_ty) = match self.type_of(iter) {
            Ty::Range => (self.emit_expr(iter), Ty::Int),
            Ty::Array(t) => {
                let base = self.emit_expr(iter);
                // Borrow the array and clone each element, so the loop never
                // consumes what it walks and the body gets owned values.
                (format!("{}.iter().cloned()", base), *t)
            }
            _ => (self.emit_expr(iter), Ty::Unknown),
        };
        self.line(format!("for {} in {} {{", svar, iter_str));
        self.indent += 1;
        self.push_scope();
        self.declare(var.to_string(), elem_ty);
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.pop_scope();
        self.indent -= 1;
        self.line("}".into());
    }

    // --- expressions -------------------------------------------------------

    fn emit_expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Int(n, _) => n.to_string(),
            Expr::Float(f, _) => format_float(*f),
            Expr::Str(s, _) => format!("\"{}\".to_string()", escape(s)),
            Expr::Bool(b, _) => b.to_string(),
            Expr::Ident(name, _) => {
                if name == "none" {
                    "None".to_string()
                } else {
                    to_snake(name)
                }
            }
            Expr::Array(els, _) => {
                let parts: Vec<String> = els.iter().map(|x| self.emit_expr(x)).collect();
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Unary { op, rhs, .. } => {
                // Unary binds tighter than any binary operator, so a binary
                // operand needs parentheses: `-(a + b)`, not `-a + b`.
                let r = if matches!(**rhs, Expr::Binary { .. }) {
                    let inner = self.emit_expr(rhs);
                    format!("({})", inner)
                } else {
                    self.emit_expr(rhs)
                };
                match op {
                    UnOp::Neg => format!("-{}", r),
                    UnOp::Not => format!("!{}", r),
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                if *op == BinOp::Add && self.type_of(lhs) == Ty::Str {
                    let l = self.display_arg(lhs);
                    let r = self.display_arg(rhs);
                    format!("format!(\"{{}}{{}}\", {}, {})", l, r)
                } else {
                    let p = bin_prec(*op);
                    let l = self.emit_child(lhs, p, false);
                    let r = self.emit_child(rhs, p, true);
                    format!("{} {} {}", l, op_str(*op), r)
                }
            }
            Expr::Index { base, index, .. } => {
                let b = self.emit_expr(base);
                let idx = if let Expr::Int(n, _) = **index {
                    n.to_string()
                } else {
                    let e = self.emit_expr(index);
                    format!("({}) as usize", e)
                };
                format!("{}[{}]", b, idx)
            }
            Expr::Range { start, end, .. } => {
                let s = self.emit_expr(start);
                let e = self.emit_expr(end);
                format!("{}..{}", s, e)
            }
            Expr::Call { name, args, .. } => self.emit_call(name, args),
            Expr::StructLit { name, fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| {
                        let val = self.emit_expr(v);
                        format!("{}: {}", to_snake(k), val)
                    })
                    .collect();
                format!("{} {{ {} }}", name, parts.join(", "))
            }
            Expr::EnumLit {
                enum_name,
                variant,
                fields,
                ..
            } => self.emit_enum_lit(enum_name, variant, fields),
            Expr::Field { base, field, .. } => {
                // `Shape.dot` parses as a field access but is a payload-less
                // enum case — emit it as construction.
                if let Expr::Ident(n, _) = &**base
                    && let Some(variants) = self.env.enums.get(n)
                    && variants.iter().any(|v| v.name == *field)
                {
                    return format!("{}::{}", n, to_pascal(field));
                }
                let b = self.emit_expr(base);
                format!("{}.{}", b, to_snake(field))
            }
            Expr::Match { scrutinee, arms, .. } => self.emit_match(scrutinee, arms),
        }
    }

    /// Emit a binary operand, wrapping it in parentheses only when its operator
    /// binds more loosely than the parent's. The right operand of a left-
    /// associative operator also needs them at equal precedence (`a - (b - c)`).
    fn emit_child(&mut self, e: &Expr, parent: u8, is_right: bool) -> String {
        let s = self.emit_expr(e);
        if let Expr::Binary { op, .. } = e {
            let p = bin_prec(*op);
            let wrap = if is_right { p <= parent } else { p < parent };
            if wrap {
                return format!("({})", s);
            }
        }
        s
    }

    /// A user-function argument. lux passes values by copy and the caller keeps
    /// its own, so a named value of a non-`Copy` type is cloned at the call
    /// site to preserve that — exactly what lux does under the hood.
    fn emit_call_arg(&mut self, a: &Expr) -> String {
        let clone = !is_copy(&self.type_of(a)) && is_place(a);
        let s = self.emit_expr(a);
        if clone {
            format!("{}.clone()", s)
        } else {
            s
        }
    }

    /// An argument in `print` or string concatenation, where a bare string
    /// literal can stay a clean `&str` instead of an owned `String`.
    fn display_arg(&mut self, e: &Expr) -> String {
        if let Expr::Str(s, _) = e {
            format!("\"{}\"", escape(s))
        } else {
            self.emit_expr(e)
        }
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => {
                let mut fmt = String::new();
                let mut parts = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        fmt.push(' ');
                    }
                    // `{:?}` on an f64 keeps the decimal point (`9.0`), matching
                    // how lux prints floats; plain scalars use `{}`.
                    let ty = self.type_of(a);
                    fmt.push_str(if ty == Ty::Float || !ty.is_scalar() {
                        "{:?}"
                    } else {
                        "{}"
                    });
                    parts.push(self.display_arg(a));
                }
                if parts.is_empty() {
                    "println!()".to_string()
                } else {
                    format!("println!(\"{}\", {})", fmt, parts.join(", "))
                }
            }
            "string" => {
                // `{:?}` keeps a whole float's decimal point, the way lux's
                // `string(2.0)` yields "2.0" rather than "2".
                let is_float = self.type_of(&args[0]) == Ty::Float;
                let e = self.emit_expr(&args[0]);
                if is_float {
                    format!("format!(\"{{:?}}\", {})", e)
                } else {
                    format!("({}).to_string()", e)
                }
            }
            "int" => {
                let inner = self.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Str => format!("({}).parse::<i64>().unwrap()", e),
                    Ty::Float => format!("({}) as i64", e),
                    Ty::Int => e,
                    _ => format!("({}) as i64", e),
                }
            }
            "float" => {
                let inner = self.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Int => format!("({}) as f64", e),
                    Ty::Str => format!("({}).parse::<f64>().unwrap()", e),
                    Ty::Float => e,
                    _ => format!("({}) as f64", e),
                }
            }
            "length" => {
                let inner = self.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                if inner == Ty::Str {
                    format!("({}).chars().count() as i64", e)
                } else {
                    format!("({}).len() as i64", e)
                }
            }
            "some" => {
                let e = self.emit_expr(&args[0]);
                format!("Some({})", e)
            }
            "ok" => {
                let e = self.emit_expr(&args[0]);
                format!("Ok({})", e)
            }
            "err" => {
                let e = self.emit_expr(&args[0]);
                format!("Err({})", e)
            }
            _ => {
                let parts: Vec<String> = args.iter().map(|a| self.emit_call_arg(a)).collect();
                format!("{}({})", to_snake(name), parts.join(", "))
            }
        }
    }

    fn emit_enum_lit(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, Expr)],
    ) -> String {
        // Tuple variants are positional, so emit the values in the order the
        // enum declared its fields, not the order they were written.
        let order: Option<Vec<String>> = self.env.enums.get(enum_name).and_then(|variants| {
            variants
                .iter()
                .find(|v| v.name == variant)
                .map(|v| v.fields.iter().map(|f| f.name.clone()).collect())
        });
        let args: Vec<String> = match order {
            Some(names) => names
                .iter()
                .map(|fname| {
                    let expr = fields.iter().find(|(k, _)| k == fname).map(|(_, e)| e);
                    match expr {
                        Some(e) => self.emit_expr(e),
                        None => "()".to_string(),
                    }
                })
                .collect(),
            None => fields.iter().map(|(_, e)| self.emit_expr(e)).collect(),
        };
        if args.is_empty() {
            format!("{}::{}", enum_name, to_pascal(variant))
        } else {
            format!("{}::{}({})", enum_name, to_pascal(variant), args.join(", "))
        }
    }

    fn emit_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> String {
        let base = self.indent;
        let ind = indent(base);
        let ind1 = indent(base + 1);
        let st = self.type_of(scrutinee);
        let needs_as_str = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Str(..)));
        let scrut = if needs_as_str {
            let s = self.emit_expr(scrutinee);
            format!("{}.as_str()", s)
        } else {
            self.emit_expr(scrutinee)
        };
        let mut s = format!("match {} {{\n", scrut);
        for arm in arms {
            let pat = self.emit_pattern(&arm.pattern, &st);
            // Bring the pattern's captures into scope so the arm body types
            // correctly (a captured string should print without quotes).
            self.push_scope();
            self.declare_bindings(&arm.pattern, &st);
            // Arm bodies sit one level in, so a nested match nests cleanly.
            self.indent = base + 1;
            let body = self.emit_expr(&arm.body);
            self.indent = base;
            self.pop_scope();
            s.push_str(&format!("{}{} => {},\n", ind1, pat, body));
        }
        s.push_str(&format!("{}}}", ind));
        s
    }

    fn emit_pattern(&mut self, pat: &Pattern, st: &Ty) -> String {
        match pat {
            Pattern::Wildcard(_) => "_".to_string(),
            Pattern::Int(n, _) => n.to_string(),
            Pattern::Str(s, _) => format!("\"{}\"", escape(s)),
            Pattern::Bool(b, _) => b.to_string(),
            Pattern::Variant { name, bindings, .. } => {
                let binds: Vec<String> = bindings.iter().map(|b| to_snake(b)).collect();
                let inner = if binds.is_empty() {
                    String::new()
                } else {
                    format!("({})", binds.join(", "))
                };
                match st {
                    Ty::Option(_) => match name.as_str() {
                        "some" => format!("Some{}", paren_or_empty(&binds)),
                        _ => "None".to_string(),
                    },
                    Ty::Result(_, _) => match name.as_str() {
                        "ok" => format!("Ok{}", paren_or_empty(&binds)),
                        _ => format!("Err{}", paren_or_empty(&binds)),
                    },
                    Ty::User(en) => format!("{}::{}{}", en, to_pascal(name), inner),
                    _ => format!("{}{}", to_pascal(name), inner),
                }
            }
        }
    }

    /// Record the types of a pattern's captures in the current scope.
    fn declare_bindings(&mut self, pat: &Pattern, st: &Ty) {
        let Pattern::Variant { name, bindings, .. } = pat else {
            return;
        };
        let types: Vec<Ty> = match st {
            Ty::Option(t) if name == "some" => vec![(**t).clone()],
            Ty::Result(o, _) if name == "ok" => vec![(**o).clone()],
            Ty::Result(_, e) if name == "err" => vec![(**e).clone()],
            Ty::User(en) => self
                .env
                .enums
                .get(en)
                .and_then(|vs| vs.iter().find(|v| v.name == *name))
                .map(|v| v.fields.iter().map(|f| ty_from_ann(&f.ty)).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for (b, t) in bindings.iter().zip(types) {
            self.declare(b.clone(), t);
        }
    }

    // --- type inference ----------------------------------------------------

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
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
                | BinOp::And | BinOp::Or => Ty::Bool,
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

fn paren_or_empty(binds: &[String]) -> String {
    if binds.is_empty() {
        String::new()
    } else {
        format!("({})", binds.join(", "))
    }
}

/// Binding strength of a binary operator, loosest (`||`) to tightest (`*`).
/// Used to decide which operands actually need parentheses.
fn bin_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 3,
        BinOp::Add | BinOp::Sub => 4,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 5,
    }
}

/// Types that copy on use in Rust, so passing them never moves the original.
fn is_copy(t: &Ty) -> bool {
    matches!(t, Ty::Int | Ty::Float | Ty::Bool)
}

/// A "place" — a named value that could still be used after a call, as opposed
/// to a fresh temporary like a literal or another call's result.
fn is_place(e: &Expr) -> bool {
    match e {
        Expr::Ident(n, _) => n != "none",
        Expr::Field { .. } | Expr::Index { .. } => true,
        _ => false,
    }
}

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

/// `firstEven` becomes `first_even` — lux's camelCase identifiers become Rust's
/// snake_case for functions, variables, and fields.
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

/// `circle` becomes `Circle` — lux's lowercase enum cases become Rust's
/// PascalCase variants.
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

/// Render a float so it always carries a decimal point, the way a Rust `f64`
/// literal must: `2.0`, not `2`.
fn format_float(f: f64) -> String {
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{}.0", s)
    }
}

//! The Rust backend: emit real Rust source.
//!
//! Rust is the closest match to lux's shape — enums with values, `Option` and
//! `Result`, exhaustive `match`, value semantics — so most of the work is
//! cosmetic: `func` becomes `fn`, lux's lowercase enum cases become PascalCase
//! variants, camelCase names become snake_case. The one real wrinkle is that
//! Rust *moves* a non-`Copy` value when you pass it, while lux copies, so a
//! named place argument is cloned at the call site to preserve lux's semantics.

use crate::ast::*;

use super::{
    bin_prec, escape, format_float, indent, op_str, to_pascal, to_snake, ty_from_ann, Ty, Types,
};

struct Gen {
    t: Types,
    out: String,
    indent: usize,
}

/// Translate a whole program to Rust source text.
pub fn to_rust(program: &[Stmt]) -> String {
    let mut g = Gen {
        t: Types::new(program),
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
    g.t.push_scope();
    for stmt in program {
        if !matches!(
            stmt,
            Stmt::Struct { .. } | Stmt::Enum { .. } | Stmt::Func { .. }
        ) {
            g.emit_stmt(stmt);
        }
    }
    g.t.pop_scope();
    g.indent -= 1;
    g.line("}".into());

    g.out
}

/// A lux type as Rust source text.
fn ty_text(t: &Ty) -> String {
    match t {
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "bool".into(),
        Ty::Array(t) => format!("Vec<{}>", ty_text(t)),
        Ty::User(n) => n.clone(),
        Ty::Option(t) => format!("Option<{}>", ty_text(t)),
        Ty::Result(a, b) => format!("Result<{}, {}>", ty_text(a), ty_text(b)),
        Ty::Range => "std::ops::Range<i64>".into(),
        Ty::Unit => "()".into(),
        Ty::Unknown => "_".into(),
    }
}

/// The natural empty value for a `var` that was declared without one.
fn zero(t: &Ty) -> String {
    match t {
        Ty::Int => "0".into(),
        Ty::Float => "0.0".into(),
        Ty::Bool => "false".into(),
        Ty::Str => "String::new()".into(),
        Ty::Array(_) => "Vec::new()".into(),
        Ty::Option(_) => "None".into(),
        _ => "Default::default()".into(),
    }
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

    // --- declarations ------------------------------------------------------

    fn emit_struct(&mut self, name: &str, fields: &[FieldDef]) {
        self.line("#[derive(Debug, Clone, PartialEq)]".into());
        self.line(format!("struct {} {{", name));
        for f in fields {
            self.line(format!(
                "    {}: {},",
                to_snake(&f.name),
                ty_text(&ty_from_ann(&f.ty))
            ));
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
                let tys: Vec<String> =
                    v.fields.iter().map(|f| ty_text(&ty_from_ann(&f.ty))).collect();
                self.line(format!("    {}({}),", to_pascal(&v.name), tys.join(", ")));
            }
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        let ps: Vec<String> = params
            .iter()
            .map(|p| format!("{}: {}", to_snake(&p.name), ty_text(&ty_from_ann(&p.ty))))
            .collect();
        let r = ret
            .map(|t| format!(" -> {}", ty_text(&ty_from_ann(t))))
            .unwrap_or_default();
        self.line(format!("fn {}({}){} {{", to_snake(name), ps.join(", "), r));
        self.indent += 1;
        self.t.push_scope();
        for p in params {
            self.t.declare(p.name.clone(), ty_from_ann(&p.ty));
        }
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.t.pop_scope();
        self.indent -= 1;
        self.line("}".into());
        self.blank();
    }

    // --- statements --------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name, ty, value, ..
            } => self.emit_binding(name, ty.as_ref(), value, false),
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
                let z = zero(&vty);
                let snake = to_snake(name);
                self.t.declare(name.clone(), vty.clone());
                self.line(format!("let mut {}: {} = {};", snake, ty_text(&vty), z));
            }
            Stmt::Var { value: None, .. } => {} // a var with neither type nor value can't occur
            Stmt::Assign {
                name, op, value, ..
            } => self.emit_assign(name, *op, value),
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
            Stmt::For {
                var, iter, body, ..
            } => self.emit_for(var, iter, body),
            Stmt::Expr(e) => {
                let s = self.emit_expr(e);
                self.line(format!("{};", s));
            }
            // Declarations are hoisted to module scope in the top-level pass.
            Stmt::Func {
                name,
                params,
                ret,
                body,
                ..
            } => self.emit_func(name, params, ret.as_ref(), body),
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }

    /// A block that owns its own scope, indented one level.
    fn block(&mut self, body: &[Stmt]) {
        self.indent += 1;
        self.t.push_scope();
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.t.pop_scope();
        self.indent -= 1;
    }

    fn emit_binding(&mut self, name: &str, ann: Option<&TypeAnn>, value: &Expr, mutable: bool) {
        let snake = to_snake(name);
        let vty = ann.map(ty_from_ann).unwrap_or_else(|| self.t.type_of(value));
        // A bare `none` (or anything that leaves its type open) carries no type
        // of its own, so when the source named one, write it down for Rust.
        let value_open = self.t.type_of(value).has_unknown();
        let annotate = !vty.has_unknown() && ((ann.is_some() && value_open) || vty.has_int());
        let kw = if mutable { "let mut" } else { "let" };
        let expr = self.emit_expr(value);
        if annotate {
            self.line(format!("{} {}: {} = {};", kw, snake, ty_text(&vty), expr));
        } else {
            self.line(format!("{} {} = {};", kw, snake, expr));
        }
        self.t.declare(name.to_string(), vty);
    }

    fn emit_assign(&mut self, name: &str, op: AssignOp, value: &Expr) {
        let snake = to_snake(name);
        let lty = self.t.lookup(name);
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
        let (iter_str, elem_ty) = match self.t.type_of(iter) {
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
        self.t.push_scope();
        self.t.declare(var.to_string(), elem_ty);
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.t.pop_scope();
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
                if *op == BinOp::Add && self.t.type_of(lhs) == Ty::Str {
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
                    && let Some(variants) = self.t.env.enums.get(n)
                    && variants.iter().any(|v| v.name == *field)
                {
                    return format!("{}::{}", n, to_pascal(field));
                }
                let b = self.emit_expr(base);
                format!("{}.{}", b, to_snake(field))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => self.emit_match(scrutinee, arms),
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
        let clone = !is_copy(&self.t.type_of(a)) && is_place(a);
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
                    let ty = self.t.type_of(a);
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
                let is_float = self.t.type_of(&args[0]) == Ty::Float;
                let e = self.emit_expr(&args[0]);
                if is_float {
                    format!("format!(\"{{:?}}\", {})", e)
                } else {
                    format!("({}).to_string()", e)
                }
            }
            "int" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Str => format!("({}).parse::<i64>().unwrap()", e),
                    Ty::Float => format!("({}) as i64", e),
                    Ty::Int => e,
                    _ => format!("({}) as i64", e),
                }
            }
            "float" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Int => format!("({}) as f64", e),
                    Ty::Str => format!("({}).parse::<f64>().unwrap()", e),
                    Ty::Float => e,
                    _ => format!("({}) as f64", e),
                }
            }
            "length" => {
                let inner = self.t.type_of(&args[0]);
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

    fn emit_enum_lit(&mut self, enum_name: &str, variant: &str, fields: &[(String, Expr)]) -> String {
        // Tuple variants are positional, so emit the values in the order the
        // enum declared its fields, not the order they were written.
        let order: Option<Vec<String>> = self.t.env.enums.get(enum_name).and_then(|variants| {
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
        let st = self.t.type_of(scrutinee);
        let needs_as_str = arms.iter().any(|a| matches!(a.pattern, Pattern::Str(..)));
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
            self.t.push_scope();
            self.declare_bindings(&arm.pattern, &st);
            // Arm bodies sit one level in, so a nested match nests cleanly.
            self.indent = base + 1;
            let body = self.emit_expr(&arm.body);
            self.indent = base;
            self.t.pop_scope();
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
                .t
                .env
                .enums
                .get(en)
                .and_then(|vs| vs.iter().find(|v| v.name == *name))
                .map(|v| v.fields.iter().map(|f| ty_from_ann(&f.ty)).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for (b, t) in bindings.iter().zip(types) {
            self.t.declare(b.clone(), t);
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

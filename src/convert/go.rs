//! The Go backend: emit real Go source.
//!
//! Go is the target furthest from lux's shape, which is exactly the lesson: down
//! the ladder you keep the same ideas but rebuild a few conveniences by hand.
//! lux has none of Go's gaps, so this backend is where the translation does the
//! most work. Enums with associated values have no Go equivalent, so each
//! becomes an interface plus one struct per case, taken apart with a type
//! switch. lux's `match` is an expression but Go's `switch` is a statement, so a
//! returning match pushes its `return` into every arm. `Option<T>` becomes a
//! pointer (`nil` is none) and `Result<T, E>` becomes Go's `(value, error)`
//! pair, the way the standard library returns them.
//!
//! Two seams are worth naming. lux is immutable by default; Go's `const` only
//! holds compile-time constants, so that distinction is dropped here — `let` and
//! `var` both become `:=`. And `fmt.Println` renders a whole float as `9`, not
//! `9.0`, and a struct as `{3 4}`; the values are identical, only the rendering
//! is Go's own.

use crate::ast::*;

use super::{bin_prec, escape, format_float, op_str, to_pascal, ty_from_ann, Ty, Types};

struct Gen {
    t: Types,
    out: String,
    indent: usize,
    /// The enclosing function's return type, so a `return ok(..)` knows to emit
    /// Go's `value, nil` pair.
    ret: Option<Ty>,
    uses_fmt: bool,
    uses_errors: bool,
    uses_ptr: bool,
    uses_os: bool,
    uses_bufio: bool,
    uses_strings: bool,
    /// The outside-world helpers: each adapts Go's standard-library shape to the
    /// `(value, error)` pair lux's `Result` lowers to, emitted only when used.
    uses_read_file: bool,
    uses_write_file: bool,
    uses_read_line: bool,
}

/// Translate a whole program to Go source text.
pub fn to_go(program: &[Stmt]) -> String {
    let mut g = Gen {
        t: Types::new(program),
        out: String::new(),
        indent: 0,
        ret: None,
        uses_fmt: false,
        uses_errors: false,
        uses_ptr: false,
        uses_os: false,
        uses_bufio: false,
        uses_strings: false,
        uses_read_file: false,
        uses_write_file: false,
        uses_read_line: false,
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

    g.line("func main() {".into());
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

    g.assemble()
}

/// A lux type as Go source text. `Result` only ever reaches here as a function
/// return, where it expands to the `(value, error)` pair Go uses.
fn ty_text(t: &Ty) -> String {
    match t {
        Ty::Int => "int".into(),
        Ty::Float => "float64".into(),
        Ty::Str => "string".into(),
        Ty::Bool => "bool".into(),
        Ty::Array(t) => format!("[]{}", ty_text(t)),
        Ty::User(n) => n.clone(),
        Ty::Option(t) => format!("*{}", ty_text(t)),
        // A `Result` whose success carries nothing is an operation that can only
        // fail, so Go returns just an `error`, the way the standard library does.
        Ty::Result(a, _) => match a.as_ref() {
            Ty::Unit => "error".into(),
            _ => format!("({}, error)", ty_text(a)),
        },
        Ty::Range => "int".into(),
        Ty::Unit => String::new(),
        Ty::Unknown => "any".into(),
    }
}

/// Go's zero value for a type, used to fill the value slot of a failing
/// `(value, error)` return.
fn zero(t: &Ty) -> String {
    match t {
        Ty::Int | Ty::Float => "0".into(),
        Ty::Bool => "false".into(),
        Ty::Str => "\"\"".into(),
        Ty::User(n) => format!("{}{{}}", n),
        _ => "nil".into(),
    }
}

impl Gen {
    fn line(&mut self, s: String) {
        for _ in 0..self.indent {
            self.out.push('\t');
        }
        self.out.push_str(&s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    /// Wrap the emitted declarations in a package clause, the imports actually
    /// used, and any helper the program leans on.
    fn assemble(&self) -> String {
        let mut head = String::from("package main\n\n");
        // Go orders imports alphabetically; gofmt would anyway, so list them so.
        let mut imports: Vec<&str> = Vec::new();
        if self.uses_bufio {
            imports.push("bufio");
        }
        if self.uses_errors {
            imports.push("errors");
        }
        if self.uses_fmt {
            imports.push("fmt");
        }
        if self.uses_os {
            imports.push("os");
        }
        if self.uses_strings {
            imports.push("strings");
        }
        match imports.len() {
            0 => {}
            1 => head.push_str(&format!("import \"{}\"\n\n", imports[0])),
            _ => {
                head.push_str("import (\n");
                for i in &imports {
                    head.push_str(&format!("\t\"{}\"\n", i));
                }
                head.push_str(")\n\n");
            }
        }
        if self.uses_ptr {
            // Go has no literal for "a pointer to this value", so the some(...)
            // case borrows one through a tiny generic helper.
            head.push_str("func ptr[T any](v T) *T {\n\treturn &v\n}\n\n");
        }
        if self.uses_read_file {
            // os.ReadFile hands back bytes; lux reads a string, so decode here.
            head.push_str(
                "func readFile(path string) (string, error) {\n\
                 \tdata, err := os.ReadFile(path)\n\
                 \treturn string(data), err\n\
                 }\n\n",
            );
        }
        if self.uses_write_file {
            head.push_str(
                "func writeFile(path string, contents string) error {\n\
                 \treturn os.WriteFile(path, []byte(contents), 0644)\n\
                 }\n\n",
            );
        }
        if self.uses_read_line {
            // One reader, made once and kept, so a loop never drops buffered
            // input between calls. nil means end of input.
            head.push_str("var stdin = bufio.NewReader(os.Stdin)\n\n");
            head.push_str(
                "func readLine() *string {\n\
                 \tline, err := stdin.ReadString('\\n')\n\
                 \tif err != nil && line == \"\" {\n\
                 \t\treturn nil\n\
                 \t}\n\
                 \tline = strings.TrimRight(line, \"\\r\\n\")\n\
                 \treturn &line\n\
                 }\n\n",
            );
        }
        head.push_str(&self.out);
        head
    }

    // --- declarations ------------------------------------------------------

    fn emit_struct(&mut self, name: &str, fields: &[FieldDef]) {
        self.line(format!("type {} struct {{", name));
        self.emit_fields(fields);
        self.line("}".into());
        self.blank();
    }

    /// Emit struct fields with their type columns aligned, the way `gofmt` lays
    /// them out: each name is padded to the widest in the group.
    fn emit_fields(&mut self, fields: &[FieldDef]) {
        let w = fields.iter().map(|f| f.name.len()).max().unwrap_or(0);
        for f in fields {
            self.line(format!(
                "\t{:w$} {}",
                f.name,
                ty_text(&ty_from_ann(&f.ty)),
                w = w
            ));
        }
    }

    /// An enum has no Go equivalent, so it becomes a marker interface and one
    /// struct per case — the standard way to fake a sum type.
    fn emit_enum(&mut self, name: &str, variants: &[VariantDef]) {
        let marker = format!("is{}", name);
        self.line(format!("type {} interface{{ {}() }}", name, marker));
        self.blank();
        for v in variants {
            let case = format!("{}{}", name, to_pascal(&v.name));
            if v.fields.is_empty() {
                self.line(format!("type {} struct{{}}", case));
            } else {
                self.line(format!("type {} struct {{", case));
                self.emit_fields(&v.fields);
                self.line("}".into());
            }
            self.blank();
            self.line(format!("func ({}) {}() {{}}", case, marker));
            self.blank();
        }
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        let ps: Vec<String> = params
            .iter()
            .map(|p| format!("{} {}", p.name, ty_text(&ty_from_ann(&p.ty))))
            .collect();
        let rty = ret.map(ty_from_ann);
        let rtext = match &rty {
            None | Some(Ty::Unit) => String::new(),
            Some(t) => format!(" {}", ty_text(t)),
        };
        self.line(format!("func {}({}){} {{", name, ps.join(", "), rtext));
        self.indent += 1;
        self.t.push_scope();
        let saved = self.ret.take();
        self.ret = rty;
        for p in params {
            self.t.declare(p.name.clone(), ty_from_ann(&p.ty));
        }
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.ret = saved;
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
            } => self.emit_binding(name, ty.as_ref(), value),
            Stmt::Var {
                name,
                ty,
                value: Some(value),
                ..
            } => self.emit_binding(name, ty.as_ref(), value),
            Stmt::Var {
                name,
                ty: Some(ann),
                value: None,
                ..
            } => {
                let vty = ty_from_ann(ann);
                self.t.declare(name.clone(), vty.clone());
                // Go zero-initialises a plain `var`, so no value is needed.
                self.line(format!("var {} {}", name, ty_text(&vty)));
            }
            Stmt::Var { value: None, .. } => {}
            Stmt::Assign {
                name, op, value, ..
            } => self.emit_assign(name, *op, value),
            Stmt::Return { value, .. } => self.emit_return(value.as_ref()),
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => self.emit_if(cond, then_body, else_body.as_deref()),
            Stmt::While { cond, body, .. } => {
                let c = self.emit_expr(cond);
                self.line(format!("for {} {{", c));
                self.block(body);
                self.line("}".into());
            }
            Stmt::For {
                var, iter, body, ..
            } => self.emit_for(var, iter, body),
            Stmt::Expr(Expr::Match {
                scrutinee, arms, ..
            }) => self.emit_match(scrutinee, arms, false),
            Stmt::Expr(e) => {
                let s = self.emit_expr(e);
                self.line(s);
            }
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

    fn block(&mut self, body: &[Stmt]) {
        self.indent += 1;
        self.t.push_scope();
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.t.pop_scope();
        self.indent -= 1;
    }

    fn emit_return(&mut self, value: Option<&Expr>) {
        let Some(v) = value else {
            self.line("return".into());
            return;
        };
        // A function returning Result pairs its value with an error.
        if let Some(Ty::Result(t, _)) = self.ret.clone()
            && let Expr::Call { name, args, .. } = v
        {
            match name.as_str() {
                "ok" => {
                    let e = self.emit_expr(&args[0]);
                    self.line(format!("return {}, nil", e));
                    return;
                }
                "err" => {
                    let e = self.emit_expr(&args[0]);
                    self.uses_errors = true;
                    self.line(format!("return {}, errors.New({})", zero(&t), e));
                    return;
                }
                _ => {}
            }
        }
        if let Expr::Match {
            scrutinee, arms, ..
        } = v
        {
            self.emit_match(scrutinee, arms, true);
            return;
        }
        let e = self.emit_expr(v);
        self.line(format!("return {}", e));
    }

    fn emit_binding(&mut self, name: &str, ann: Option<&TypeAnn>, value: &Expr) {
        let vty = ann.map(ty_from_ann).unwrap_or_else(|| self.t.type_of(value));
        let expr = self.emit_expr(value);
        self.line(format!("{} := {}", name, expr));
        self.t.declare(name.to_string(), vty);
    }

    fn emit_assign(&mut self, name: &str, op: AssignOp, value: &Expr) {
        let lty = self.t.lookup(name);
        match op {
            AssignOp::Set => {
                let e = self.emit_expr(value);
                self.line(format!("{} = {}", name, e));
            }
            AssignOp::Add => match lty {
                // lux `+=` on an array appends one element.
                Ty::Array(_) => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} = append({}, {})", name, name, e));
                }
                // Strings and numbers both take Go's `+=` directly.
                _ => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} += {}", name, e));
                }
            },
            AssignOp::Sub => {
                let e = self.emit_expr(value);
                self.line(format!("{} -= {}", name, e));
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
        let (header, elem_ty) = match self.t.type_of(iter) {
            // A range becomes a counted loop; lux ranges are end-exclusive.
            Ty::Range => {
                if let Expr::Range { start, end, .. } = iter {
                    let s = self.emit_expr(start);
                    let e = self.emit_expr(end);
                    (
                        format!("for {} := {}; {} < {}; {}++ {{", var, s, var, e, var),
                        Ty::Int,
                    )
                } else {
                    let it = self.emit_expr(iter);
                    (format!("for _, {} := range {} {{", var, it), Ty::Unknown)
                }
            }
            Ty::Array(t) => {
                let it = self.emit_expr(iter);
                (format!("for _, {} := range {} {{", var, it), *t)
            }
            _ => {
                let it = self.emit_expr(iter);
                (format!("for _, {} := range {} {{", var, it), Ty::Unknown)
            }
        };
        self.line(header);
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

    // --- match -------------------------------------------------------------

    /// Lower a `match` to whatever Go shape fits the scrutinee: a type switch
    /// for an enum, a nil check for an `Option`, an error check for a `Result`,
    /// or a plain value switch otherwise.
    fn emit_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        match self.t.type_of(scrutinee) {
            Ty::Option(_) => self.emit_match_option(scrutinee, arms, ret),
            Ty::Result(..) => self.emit_match_result(scrutinee, arms, ret),
            Ty::User(en) if self.t.env.enums.contains_key(&en) => {
                self.emit_match_enum(scrutinee, &en, arms, ret)
            }
            _ => self.emit_match_value(scrutinee, arms, ret),
        }
    }

    /// The body of one arm, either run for effect or turned into a `return`.
    fn emit_arm_body(&mut self, body: &Expr, ret: bool) {
        match body {
            Expr::Match {
                scrutinee, arms, ..
            } => self.emit_match(scrutinee, arms, ret),
            _ => {
                let e = self.emit_expr(body);
                self.line(if ret { format!("return {}", e) } else { e });
            }
        }
    }

    fn emit_match_value(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        let s = self.emit_expr(scrutinee);
        let has_default = arms.iter().any(|a| matches!(a.pattern, Pattern::Wildcard(_)));
        self.line(format!("switch {} {{", s));
        for arm in arms {
            let label = match &arm.pattern {
                Pattern::Wildcard(_) => "default".to_string(),
                Pattern::Int(n, _) => format!("case {}", n),
                Pattern::Str(s, _) => format!("case \"{}\"", escape(s)),
                Pattern::Bool(b, _) => format!("case {}", b),
                Pattern::Variant { name, .. } => format!("case {}", name),
            };
            self.line(format!("{}:", label));
            self.indent += 1;
            self.emit_arm_body(&arm.body, ret);
            self.indent -= 1;
        }
        self.line("}".into());
        // A switch without a default isn't a terminating statement to Go, so a
        // returning one needs an unreachable tail to satisfy the compiler.
        if ret && !has_default {
            self.line("panic(\"unreachable\")".into());
        }
    }

    fn emit_match_enum(&mut self, scrutinee: &Expr, enum_name: &str, arms: &[MatchArm], ret: bool) {
        let s = self.emit_expr(scrutinee);
        let any_bind = arms.iter().any(
            |a| matches!(&a.pattern, Pattern::Variant { bindings, .. } if !bindings.is_empty()),
        );
        let head = if any_bind {
            format!("switch v := {}.(type) {{", s)
        } else {
            format!("switch {}.(type) {{", s)
        };
        self.line(head);
        for arm in arms {
            let Pattern::Variant { name, bindings, .. } = &arm.pattern else {
                continue;
            };
            let case = format!("{}{}", enum_name, to_pascal(name));
            self.line(format!("case {}:", case));
            self.indent += 1;
            self.t.push_scope();
            // Pull each captured value out of the case struct by its field name.
            let field_names: Vec<String> = self
                .t
                .env
                .enums
                .get(enum_name)
                .and_then(|vs| vs.iter().find(|v| v.name == *name))
                .map(|v| v.fields.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default();
            self.declare_variant_bindings(enum_name, name, bindings);
            for (b, fname) in bindings.iter().zip(&field_names) {
                self.line(format!("{} := v.{}", b, fname));
            }
            self.emit_arm_body(&arm.body, ret);
            self.t.pop_scope();
            self.indent -= 1;
        }
        self.line("}".into());
        // The type switch lists every case but Go can't see that, so a returning
        // one needs an unreachable tail.
        if ret {
            self.line("panic(\"unreachable\")".into());
        }
    }

    fn emit_match_option(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        let inner = match self.t.type_of(scrutinee) {
            Ty::Option(t) => *t,
            _ => Ty::Unknown,
        };
        let some_arm = arms.iter().find(|a| arm_name(a) == Some("some"));
        let none_arm = arms.iter().find(|a| arm_name(a) == Some("none"));
        let bind = some_arm.and_then(|a| match &a.pattern {
            Pattern::Variant { bindings, .. } => bindings.first().cloned(),
            _ => None,
        });
        let ptr = bind
            .as_ref()
            .map(|b| format!("{}Opt", b))
            .unwrap_or_else(|| "opt".to_string());
        let s = self.emit_expr(scrutinee);
        self.line(format!("if {} := {}; {} != nil {{", ptr, s, ptr));
        self.indent += 1;
        self.t.push_scope();
        if let Some(b) = &bind {
            self.t.declare(b.clone(), inner);
            self.line(format!("{} := *{}", b, ptr));
        }
        if let Some(a) = some_arm {
            self.emit_arm_body(&a.body, ret);
        }
        self.t.pop_scope();
        self.indent -= 1;
        self.line("} else {".into());
        self.indent += 1;
        if let Some(a) = none_arm {
            self.emit_arm_body(&a.body, ret);
        }
        self.indent -= 1;
        self.line("}".into());
    }

    fn emit_match_result(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        let (ok_ty, err_ty) = match self.t.type_of(scrutinee) {
            Ty::Result(o, e) => (*o, *e),
            _ => (Ty::Unknown, Ty::Unknown),
        };
        let ok_arm = arms.iter().find(|a| arm_name(a) == Some("ok"));
        let err_arm = arms.iter().find(|a| arm_name(a) == Some("err"));
        let ok_bind = ok_arm.and_then(|a| match &a.pattern {
            Pattern::Variant { bindings, .. } => bindings.first().cloned(),
            _ => None,
        });
        let err_bind = err_arm.and_then(|a| match &a.pattern {
            Pattern::Variant { bindings, .. } => bindings.first().cloned(),
            _ => None,
        });
        let s = self.emit_expr(scrutinee);
        // An if-init scopes the value and error to this match, so two reads in
        // one block don't collide on the names. A success that carries nothing
        // leaves only the error to bind.
        if ok_ty == Ty::Unit {
            self.line(format!("if err := {}; err == nil {{", s));
        } else {
            let lhs = ok_bind.clone().unwrap_or_else(|| "_".to_string());
            self.line(format!("if {}, err := {}; err == nil {{", lhs, s));
        }
        self.indent += 1;
        self.t.push_scope();
        if let Some(b) = &ok_bind {
            self.t.declare(b.clone(), ok_ty);
        }
        if let Some(a) = ok_arm {
            self.emit_arm_body(&a.body, ret);
        }
        self.t.pop_scope();
        self.indent -= 1;
        self.line("} else {".into());
        self.indent += 1;
        self.t.push_scope();
        if let Some(b) = &err_bind {
            // lux carries the reason as a string; Go's error gives it back.
            self.t.declare(b.clone(), err_ty);
            self.line(format!("{} := err.Error()", b));
        }
        if let Some(a) = err_arm {
            self.emit_arm_body(&a.body, ret);
        }
        self.t.pop_scope();
        self.indent -= 1;
        self.line("}".into());
    }

    fn declare_variant_bindings(&mut self, enum_name: &str, variant: &str, bindings: &[String]) {
        let types: Vec<Ty> = self
            .t
            .env
            .enums
            .get(enum_name)
            .and_then(|vs| vs.iter().find(|v| v.name == variant))
            .map(|v| v.fields.iter().map(|f| ty_from_ann(&f.ty)).collect())
            .unwrap_or_default();
        for (b, t) in bindings.iter().zip(types) {
            self.t.declare(b.clone(), t);
        }
    }

    // --- expressions -------------------------------------------------------

    fn emit_expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Int(n, _) => n.to_string(),
            Expr::Float(f, _) => format_float(*f),
            Expr::Str(s, _) => format!("\"{}\"", escape(s)),
            Expr::Bool(b, _) => b.to_string(),
            Expr::Ident(name, _) => {
                if name == "none" {
                    "nil".to_string()
                } else {
                    name.clone()
                }
            }
            Expr::Array(els, _) => {
                let et = match els.first() {
                    Some(first) => ty_text(&self.t.type_of(first)),
                    None => "any".to_string(),
                };
                let parts: Vec<String> = els.iter().map(|x| self.emit_expr(x)).collect();
                format!("[]{}{{{}}}", et, parts.join(", "))
            }
            Expr::Unary { op, rhs, .. } => {
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
            // Go's `+` already concatenates strings, so string and numeric `+`
            // need no distinction here.
            Expr::Binary { op, lhs, rhs, .. } => {
                let p = bin_prec(*op);
                let l = self.emit_child(lhs, p, false);
                let r = self.emit_child(rhs, p, true);
                format!("{} {} {}", l, op_str(*op), r)
            }
            Expr::Index { base, index, .. } => {
                let b = self.emit_expr(base);
                let idx = self.emit_expr(index);
                format!("{}[{}]", b, idx)
            }
            Expr::Range { start, end, .. } => {
                // A bare range only reaches here outside a `for`; Go has no range
                // value, so emit its bounds for whatever context wrapped it.
                let s = self.emit_expr(start);
                let e = self.emit_expr(end);
                format!("{}, {}", s, e)
            }
            Expr::Call { name, args, .. } => self.emit_call(name, args),
            Expr::StructLit { name, fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| {
                        let val = self.emit_expr(v);
                        format!("{}: {}", k, val)
                    })
                    .collect();
                format!("{}{{{}}}", name, parts.join(", "))
            }
            Expr::EnumLit {
                enum_name,
                variant,
                fields,
                ..
            } => self.emit_enum_lit(enum_name, variant, fields),
            Expr::Field { base, field, .. } => {
                if let Expr::Ident(n, _) = &**base
                    && let Some(variants) = self.t.env.enums.get(n)
                    && variants.iter().any(|v| v.name == *field)
                {
                    return format!("{}{}{{}}", n, to_pascal(field));
                }
                let b = self.emit_expr(base);
                format!("{}.{}", b, field)
            }
            // A match used as a value, which lux's examples never do; a closure
            // keeps it translatable.
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let rt = ty_text(&self.t.type_of(e));
                let body = self.match_to_string(scrutinee, arms);
                let mut close = String::new();
                for _ in 0..self.indent {
                    close.push('\t');
                }
                format!("func() {} {{\n{}{}}}()", rt, body, close)
            }
        }
    }

    fn match_to_string(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> String {
        let saved = std::mem::take(&mut self.out);
        self.indent += 1;
        self.emit_match(scrutinee, arms, true);
        self.indent -= 1;
        std::mem::replace(&mut self.out, saved)
    }

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

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => {
                self.uses_fmt = true;
                let parts: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("fmt.Println({})", parts.join(", "))
            }
            "eprint" => {
                self.uses_fmt = true;
                self.uses_os = true;
                let mut parts = vec!["os.Stderr".to_string()];
                parts.extend(args.iter().map(|a| self.emit_expr(a)));
                format!("fmt.Fprintln({})", parts.join(", "))
            }
            // The outside-world calls lower to package-level helpers (assembled
            // above); naming them here records which ones the program needs.
            "readFile" => {
                self.uses_read_file = true;
                self.uses_os = true;
                let p = self.emit_expr(&args[0]);
                format!("readFile({})", p)
            }
            "writeFile" => {
                self.uses_write_file = true;
                self.uses_os = true;
                let p = self.emit_expr(&args[0]);
                let c = self.emit_expr(&args[1]);
                format!("writeFile({}, {})", p, c)
            }
            "args" => {
                self.uses_os = true;
                "os.Args".to_string()
            }
            "readLine" => {
                self.uses_read_line = true;
                self.uses_os = true;
                self.uses_bufio = true;
                self.uses_strings = true;
                "readLine()".to_string()
            }
            "string" => {
                // `%v` is Go's general rendering; it keeps int and bool exact and
                // matches Go's own (decimal-less) take on whole floats.
                self.uses_fmt = true;
                let e = self.emit_expr(&args[0]);
                format!("fmt.Sprintf(\"%v\", {})", e)
            }
            "int" => {
                // Go conversions truncate a float and pass an int through.
                let e = self.emit_expr(&args[0]);
                format!("int({})", e)
            }
            "float" => {
                let e = self.emit_expr(&args[0]);
                format!("float64({})", e)
            }
            "length" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                if inner == Ty::Str {
                    // lux counts characters, so count runes rather than bytes.
                    format!("len([]rune({}))", e)
                } else {
                    format!("len({})", e)
                }
            }
            "some" => {
                self.uses_ptr = true;
                let e = self.emit_expr(&args[0]);
                format!("ptr({})", e)
            }
            // ok/err in a return are handled there; reaching here is degenerate.
            "ok" | "err" => self.emit_expr(&args[0]),
            _ => {
                let parts: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{}({})", name, parts.join(", "))
            }
        }
    }

    fn emit_enum_lit(&mut self, enum_name: &str, variant: &str, fields: &[(String, Expr)]) -> String {
        let case = format!("{}{}", enum_name, to_pascal(variant));
        if fields.is_empty() {
            format!("{}{{}}", case)
        } else {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, e)| format!("{}: {}", k, self.emit_expr(e)))
                .collect();
            format!("{}{{{}}}", case, parts.join(", "))
        }
    }
}

/// The case name of a variant pattern, for matching `some`/`none`/`ok`/`err`.
fn arm_name(arm: &MatchArm) -> Option<&str> {
    match &arm.pattern {
        Pattern::Variant { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

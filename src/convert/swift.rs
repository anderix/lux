//! The Swift backend: emit real Swift source.
//!
//! Swift is the closest fit of the three targets. It shares lux's value
//! semantics (structs, enums, and arrays copy, so there is no clone dance),
//! enums with associated values, a native `Optional`, and a `print` that already
//! renders floats and structs the way lux does. lux's camelCase is already
//! Swift's house style, so almost nothing is renamed. The two adjustments worth
//! naming: every function parameter gets an `_` label so calls stay positional
//! like lux's, and `match` becomes a `switch` whose arms each `return` (rather
//! than a switch-expression) so the output works on any modern Swift.
//!
//! lux's `Result<T, string>` is the one rough edge: Swift's `Result` requires
//! its error type to be an `Error`, and `String` isn't one by default, so when a
//! string-carrying `Result` appears we emit a one-line retroactive conformance.

use crate::ast::*;

use super::{Ty, Types, bin_prec, escape, format_float, indent, op_str, ty_from_ann};

struct Gen {
    t: Types,
    out: String,
    indent: usize,
    /// The outside-world helpers, emitted only when used. Swift reads files by
    /// throwing, so each wraps a `do`/`catch` back into the `Result` lux uses.
    uses_read_file: bool,
    uses_write_file: bool,
    uses_eprint: bool,
    /// `run` needs Foundation's `Process`, the built-in `Output` struct, and the
    /// `String: Error` conformance its `Result` shares with the file helpers.
    uses_run: bool,
}

/// Translate a whole program to Swift source text.
pub fn to_swift(program: &[Stmt]) -> String {
    let mut g = Gen {
        t: Types::new(program),
        out: String::new(),
        indent: 0,
        uses_read_file: false,
        uses_write_file: false,
        uses_eprint: false,
        uses_run: false,
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

    // Swift's top level is the program's entry point, and global functions and
    // types are visible regardless of order, so the statements run as written.
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

    g.assemble(program)
}

/// A lux type as Swift source text.
fn ty_text(t: &Ty) -> String {
    match t {
        Ty::Int => "Int".into(),
        Ty::Float => "Double".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "Bool".into(),
        Ty::Array(t) => format!("[{}]", ty_text(t)),
        Ty::User(n) => n.clone(),
        Ty::Option(t) => format!("{}?", ty_text(t)),
        Ty::Result(a, b) => format!("Result<{}, {}>", ty_text(a), ty_text(b)),
        Ty::Range => "Range<Int>".into(),
        Ty::Unit => "Void".into(),
        Ty::Unknown => "Any".into(),
    }
}

/// The natural empty value for a `var` declared without one.
fn zero(t: &Ty) -> String {
    match t {
        Ty::Int => "0".into(),
        Ty::Float => "0.0".into(),
        Ty::Bool => "false".into(),
        Ty::Str => "\"\"".into(),
        Ty::Array(_) => "[]".into(),
        _ => "nil".into(),
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

    /// Prepend the file's preamble — the one import the outside-world helpers
    /// need, the `String: Error` conformance a string-carrying `Result` needs,
    /// and the helpers themselves — to the already-emitted body.
    fn assemble(&self, program: &[Stmt]) -> String {
        let uses_io =
            self.uses_read_file || self.uses_write_file || self.uses_eprint || self.uses_run;
        // readFile/writeFile/run all produce a `Result<_, String>`, so they pull
        // in the same conformance an annotated one would.
        let needs_error = needs_string_error(program)
            || self.uses_read_file
            || self.uses_write_file
            || self.uses_run;

        let mut head = String::new();
        if uses_io {
            // Foundation supplies the file reading/writing and the stderr handle.
            head.push_str("import Foundation\n\n");
        }
        if needs_error {
            head.push_str("// lux's Result carries a plain string error; Swift's Result needs\n");
            head.push_str("// its error type to be an Error, so we let a String be one.\n");
            head.push_str("extension String: @retroactive Error {}\n\n");
        }
        if self.uses_read_file {
            head.push_str(
                "func readFile(_ path: String) -> Result<String, String> {\n\
                 \tdo {\n\
                 \t\treturn .success(try String(contentsOfFile: path, encoding: .utf8))\n\
                 \t} catch {\n\
                 \t\treturn .failure(\"\\(error)\")\n\
                 \t}\n\
                 }\n\n",
            );
        }
        if self.uses_write_file {
            head.push_str(
                "func writeFile(_ path: String, _ contents: String) -> Result<Void, String> {\n\
                 \tdo {\n\
                 \t\ttry contents.write(toFile: path, atomically: true, encoding: .utf8)\n\
                 \t\treturn .success(())\n\
                 \t} catch {\n\
                 \t\treturn .failure(\"\\(error)\")\n\
                 \t}\n\
                 }\n\n",
            );
        }
        if self.uses_eprint {
            head.push_str(
                "func eprint(_ items: Any...) {\n\
                 \tlet line = items.map { \"\\($0)\" }.joined(separator: \" \")\n\
                 \tFileHandle.standardError.write(Data((line + \"\\n\").utf8))\n\
                 }\n\n",
            );
        }
        if self.uses_run {
            // /usr/bin/env gives the same PATH lookup Rust and Go do from a bare
            // program name; the child's input is the null device. A throw means
            // it never launched (lux's err); a non-zero exit rides in Output.
            head.push_str(
                "struct Output: Equatable {\n\
                 \tlet status: Int\n\
                 \tlet stdout: String\n\
                 \tlet stderr: String\n\
                 }\n\n",
            );
            head.push_str(
                "func run(_ program: String, _ args: [String]) -> Result<Output, String> {\n\
                 \tlet process = Process()\n\
                 \tprocess.executableURL = URL(fileURLWithPath: \"/usr/bin/env\")\n\
                 \tprocess.arguments = [program] + args\n\
                 \tlet outPipe = Pipe()\n\
                 \tlet errPipe = Pipe()\n\
                 \tprocess.standardOutput = outPipe\n\
                 \tprocess.standardError = errPipe\n\
                 \tprocess.standardInput = FileHandle.nullDevice\n\
                 \tdo {\n\
                 \t\ttry process.run()\n\
                 \t} catch {\n\
                 \t\treturn .failure(\"\\(error)\")\n\
                 \t}\n\
                 \tlet outData = outPipe.fileHandleForReading.readDataToEndOfFile()\n\
                 \tlet errData = errPipe.fileHandleForReading.readDataToEndOfFile()\n\
                 \tprocess.waitUntilExit()\n\
                 \treturn .success(Output(\n\
                 \t\tstatus: Int(process.terminationStatus),\n\
                 \t\tstdout: String(data: outData, encoding: .utf8) ?? \"\",\n\
                 \t\tstderr: String(data: errData, encoding: .utf8) ?? \"\"\n\
                 \t))\n\
                 }\n\n",
            );
        }
        head.push_str(&self.out);
        head
    }

    // --- declarations ------------------------------------------------------

    fn emit_struct(&mut self, name: &str, fields: &[FieldDef]) {
        self.line(format!("struct {}: Equatable {{", name));
        for f in fields {
            self.line(format!(
                "    let {}: {}",
                f.name,
                ty_text(&ty_from_ann(&f.ty))
            ));
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_enum(&mut self, name: &str, variants: &[VariantDef]) {
        self.line(format!("enum {}: Equatable {{", name));
        for v in variants {
            if v.fields.is_empty() {
                self.line(format!("    case {}", v.name));
            } else {
                // Swift keeps the field labels lux wrote, so construction reads
                // the same on both sides: `.circle(radius: 2.0)`.
                let parts: Vec<String> = v
                    .fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, ty_text(&ty_from_ann(&f.ty))))
                    .collect();
                self.line(format!("    case {}({})", v.name, parts.join(", ")));
            }
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        // The `_` label keeps calls positional, the way lux writes them.
        let ps: Vec<String> = params
            .iter()
            .map(|p| format!("_ {}: {}", p.name, ty_text(&ty_from_ann(&p.ty))))
            .collect();
        let r = ret
            .map(|t| format!(" -> {}", ty_text(&ty_from_ann(t))))
            .unwrap_or_default();
        self.line(format!("func {}({}){} {{", name, ps.join(", "), r));
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
                self.t.declare(name.clone(), vty.clone());
                self.line(format!("var {}: {} = {}", name, ty_text(&vty), z));
            }
            Stmt::Var { value: None, .. } => {}
            Stmt::Assign {
                name, op, value, ..
            } => self.emit_assign(name, *op, value),
            Stmt::Return { value, .. } => match value {
                // `return match ...` becomes a switch whose arms each return.
                Some(Expr::Match {
                    scrutinee, arms, ..
                }) => self.emit_switch(scrutinee, arms, true),
                Some(v) => {
                    let e = self.emit_expr(v);
                    self.line(format!("return {}", e));
                }
                None => self.line("return".into()),
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
            Stmt::Expr(Expr::Match {
                scrutinee, arms, ..
            }) => self.emit_switch(scrutinee, arms, false),
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

    fn emit_binding(&mut self, name: &str, ann: Option<&TypeAnn>, value: &Expr, mutable: bool) {
        let vty = ann
            .map(ty_from_ann)
            .unwrap_or_else(|| self.t.type_of(value));
        // Only annotate when the value can't pin its own type (a bare `none`),
        // since Swift infers the rest.
        let value_open = self.t.type_of(value).has_unknown();
        let kw = if mutable { "var" } else { "let" };
        let expr = self.emit_expr(value);
        if ann.is_some() && value_open && !vty.has_unknown() {
            self.line(format!("{} {}: {} = {}", kw, name, ty_text(&vty), expr));
        } else {
            self.line(format!("{} {} = {}", kw, name, expr));
        }
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
                    self.line(format!("{}.append({})", name, e));
                }
                // Strings and numbers both take Swift's `+=` directly.
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
        let elem_ty = match self.t.type_of(iter) {
            Ty::Range => Ty::Int,
            Ty::Array(t) => *t,
            _ => Ty::Unknown,
        };
        let it = self.emit_expr(iter);
        self.line(format!("for {} in {} {{", var, it));
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

    /// Emit a `match` as a `switch`. In return position each arm's body becomes
    /// a `return`, with nested matches recursing the same way; in statement
    /// position each body runs for its effect.
    fn emit_switch(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        let st = self.t.type_of(scrutinee);
        let s = self.emit_expr(scrutinee);
        self.line(format!("switch {} {{", s));
        for arm in arms {
            let label = self.case_label(&arm.pattern, &st);
            self.line(format!("{}:", label));
            self.indent += 1;
            self.t.push_scope();
            self.declare_bindings(&arm.pattern, &st);
            match &arm.body {
                // A nested match in a returning arm returns from inside.
                Expr::Match {
                    scrutinee, arms, ..
                } if ret => self.emit_switch(scrutinee, arms, true),
                body => {
                    let e = self.emit_expr(body);
                    self.line(if ret { format!("return {}", e) } else { e });
                }
            }
            self.t.pop_scope();
            self.indent -= 1;
        }
        self.line("}".into());
    }

    /// The `case ...` (or `default`) label for one pattern.
    fn case_label(&self, pat: &Pattern, st: &Ty) -> String {
        match pat {
            Pattern::Wildcard(_) => "default".to_string(),
            Pattern::Int(n, _) => format!("case {}", n),
            Pattern::Str(s, _) => format!("case \"{}\"", escape(s)),
            Pattern::Bool(b, _) => format!("case {}", b),
            Pattern::Variant { name, bindings, .. } => {
                // A `_` binding discards; `let _` would only draw a warning.
                let binds: Vec<String> = bindings
                    .iter()
                    .map(|b| {
                        if b == "_" {
                            "_".to_string()
                        } else {
                            format!("let {}", b)
                        }
                    })
                    .collect();
                let inner = if binds.is_empty() {
                    String::new()
                } else {
                    format!("({})", binds.join(", "))
                };
                let case = match st {
                    Ty::Option(_) if name == "some" => "some".to_string(),
                    Ty::Option(_) => "none".to_string(),
                    Ty::Result(_, _) if name == "ok" => "success".to_string(),
                    Ty::Result(_, _) => "failure".to_string(),
                    _ => name.clone(),
                };
                format!("case .{}{}", case, inner)
            }
        }
    }

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
                let parts: Vec<String> = els.iter().map(|x| self.emit_expr(x)).collect();
                format!("[{}]", parts.join(", "))
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
            // Swift's `+` already concatenates strings, so string and numeric
            // `+` need no distinction here.
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
                let s = self.emit_expr(start);
                let e = self.emit_expr(end);
                format!("{}..<{}", s, e)
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
                format!("{}({})", name, parts.join(", "))
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
                    return format!("{}.{}", n, field);
                }
                let b = self.emit_expr(base);
                format!("{}.{}", b, field)
            }
            // A match used as a value (not in return or statement position) is
            // wrapped in an immediately-called closure. lux's examples never hit
            // this, but it keeps any program translatable.
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let rt = ty_text(&self.t.type_of(e));
                let body = self.switch_to_string(scrutinee, arms);
                let close = indent(self.indent);
                format!("{{ () -> {} in\n{}{}}}()", rt, body, close)
            }
        }
    }

    /// Render a returning `switch` into its own string, for the closure form.
    fn switch_to_string(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> String {
        let saved = std::mem::take(&mut self.out);
        self.indent += 1;
        self.emit_switch(scrutinee, arms, true);
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
                let parts: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("print({})", parts.join(", "))
            }
            "eprint" => {
                self.uses_eprint = true;
                let parts: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("eprint({})", parts.join(", "))
            }
            // readFile/writeFile lower to the do/catch helpers; args and readLine
            // map straight onto Swift's own globals.
            "readFile" => {
                self.uses_read_file = true;
                let p = self.emit_expr(&args[0]);
                format!("readFile({})", p)
            }
            "writeFile" => {
                self.uses_write_file = true;
                let p = self.emit_expr(&args[0]);
                let c = self.emit_expr(&args[1]);
                format!("writeFile({}, {})", p, c)
            }
            "args" => "CommandLine.arguments".to_string(),
            "readLine" => "readLine()".to_string(),
            "run" => {
                self.uses_run = true;
                let p = self.emit_expr(&args[0]);
                let a = self.emit_expr(&args[1]);
                format!("run({}, {})", p, a)
            }
            // Swift's `String(...)` keeps a whole float's decimal point, the way
            // lux's `string(2.0)` yields "2.0".
            "string" => {
                let e = self.emit_expr(&args[0]);
                format!("String({})", e)
            }
            "int" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    // Int(String) is failing, so force-unwrap to match lux.
                    Ty::Str => format!("Int({})!", e),
                    _ => format!("Int({})", e),
                }
            }
            "float" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Str => format!("Double({})!", e),
                    _ => format!("Double({})", e),
                }
            }
            "length" => {
                let e = self.emit_expr(&args[0]);
                format!("{}.count", e)
            }
            "some" => {
                let e = self.emit_expr(&args[0]);
                format!(".some({})", e)
            }
            "ok" => {
                let e = self.emit_expr(&args[0]);
                format!(".success({})", e)
            }
            "err" => {
                let e = self.emit_expr(&args[0]);
                format!(".failure({})", e)
            }
            _ => {
                // Value semantics match lux's, so arguments pass straight through.
                let parts: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{}({})", name, parts.join(", "))
            }
        }
    }

    fn emit_enum_lit(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, Expr)],
    ) -> String {
        // Keep the labels and the declared order, the way Swift writes them.
        let order: Option<Vec<String>> = self.t.env.enums.get(enum_name).and_then(|variants| {
            variants
                .iter()
                .find(|v| v.name == variant)
                .map(|v| v.fields.iter().map(|f| f.name.clone()).collect())
        });
        let parts: Vec<String> = match order {
            Some(names) => names
                .iter()
                .filter_map(|fname| {
                    fields
                        .iter()
                        .find(|(k, _)| k == fname)
                        .map(|(_, e)| (fname.clone(), self.emit_expr(e)))
                })
                .map(|(label, val)| format!("{}: {}", label, val))
                .collect(),
            None => fields
                .iter()
                .map(|(k, e)| format!("{}: {}", k, self.emit_expr(e)))
                .collect(),
        };
        if parts.is_empty() {
            format!("{}.{}", enum_name, variant)
        } else {
            format!("{}.{}({})", enum_name, variant, parts.join(", "))
        }
    }
}

/// Does the program use a `Result` whose error is a string? If so, the Swift
/// backend must teach `String` to be an `Error`.
fn needs_string_error(program: &[Stmt]) -> bool {
    fn ann_has(a: &TypeAnn) -> bool {
        match &a.kind {
            TypeKind::Named(_) => false,
            TypeKind::Array(inner) => ann_has(inner),
            TypeKind::Generic(name, args) => {
                (name == "Result"
                    && matches!(args.get(1).map(|t| &t.kind), Some(TypeKind::Named(n)) if n == "string"))
                    || args.iter().any(ann_has)
            }
        }
    }
    program.iter().any(|stmt| match stmt {
        Stmt::Func { params, ret, .. } => {
            ret.as_ref().is_some_and(ann_has) || params.iter().any(|p| ann_has(&p.ty))
        }
        Stmt::Struct { fields, .. } => fields.iter().any(|f| ann_has(&f.ty)),
        Stmt::Enum { variants, .. } => variants
            .iter()
            .any(|v| v.fields.iter().any(|f| ann_has(&f.ty))),
        _ => false,
    })
}

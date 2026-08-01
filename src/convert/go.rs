//! The Go backend: emit real Go source.
//!
//! Go is the target furthest from lux's shape, which is exactly the lesson: down
//! the ladder you keep the same ideas but rebuild a few conveniences by hand.
//! lux has none of Go's gaps, so this backend is where the translation does the
//! most work. Enums with associated values have no Go equivalent, so each
//! becomes an interface plus one struct per case, taken apart with a type
//! switch. lux's `match` is an expression but Go's `switch` is a statement, so a
//! returning match pushes its `return` into every arm. `Option<T>` becomes a
//! pointer with `nil` for none — except when `T` is an enum, already a nil-able
//! interface, which stands on its own — and `Result<T, E>` becomes Go's
//! `(value, error)` pair, the way the standard library returns them.
//!
//! Two seams are worth naming. lux is immutable by default; Go's `const` only
//! holds compile-time constants, so that distinction is dropped here — `let` and
//! `var` both become `:=`. And `fmt.Println` renders a whole float as `9`, not
//! `9.0`, and a struct as `{3 4}`; the values are identical, only the rendering
//! is Go's own.

use crate::ast::*;

use super::{
    Ty, Types, bin_prec, escape, expr_mentions, format_float, go_ident, op_str, to_pascal,
    ty_from_ann,
};

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
    uses_strconv: bool,
    /// The outside-world helpers: each adapts Go's standard-library shape to the
    /// `(value, error)` pair lux's `Result` lowers to, emitted only when used.
    uses_read_file: bool,
    uses_write_file: bool,
    uses_read_line: bool,
    /// `input()` prompts and reads a plain line; it lowers to a helper over
    /// `readLine`, so it pulls that reader in too.
    uses_input: bool,
    /// Text-to-number parsing, each emitted as a `*T` so a failed parse is the
    /// nil that lux reads as `none`.
    uses_parse_int: bool,
    uses_parse_float: bool,
    /// `run` pulls in `bytes` and `os/exec`, the built-in `Output` struct, and a
    /// helper that adapts `exec`'s error into lux's launch-or-status split.
    uses_run: bool,
    /// The type-switch subjects and error names the emitter has introduced in the
    /// current nesting. A scratch name must dodge these too, so an inner match
    /// doesn't reuse the name an enclosing one is still holding.
    scratches: Vec<String>,
    /// `print`/`eprint` of a compound value (array, struct, enum, `Option`) routes
    /// through the generated `luxShow` renderer so the output reads the way lux
    /// renders it — `[1, 2, 3]`, `P(x: 1, y: 2)`, `Shape.circle(radius: 5)` —
    /// rather than `fmt`'s `[1 2 3]`, `{1 2}`, `{5}`.
    uses_lux_show: bool,
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
        uses_strconv: false,
        uses_read_file: false,
        uses_write_file: false,
        uses_read_line: false,
        uses_input: false,
        uses_parse_int: false,
        uses_parse_float: false,
        uses_run: false,
        scratches: Vec::new(),
        uses_lux_show: false,
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

    /// Is this type one of the program's enums? An enum lowers to a Go interface,
    /// which is already nil-able, so it needs no pointer wrapper for `Option`.
    fn is_enum_ty(&self, t: &Ty) -> bool {
        matches!(t, Ty::User(n) if self.t.env.enums.contains_key(n))
    }

    /// Go's zero value, aware that an enum is a nil-able interface: `zero`'s
    /// `Room{}` would try to instantiate the interface, so an enum's zero is `nil`.
    fn zero_value(&self, t: &Ty) -> String {
        if self.is_enum_ty(t) {
            "nil".to_string()
        } else {
            zero(t)
        }
    }

    /// A lux type as Go source text. `Result` only ever reaches here as a function
    /// return, where it expands to the `(value, error)` pair Go uses.
    fn ty_text(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "int".into(),
            Ty::Float => "float64".into(),
            Ty::Str => "string".into(),
            Ty::Bool => "bool".into(),
            Ty::Array(t) => format!("[]{}", self.ty_text(t)),
            Ty::User(n) => n.clone(),
            // `Option<T>` is `*T`, using nil for `none` — except when `T` is an
            // enum, which is already a nil-able interface. Wrapping that in a
            // pointer gives `*Interface`, which almost nothing satisfies, so the
            // bare interface stands in and nil is still `none`.
            Ty::Option(inner) => {
                if self.is_enum_ty(inner) {
                    self.ty_text(inner)
                } else {
                    format!("*{}", self.ty_text(inner))
                }
            }
            // A `Result` whose success carries nothing is an operation that can
            // only fail, so Go returns just an `error`, the way the stdlib does.
            Ty::Result(a, _) => match a.as_ref() {
                Ty::Unit => "error".into(),
                _ => format!("({}, error)", self.ty_text(a)),
            },
            Ty::Range => "int".into(),
            Ty::Unit => String::new(),
            Ty::Unknown => "any".into(),
        }
    }

    /// Wrap the emitted declarations in a package clause, the imports actually
    /// used, and any helper the program leans on.
    fn assemble(&self) -> String {
        let mut head = String::from("package main\n\n");
        // Collect what's used, then sort so the block reads the way gofmt orders
        // it — which is the plain lexical order of the import paths.
        let mut imports: Vec<&str> = Vec::new();
        if self.uses_bufio {
            imports.push("bufio");
        }
        if self.uses_run {
            imports.push("bytes");
            imports.push("os/exec");
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
        // `luxShow` walks a slice or a pointer of any type by reflection.
        if self.uses_lux_show {
            imports.push("reflect");
        }
        if self.uses_strconv {
            imports.push("strconv");
        }
        if self.uses_strings || self.uses_lux_show {
            imports.push("strings");
        }
        imports.sort_unstable();
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
        if self.uses_lux_show {
            head.push_str(&self.lux_show_fn());
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
        if self.uses_input {
            // Prompt inline, then read one line through the shared reader,
            // folding end of input into the empty string.
            head.push_str(
                "func input(prompt string) string {\n\
                 \tfmt.Print(prompt)\n\
                 \tif line := readLine(); line != nil {\n\
                 \t\treturn *line\n\
                 \t}\n\
                 \treturn \"\"\n\
                 }\n\n",
            );
        }
        if self.uses_parse_int {
            // A failed parse is nil, the pointer lux reads as `none`.
            head.push_str(
                "func parseInt(s string) *int {\n\
                 \tn, err := strconv.Atoi(strings.TrimSpace(s))\n\
                 \tif err != nil {\n\
                 \t\treturn nil\n\
                 \t}\n\
                 \treturn &n\n\
                 }\n\n",
            );
        }
        if self.uses_parse_float {
            head.push_str(
                "func parseFloat(s string) *float64 {\n\
                 \tf, err := strconv.ParseFloat(strings.TrimSpace(s), 64)\n\
                 \tif err != nil {\n\
                 \t\treturn nil\n\
                 \t}\n\
                 \treturn &f\n\
                 }\n\n",
            );
        }
        if self.uses_run {
            // exec splits failure two ways: an *ExitError means it ran and
            // reported a non-zero status (still a launch, so the status rides
            // back in Output); any other error means it never launched, which is
            // lux's err. A nil Stdin gives the child an empty input.
            head.push_str(
                "type Output struct {\n\
                 \tstatus int\n\
                 \tstdout string\n\
                 \tstderr string\n\
                 }\n\n",
            );
            head.push_str(
                "func run(program string, args []string) (Output, error) {\n\
                 \tcmd := exec.Command(program, args...)\n\
                 \tvar stdout, stderr bytes.Buffer\n\
                 \tcmd.Stdout = &stdout\n\
                 \tcmd.Stderr = &stderr\n\
                 \terr := cmd.Run()\n\
                 \tif err != nil {\n\
                 \t\tif exit, ok := err.(*exec.ExitError); ok {\n\
                 \t\t\treturn Output{status: exit.ExitCode(), stdout: stdout.String(), stderr: stderr.String()}, nil\n\
                 \t\t}\n\
                 \t\treturn Output{}, err\n\
                 \t}\n\
                 \treturn Output{status: 0, stdout: stdout.String(), stderr: stderr.String()}, nil\n\
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
                self.ty_text(&ty_from_ann(&f.ty)),
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
            .map(|p| {
                format!(
                    "{} {}",
                    go_ident(&p.name),
                    self.ty_text(&ty_from_ann(&p.ty))
                )
            })
            .collect();
        let rty = ret.map(ty_from_ann);
        let rtext = match &rty {
            None | Some(Ty::Unit) => String::new(),
            Some(t) => format!(" {}", self.ty_text(t)),
        };
        self.line(format!(
            "func {}({}){} {{",
            go_ident(name),
            ps.join(", "),
            rtext
        ));
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
                self.line(format!("var {} {}", name, self.ty_text(&vty)));
            }
            Stmt::Var { value: None, .. } => {}
            Stmt::Assign {
                target, op, value, ..
            } => self.emit_assign(target, *op, value),
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
                    // Type the success value against the Result's ok slot, so a
                    // returned empty array lands as `[]int{}`, not `[]any{}`.
                    let e = self.emit_expr_typed(&args[0], &t);
                    self.line(format!("return {}, nil", e));
                    return;
                }
                "err" => {
                    let e = self.emit_expr(&args[0]);
                    self.uses_errors = true;
                    self.line(format!("return {}, errors.New({})", self.zero_value(&t), e));
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
        // Type the returned value against the function's return type, so an empty
        // array literal returned directly (`return []`) takes the declared element
        // type rather than Go's untyped `[]any{}`.
        let e = match self.ret.clone() {
            Some(t) => self.emit_expr_typed(v, &t),
            None => self.emit_expr(v),
        };
        self.line(format!("return {}", e));
    }

    /// A value bound to a name must become an independent copy where lux's value
    /// semantics call for one but Go's don't. A Go slice is a reference, so
    /// `xs := input` would alias the caller's row — and a later in-place sort would
    /// reach back through it, mutating a row the program was told stays untouched.
    /// Copy an array bound from anything but a fresh literal, which is already its
    /// own. (A struct is a Go value type and copies on assignment on its own; a
    /// slice *inside* a struct is the same reference underneath — a deeper seam
    /// than the flat sort row this covers.)
    fn copy_on_bind(&mut self, vty: &Ty, value: &Expr, expr: String) -> String {
        match vty {
            Ty::Array(elem) if !matches!(value, Expr::Array(..)) => {
                format!("append([]{}{{}}, {}...)", self.ty_text(elem), expr)
            }
            _ => expr,
        }
    }

    fn emit_binding(&mut self, name: &str, ann: Option<&TypeAnn>, value: &Expr) {
        let vty = ann
            .map(ty_from_ann)
            .unwrap_or_else(|| self.t.type_of(value));
        let typed = self.emit_expr_typed(value, &vty);
        let expr = self.copy_on_bind(&vty, value, typed);
        // An enum lowers to an interface. Initialising with `:=` would infer the
        // concrete case struct (`ColourRed`), so a later `c = Colour.blue` — a
        // different case — wouldn't assign, and a `switch c.(type)` on the concrete
        // value wouldn't even be a valid type switch. Pin the interface type, the
        // way you'd accumulate any enum value: `var out List = List.nil`.
        if self.is_enum_ty(&vty) {
            self.line(format!(
                "var {} {} = {}",
                go_ident(name),
                self.ty_text(&vty),
                expr
            ));
        } else {
            self.line(format!("{} := {}", go_ident(name), expr));
        }
        self.t.declare(name.to_string(), vty);
    }

    /// Emit `value` where the surrounding type is already known — an annotated
    /// binding or a struct field. It changes only an empty array literal, which
    /// carries no element to infer from: with the expected type in hand it emits
    /// `[]int{}` rather than Go's untyped `[]any{}`, which won't assign to a typed
    /// slice. Every other value emits exactly as `emit_expr` would.
    fn emit_expr_typed(&mut self, value: &Expr, expected: &Ty) -> String {
        if let (Expr::Array(els, _), Ty::Array(elem)) = (value, expected)
            && els.is_empty()
        {
            return format!("[]{}{{}}", self.ty_text(elem));
        }
        self.emit_expr(value)
    }

    /// The declared type of `field` on struct `name`, when both are known — so a
    /// struct literal can type an empty-array field from the field's own type.
    fn field_ty(&self, name: &str, field: &str) -> Option<Ty> {
        self.t
            .env
            .structs
            .get(name)?
            .iter()
            .find(|f| f.name == field)
            .map(|f| ty_from_ann(&f.ty))
    }

    fn emit_assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) {
        // The place emits the same on the left as when read — `w.doorOpen`,
        // `items[i]`, or a plain name — and its type picks how `+=` lowers.
        let lhs = self.emit_expr(target);
        let lty = self.t.type_of(target);
        match op {
            AssignOp::Set => {
                let e = self.emit_expr(value);
                self.line(format!("{} = {}", lhs, e));
            }
            AssignOp::Add => match lty {
                // lux `+=` on an array appends one element.
                Ty::Array(_) => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} = append({}, {})", lhs, lhs, e));
                }
                // Strings and numbers both take Go's `+=` directly.
                _ => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} += {}", lhs, e));
                }
            },
            AssignOp::Sub => {
                let e = self.emit_expr(value);
                self.line(format!("{} -= {}", lhs, e));
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

    /// The body of one arm, either run for effect or turned into a `return`. A
    /// returning arm goes through `emit_return`, so an arm that forwards a
    /// `Result` — `err(why) => err(why)` — gets the same `(value, error)` pair
    /// lowering a top-level `return err(why)` does, rather than a bare value where
    /// Go wants two. It also picks up the return-position typing that an empty
    /// array literal (`empty => []`) needs.
    fn emit_arm_body(&mut self, body: &Expr, ret: bool) {
        if ret {
            self.emit_return(Some(body));
            return;
        }
        match body {
            Expr::Match {
                scrutinee, arms, ..
            } => self.emit_match(scrutinee, arms, false),
            _ => {
                let e = self.emit_expr(body);
                self.line(e);
            }
        }
    }

    fn emit_match_value(&mut self, scrutinee: &Expr, arms: &[MatchArm], ret: bool) {
        let s = self.emit_expr(scrutinee);
        let has_default = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard(_)));
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
        // `switch v := s.(type)` only when some arm actually reads its payload;
        // otherwise `v` itself would be an unused local, which Go rejects. An arm
        // that binds a value it never uses falls through to a plain type switch.
        let any_bind = arms.iter().any(arm_uses_a_binding);
        // The subject steps aside if its name is already taken — by an arm binding
        // (`full(let v, …)` pulling a field off the subject), or by anything an
        // enclosing match left in scope, so a nested switch doesn't reuse it.
        let subj = self.fresh_scratch("v", arms);
        let head = if any_bind {
            format!("switch {} := {}.(type) {{", subj, s)
        } else {
            format!("switch {}.(type) {{", s)
        };
        self.line(head);
        // Only the binding form introduces the subject as a live name.
        if any_bind {
            self.scratches.push(subj.clone());
        }
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
                // Only pull out a binding the arm actually reads — an unused local
                // is a compile error in Go, not a warning.
                if b != "_" && expr_mentions(&arm.body, b) {
                    self.line(format!("{} := {}.{}", b, subj, fname));
                }
            }
            self.emit_arm_body(&arm.body, ret);
            self.t.pop_scope();
            self.indent -= 1;
        }
        if any_bind {
            self.scratches.pop();
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
        // An enum `Option` is the bare interface, so the scrutinee is the value
        // itself — nil-tested directly and bound without a pointer deref.
        let enum_inner = self.is_enum_ty(&inner);
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
            // Bind the inner value only when the arm reads it. An unused `_` — or a
            // name the body never touches — is skipped: `_ := *ptr` is invalid Go,
            // and the pointer is already used by the `!= nil` test above.
            let used = some_arm.is_some_and(|a| b != "_" && expr_mentions(&a.body, b));
            if used {
                self.t.declare(b.clone(), inner.clone());
                if enum_inner {
                    self.line(format!("{} := {}", b, ptr));
                } else {
                    self.line(format!("{} := *{}", b, ptr));
                }
            }
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
        // The error scratch steps aside if its name is taken — by an arm binding
        // (`err(let err)` would redeclare the test variable) or by an enclosing
        // match that's still holding it.
        let ev = self.fresh_scratch("err", arms);
        // An if-init scopes the value and error to this match, so two reads in
        // one block don't collide on the names. A success that carries nothing
        // leaves only the error to bind.
        if ok_ty == Ty::Unit {
            self.line(format!("if {} := {}; {} == nil {{", ev, s, ev));
        } else {
            // Bind the ok value only when the arm reads it; otherwise `_`, since Go
            // rejects an unused local. The error scratch is always used by the test.
            let lhs = match &ok_bind {
                Some(b) if b != "_" && ok_arm.is_some_and(|a| expr_mentions(&a.body, b)) => {
                    b.clone()
                }
                _ => "_".to_string(),
            };
            self.line(format!("if {}, {} := {}; {} == nil {{", lhs, ev, s, ev));
        }
        // The error scratch is live across both branches, so a nested match inside
        // either one must not reuse it.
        self.scratches.push(ev.clone());
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
            // lux carries the reason as a string; Go's error gives it back. Bind it
            // only when the arm reads it — an unused `_`, or a name the body never
            // touches, is skipped, since `err` is already used by the test above.
            let used = err_arm.is_some_and(|a| b != "_" && expr_mentions(&a.body, b));
            if used {
                self.t.declare(b.clone(), err_ty);
                self.line(format!("{} := {}.Error()", b, ev));
            }
        }
        if let Some(a) = err_arm {
            self.emit_arm_body(&a.body, ret);
        }
        self.t.pop_scope();
        self.indent -= 1;
        self.scratches.pop();
        self.line("}".into());
    }

    /// A scratch name clear of everything it could collide with: the match's own
    /// captures, any lux binding still live in an enclosing scope, and the scratch
    /// names outer matches are still holding. Starts from `base` and steps aside a
    /// `_` at a time, so the common case keeps the readable `v` / `err`.
    fn fresh_scratch(&self, base: &str, arms: &[MatchArm]) -> String {
        let mut name = base.to_string();
        while arm_bindings(arms).any(|b| b == name)
            || self.t.in_scope(&name)
            || self.scratches.contains(&name)
        {
            name.push('_');
        }
        name
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
                    go_ident(name)
                }
            }
            Expr::Array(els, _) => {
                let et = match els.first() {
                    Some(first) => self.ty_text(&self.t.type_of(first)),
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
                        // Type each field from the struct's declaration, so an
                        // empty-array field lands as `[]int{}`, not `[]any{}`.
                        let val = match self.field_ty(name, k) {
                            Some(t) => self.emit_expr_typed(v, &t),
                            None => self.emit_expr(v),
                        };
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
                    // Parenthesised so a payload-less case stays a valid
                    // operand inside an `if`/`switch` condition, where Go would
                    // otherwise read the `{` as the start of the block.
                    return format!("({}{}{{}})", n, to_pascal(field));
                }
                let b = self.emit_expr(base);
                format!("{}.{}", b, field)
            }
            // A match used as a value, which lux's examples never do; a closure
            // keeps it translatable.
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let rt = self.ty_text(&self.t.type_of(e));
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

    /// The generated `luxShow(v any) string`: renders any value the way lux does,
    /// where `fmt`'s defaults would read differently — a struct as `{1 2}`, an enum
    /// case as `{5}`, a slice space-separated. A type switch names each struct and
    /// enum case (with lux's labels and its `Enum.case` form); reflection handles a
    /// slice or an `Option` pointer of any element type and recurses; scalars fall
    /// through to `fmt`. Cases are emitted in name order so the output is stable.
    fn lux_show_fn(&self) -> String {
        let mut cases = String::new();

        // Structs: `Name(field: value, …)`. `Output` is the built-in `run` returns,
        // and its Go type is only emitted when `run` is used, so name it only then.
        let mut struct_names: Vec<&String> = self.t.env.structs.keys().collect();
        struct_names.sort();
        for name in struct_names {
            if name == "Output" && !self.uses_run {
                continue;
            }
            let fields = &self.t.env.structs[name];
            cases.push_str(&format!(
                "\tcase {}:\n\t\treturn {}\n",
                name,
                render_labelled(name, fields)
            ));
        }

        // Enum cases: `Enum.case` alone, or `Enum.case(field: value, …)` with a
        // payload. The Go type is the per-case struct (`ShapeCircle`); the rendered
        // label is lux's own `Shape.circle`.
        let mut enum_names: Vec<&String> = self.t.env.enums.keys().collect();
        enum_names.sort();
        for ename in enum_names {
            for v in &self.t.env.enums[ename] {
                let case = format!("{}{}", ename, to_pascal(&v.name));
                if v.fields.is_empty() {
                    cases.push_str(&format!(
                        "\tcase {}:\n\t\treturn \"{}.{}\"\n",
                        case, ename, v.name
                    ));
                } else {
                    let head = format!("{}.{}", ename, v.name);
                    cases.push_str(&format!(
                        "\tcase {}:\n\t\treturn {}\n",
                        case,
                        render_labelled(&head, &v.fields)
                    ));
                }
            }
        }

        format!(
            "func luxShow(v any) string {{\n\
             \tswitch x := v.(type) {{\n\
             \tcase string:\n\t\treturn x\n\
             {cases}\
             \t}}\n\
             \trv := reflect.ValueOf(v)\n\
             \tswitch rv.Kind() {{\n\
             \tcase reflect.Slice:\n\
             \t\tparts := make([]string, rv.Len())\n\
             \t\tfor i := range parts {{\n\
             \t\t\tparts[i] = luxShow(rv.Index(i).Interface())\n\
             \t\t}}\n\
             \t\treturn \"[\" + strings.Join(parts, \", \") + \"]\"\n\
             \tcase reflect.Pointer:\n\
             \t\tif rv.IsNil() {{\n\
             \t\t\treturn \"none\"\n\
             \t\t}}\n\
             \t\treturn \"some(\" + luxShow(rv.Elem().Interface()) + \")\"\n\
             \t}}\n\
             \treturn fmt.Sprintf(\"%v\", v)\n\
             }}\n\n"
        )
    }

    /// One argument to `print`/`eprint`. A compound value — array, struct, enum, or
    /// `Option` — is wrapped in `luxShow` so it reads the way lux renders it rather
    /// than `fmt`'s default. A scalar prints as `fmt` already renders it.
    fn print_arg(&mut self, a: &Expr) -> String {
        let e = self.emit_expr(a);
        if matches!(
            self.t.type_of(a),
            Ty::Array(_) | Ty::User(_) | Ty::Option(_) | Ty::Result(..)
        ) {
            self.uses_lux_show = true;
            format!("luxShow({})", e)
        } else {
            e
        }
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => {
                self.uses_fmt = true;
                let parts: Vec<String> = args.iter().map(|a| self.print_arg(a)).collect();
                format!("fmt.Println({})", parts.join(", "))
            }
            "eprint" => {
                self.uses_fmt = true;
                self.uses_os = true;
                let mut parts = vec!["os.Stderr".to_string()];
                parts.extend(args.iter().map(|a| self.print_arg(a)));
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
            "run" => {
                self.uses_run = true;
                let p = self.emit_expr(&args[0]);
                // run's arguments are always [string]; emit the element type
                // outright so an empty list is `[]string{}`, not Go's `[]any{}`.
                let a = match &args[1] {
                    Expr::Array(els, _) => {
                        let parts: Vec<String> = els.iter().map(|x| self.emit_expr(x)).collect();
                        format!("[]string{{{}}}", parts.join(", "))
                    }
                    other => self.emit_expr(other),
                };
                format!("run({}, {})", p, a)
            }
            "readLine" => {
                self.uses_read_line = true;
                self.uses_os = true;
                self.uses_bufio = true;
                self.uses_strings = true;
                "readLine()".to_string()
            }
            "input" => {
                self.uses_input = true;
                self.uses_read_line = true;
                self.uses_os = true;
                self.uses_bufio = true;
                self.uses_strings = true;
                self.uses_fmt = true;
                let p = match args.first() {
                    Some(a) => self.emit_expr(a),
                    None => "\"\"".to_string(),
                };
                format!("input({})", p)
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
            "parseInt" => {
                self.uses_parse_int = true;
                self.uses_strconv = true;
                self.uses_strings = true;
                let e = self.emit_expr(&args[0]);
                format!("parseInt({})", e)
            }
            "parseFloat" => {
                self.uses_parse_float = true;
                self.uses_strconv = true;
                self.uses_strings = true;
                let e = self.emit_expr(&args[0]);
                format!("parseFloat({})", e)
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
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                if self.is_enum_ty(&inner) {
                    // `Option<enum>` is the bare interface, already nil-able, so
                    // the value stands on its own with no pointer wrapper.
                    e
                } else {
                    self.uses_ptr = true;
                    format!("ptr({})", e)
                }
            }
            // ok/err in a return are handled there; reaching here is degenerate.
            "ok" | "err" => self.emit_expr(&args[0]),
            _ => {
                // Type each argument against the parameter it fills, so an empty
                // array literal passed straight in — `total([])` — takes the
                // parameter's element type instead of Go's untyped `[]any{}`.
                let param_tys: Option<Vec<Ty>> = self
                    .t
                    .env
                    .funcs
                    .get(name)
                    .map(|(ps, _)| ps.iter().map(|p| ty_from_ann(&p.ty)).collect());
                let parts: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| match &param_tys {
                        Some(ts) if i < ts.len() => self.emit_expr_typed(a, &ts[i]),
                        _ => self.emit_expr(a),
                    })
                    .collect();
                format!("{}({})", go_ident(name), parts.join(", "))
            }
        }
    }

    fn emit_enum_lit(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, Expr)],
    ) -> String {
        let case = format!("{}{}", enum_name, to_pascal(variant));
        if fields.is_empty() {
            // Parenthesised: see the note in the `Expr::Field` arm above — a bare
            // `Case{}` is misread as a block when it sits in a condition.
            format!("({}{{}})", case)
        } else {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, e)| format!("{}: {}", k, self.emit_expr(e)))
                .collect();
            format!("{}{{{}}}", case, parts.join(", "))
        }
    }
}

/// Build the `luxShow` body for a labelled value — a struct or an enum case with
/// a payload — as a Go string expression: `"head(" + "f: " + luxShow(x.f) + …)`.
/// The label is each field's lux name; the access is `x.f`, which Go keeps as the
/// same name. Fields are read in declared order.
fn render_labelled(head: &str, fields: &[FieldDef]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|f| format!("\"{}: \" + luxShow(x.{})", f.name, f.name))
        .collect();
    format!("\"{}(\" + {} + \")\"", head, parts.join(" + \", \" + "))
}

/// The case name of a variant pattern, for matching `some`/`none`/`ok`/`err`.
fn arm_name(arm: &MatchArm) -> Option<&str> {
    match &arm.pattern {
        Pattern::Variant { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

/// Every binding name captured across a match's arms.
fn arm_bindings(arms: &[MatchArm]) -> impl Iterator<Item = &str> {
    arms.iter().flat_map(|arm| match &arm.pattern {
        Pattern::Variant { bindings, .. } => bindings.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    })
}

/// Does this arm read the value it captures — i.e. is at least one non-`_`
/// binding used in its body?
fn arm_uses_a_binding(arm: &MatchArm) -> bool {
    match &arm.pattern {
        Pattern::Variant { bindings, .. } => bindings
            .iter()
            .any(|b| b != "_" && expr_mentions(&arm.body, b)),
        _ => false,
    }
}

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

use super::{
    Ty, Types, bin_prec, dodge_type_name, escape, expr_mentions, format_float, indent,
    mutated_roots, op_str, stmts_mention, swift_case, swift_ident, ty_from_ann,
};

struct Gen {
    t: Types,
    out: String,
    indent: usize,
    /// The outside-world helpers, emitted only when used. Swift reads files by
    /// throwing, so each wraps a `do`/`catch` back into the `Result` lux uses.
    uses_read_file: bool,
    uses_write_file: bool,
    uses_eprint: bool,
    /// `input()` prompts and reads a plain line, lowering to a small helper over
    /// Swift's own `readLine()`.
    uses_input: bool,
    /// `run` needs Foundation's `Process`, the built-in `Output` struct, and the
    /// `String: Error` conformance its `Result` shares with the file helpers.
    uses_run: bool,
    /// Names ever mutated in the program, so a `var` that's only read binds with
    /// `let` and doesn't draw Swift's "never mutated, consider let" warning.
    mutated: std::collections::HashSet<String>,
    /// `print` of a compound value routes through a generated `LuxShow` protocol
    /// so the output reads the way lux renders it. Swift already prints a struct as
    /// `P(x: 1, y: 2)`, but an enum case drops its type (`circle(radius: 5)`) and an
    /// array leaks the module name (`[main.P(...)]`); `luxShow` fixes both and keeps
    /// every backend rendering the same way. Emitted only when a compound is printed.
    uses_lux_show: bool,
    /// `print` and `string` of a float route through `luxFloat`, which keeps the
    /// output positional at every magnitude and normalises `inf`/`-inf`/`NaN` —
    /// Swift's `String(Double)` uses exponent notation for small values, text lux
    /// can't read back, and writes `-nan` (#47). Also used by `luxShow` for a float
    /// inside an array, so it's emitted whenever either is.
    uses_lux_float: bool,
    /// `int` of a float routes through `luxInt`, which saturates a non-finite or
    /// out-of-range value the way the interpreter and the other targets do — Swift's
    /// `Int(Double)` traps on `inf`/`NaN`, a crash the learner can't read (#52).
    uses_lux_int: bool,
    /// `parseInt`/`parseFloat` trim surrounding whitespace through `luxTrim` before
    /// converting — Swift's `Int(_:)`/`Double(_:)` reject a leading or trailing
    /// space the interpreter and the other targets accept, silently returning `none`
    /// for input a column-aligned file or a paste routinely has (#41).
    uses_lux_parse: bool,
    /// Integer `/` and `%` route through guard helpers that report a lux error on a
    /// zero divisor and exit 1, so a learner meets `division by zero` rather than
    /// Swift's illegal-instruction trap and register dump (#34). Emitted only when
    /// used, and they pull in Foundation for the stderr handle.
    uses_lux_div: bool,
    uses_lux_mod: bool,
    /// Array indexing routes through bounds-checking helpers that report a lux error
    /// on an out-of-range index rather than trapping (#38). Pulls in Foundation.
    uses_lux_bounds: bool,
    /// True while emitting an assignment's target, so an indexed place stays a plain
    /// assignable subscript (its bounds check emitted separately as a statement)
    /// rather than the read helper, which yields a value.
    assigning: bool,
    /// The name of the generated `LuxShow` protocol, stepped aside if the program
    /// declares a type of that name so the two can't clash (#37).
    show_name: String,
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
        uses_input: false,
        uses_run: false,
        mutated: mutated_roots(program),
        uses_lux_show: false,
        uses_lux_float: false,
        uses_lux_int: false,
        uses_lux_parse: false,
        uses_lux_div: false,
        uses_lux_mod: false,
        uses_lux_bounds: false,
        assigning: false,
        show_name: dodge_type_name("LuxShow", program),
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

    // A user `func main` is emitted like any function, then run by one top-level
    // call. Swift, like lux, lets the file be the program, so unlike Rust and Go it
    // won't start `main` on its own — the call is how the entry point the student
    // learned actually runs (checks guarantee no other top-level code is present).
    let has_main = program
        .iter()
        .any(|s| matches!(s, Stmt::Func { name, .. } if name == "main"));
    if has_main {
        g.line("main()".into());
    }

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
        let uses_io = self.uses_read_file
            || self.uses_write_file
            || self.uses_eprint
            || self.uses_run
            || self.uses_lux_div
            || self.uses_lux_mod
            || self.uses_lux_bounds;
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
        // luxShow renders a float inside an array through luxFloat, so printing a
        // plain float and printing an array of floats share one helper.
        if self.uses_lux_float || self.uses_lux_show {
            head.push_str(LUX_FLOAT_HELPER);
        }
        if self.uses_lux_int {
            // Saturate rather than trap: `inf` to Int.max, `-inf` to Int.min, `NaN`
            // to 0, matching the interpreter's `as i64` and the other targets.
            head.push_str(
                "func luxInt(_ f: Double) -> Int {\n\
                 \tif f.isNaN { return 0 }\n\
                 \tif f >= Double(Int.max) { return Int.max }\n\
                 \tif f <= Double(Int.min) { return Int.min }\n\
                 \treturn Int(f)\n\
                 }\n\n",
            );
        }
        if self.uses_lux_parse {
            // Trim surrounding whitespace before a parse, matching Rust's `.trim()`
            // and Go's `strings.TrimSpace`; `Character.isWhitespace` is stdlib, so no
            // Foundation is pulled in for a program that only parses numbers.
            head.push_str(
                "func luxTrim(_ s: String) -> Substring {\n\
                 \tvar t = Substring(s)\n\
                 \twhile let c = t.first, c.isWhitespace { t = t.dropFirst() }\n\
                 \twhile let c = t.last, c.isWhitespace { t = t.dropLast() }\n\
                 \treturn t\n\
                 }\n\n",
            );
        }
        if self.uses_lux_show {
            // The protocol name steps aside from any user type of the same name, so
            // the prelude and a `struct LuxShow` can't collide (#37).
            head.push_str(&LUX_SHOW_PREAMBLE.replace("LuxShow", &self.show_name));
            // One conformance per user type, in declaration order.
            for stmt in program {
                match stmt {
                    Stmt::Struct { name, fields, .. } => {
                        head.push_str(&lux_show_struct(&self.show_name, name, fields))
                    }
                    Stmt::Enum { name, variants, .. } => {
                        head.push_str(&lux_show_enum(&self.show_name, name, variants))
                    }
                    _ => {}
                }
            }
        }
        if needs_error {
            head.push_str("// lux's Result carries a plain string error; Swift's Result needs\n");
            head.push_str("// its error type to be an Error, so we let a String be one.\n");
            head.push_str("extension String: @retroactive Error {}\n\n");
        }
        if self.uses_read_file {
            // Read through POSIX rather than Foundation's `String(contentsOfFile:)`,
            // whose thrown error is Objective-C vocabulary (`NSCocoaErrorDomain`) and
            // is factually wrong on a permission failure — it reports "file doesn't
            // exist" for a file that exists (#43). `strerror(errno)` gives the same
            // reason the interpreter and the other targets do, in lux's own shape.
            head.push_str(
                "func readFile(_ path: String) -> Result<String, String> {\n\
                 \tlet fd = open(path, O_RDONLY)\n\
                 \tif fd < 0 {\n\
                 \t\treturn .failure(\"could not read \\(path): \\(String(cString: strerror(errno)))\")\n\
                 \t}\n\
                 \tdefer { close(fd) }\n\
                 \tvar data = [UInt8]()\n\
                 \tvar buf = [UInt8](repeating: 0, count: 65536)\n\
                 \twhile true {\n\
                 \t\tlet n = read(fd, &buf, buf.count)\n\
                 \t\tif n < 0 {\n\
                 \t\t\treturn .failure(\"could not read \\(path): \\(String(cString: strerror(errno)))\")\n\
                 \t\t}\n\
                 \t\tif n == 0 { break }\n\
                 \t\tdata.append(contentsOf: buf[0..<n])\n\
                 \t}\n\
                 \treturn .success(String(decoding: data, as: UTF8.self))\n\
                 }\n\n",
            );
        }
        if self.uses_write_file {
            head.push_str(
                "func writeFile(_ path: String, _ contents: String) -> Result<Void, String> {\n\
                 \tlet fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0o644)\n\
                 \tif fd < 0 {\n\
                 \t\treturn .failure(\"could not write \\(path): \\(String(cString: strerror(errno)))\")\n\
                 \t}\n\
                 \tdefer { close(fd) }\n\
                 \tlet bytes = Array(contents.utf8)\n\
                 \tvar off = 0\n\
                 \twhile off < bytes.count {\n\
                 \t\tlet n = bytes[off...].withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }\n\
                 \t\tif n < 0 {\n\
                 \t\t\treturn .failure(\"could not write \\(path): \\(String(cString: strerror(errno)))\")\n\
                 \t\t}\n\
                 \t\toff += n\n\
                 \t}\n\
                 \treturn .success(())\n\
                 }\n\n",
            );
        }
        if self.uses_eprint {
            head.push_str(
                "func eprint(_ items: Any...) {\n\
                 \tlet line = items.map { \"\\($0)\" }.joined(separator: \" \")\n\
                 \t// Flush stdout first, so a warning lands after the output it follows\n\
                 \t// rather than jumping ahead of it when both streams are piped into\n\
                 \t// one — Swift block-buffers stdout off a terminal but writes stderr\n\
                 \t// through, so without this the warnings pile up at the top (#51).\n\
                 \tfflush(stdout)\n\
                 \tFileHandle.standardError.write(Data((line + \"\\n\").utf8))\n\
                 }\n\n",
            );
        }
        if self.uses_input {
            // Swift's own readLine() returns an optional; input() shows the
            // prompt inline and folds the end-of-input case into "".
            head.push_str(
                "func input(_ prompt: String) -> String {\n\
                 \tif !prompt.isEmpty { print(prompt, terminator: \"\") }\n\
                 \treturn readLine() ?? \"\"\n\
                 }\n\n",
            );
        }
        if self.uses_run {
            // Resolve a bare program name against PATH ourselves and launch it
            // directly, so a program that isn't there is a launch failure (lux's err)
            // — going through `/usr/bin/env` made a missing program the wrapper's
            // status 127, an `ok` where the other three return `err` (#48). The
            // child's input is the null device; a non-zero exit rides in Output.
            head.push_str(
                "struct Output: Equatable {\n\
                 \tlet status: Int\n\
                 \tlet stdout: String\n\
                 \tlet stderr: String\n\
                 }\n\n",
            );
            head.push_str(
                "func run(_ program: String, _ args: [String]) -> Result<Output, String> {\n\
                 \tvar resolved = program\n\
                 \tif !program.contains(\"/\") {\n\
                 \t\tlet dirs = (ProcessInfo.processInfo.environment[\"PATH\"] ?? \"\").split(separator: \":\")\n\
                 \t\tguard let found = dirs.map({ \"\\($0)/\\(program)\" }).first(where: {\n\
                 \t\t\tFileManager.default.isExecutableFile(atPath: $0)\n\
                 \t\t}) else {\n\
                 \t\t\treturn .failure(\"could not run \\(program): No such file or directory\")\n\
                 \t\t}\n\
                 \t\tresolved = found\n\
                 \t}\n\
                 \tlet process = Process()\n\
                 \tprocess.executableURL = URL(fileURLWithPath: resolved)\n\
                 \tprocess.arguments = args\n\
                 \tlet outPipe = Pipe()\n\
                 \tlet errPipe = Pipe()\n\
                 \tprocess.standardOutput = outPipe\n\
                 \tprocess.standardError = errPipe\n\
                 \tprocess.standardInput = FileHandle.nullDevice\n\
                 \tdo {\n\
                 \t\ttry process.run()\n\
                 \t} catch {\n\
                 \t\treturn .failure(\"could not run \\(program): \\(error.localizedDescription)\")\n\
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
        if self.uses_lux_div {
            // Report a zero divisor as a lux error and exit 1, rather than trapping
            // on the illegal instruction Swift raises for `a / 0`.
            head.push_str(
                "func luxDiv(_ a: Int, _ b: Int) -> Int {\n\
                 \tif b == 0 {\n\
                 \t\tfflush(stdout)\n\
                 \t\tFileHandle.standardError.write(Data(\"error: division by zero\\n\".utf8))\n\
                 \t\tFoundation.exit(1)\n\
                 \t}\n\
                 \treturn a / b\n\
                 }\n\n",
            );
        }
        if self.uses_lux_mod {
            head.push_str(
                "func luxMod(_ a: Int, _ b: Int) -> Int {\n\
                 \tif b == 0 {\n\
                 \t\tfflush(stdout)\n\
                 \t\tFileHandle.standardError.write(Data(\"error: remainder by zero\\n\".utf8))\n\
                 \t\tFoundation.exit(1)\n\
                 \t}\n\
                 \treturn a % b\n\
                 }\n\n",
            );
        }
        if self.uses_lux_bounds {
            // Report an out-of-range index as a lux error and exit, rather than
            // trapping on the illegal instruction Swift raises. `@discardableResult`
            // so a write's bare check statement draws no unused-result warning;
            // `luxIndex` reads through it, and copy-on-write means passing the array
            // in doesn't copy it. stdout is flushed first so output already printed
            // comes out ahead of the error.
            head.push_str(
                "@discardableResult\n\
                 func luxCheck(_ i: Int, _ n: Int) -> Int {\n\
                 \tif i < 0 || i >= n {\n\
                 \t\tfflush(stdout)\n\
                 \t\tFileHandle.standardError.write(Data(\"error: index \\(i) is out of bounds for an array of length \\(n)\\n\".utf8))\n\
                 \t\tif n == 0 {\n\
                 \t\t\tFileHandle.standardError.write(Data(\"note: this array is empty\\n\".utf8))\n\
                 \t\t} else {\n\
                 \t\t\tFileHandle.standardError.write(Data(\"note: valid indices are 0 to \\(n - 1)\\n\".utf8))\n\
                 \t\t}\n\
                 \t\tFileHandle.standardError.write(Data(\"help: `lux learn arrays` — the first element is 0, so the last is length minus 1\\n\".utf8))\n\
                 \t\tFoundation.exit(1)\n\
                 \t}\n\
                 \treturn i\n\
                 }\n\n",
            );
            head.push_str(
                "func luxIndex<T>(_ xs: [T], _ i: Int) -> T {\n\
                 \treturn xs[luxCheck(i, xs.count)]\n\
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
            // `var` properties, so a `var` instance can assign a field while a
            // `let` one still can't — Swift enforces the same gate lux does at the
            // binding, and an unmutated struct property draws no warning.
            self.line(format!(
                "    var {}: {}",
                f.name,
                ty_text(&ty_from_ann(&f.ty))
            ));
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_enum(&mut self, name: &str, variants: &[VariantDef]) {
        // A field whose type re-enters this enum — directly (`node(left: Tree)`)
        // or through a cycle (`Expr` holds a `Fn` that holds an `Expr`) — needs
        // indirection to have a finite size. Swift's `indirect` boxes exactly
        // those cases for us, so marking the enum is all it takes — construction
        // and matching read the same.
        let recursive = variants.iter().any(|v| {
            v.fields.iter().any(
                |f| matches!(ty_from_ann(&f.ty), Ty::User(ref n) if self.t.enum_reaches(n, name)),
            )
        });
        let kw = if recursive { "indirect enum" } else { "enum" };
        self.line(format!("{} {}: Equatable {{", kw, name));
        for v in variants {
            if v.fields.is_empty() {
                self.line(format!("    case {}", swift_case(&v.name)));
            } else {
                // Swift keeps the field labels lux wrote, so construction reads
                // the same on both sides: `.circle(radius: 2.0)`.
                let parts: Vec<String> = v
                    .fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, ty_text(&ty_from_ann(&f.ty))))
                    .collect();
                self.line(format!(
                    "    case {}({})",
                    swift_case(&v.name),
                    parts.join(", ")
                ));
            }
        }
        self.line("}".into());
        self.blank();
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        // The `_` label keeps calls positional, the way lux writes them.
        let ps: Vec<String> = params
            .iter()
            .map(|p| {
                format!(
                    "_ {}: {}",
                    swift_ident(&p.name),
                    ty_text(&ty_from_ann(&p.ty))
                )
            })
            .collect();
        let r = ret
            .map(|t| format!(" -> {}", ty_text(&ty_from_ann(t))))
            .unwrap_or_default();
        self.line(format!(
            "func {}({}){} {{",
            swift_ident(name),
            ps.join(", "),
            r
        ));
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
                target, op, value, ..
            } => self.emit_assign(target, *op, value),
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
        // Annotate when the value can't stand on its own type: a bare `none`
        // (its element type is open), or a leading-dot `.some(…)` / `.success(…)`
        // / `.failure(…)`, which Swift can't resolve without a contextual type.
        // The source supplied the annotation — `let a: Option<int> = some(5)` — so
        // carry it through rather than dropping it and leaving `.some(5)` to infer
        // from nothing (#33).
        let value_open = self.t.type_of(value).has_unknown() || needs_type_context(value);
        // `var` only when the binding is actually mutated; a `var` that's only
        // read binds with `let`, so Swift doesn't warn it was never mutated.
        let kw = if mutable && self.mutated.contains(name) {
            "var"
        } else {
            "let"
        };
        let ident = swift_ident(name);
        let expr = self.emit_expr(value);
        if ann.is_some() && value_open && !vty.has_unknown() {
            self.line(format!("{} {}: {} = {}", kw, ident, ty_text(&vty), expr));
        } else {
            self.line(format!("{} {} = {}", kw, ident, expr));
        }
        self.t.declare(name.to_string(), vty);
    }

    /// Emit a bounds-check statement for each array index in an assignment target,
    /// innermost first, so a write past the end reports a lux error before it runs
    /// (#38). Kept out of the target subscript itself and safe to name the base
    /// again, since an assignment target is rooted at a variable.
    fn emit_index_guards(&mut self, target: &Expr) {
        match target {
            Expr::Index { base, index, .. } => {
                self.emit_index_guards(base);
                if matches!(self.t.type_of(base), Ty::Array(_)) {
                    self.uses_lux_bounds = true;
                    let idx = self.emit_expr(index);
                    self.assigning = true;
                    let b = self.emit_expr(base);
                    self.assigning = false;
                    self.line(format!("luxCheck({}, {}.count)", idx, b));
                }
            }
            Expr::Field { base, .. } => self.emit_index_guards(base),
            _ => {}
        }
    }

    fn emit_assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) {
        // The place emits the same on the left as when read — `w.doorOpen`,
        // `items[i]`, or a plain name — and its type picks how `+=` lowers. An
        // indexed target is bounds-checked first, then emitted as a plain subscript.
        self.emit_index_guards(target);
        self.assigning = true;
        let lhs = self.emit_expr(target);
        self.assigning = false;
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
                    self.line(format!("{}.append({})", lhs, e));
                }
                // Strings and numbers both take Swift's `+=` directly.
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
        let elem_ty = match self.t.type_of(iter) {
            Ty::Range => Ty::Int,
            Ty::Array(t) => *t,
            _ => Ty::Unknown,
        };
        // A range whose end falls below its start is empty everywhere else — the
        // interpreter, Rust, and Go all iterate zero times — but Swift's `..<`
        // traps on out-of-order bounds. `stride(from:to:by:)` gives the same empty
        // iteration without the crash, so a bound that goes negative from ordinary
        // arithmetic (a shrinking inner loop over an emptying row) just doesn't run.
        let it = match (self.t.type_of(iter), iter) {
            (Ty::Range, Expr::Range { start, end, .. }) => {
                let s = self.emit_expr(start);
                let e = self.emit_expr(end);
                format!("stride(from: {}, to: {}, by: 1)", s, e)
            }
            _ => self.emit_expr(iter),
        };
        // A loop variable the body never reads becomes `_`, so Swift doesn't warn
        // about an unused immutable value in code the learner didn't write — the
        // same elision the match arms already do.
        let binder = if stmts_mention(body, var) {
            swift_ident(var)
        } else {
            "_".to_string()
        };
        self.line(format!("for {} in {} {{", binder, it));
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
            let label = self.case_label(&arm.pattern, &st, &arm.body);
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
    fn case_label(&self, pat: &Pattern, st: &Ty, body: &Expr) -> String {
        match pat {
            Pattern::Wildcard(_) => "default".to_string(),
            Pattern::Int(n, _) => format!("case {}", n),
            Pattern::Str(s, _) => format!("case \"{}\"", escape(s)),
            Pattern::Bool(b, _) => format!("case {}", b),
            Pattern::Variant { name, bindings, .. } => {
                // A `_` binding discards, and so does one the arm never reads —
                // Swift only warns on an unused capture, but the bar is a clean
                // build. `let _` would itself warn, so an unread binding is bare `_`.
                let binds: Vec<String> = bindings
                    .iter()
                    .map(|b| {
                        if b == "_" || !expr_mentions(body, b) {
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
                    _ => swift_case(name),
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
                if name == "none" && !self.t.in_scope("none") {
                    "nil".to_string()
                } else {
                    swift_ident(name)
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
                if matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
                ) && self.t.type_of(lhs) == Ty::Str
                {
                    // Compare strings by Unicode scalar, the way the interpreter, Rust,
                    // and Go do. Swift's own `==` and `<` fold canonically-equivalent
                    // spellings together and order by grapheme, so two byte-different
                    // strings could compare equal and a sort could disagree (#49).
                    let l0 = self.emit_expr(lhs);
                    let r0 = self.emit_expr(rhs);
                    let paren = |e: &Expr, s: String| {
                        if matches!(e, Expr::Binary { .. } | Expr::Unary { .. }) {
                            format!("({})", s)
                        } else {
                            s
                        }
                    };
                    let a = format!("{}.unicodeScalars", paren(lhs, l0));
                    let b = format!("{}.unicodeScalars", paren(rhs, r0));
                    match op {
                        BinOp::Eq => format!("{}.elementsEqual({})", a, b),
                        BinOp::Ne => format!("!{}.elementsEqual({})", a, b),
                        BinOp::Lt => format!("{}.lexicographicallyPrecedes({})", a, b),
                        BinOp::Gt => format!("{}.lexicographicallyPrecedes({})", b, a),
                        BinOp::Le => format!("!{}.lexicographicallyPrecedes({})", b, a),
                        _ => format!("!{}.lexicographicallyPrecedes({})", a, b),
                    }
                } else if matches!(op, BinOp::Div | BinOp::Mod) && self.t.type_of(lhs) == Ty::Int {
                    // Integer `/` and `%` guard the divisor, so a zero reports a lux
                    // error instead of trapping. Operands are call arguments, so no
                    // precedence parens; a nested division recurses here (#34).
                    let l = self.emit_expr(lhs);
                    let r = self.emit_expr(rhs);
                    let helper = if *op == BinOp::Div {
                        self.uses_lux_div = true;
                        "luxDiv"
                    } else {
                        self.uses_lux_mod = true;
                        "luxMod"
                    };
                    format!("{}({}, {})", helper, l, r)
                } else {
                    let p = bin_prec(*op);
                    let l = self.emit_child(lhs, p, false);
                    let r = self.emit_child(rhs, p, true);
                    // Integer +, -, * wrap on overflow, so Swift takes its masking
                    // operators rather than trapping — matching the interpreter and
                    // the other targets, which all wrap (#35). String and float `+`
                    // keep the plain operator; the masking forms share the same
                    // precedence, so nothing about parenthesising changes.
                    let sym = if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                        && self.t.type_of(lhs) == Ty::Int
                    {
                        match op {
                            BinOp::Add => "&+",
                            BinOp::Sub => "&-",
                            _ => "&*",
                        }
                    } else {
                        op_str(*op)
                    };
                    format!("{} {} {}", l, sym, r)
                }
            }
            Expr::Index { base, index, .. } => {
                let b = self.emit_expr(base);
                let idx = self.emit_expr(index);
                // Bounds-check an array read through the helper, which reports a lux
                // error instead of trapping (#38); Swift arrays are copy-on-write, so
                // passing the base to it doesn't copy. A write target stays a plain
                // subscript, its check emitted as a preceding statement.
                if matches!(self.t.type_of(base), Ty::Array(_)) && !self.assigning {
                    self.uses_lux_bounds = true;
                    format!("luxIndex({}, {})", b, idx)
                } else {
                    format!("{}[{}]", b, idx)
                }
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
                    return format!("{}.{}", n, swift_case(field));
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

    /// One argument to `print`/`eprint`. A compound value — array, struct, enum,
    /// or `Option` — is rendered through `luxShow` so it reads the way lux does;
    /// Swift's default drops an enum case's type and leaks the module name into an
    /// array. A scalar prints as Swift already renders it.
    fn print_arg(&mut self, a: &Expr) -> String {
        let ty = self.t.type_of(a);
        if matches!(ty, Ty::Array(_) | Ty::User(_) | Ty::Option(_)) {
            self.uses_lux_show = true;
            let e = self.print_show_expr(a, &ty);
            format!("({}).luxShow()", e)
        } else if ty == Ty::Float {
            // A float renders through `luxFloat`, positional and lux-readable, rather
            // than Swift's `String(Double)` exponent form for small values (#47).
            self.uses_lux_float = true;
            format!("luxFloat({})", self.emit_expr(a))
        } else {
            self.emit_expr(a)
        }
    }

    /// The Swift expression for a compound `print` argument. A bare `some`/`none`
    /// in print position has no context to fix the `Optional`'s element type, so
    /// name it explicitly (`Optional<Int>.some(7)`); everything else — a variable,
    /// a call — already carries its type. A bare `none` renders `none` whatever the
    /// element type, so an unknown one defaults harmlessly.
    fn print_show_expr(&mut self, a: &Expr, ty: &Ty) -> String {
        if let Ty::Option(inner) = ty {
            let inner_txt = match inner.as_ref() {
                Ty::Unknown => "Int".to_string(),
                t => ty_text(t),
            };
            if matches!(a, Expr::Ident(n, _) if n == "none") {
                return format!("Optional<{}>.none", inner_txt);
            }
            if let Expr::Call { name, args, .. } = a
                && name == "some"
            {
                let inner_e = self.emit_expr(&args[0]);
                return format!("Optional<{}>.some({})", inner_txt, inner_e);
            }
        }
        self.emit_expr(a)
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => {
                let parts: Vec<String> = args.iter().map(|a| self.print_arg(a)).collect();
                format!("print({})", parts.join(", "))
            }
            "eprint" => {
                self.uses_eprint = true;
                let parts: Vec<String> = args.iter().map(|a| self.print_arg(a)).collect();
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
            "input" => {
                self.uses_input = true;
                let p = match args.first() {
                    Some(a) => self.emit_expr(a),
                    None => "\"\"".to_string(),
                };
                format!("input({})", p)
            }
            "run" => {
                self.uses_run = true;
                let p = self.emit_expr(&args[0]);
                let a = self.emit_expr(&args[1]);
                format!("run({}, {})", p, a)
            }
            // `string` renders a value exactly as `print` does. A float goes through
            // `luxFloat` (positional, `.0` kept); a compound goes through `luxShow`,
            // the same as print, since `String(describing:)` renders a struct Swift's
            // way and an enum won't build at all (#54). A scalar uses `String(...)`.
            "string" => {
                let ty = self.t.type_of(&args[0]);
                if ty == Ty::Float {
                    self.uses_lux_float = true;
                    format!("luxFloat({})", self.emit_expr(&args[0]))
                } else if matches!(ty, Ty::Array(_) | Ty::User(_) | Ty::Option(_)) {
                    self.uses_lux_show = true;
                    let e = self.print_show_expr(&args[0], &ty);
                    format!("({}).luxShow()", e)
                } else {
                    format!("String({})", self.emit_expr(&args[0]))
                }
            }
            "int" => {
                let e = self.emit_expr(&args[0]);
                // A float goes through `luxInt`, which saturates `inf`/`NaN`/huge like
                // the other three rather than trapping as Swift's `Int(Double)` does
                // (#52); an int argument is already an Int.
                if self.t.type_of(&args[0]) == Ty::Float {
                    self.uses_lux_int = true;
                    format!("luxInt({})", e)
                } else {
                    format!("Int({})", e)
                }
            }
            "float" => {
                let e = self.emit_expr(&args[0]);
                format!("Double({})", e)
            }
            // Int(String) / Double(String) are failable, yielding the Optional
            // that is lux's Option here.
            "parseInt" => {
                self.uses_lux_parse = true;
                let e = self.emit_expr(&args[0]);
                format!("Int(luxTrim({}))", e)
            }
            "parseFloat" => {
                self.uses_lux_parse = true;
                let e = self.emit_expr(&args[0]);
                format!("Double(luxTrim({}))", e)
            }
            "length" => {
                let arg = &args[0];
                let e = self.emit_expr(arg);
                // Parenthesise a compound argument so `.count` binds to the whole
                // value, not just its right operand — `length(a + b)` (#50).
                let base = if matches!(arg, Expr::Binary { .. }) {
                    format!("({})", e)
                } else {
                    e
                };
                if self.t.type_of(arg) == Ty::Str {
                    // A lux string is a sequence of Unicode scalars, like the other
                    // three targets; Swift's `.count` is grapheme clusters, so a family
                    // emoji would measure 1 here and 5 everywhere else (#49).
                    format!("{}.unicodeScalars.count", base)
                } else {
                    format!("{}.count", base)
                }
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
                format!("{}({})", swift_ident(name), parts.join(", "))
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
            format!("{}.{}", enum_name, swift_case(variant))
        } else {
            format!(
                "{}.{}({})",
                enum_name,
                swift_case(variant),
                parts.join(", ")
            )
        }
    }
}

/// Render a float the way the interpreter does: positional, with a decimal point,
/// never exponent notation — the only form lux can parse back, since it has no
/// exponent literal (#47). `String(Double)` uses exponent form for small values, so
/// `luxExpand` rewrites any `1e-05` into `0.00001` from the same shortest digits;
/// infinities and NaN are normalised to `inf`/`-inf`/`NaN`, dropping Swift's `-nan`.
/// Stdlib only — no Foundation — so a float-only program stays dependency-free.
const LUX_FLOAT_HELPER: &str = "\
func luxFloat(_ f: Double) -> String {
    if f.isNaN { return \"NaN\" }
    if f.isInfinite { return f < 0 ? \"-inf\" : \"inf\" }
    var s = String(f)
    if let e = s.firstIndex(where: { $0 == \"e\" || $0 == \"E\" }) {
        let exp = Int(s[s.index(after: e)...]) ?? 0
        s = luxExpand(String(s[..<e]), exp)
    }
    if !s.contains(\".\") { s += \".0\" }
    return s
}

func luxExpand(_ mantissa: String, _ exp: Int) -> String {
    var m = mantissa
    var sign = \"\"
    if m.hasPrefix(\"-\") { sign = \"-\"; m.removeFirst() }
    else if m.hasPrefix(\"+\") { m.removeFirst() }
    let intLen = m.firstIndex(of: \".\").map { m.distance(from: m.startIndex, to: $0) } ?? m.count
    var digits = m
    if let d = digits.firstIndex(of: \".\") { digits.remove(at: d) }
    let point = intLen + exp
    let body: String
    if point <= 0 {
        body = \"0.\" + String(repeating: \"0\", count: -point) + digits
    } else if point >= digits.count {
        body = digits + String(repeating: \"0\", count: point - digits.count)
    } else {
        let idx = digits.index(digits.startIndex, offsetBy: point)
        body = String(digits[..<idx]) + \".\" + String(digits[idx...])
    }
    return sign + body
}
";

/// The `LuxShow` protocol and its conformances for the built-in types. Swift's
/// own printing renders a struct the way lux does but drops an enum case's type
/// and leaks the module name through an array, so print routes a compound value
/// through this instead. A user struct or enum gets its own conformance, generated
/// per program. A `Double` renders through `luxFloat`, positional at every scale.
const LUX_SHOW_PREAMBLE: &str = "\
protocol LuxShow {
    func luxShow() -> String
}

extension Int: LuxShow {
    func luxShow() -> String { String(self) }
}

extension Double: LuxShow {
    func luxShow() -> String { luxFloat(self) }
}

extension Bool: LuxShow {
    func luxShow() -> String { self ? \"true\" : \"false\" }
}

extension String: LuxShow {
    func luxShow() -> String { self }
}

extension Array: LuxShow where Element: LuxShow {
    func luxShow() -> String {
        \"[\" + self.map { $0.luxShow() }.joined(separator: \", \") + \"]\"
    }
}

extension Optional: LuxShow where Wrapped: LuxShow {
    func luxShow() -> String {
        switch self {
        case .some(let v): return \"some(\" + v.luxShow() + \")\"
        case .none: return \"none\"
        }
    }
}

";

/// A `LuxShow` conformance for one struct: `Name(field: value, …)`, each field
/// labelled with its lux name and read off `self`.
fn lux_show_struct(protocol_name: &str, name: &str, fields: &[FieldDef]) -> String {
    let body = if fields.is_empty() {
        format!("\"{}()\"", name)
    } else {
        let parts: Vec<String> = fields
            .iter()
            .map(|f| format!("\"{}: \" + self.{}.luxShow()", f.name, f.name))
            .collect();
        format!("\"{}(\" + {} + \")\"", name, parts.join(" + \", \" + "))
    };
    format!(
        "extension {}: {} {{\n    func luxShow() -> String {{\n        {}\n    }}\n}}\n\n",
        name, protocol_name, body
    )
}

/// A `LuxShow` conformance for one enum: `Enum.case` alone, or
/// `Enum.case(field: value, …)` with its associated values bound by label.
fn lux_show_enum(protocol_name: &str, name: &str, variants: &[VariantDef]) -> String {
    let mut arms = String::new();
    for v in variants {
        if v.fields.is_empty() {
            arms.push_str(&format!(
                "        case .{}: return \"{}.{}\"\n",
                swift_case(&v.name),
                name,
                v.name
            ));
        } else {
            let binds: Vec<String> = v.fields.iter().map(|f| format!("let {}", f.name)).collect();
            let parts: Vec<String> = v
                .fields
                .iter()
                .map(|f| format!("\"{}: \" + {}.luxShow()", f.name, f.name))
                .collect();
            arms.push_str(&format!(
                "        case .{}({}): return \"{}.{}(\" + {} + \")\"\n",
                swift_case(&v.name),
                binds.join(", "),
                name,
                v.name,
                parts.join(" + \", \" + ")
            ));
        }
    }
    format!(
        "extension {}: {} {{\n    func luxShow() -> String {{\n        switch self {{\n{}        }}\n    }}\n}}\n\n",
        name, protocol_name, arms
    )
}

/// Does this value emit as a leading-dot form — `.some(…)`, `.success(…)`,
/// `.failure(…)` — that Swift can't resolve without a contextual type? A bound
/// `some`/`ok`/`err` does, so an annotated binding to one must keep its annotation
/// for the type to land. A bare `none` is handled separately, through its open type.
fn needs_type_context(value: &Expr) -> bool {
    matches!(value, Expr::Call { name, .. } if name == "some" || name == "ok" || name == "err")
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

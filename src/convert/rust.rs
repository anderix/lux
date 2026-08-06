//! The Rust backend: emit real Rust source.
//!
//! Rust is the closest match to lux's shape — enums with values, `Option` and
//! `Result`, exhaustive `match`, value semantics — so most of the work is
//! cosmetic: `func` becomes `fn`, lux's lowercase enum cases become PascalCase
//! variants, camelCase names become snake_case. The one real wrinkle is that
//! Rust *moves* a non-`Copy` value when you pass or store it, while lux copies,
//! so a named place is cloned when it's moved into a call, an array, or a struct
//! field, preserving lux's semantics (see `emit_moved`).

use crate::ast::*;

use super::{
    Ty, Types, bin_prec, dodge_type_name, escape, expr_mentions, format_float, indent, is_place,
    mutated_roots, op_str, rust_ident, stmts_mention, to_pascal, to_snake, ty_from_ann,
};

struct Gen {
    t: Types,
    out: String,
    indent: usize,
    /// `readLine()` reads from the shared stdin handle the same way each call,
    /// which is a few lines; emit it once as a helper when the program asks.
    uses_read_line: bool,
    /// `input()` prompts and reads a plain line; it lowers to a helper built on
    /// `read_line`, so asking for it also pulls that one in.
    uses_input: bool,
    /// `run` needs the built-in `Output` struct and a helper that spawns a
    /// command, emitted once when the program reaches for it.
    uses_run: bool,
    /// Match bindings currently in scope that hold a `Box` — a recursive enum
    /// field. A read of one derefs the box so the arm sees the value, not the
    /// pointer. Used as a stack: an arm pushes its boxed captures and truncates
    /// back on the way out, so nested matches stay balanced.
    boxed: Vec<String>,
    /// Names ever mutated in the program, so a `var` that's only ever read binds
    /// immutably and doesn't draw Rust's "does not need to be mutable" warning.
    mutated: std::collections::HashSet<String>,
    /// The `var name: T` declarations (no starting value) that Rust can prove are
    /// assigned before they're read, keyed by span, so they defer their value
    /// (`let name: T;`) instead of taking an injected zero that would warn (#69).
    deferred_init: std::collections::HashSet<(usize, usize)>,
    /// The subset of `deferred_init` that a later path reassigns, so its deferred
    /// binding earns `let mut`; the rest stay a plain immutable `let`.
    deferred_mut: std::collections::HashSet<(usize, usize)>,
    /// `print` of a compound value routes through a generated `LuxShow` trait so
    /// the output reads the way lux renders it — `P(x: 1, y: 2)`,
    /// `Shape.circle(radius: 5)` — rather than Rust's `{:?}` (`P { x: 1, y: 2 }`,
    /// `Circle(5)`). Emitted only when the program prints a compound value.
    uses_lux_show: bool,
    /// The current function's array parameters emitted as `&Vec<…>` rather than by
    /// value — a read-only borrow, so a caller passing a grid to an accessor doesn't
    /// clone it every call (#28). Only functions that return a scalar qualify, since
    /// nothing slice-backed can then escape the callee, and lux already forbids
    /// writing through a parameter. Set on entry to such a function, cleared on exit.
    ref_params: std::collections::HashSet<String>,
    /// The name of the generated `LuxShow` trait, stepped aside if the program
    /// declares a type of that name so the two can't clash (#37).
    show_name: String,
    /// Integer `/` and `%` route through guard helpers that report a lux error on a
    /// zero divisor instead of panicking, so a learner meets `division by zero`
    /// rather than a Rust panic trace (#34). Emitted only when used.
    uses_lux_div: bool,
    uses_lux_mod: bool,
    /// Array indexing routes through bounds-checking helpers that report a lux error
    /// on an out-of-range index instead of panicking (#38). Emitted only when the
    /// program indexes an array.
    uses_lux_bounds: bool,
    /// True while emitting an assignment's target, so an indexed place emits as a
    /// checked write (`xs[lux_check(i, xs.len())]`) that stays assignable, rather
    /// than the read helper, which yields a borrow.
    assigning: bool,
    /// `print` and `string` of a float route through a `lux_float` helper so the
    /// output stays positional (`0.00001`, not `1e-5`) and lux can read it back —
    /// Rust's `{:?}` on an f64 uses exponent notation at the extremes (#47).
    uses_lux_float: bool,
    /// The string operations `contains`/`replace`/`split` each lower to a helper
    /// that guards the empty-pattern case; gated one at a time so an unused one
    /// never draws a dead-code warning.
    uses_lux_contains: bool,
    uses_lux_replace: bool,
    uses_lux_split: bool,
    /// `readFile`/`writeFile` route their error reason through `lux_io_reason`, which
    /// strips the ` (os error <n>)` tail Rust's `io::Error` appends, so the reason
    /// reads the same as it does on Swift and Go (#62). Emitted only when used.
    uses_io_reason: bool,
}

/// Render a float the way the interpreter does: positional, with a decimal point,
/// never exponent notation — which is the only form lux can parse back, since it
/// has no exponent literal (#47). A whole value keeps its `.0`; `{}` on an f64 is
/// already positional and prints `inf`/`-inf`/`NaN`, unlike `{:?}`.
const LUX_FLOAT_HELPER: &str = "\
fn lux_float(f: f64) -> String {
    if f.is_finite() && f == f.trunc() {
        format!(\"{:.1}\", f)
    } else {
        format!(\"{}\", f)
    }
}
";

/// Integer division and remainder that report a lux error on a zero divisor and
/// exit 1, the way the interpreter does, rather than letting Rust panic with a
/// backtrace about code the learner didn't write (#34).
const LUX_DIV_HELPER: &str = "\
fn lux_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        eprintln!(\"error: division by zero\");
        std::process::exit(1);
    }
    a / b
}
";

const LUX_MOD_HELPER: &str = "\
fn lux_mod(a: i64, b: i64) -> i64 {
    if b == 0 {
        eprintln!(\"error: remainder by zero\");
        std::process::exit(1);
    }
    a % b
}
";

/// Array bounds checking: `lux_check` validates an index and reports the
/// interpreter's own out-of-bounds error rather than letting Rust panic, and
/// `lux_index` borrows an element through it so a read evaluates its base once
/// (#38). A write indexes with `lux_check` directly, in place.
const LUX_BOUNDS_HELPER: &str = "\
fn lux_check(i: i64, len: usize) -> usize {
    if i < 0 || i as usize >= len {
        eprintln!(\"error: index {} is out of bounds for an array of length {}\", i, len);
        if len == 0 {
            eprintln!(\"note: this array is empty\");
        } else {
            eprintln!(\"note: valid indices are 0 to {}\", len - 1);
        }
        eprintln!(\"help: `lux learn arrays` — the first element is 0, so the last is length minus 1\");
        std::process::exit(1);
    }
    i as usize
}

fn lux_index<T>(xs: &[T], i: i64) -> &T {
    &xs[lux_check(i, xs.len())]
}
";

/// The string operations, each refusing an empty search/separator with the
/// interpreter's own error rather than one of the three answers the targets
/// disagree on. Rust's `str` methods are scalar-correct already — UTF-8 is
/// self-synchronizing, so they never match a partial scalar — so these wrap them.
/// Gated one at a time so an unused one never trips the warning-clean bar.
const LUX_CONTAINS_HELPER: &str = "\
fn lux_contains(s: &str, needle: &str) -> bool {
    if needle.is_empty() {
        eprintln!(\"error: the search text is empty\");
        eprintln!(\"note: contains looks for one piece of text inside another; there is nothing to look for here — check whether a variable came up empty\");
        eprintln!(\"help: `lux learn strings` — contains asks whether one string appears inside another\");
        std::process::exit(1);
    }
    s.contains(needle)
}
";

const LUX_REPLACE_HELPER: &str = "\
fn lux_replace(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        eprintln!(\"error: the text to replace is empty\");
        eprintln!(\"note: replace swaps one piece of text for another; there is nothing to swap out here — check whether a variable came up empty\");
        eprintln!(\"help: `lux learn strings` — replace swaps every occurrence of one string for another\");
        std::process::exit(1);
    }
    s.replace(from, to)
}
";

const LUX_SPLIT_HELPER: &str = "\
fn lux_split(s: &str, sep: &str) -> Vec<String> {
    if sep.is_empty() {
        eprintln!(\"error: the separator is empty\");
        eprintln!(\"note: split breaks text apart at a separator; an empty separator has no place to break — check whether a variable came up empty\");
        eprintln!(\"help: `lux learn arrays` — split breaks a string into an array of pieces at a separator\");
        std::process::exit(1);
    }
    s.split(sep).map(String::from).collect()
}
";

/// Reading one line, returning `None` at end of input — the helper `readLine()`
/// lowers to. Pulled out so a loop over input reads as one clean call.
const READ_LINE_HELPER: &str = "\
fn read_line() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\\n', '\\r']).to_string()),
    }
}
";

/// The helper `input()` lowers to: show the prompt on the same line, then read
/// one line, treating end of input as an empty string. Built on `read_line`.
const INPUT_HELPER: &str = "\
fn input(prompt: String) -> String {
    use std::io::Write;
    print!(\"{}\", prompt);
    let _ = std::io::stdout().flush();
    read_line().unwrap_or_default()
}
";

/// The built-in `Output` struct and the helper `run` lowers to. Rust's
/// `Command` mirrors lux's shape closely: a launch failure is the `Err`, and the
/// exit code rides inside `Output` on success. The child's input is closed off.
const RUN_HELPER: &str = "\
#[derive(Debug, Clone, PartialEq)]
struct Output {
    status: i64,
    stdout: String,
    stderr: String,
}

fn run(program: String, args: Vec<String>) -> Result<Output, String> {
    match std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) => Ok(Output {
            status: out.status.code().unwrap_or(-1) as i64,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }),
        Err(e) => Err(format!(\"could not run {}: {}\", program, e)),
    }
}
";

/// Render a file error's reason the way the other three legs do. `to_string()` on a
/// `std::io::Error` appends ` (os error <n>)` — the platform detail Swift's `strerror`
/// and Go's error string don't carry — so strip that tail and the reason matches:
/// `No such file or directory`, not `... (os error 2)` (#62).
const LUX_IO_REASON_HELPER: &str = "\
fn lux_io_reason(e: &std::io::Error) -> String {
    let full = e.to_string();
    match full.rfind(\" (os error \") {
        Some(cut)
            if full.ends_with(')')
                && cut + 11 < full.len() - 1
                && full[cut + 11..full.len() - 1].bytes().all(|b| b.is_ascii_digit()) =>
        {
            full[..cut].to_string()
        }
        _ => full,
    }
}
";

/// Translate a whole program to Rust source text.
pub fn to_rust(program: &[Stmt]) -> String {
    let (deferred_init, deferred_mut) = plan_deferred_vars(program);
    let mut g = Gen {
        t: Types::new(program),
        out: String::new(),
        indent: 0,
        uses_read_line: false,
        uses_input: false,
        uses_run: false,
        boxed: Vec::new(),
        mutated: mutated_roots(program),
        deferred_init,
        deferred_mut,
        uses_lux_show: false,
        ref_params: std::collections::HashSet::new(),
        uses_lux_div: false,
        uses_lux_mod: false,
        uses_lux_bounds: false,
        assigning: false,
        show_name: dodge_type_name("LuxShow", program),
        uses_lux_float: false,
        uses_lux_contains: false,
        uses_lux_replace: false,
        uses_lux_split: false,
        uses_io_reason: false,
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

    // A user `func main` is Rust's `fn main` directly — the entry point the student
    // just learned, generated as the idiomatic thing a Rust program would write, with
    // no wrapper to collide with (checks guarantee no top-level code runs beside it).
    // With no `main`, the top-level statements become the body of a generated `fn
    // main` instead.
    let has_main = program
        .iter()
        .any(|s| matches!(s, Stmt::Func { name, .. } if name == "main"));
    if !has_main {
        g.line("fn main() {".into());
        g.indent += 1;
        g.t.push_scope();
        g.emit_sigpipe_default();
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
    }

    let mut preamble = String::new();
    if g.uses_lux_float {
        preamble.push_str(LUX_FLOAT_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_div {
        preamble.push_str(LUX_DIV_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_mod {
        preamble.push_str(LUX_MOD_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_bounds {
        preamble.push_str(LUX_BOUNDS_HELPER);
        preamble.push('\n');
    }
    if g.uses_run {
        preamble.push_str(RUN_HELPER);
        preamble.push('\n');
    }
    if g.uses_io_reason {
        preamble.push_str(LUX_IO_REASON_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_contains {
        preamble.push_str(LUX_CONTAINS_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_replace {
        preamble.push_str(LUX_REPLACE_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_split {
        preamble.push_str(LUX_SPLIT_HELPER);
        preamble.push('\n');
    }
    if g.uses_read_line {
        preamble.push_str(READ_LINE_HELPER);
        preamble.push('\n');
    }
    if g.uses_input {
        preamble.push_str(INPUT_HELPER);
        preamble.push('\n');
    }
    if g.uses_lux_show {
        // The trait name steps aside from any user type of the same name, so the
        // prelude and a `struct LuxShow` can't collide (#37).
        preamble.push_str(&LUX_SHOW_PREAMBLE.replace("LuxShow", &g.show_name));
        // One `impl LuxShow` per user type, in declaration order, so a struct or
        // enum prints with lux's labels and its `Enum.case` form.
        for stmt in program {
            match stmt {
                Stmt::Struct { name, fields, .. } => {
                    preamble.push_str(&lux_show_struct(&g.show_name, name, fields))
                }
                Stmt::Enum { name, variants, .. } => {
                    preamble.push_str(&lux_show_enum(&g.show_name, name, variants))
                }
                _ => {}
            }
        }
    }
    format!("{}{}", preamble, g.out)
}

/// The `LuxShow` trait and its impls for the built-in types. A user struct or
/// enum gets its own impl, generated per program. Rust's own `{:?}` would render
/// a struct as `P { x: 1, y: 2 }` and a string as `"hi"`; lux renders `P(x: 1,
/// y: 2)` and bare text, so print routes a compound value through this instead.
/// The trait is ours, so implementing it for `Vec`/`Option`/`Result` is allowed
/// where implementing `Display` for them would not be.
const LUX_SHOW_PREAMBLE: &str = "\
trait LuxShow {
    fn lux_show(&self) -> String;
}

impl LuxShow for i64 {
    fn lux_show(&self) -> String {
        self.to_string()
    }
}

impl LuxShow for f64 {
    fn lux_show(&self) -> String {
        if self.is_finite() && *self == self.trunc() {
            format!(\"{:.1}\", self)
        } else {
            format!(\"{}\", self)
        }
    }
}

impl LuxShow for bool {
    fn lux_show(&self) -> String {
        self.to_string()
    }
}

impl LuxShow for String {
    fn lux_show(&self) -> String {
        self.clone()
    }
}

impl<T: LuxShow> LuxShow for Vec<T> {
    fn lux_show(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.lux_show()).collect();
        format!(\"[{}]\", parts.join(\", \"))
    }
}

impl<T: LuxShow> LuxShow for Option<T> {
    fn lux_show(&self) -> String {
        match self {
            Some(v) => format!(\"some({})\", v.lux_show()),
            None => \"none\".to_string(),
        }
    }
}

impl<T: LuxShow, E: LuxShow> LuxShow for Result<T, E> {
    fn lux_show(&self) -> String {
        match self {
            Ok(v) => format!(\"ok({})\", v.lux_show()),
            Err(e) => format!(\"err({})\", e.lux_show()),
        }
    }
}

";

/// `impl LuxShow` for one struct: `Name(field: value, …)`, each field labelled
/// with its lux name and read through its snake_case Rust field.
fn lux_show_struct(trait_name: &str, name: &str, fields: &[FieldDef]) -> String {
    let body = if fields.is_empty() {
        format!("\"{}()\".to_string()", name)
    } else {
        let parts: Vec<String> = fields
            .iter()
            .map(|f| {
                format!(
                    "format!(\"{}: {{}}\", self.{}.lux_show())",
                    f.name,
                    rust_ident(&to_snake(&f.name))
                )
            })
            .collect();
        format!(
            "let fields = [{}];\n        format!(\"{}({{}})\", fields.join(\", \"))",
            parts.join(", "),
            name
        )
    };
    format!(
        "impl {} for {} {{\n    fn lux_show(&self) -> String {{\n        {}\n    }}\n}}\n\n",
        trait_name, name, body
    )
}

/// `impl LuxShow` for one enum: `Enum.case` alone, or `Enum.case(field: value, …)`
/// with a payload bound positionally out of the tuple variant.
fn lux_show_enum(trait_name: &str, name: &str, variants: &[VariantDef]) -> String {
    let mut arms = String::new();
    for v in variants {
        let variant = to_pascal(&v.name);
        if v.fields.is_empty() {
            arms.push_str(&format!(
                "            {}::{} => \"{}.{}\".to_string(),\n",
                name, variant, name, v.name
            ));
        } else {
            let binds: Vec<String> = (0..v.fields.len()).map(|i| format!("f{}", i)).collect();
            let parts: Vec<String> = v
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| format!("format!(\"{}: {{}}\", f{}.lux_show())", f.name, i))
                .collect();
            arms.push_str(&format!(
                "            {}::{}({}) => {{\n                let fields = [{}];\n                format!(\"{}.{}({{}})\", fields.join(\", \"))\n            }}\n",
                name,
                variant,
                binds.join(", "),
                parts.join(", "),
                name,
                v.name
            ));
        }
    }
    format!(
        "impl {} for {} {{\n    fn lux_show(&self) -> String {{\n        match self {{\n{}        }}\n    }}\n}}\n\n",
        trait_name, name, arms
    )
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

/// The natural empty value for a `var` declared without one — the initializer a
/// deferred binding falls back to when Rust can't prove the variable is set before
/// it's read (see `plan_deferred_vars`).
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

/// A set of declarations, each identified by its source span `(start, end)`.
type SpanSet = std::collections::HashSet<(usize, usize)>;

/// How each `var name: T` — written with no starting value — should be emitted in
/// Rust, keyed by the declaration's span (so same-named vars in sibling scopes stay
/// distinct). Two sets:
///
/// - `deferred`: the variable is provably assigned before it is ever read, on every
///   path Rust's own analysis follows — straight-line, or both branches of an `if`.
///   Then the binding defers its value (`let name: T;`) and takes it on first
///   assignment, immutably. An injected `= zero` here would be a value Rust sees is
///   never read, and warn — the whole point of #69.
/// - `mutable`: the subset of `deferred` that some later path assigns a second time
///   — a sequential reassignment, or one inside a loop — and so must be `let mut`.
///
/// A declaration in neither set keeps the old `let mut name: T = zero` form: Rust
/// could not prove definite assignment (the only assignment is inside a loop, say),
/// so the initializer is genuinely reachable — it compiles, and reads cleanly,
/// because that zero really can be the value the program uses.
fn plan_deferred_vars(program: &[Stmt]) -> (SpanSet, SpanSet) {
    fn walk(stmts: &[Stmt], deferred: &mut SpanSet, mutable: &mut SpanSet) {
        for (i, s) in stmts.iter().enumerate() {
            if let Stmt::Var {
                value: None,
                ty: Some(_),
                name,
                span,
            } = s
            {
                // Assignments to and reads of the variable live in the statements
                // that follow it in this scope and their nested blocks.
                let rest = &stmts[i + 1..];
                let (reads_before_assign, _) = scan_before_assign(rest, name, false);
                if !reads_before_assign {
                    deferred.insert((span.start, span.end));
                    if assign_path_max(rest, name) >= 2 {
                        mutable.insert((span.start, span.end));
                    }
                }
            }
            match s {
                Stmt::Func { body, .. } | Stmt::While { body, .. } | Stmt::For { body, .. } => {
                    walk(body, deferred, mutable)
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, deferred, mutable);
                    if let Some(e) = else_body {
                        walk(e, deferred, mutable);
                    }
                }
                _ => {}
            }
        }
    }
    let mut deferred = std::collections::HashSet::new();
    let mut mutable = std::collections::HashSet::new();
    walk(program, &mut deferred, &mut mutable);
    (deferred, mutable)
}

/// Does some path read `name` before it is assigned? Modelled on Rust's own
/// definite-assignment analysis, and deliberately conservative: it counts a name
/// as assigned only where Rust surely would — a plain `name = …`, or both branches
/// of an `if` — so a loop body (which may not run) never marks it assigned. Returns
/// `(read_before_assign, assigned_on_every_path)`, given whether it arrived
/// assigned. When the answer is uncertain it errs toward "read before assign", so
/// the caller keeps the always-safe zero initializer rather than risk a deferred
/// binding Rust would reject.
fn scan_before_assign(stmts: &[Stmt], name: &str, mut assigned: bool) -> (bool, bool) {
    for s in stmts {
        match s {
            Stmt::Let { value, .. }
            | Stmt::Var {
                value: Some(value), ..
            } if !assigned && expr_mentions(value, name) => return (true, assigned),
            Stmt::Assign {
                target, op, value, ..
            } => {
                if !assigned && (expr_mentions(value, name) || target_index_reads(target, name)) {
                    return (true, assigned);
                }
                if target.place_root() == Some(name) {
                    // A plain `name = …` sets it; a compound target — `name.f = …`,
                    // `name[i] = …` — or a `+=`/`-=` reads it first.
                    if matches!(target, Expr::Ident(..)) && *op == AssignOp::Set {
                        assigned = true;
                    } else if !assigned {
                        return (true, assigned);
                    }
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                if !assigned && expr_mentions(cond, name) {
                    return (true, assigned);
                }
                let (v1, a1) = scan_before_assign(then_body, name, assigned);
                if v1 {
                    return (true, assigned);
                }
                let (v2, a2) = match else_body {
                    Some(e) => scan_before_assign(e, name, assigned),
                    None => (false, assigned),
                };
                if v2 {
                    return (true, assigned);
                }
                assigned = a1 && a2;
            }
            Stmt::While { cond, body, .. } => {
                if !assigned && expr_mentions(cond, name) {
                    return (true, assigned);
                }
                // The body may run zero times, so it can't make `name` assigned.
                if scan_before_assign(body, name, assigned).0 {
                    return (true, assigned);
                }
            }
            Stmt::For { iter, body, .. } => {
                if !assigned && expr_mentions(iter, name) {
                    return (true, assigned);
                }
                if scan_before_assign(body, name, assigned).0 {
                    return (true, assigned);
                }
            }
            Stmt::Return { value: Some(v), .. } | Stmt::Expr(v)
                if !assigned && expr_mentions(v, name) =>
            {
                return (true, assigned);
            }
            // A sibling function has its own scope and can't see this local.
            _ => {}
        }
    }
    (false, assigned)
}

/// Does an assignment target read `name` in one of its index positions —
/// `arr[name] = …` — as opposed to naming it as the place being written?
fn target_index_reads(target: &Expr, name: &str) -> bool {
    match target {
        Expr::Index { base, index, .. } => {
            expr_mentions(index, name) || target_index_reads(base, name)
        }
        Expr::Field { base, .. } => target_index_reads(base, name),
        _ => false,
    }
}

/// The most times `name` could be assigned on a single path through `stmts`,
/// capped at 2 — all the caller needs is whether it can exceed one. Branches of an
/// `if` are alternatives, so a path takes the heavier one; a loop that assigns at
/// all can repeat that assignment, so it counts as two.
fn assign_path_max(stmts: &[Stmt], name: &str) -> usize {
    let mut total = 0;
    for s in stmts {
        total = (total + stmt_assign_max(s, name)).min(2);
        if total >= 2 {
            return 2;
        }
    }
    total
}

fn stmt_assign_max(s: &Stmt, name: &str) -> usize {
    match s {
        Stmt::Assign { target, .. } => (target.place_root() == Some(name)) as usize,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            let t = assign_path_max(then_body, name);
            let e = else_body.as_ref().map_or(0, |b| assign_path_max(b, name));
            t.max(e)
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } if assign_path_max(body, name) >= 1 => 2,
        _ => 0,
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
                rust_ident(&to_snake(&f.name)),
                ty_text(&ty_from_ann(&f.ty))
            ));
        }
        self.line("}".into());
        self.blank();
    }

    /// A field needs a `Box` when its type re-enters the enum it sits in, by
    /// value — directly (`node(left: Tree)`) or through a cycle (`Expr` holding a
    /// `Fn` that holds an `Expr`). Without it the type has no finite size. An array
    /// or `Option` of the enum already carries its own indirection, so it isn't a
    /// `Ty::User` here and doesn't count.
    fn is_recursive_field(&self, enum_name: &str, ty: &Ty) -> bool {
        matches!(ty, Ty::User(n) if self.t.enum_reaches(n, enum_name))
    }

    fn emit_enum(&mut self, name: &str, variants: &[VariantDef]) {
        self.line("#[derive(Debug, Clone, PartialEq)]".into());
        self.line(format!("enum {} {{", name));
        for v in variants {
            if v.fields.is_empty() {
                self.line(format!("    {},", to_pascal(&v.name)));
            } else {
                let tys: Vec<String> = v
                    .fields
                    .iter()
                    .map(|f| {
                        let t = ty_from_ann(&f.ty);
                        let text = ty_text(&t);
                        // A field that stores the enum itself would make the type
                        // infinitely sized; a Box gives it a finite footprint.
                        if self.is_recursive_field(name, &t) {
                            format!("Box<{}>", text)
                        } else {
                            text
                        }
                    })
                    .collect();
                self.line(format!("    {}({}),", to_pascal(&v.name), tys.join(", ")));
            }
        }
        self.line("}".into());
        self.blank();
    }

    /// Does calling `name` return a scalar (or nothing)? Such a function can't hand
    /// any slice-backing back to its caller, so its array parameters are read-only
    /// borrows — see `param_is_ref` and `ref_params`. An unknown name is a built-in,
    /// which takes its arguments by value on its own path.
    fn returns_scalar(&self, name: &str) -> bool {
        match self.t.env.funcs.get(name) {
            Some((_, ret)) => {
                let rt = ret.as_ref().map(ty_from_ann).unwrap_or(Ty::Unit);
                rt.is_scalar() || rt == Ty::Unit
            }
            None => false,
        }
    }

    /// Is the `idx`-th parameter of `callee` emitted as a borrow? Only an array
    /// parameter of a scalar-returning function is: arrays are the values whose
    /// per-call clone actually hurts (a grid asked its size inside a loop, #28), and
    /// they read through a borrow cleanly — indexing, `len`, iteration, printing all
    /// auto-deref — where a matched enum or a string would need more care.
    fn param_is_ref(&self, callee: &str, idx: usize) -> bool {
        if !self.returns_scalar(callee) {
            return false;
        }
        self.t
            .env
            .funcs
            .get(callee)
            .and_then(|(ps, _)| ps.get(idx))
            .is_some_and(|p| matches!(ty_from_ann(&p.ty), Ty::Array(_)))
    }

    /// The first thing `main` does: hand SIGPIPE back to its default. Rust's runtime
    /// ignores SIGPIPE, so a closed pipe surfaces as an error `println!` unwraps into a
    /// panic — `program | head` prints a backtrace note about a line the learner never
    /// wrote. The interpreter and the Go and Swift builds all end quietly on the signal;
    /// this makes the Rust build the fourth to agree. `#[cfg(unix)]`, since SIGPIPE is a
    /// Unix idea and the panic it prevents is a Unix pipe.
    fn emit_sigpipe_default(&mut self) {
        self.line("// End quietly when a pipe closes (as `program | head` does), the way".into());
        self.line("// `lux run` and the Go and Swift builds do; Rust otherwise panics.".into());
        self.line("#[cfg(unix)]".into());
        self.line("{".into());
        self.indent += 1;
        self.line("unsafe extern \"C\" {".into());
        self.indent += 1;
        self.line("fn signal(sig: i32, handler: usize) -> usize;".into());
        self.indent -= 1;
        self.line("}".into());
        self.line("unsafe { signal(13, 0); } // SIGPIPE -> SIG_DFL".into());
        self.indent -= 1;
        self.line("}".into());
    }

    fn emit_func(&mut self, name: &str, params: &[Param], ret: Option<&TypeAnn>, body: &[Stmt]) {
        // An array parameter of a scalar-returning function is borrowed, not owned,
        // so a caller never clones it to pass it. Note which ones for the length of
        // this function, so a read of one derefs where it needs an owned value.
        let by_ref = ret
            .map(ty_from_ann)
            .map(|rt| rt.is_scalar() || rt == Ty::Unit)
            .unwrap_or(true);
        let this_refs: std::collections::HashSet<String> = params
            .iter()
            .filter(|p| by_ref && matches!(ty_from_ann(&p.ty), Ty::Array(_)))
            .map(|p| rust_ident(&to_snake(&p.name)))
            .collect();
        // Restore on exit, so a nested function or the top-level `main` doesn't
        // inherit this function's borrowed names.
        let saved_refs = std::mem::replace(&mut self.ref_params, this_refs);
        let ps: Vec<String> = params
            .iter()
            .map(|p| {
                let pty = ty_from_ann(&p.ty);
                let ident = rust_ident(&to_snake(&p.name));
                let text = ty_text(&pty);
                if self.ref_params.contains(&ident) {
                    format!("{}: &{}", ident, text)
                } else {
                    format!("{}: {}", ident, text)
                }
            })
            .collect();
        let r = ret
            .map(|t| format!(" -> {}", ty_text(&ty_from_ann(t))))
            .unwrap_or_default();
        self.line(format!(
            "fn {}({}){} {{",
            rust_ident(&to_snake(name)),
            ps.join(", "),
            r
        ));
        self.indent += 1;
        self.t.push_scope();
        for p in params {
            self.t.declare(p.name.clone(), ty_from_ann(&p.ty));
        }
        if name == "main" {
            self.emit_sigpipe_default();
        }
        for stmt in body {
            self.emit_stmt(stmt);
        }
        self.t.pop_scope();
        self.ref_params = saved_refs;
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
                span,
            } => {
                let vty = ty_from_ann(ann);
                let snake = rust_ident(&to_snake(name));
                let key = (span.start, span.end);
                self.t.declare(name.clone(), vty.clone());
                if self.deferred_init.contains(&key) {
                    // lux fills the variable in before it is read, and Rust can see
                    // that here — so defer its value rather than inject a zero it
                    // would flag as never read. `mut` only if a path reassigns it.
                    let kw = if self.deferred_mut.contains(&key) {
                        "let mut"
                    } else {
                        "let"
                    };
                    self.line(format!("{} {}: {};", kw, snake, ty_text(&vty)));
                } else {
                    // Rust couldn't prove definite assignment (an assignment only
                    // inside a loop, say), so keep the reachable zero initializer.
                    self.line(format!(
                        "let mut {}: {} = {};",
                        snake,
                        ty_text(&vty),
                        zero(&vty)
                    ));
                }
            }
            Stmt::Var { value: None, .. } => {} // a var with neither type nor value can't occur
            Stmt::Assign {
                target, op, value, ..
            } => self.emit_assign(target, *op, value),
            Stmt::Return { value, .. } => match value {
                // A returned value flows out of the function like any other value,
                // so it copies a non-Copy place the same way a binding or a call
                // argument does. Without this, `return row[c]` won't compile —
                // Rust can't move a `String` out of a `Vec` index (#20).
                Some(v) => {
                    let e = self.emit_moved(v);
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
        let snake = rust_ident(&to_snake(name));
        let vty = ann
            .map(ty_from_ann)
            .unwrap_or_else(|| self.t.type_of(value));
        // A bare `none` (or anything that leaves its type open) carries no type
        // of its own, so when the source named one, write it down for Rust.
        let value_open = self.t.type_of(value).has_unknown();
        let annotate = !vty.has_unknown() && ((ann.is_some() && value_open) || vty.has_int());
        // `let mut` only when the binding is actually mutated somewhere; a `var`
        // that's only read binds plainly, so Rust doesn't warn about an idle `mut`.
        let kw = if mutable && self.mutated.contains(name) {
            "let mut"
        } else {
            "let"
        };
        // Binding to a named non-Copy value copies it (lux semantics), so the
        // source stays usable — `var a = w` leaves `w` intact for a later read.
        let expr = self.emit_moved(value);
        if annotate {
            self.line(format!("{} {}: {} = {};", kw, snake, ty_text(&vty), expr));
        } else {
            self.line(format!("{} {} = {};", kw, snake, expr));
        }
        self.t.declare(name.to_string(), vty);
    }

    /// Emit a bounds-check statement for each array index in an assignment target,
    /// innermost first, so a write past the end reports a lux error before it runs
    /// (#38). Kept out of the target expression: a check borrows the array to read
    /// its length, which can't sit inside the assignment that borrows it mutably.
    /// Safe to name the base again, since an assignment target is rooted at a
    /// variable.
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
                    self.line(format!("lux_check({}, {}.len());", idx, b));
                }
            }
            Expr::Field { base, .. } => self.emit_index_guards(base),
            _ => {}
        }
    }

    fn emit_assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) {
        // The place reads the same on the left as anywhere else — `w.door_open`,
        // `items[i]`, or a plain name — and its type drives how `+=` lowers. An
        // indexed target is bounds-checked first, then emitted plainly so it stays
        // an assignable place.
        self.emit_index_guards(target);
        self.assigning = true;
        let lhs = self.emit_expr(target);
        self.assigning = false;
        let lty = self.t.type_of(target);
        match op {
            AssignOp::Set => {
                // A named non-Copy value assigned in whole is cloned, so the
                // source stays usable — lux copies where Rust would move.
                let e = self.emit_moved(value);
                self.line(format!("{} = {};", lhs, e));
            }
            AssignOp::Add => match lty {
                Ty::Str => {
                    // lux `+=` on a string appends text.
                    if let Expr::Str(s, _) = value {
                        self.line(format!("{}.push_str(\"{}\");", lhs, escape(s)));
                    } else {
                        let e = self.emit_expr(value);
                        self.line(format!("{}.push_str(&{});", lhs, e));
                    }
                }
                Ty::Array(_) => {
                    // lux `+=` on an array appends one element. Moving a named
                    // non-Copy value in would end its life, so it's cloned.
                    let e = self.emit_moved(value);
                    self.line(format!("{}.push({});", lhs, e));
                }
                _ => {
                    let e = self.emit_expr(value);
                    self.line(format!("{} += {};", lhs, e));
                }
            },
            AssignOp::Sub => {
                let e = self.emit_expr(value);
                self.line(format!("{} -= {};", lhs, e));
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
        // Through the same keyword mangler a use of the variable goes through, or
        // a name like `gen` (a Rust 2024 keyword) is written raw here but `gen_`
        // where it's read, and the two don't match.
        let svar = rust_ident(&to_snake(var));
        let (iter_str, elem_ty) = match self.t.type_of(iter) {
            Ty::Range => (self.emit_expr(iter), Ty::Int),
            Ty::Array(t) => {
                let base = self.emit_expr(iter);
                // A borrowed array parameter (`&Vec`) or an array element read
                // (`*lux_index(..)`) is behind a reference; parenthesize the deref
                // before the clone, or `.clone()` clones the pointer, not the array.
                let base = if matches!(iter, Expr::Ident(..)) && self.ref_params.contains(&base) {
                    format!("(*{})", base)
                } else if matches!(iter, Expr::Index { base: inner, .. } if matches!(self.t.type_of(inner), Ty::Array(_)))
                {
                    format!("({})", base)
                } else {
                    base
                };
                // Iterate a clone, so the loop walks a snapshot of the row as it
                // was when the loop began — and the borrow is released, letting the
                // body add to or assign into the original. `.iter().cloned()` would
                // hold the array borrowed for the whole loop, so `for x in xs { xs
                // += … }` wouldn't compile, where the interpreter, Swift's copy-on-
                // write, and Go's range-over-a-snapshot all accept it (#36).
                (format!("{}.clone()", base), *t)
            }
            _ => (self.emit_expr(iter), Ty::Unknown),
        };
        // A loop variable the body never reads becomes `_`, so Rust doesn't warn
        // about an unused variable in code the learner didn't write — the same
        // elision the match arms already do.
        let binder = if stmts_mention(body, var) {
            svar
        } else {
            "_".to_string()
        };
        self.line(format!("for {} in {} {{", binder, iter_str));
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
            // A bare integer literal defaults to `i32` in Rust, so one past that
            // range needs an explicit `i64` or it overflows the default type at
            // compile time — a large number in an expression (`3000000000 * 2`)
            // otherwise won't build, where the interpreter and the other targets
            // hold it fine. A binding already annotates its type; this covers the
            // literal wherever else it lands. Small literals stay unadorned.
            Expr::Int(n, _) => {
                if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                    n.to_string()
                } else {
                    format!("{}i64", n)
                }
            }
            Expr::Float(f, _) => format_float(*f),
            Expr::Str(s, _) => format!("\"{}\".to_string()", escape(s)),
            Expr::Bool(b, _) => b.to_string(),
            Expr::Ident(name, _) => {
                if name == "none" && !self.t.in_scope("none") {
                    "None".to_string()
                } else {
                    let id = rust_ident(&to_snake(name));
                    // A boxed capture is a `Box<T>`; deref it so the arm reads a `T`.
                    // Bare `*id`, not `(*id)`: as a match scrutinee the parens draw an
                    // `unused_parens` warning, and a following `.clone()` binds tighter
                    // than the deref anyway (`*id.clone()` is `*(id.clone())`).
                    if self.boxed.contains(&id) {
                        format!("*{}", id)
                    } else {
                        id
                    }
                }
            }
            Expr::Array(els, _) => {
                // An element takes ownership of the value put into it, so a named
                // non-Copy value is cloned — the source stays readable after the
                // array is built. Without this, naming a value and then listing it
                // (`let rect = [origin, …]`) moves it, and a later `print(origin)`
                // won't compile (#30). The same clone every other move site places.
                let parts: Vec<String> = els.iter().map(|x| self.emit_moved(x)).collect();
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
                } else if matches!(op, BinOp::Div | BinOp::Mod) && self.t.type_of(lhs) == Ty::Int {
                    // Integer `/` and `%` guard the divisor, so a zero reports a lux
                    // error instead of panicking. Operands are call arguments, so
                    // they need no precedence parens. Nesting handles itself — a
                    // divisor that is itself a division recurses through here (#34).
                    let l = self.emit_expr(lhs);
                    let r = self.emit_expr(rhs);
                    let helper = if *op == BinOp::Div {
                        self.uses_lux_div = true;
                        "lux_div"
                    } else {
                        self.uses_lux_mod = true;
                        "lux_mod"
                    };
                    format!("{}({}, {})", helper, l, r)
                } else {
                    let p = bin_prec(*op);
                    let mut l = self.emit_child(lhs, p, false);
                    // A trailing `as` cast to the left of `<` reads as the start of
                    // generic arguments in Rust (`x as i64 < 2` → `i64<…>`), so it
                    // has to be parenthesized there — and only there, or the parens
                    // are redundant and Rust warns.
                    if *op == BinOp::Lt && self.emits_trailing_cast(lhs) {
                        l = format!("({})", l);
                    }
                    let r = self.emit_child(rhs, p, true);
                    format!("{} {} {}", l, op_str(*op), r)
                }
            }
            Expr::Index { base, index, .. } => {
                let b = self.emit_expr(base);
                // Bounds-check an array index so an out-of-range one reports a lux
                // error, not a panic (#38). A read borrows through the helper, which
                // evaluates its base once, so a nested `grid[i][j]` stays clean; a
                // write checks the index in position, keeping an assignable place —
                // safe to name the base twice, since an assignment target is always
                // rooted at a variable.
                if matches!(self.t.type_of(base), Ty::Array(_)) && !self.assigning {
                    // A read borrows the element through the helper, which evaluates
                    // its base once. Borrow the base to pass it — unless it's already
                    // a borrowed array parameter (`&Vec`), passed straight through
                    // (#28). The deref is bare: parens would draw `unused_parens` in
                    // a plain value position, so a following `.clone()` or `.field`
                    // adds them itself (`emit_moved`, the `Field` arm), and a nested
                    // read re-borrows with `&*`.
                    self.uses_lux_bounds = true;
                    let idx = self.emit_expr(index);
                    let amp = if matches!(&**base, Expr::Ident(..)) && self.ref_params.contains(&b)
                    {
                        ""
                    } else {
                        "&"
                    };
                    format!("*lux_index({}{}, {})", amp, b, idx)
                } else {
                    // A write target (its bounds check emitted separately as a
                    // statement), or a non-array index: plain indexing.
                    let idx = if let Expr::Int(n, _) = **index {
                        n.to_string()
                    } else {
                        let e = self.emit_expr(index);
                        format!("({}) as usize", e)
                    };
                    format!("{}[{}]", b, idx)
                }
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
                        // A field takes ownership, so a named non-Copy value is
                        // cloned — the source stays readable afterward.
                        let val = self.emit_moved(v);
                        format!("{}: {}", rust_ident(&to_snake(k)), val)
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
                // An array element *read* is a bare deref (`*lux_index(..)`), and a
                // field access binds tighter than the deref, so parenthesize it. A
                // write target indexes plainly (no deref), so it's left as is.
                if !self.assigning
                    && matches!(&**base, Expr::Index { base: inner, .. } if matches!(self.t.type_of(inner), Ty::Array(_)))
                {
                    format!(
                        "(*{}).{}",
                        b.trim_start_matches('*'),
                        rust_ident(&to_snake(field))
                    )
                } else {
                    format!("{}.{}", b, rust_ident(&to_snake(field)))
                }
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

    /// True when this expression emits with a trailing `as` cast — `length`
    /// always, `int`/`float` unless the value is already that type. Such a cast
    /// needs parentheses before a `<`; see the `Binary` case.
    fn emits_trailing_cast(&self, e: &Expr) -> bool {
        match e {
            Expr::Call { name, args, .. } => match name.as_str() {
                "length" => true,
                "int" => args.first().is_some_and(|a| self.t.type_of(a) != Ty::Int),
                "float" => args.first().is_some_and(|a| self.t.type_of(a) != Ty::Float),
                _ => false,
            },
            _ => false,
        }
    }

    /// A value moved into a place the surrounding code still reads from — a call
    /// argument, an array element, a struct field. lux passes and stores by copy
    /// and the source keeps its own, so a named value of a non-`Copy` type is
    /// cloned to preserve that; Rust would otherwise move it and reject the next
    /// read. A temporary (a literal, a call result) is already owned, so it isn't
    /// a place and isn't cloned.
    fn emit_moved(&mut self, a: &Expr) -> String {
        let clone = !is_copy(&self.t.type_of(a)) && is_place(a, &self.t);
        let s = self.emit_expr(a);
        if clone {
            // A borrowed array parameter (`&Vec`) or an array element read
            // (`*lux_index(..)`) is behind a reference; the deref is parenthesized so
            // `.clone()` clones the value, not the pointer.
            if matches!(a, Expr::Ident(..)) && self.ref_params.contains(&s) {
                format!("(*{}).clone()", s)
            } else if matches!(a, Expr::Index { base, .. } if matches!(self.t.type_of(base), Ty::Array(_)))
            {
                format!("({}).clone()", s)
            } else {
                format!("{}.clone()", s)
            }
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

    /// A compound `print` argument rendered through `LuxShow`. A bare `some`/`none`
    /// in print position has no context to fix the `Option`'s element type, so pin
    /// it with a typed binding — everything else (a variable, a call) already
    /// carries its type. A bare `none` renders `none` whatever the element type, so
    /// an unknown one defaults harmlessly.
    fn print_show_arg(&mut self, a: &Expr) -> String {
        let e = self.emit_expr(a);
        let bare = matches!(a, Expr::Ident(n, _) if n == "none")
            || matches!(a, Expr::Call { name, .. } if name == "some");
        if bare && let Ty::Option(inner) = self.t.type_of(a) {
            let inner_txt = match *inner {
                Ty::Unknown => "i64".to_string(),
                t => ty_text(&t),
            };
            return format!(
                "{{ let __show: Option<{}> = {}; __show }}.lux_show()",
                inner_txt, e
            );
        }
        format!("({}).lux_show()", e)
    }

    /// `print` and `eprint` differ only in the macro they reach for — one writes
    /// stdout, the other stderr — so they share how arguments become a format.
    fn println_call(&mut self, mac: &str, args: &[Expr]) -> String {
        let mut fmt = String::new();
        let mut parts = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                fmt.push(' ');
            }
            let ty = self.t.type_of(a);
            // A `Result` is matched where it's produced, not printed — and Go can't
            // render it as one value — so it's kept off the faithful path on every
            // backend for consistency, staying on `{:?}` here.
            if matches!(ty, Ty::Array(_) | Ty::User(_) | Ty::Option(_)) {
                // A compound value renders lux's way through LuxShow, not Rust's
                // `{:?}` — `P(x: 1, y: 2)` rather than `P { x: 1, y: 2 }`.
                self.uses_lux_show = true;
                fmt.push_str("{}");
                parts.push(self.print_show_arg(a));
            } else if ty == Ty::Float {
                // A float renders through `lux_float`, positional at every magnitude,
                // so the output is text lux can read back — `{:?}` would print `1e-5`
                // for a small value (#47).
                self.uses_lux_float = true;
                fmt.push_str("{}");
                parts.push(format!("lux_float({})", self.emit_expr(a)));
            } else {
                // `{:?}` keeps a compound's decimal points; plain scalars use `{}`.
                fmt.push_str(if !ty.is_scalar() { "{:?}" } else { "{}" });
                parts.push(self.display_arg(a));
            }
        }
        if parts.is_empty() {
            format!("{}!()", mac)
        } else {
            format!("{}!(\"{}\", {})", mac, fmt, parts.join(", "))
        }
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => self.println_call("println", args),
            "eprint" => self.println_call("eprintln", args),
            // Each fallible call turns the target's native error into a string, so
            // the lux source stays `Result<_, string>` on this side too.
            // The failure string names the operation and the path before the
            // reason — `could not read <path>: <reason>` — the same shape the
            // interpreter and the other targets build, so one source reads the same
            // on all four (#43). The path is bound once so the closure can name it.
            "readFile" => {
                self.uses_io_reason = true;
                let p = self.emit_moved(&args[0]);
                format!(
                    "{{ let p = {p}; std::fs::read_to_string(&p).map_err(|e| format!(\"could not read {{}}: {{}}\", p, lux_io_reason(&e))) }}"
                )
            }
            "writeFile" => {
                self.uses_io_reason = true;
                let p = self.emit_moved(&args[0]);
                let c = self.emit_moved(&args[1]);
                format!(
                    "{{ let p = {p}; std::fs::write(&p, {c}).map_err(|e| format!(\"could not write {{}}: {{}}\", p, lux_io_reason(&e))) }}"
                )
            }
            "args" => "std::env::args().collect::<Vec<String>>()".to_string(),
            "readLine" => {
                self.uses_read_line = true;
                "read_line()".to_string()
            }
            "input" => {
                self.uses_input = true;
                self.uses_read_line = true;
                let p = match args.first() {
                    Some(a) => self.emit_moved(a),
                    None => "String::new()".to_string(),
                };
                format!("input({})", p)
            }
            "run" => {
                self.uses_run = true;
                let p = self.emit_moved(&args[0]);
                let a = self.emit_moved(&args[1]);
                format!("run({}, {})", p, a)
            }
            "string" => {
                // `string` renders a value exactly as `print` does. A float goes
                // through `lux_float` (positional, `.0` kept); a compound goes through
                // `LuxShow`, the same as print, since a struct or enum has no `Display`
                // and won't build under `to_string` (#54). A scalar is `to_string`.
                let ty = self.t.type_of(&args[0]);
                if ty == Ty::Float {
                    self.uses_lux_float = true;
                    format!("lux_float({})", self.emit_expr(&args[0]))
                } else if matches!(ty, Ty::Array(_) | Ty::User(_) | Ty::Option(_)) {
                    self.uses_lux_show = true;
                    self.print_show_arg(&args[0])
                } else {
                    format!("({}).to_string()", self.emit_expr(&args[0]))
                }
            }
            "int" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Int => e,
                    _ => format!("({}) as i64", e),
                }
            }
            "float" => {
                let inner = self.t.type_of(&args[0]);
                let e = self.emit_expr(&args[0]);
                match inner {
                    Ty::Float => e,
                    _ => format!("({}) as f64", e),
                }
            }
            // `.ok()` turns parse's Result into the Option lux returns.
            "parseInt" => {
                let e = self.emit_moved(&args[0]);
                format!("{}.trim().parse::<i64>().ok()", e)
            }
            "parseFloat" => {
                let e = self.emit_moved(&args[0]);
                format!("{}.trim().parse::<f64>().ok()", e)
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
            // The string operations borrow their arguments (`&(expr)` on an owned
            // String coerces to `&str`), so nothing is moved and a later read is fine.
            // A user function of the same name shadows the built-in.
            "contains" => {
                self.uses_lux_contains = true;
                format!(
                    "lux_contains(&({}), &({}))",
                    self.emit_expr(&args[0]),
                    self.emit_expr(&args[1])
                )
            }
            "replace" => {
                self.uses_lux_replace = true;
                format!(
                    "lux_replace(&({}), &({}), &({}))",
                    self.emit_expr(&args[0]),
                    self.emit_expr(&args[1]),
                    self.emit_expr(&args[2])
                )
            }
            "split" => {
                self.uses_lux_split = true;
                format!(
                    "lux_split(&({}), &({}))",
                    self.emit_expr(&args[0]),
                    self.emit_expr(&args[1])
                )
            }
            "some" => {
                let e = self.emit_moved(&args[0]);
                format!("Some({})", e)
            }
            "ok" => {
                let e = self.emit_moved(&args[0]);
                format!("Ok({})", e)
            }
            "err" => {
                let e = self.emit_moved(&args[0]);
                format!("Err({})", e)
            }
            _ => {
                let parts: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        // An array parameter of a scalar-returning callee is borrowed,
                        // so pass a reference and skip the clone. A borrowed array
                        // parameter is already a `&Vec`, so it passes straight through;
                        // any other array value is borrowed with `&`. Every other slot
                        // takes an owned value, cloning a place as before.
                        if self.param_is_ref(name, i) {
                            let s = self.emit_expr(a);
                            if matches!(a, Expr::Ident(..)) && self.ref_params.contains(&s) {
                                s
                            } else {
                                format!("&{}", s)
                            }
                        } else {
                            self.emit_moved(a)
                        }
                    })
                    .collect();
                format!("{}({})", rust_ident(&to_snake(name)), parts.join(", "))
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
        // (field name, is this field recursive) in declared order.
        let decl: Option<Vec<(String, bool)>> =
            self.t.env.enums.get(enum_name).and_then(|variants| {
                variants.iter().find(|v| v.name == variant).map(|v| {
                    v.fields
                        .iter()
                        .map(|f| {
                            (
                                f.name.clone(),
                                self.is_recursive_field(enum_name, &ty_from_ann(&f.ty)),
                            )
                        })
                        .collect()
                })
            });
        let args: Vec<String> = match decl {
            Some(names) => names
                .iter()
                .map(|(fname, rec)| {
                    let expr = fields.iter().find(|(k, _)| k == fname).map(|(_, e)| e);
                    // A field takes ownership, so a named non-Copy value is cloned.
                    let s = match expr {
                        Some(e) => self.emit_moved(e),
                        None => "()".to_string(),
                    };
                    // A recursive field is stored behind a Box, so wrap the value.
                    if *rec { format!("Box::new({})", s) } else { s }
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
            // A bare-deref array read (`*lux_index(..)`) needs parens before
            // `.as_str()`, which binds tighter than the deref.
            let s = if matches!(scrutinee, Expr::Index { base, .. } if matches!(self.t.type_of(base), Ty::Array(_)))
            {
                format!("({})", s)
            } else {
                s
            };
            format!("{}.as_str()", s)
        } else {
            self.emit_expr(scrutinee)
        };
        let mut s = format!("match {} {{\n", scrut);
        for arm in arms {
            let pat = self.emit_pattern(&arm.pattern, &st, &arm.body);
            // Bring the pattern's captures into scope so the arm body types
            // correctly (a captured string should print without quotes).
            self.t.push_scope();
            self.declare_bindings(&arm.pattern, &st);
            // A capture that came out of a boxed (recursive) field is read through
            // a deref; note which ones for the length of this arm.
            let mark = self.boxed.len();
            let captures = self.boxed_captures(&arm.pattern, &st);
            self.boxed.extend(captures);
            // Arm bodies sit one level in, so a nested match nests cleanly.
            self.indent = base + 1;
            let body = self.emit_expr(&arm.body);
            self.indent = base;
            self.boxed.truncate(mark);
            self.t.pop_scope();
            s.push_str(&format!("{}{} => {},\n", ind1, pat, body));
        }
        s.push_str(&format!("{}}}", ind));
        s
    }

    fn emit_pattern(&mut self, pat: &Pattern, st: &Ty, body: &Expr) -> String {
        match pat {
            Pattern::Wildcard(_) => "_".to_string(),
            Pattern::Int(n, _) => n.to_string(),
            Pattern::Str(s, _) => format!("\"{}\"", escape(s)),
            Pattern::Bool(b, _) => b.to_string(),
            Pattern::Variant { name, bindings, .. } => {
                // A binding the arm never reads becomes `_`: Rust only warns on an
                // unused capture, but the backend's bar is warning-clean output.
                let binds: Vec<String> = bindings
                    .iter()
                    .map(|b| {
                        if b != "_" && !expr_mentions(body, b) {
                            "_".to_string()
                        } else {
                            to_snake(b)
                        }
                    })
                    .collect();
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

    /// The emitted names of this pattern's captures that came out of a recursive
    /// (boxed) field, so a read of one can deref the box.
    fn boxed_captures(&self, pat: &Pattern, st: &Ty) -> Vec<String> {
        let Pattern::Variant { name, bindings, .. } = pat else {
            return Vec::new();
        };
        let Ty::User(en) = st else {
            return Vec::new();
        };
        let field_tys: Vec<Ty> = self
            .t
            .env
            .enums
            .get(en)
            .and_then(|vs| vs.iter().find(|v| v.name == *name))
            .map(|v| v.fields.iter().map(|f| ty_from_ann(&f.ty)).collect())
            .unwrap_or_default();
        bindings
            .iter()
            .zip(field_tys)
            .filter(|(b, t)| b.as_str() != "_" && self.is_recursive_field(en, t))
            .map(|(b, _)| rust_ident(&to_snake(b)))
            .collect()
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

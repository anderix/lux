//! Running a lux program from inside another program.
//!
//! `lux run` treats a file as a process: it starts at the top (or at `main`),
//! prints to the terminal, reads the keyboard, and runs until it ends. A host — a
//! game, an editor, a test harness — wants something else: load a program once,
//! call its functions over and over with values the host supplies, collect what
//! each call printed, and never let it hang, touch the file system, or start
//! another program. This module is that way in.
//!
//! A hosted program is declarations only: `struct`, `enum`, and `func` at the top
//! level, no loose statements and no `main`. The built-ins that reach outside the
//! program — `readLine`, `input`, `args`, `readFile`, `writeFile`, `run` — are
//! refused when the program is loaded, as check errors, because a host may be
//! running code written by someone else. `print` and `eprint` go into the call's
//! captured output instead of the terminal.
//!
//! [`load`] checks a program into a [`Program`]; [`instance`] starts an interpreter
//! for it; [`Instance::call`] runs one declared function under [`Limits`]. A call
//! that fails — a runtime error, a budget, a value grown past the cap — leaves
//! nothing behind: lux values are immutable and an instance keeps no state between
//! calls, so the host simply keeps the values it had. Every call starts from its
//! arguments alone, which is what makes swapping in a new [`Program`] between
//! calls safe; [`Program::fits`] says whether the old values still suit the new
//! declarations.
//!
//! A host may also offer the program functions of its own, with [`load_with`]: a
//! game might offer `say(text: string, room: string)`. The program calls one like
//! any function, the checker holds it to the offered signature, and the call is
//! recorded rather than run: it comes back in [`CallResult::effects`], in order,
//! with the argument values, for the host to act on once the call has returned.
//! An offered function hands nothing back, because the host isn't there while
//! the program runs, and that is what keeps the sandbox whole: the program still
//! can't reach anything the host didn't hand it.
//!
//! A two-room world, played for two turns:
//!
//! ```
//! use lux::host::{self, Limits, Value};
//!
//! let program = host::load(r#"
//! enum Room { cell  yard }
//! struct World { room: Room, steps: int }
//!
//! func fresh() -> World {
//!     return World(room: Room.cell, steps: 0)
//! }
//!
//! func step(w: World, cmd: string) -> World {
//!     if cmd == "east" {
//!         print("You step out into the yard.")
//!         return World(room: Room.yard, steps: w.steps + 1)
//!     }
//!     print("You can't go that way.")
//!     return World(room: w.room, steps: w.steps + 1)
//! }
//! "#).expect("the world checks");
//!
//! let limits = Limits { steps: 10_000, depth: None, output_bytes: 64 << 10, value_bytes: 1 << 20 };
//! let mut world = host::instance(&program, limits).expect("a worker thread");
//! let mut w = world.call("fresh", vec![]).value.expect("fresh runs");
//! for cmd in ["north", "east"] {
//!     let turn = world.call("step", vec![w.clone(), Value::string(cmd)]);
//!     for line in &turn.output {
//!         println!("{line}");
//!     }
//!     w = turn.value.expect("step runs");
//! }
//! assert_eq!(w.get_field("room").and_then(Value::variant_name), Some("yard"));
//! assert_eq!(w.get_field("steps"), Some(&Value::Int(2)));
//! ```

// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use crate::ast::{Expr, FieldDef, Param, Stmt, TypeAnn, TypeKind, VariantDef};
use crate::diagnostic::{LuxError, Span};
use crate::interpreter::{HostConfig, Interp, PAYLOAD, count, value_type};
use crate::{check, convert, lexer, parser};

pub use crate::interpreter::{Effect, OutputLine, Value};

/// How deep calls may nest in a hosted call unless [`Limits::depth`] says
/// otherwise. A hosted function that recurses thousands deep is a runaway, and a
/// low ceiling catches it in milliseconds rather than after paging in gigabytes
/// of stack. `lux run` keeps its own, higher, limit.
pub const DEFAULT_DEPTH: usize = 2_000;

/// Stack reserved per nested call on an instance's worker thread: the ratio
/// `lux run` uses, and enough for an unoptimised build's frames.
const STACK_PER_CALL: usize = 128 << 10;

/// The span of an error that has no place in the source: a value the host
/// built, a function it asked for.
const NOWHERE: Span = Span { start: 0, end: 0 };

/// The limits one hosted call runs under. Every field is required: a host
/// running someone else's code has no business with "unlimited".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most steps one call may take, counting each statement run, each pass
    /// of a loop, and each call made. Past it the call stops with an error.
    pub steps: u64,
    /// How deep calls may nest; [`DEFAULT_DEPTH`] when `None`.
    pub depth: Option<usize>,
    /// The most bytes of output one call may produce. Lines past it are dropped
    /// and [`CallResult::output_truncated`] says so.
    pub output_bytes: usize,
    /// The most bytes one value may hold: a string's bytes, plus a fixed cost for
    /// every scalar, array slot, and field, counting nested copies. A string or
    /// array that doubles on each pass of a loop meets this long before it meets
    /// the step budget.
    pub value_bytes: usize,
}

/// What one call produced.
#[derive(Debug)]
pub struct CallResult {
    /// The function's return value, or the error that stopped it: a runtime
    /// error, the step budget, the size cap, or the depth limit.
    pub value: Result<Value, LuxError>,
    /// Everything the call printed, in order.
    pub output: Vec<OutputLine>,
    /// True when output past `Limits::output_bytes` was dropped.
    pub output_truncated: bool,
    /// Steps the call took.
    pub steps_used: u64,
    /// Every call the program made to a function the host offered through
    /// [`load_with`], in order. Empty when the call failed.
    pub effects: Vec<Effect>,
}

impl CallResult {
    fn failed(err: LuxError) -> CallResult {
        CallResult {
            value: Err(err),
            output: Vec::new(),
            output_truncated: false,
            steps_used: 0,
            effects: Vec::new(),
        }
    }
}

/// A declared function's parameters and return type.
#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<Param>,
    pub ret: Option<TypeAnn>,
}

/// A declared struct's fields or enum's cases, in declared order.
#[derive(Debug, Clone, Copy)]
pub enum TypeDecl<'a> {
    Struct(&'a [FieldDef]),
    Enum(&'a [VariantDef]),
}

/// A program that has been lexed, parsed, and checked as a hosted program. It
/// knows its declarations and can say whether a value fits one of its types; an
/// [`Instance`] runs it.
#[derive(Debug, Clone)]
pub struct Program {
    stmts: Arc<Vec<Stmt>>,
    funcs: HashMap<String, FuncSig>,
    structs: HashMap<String, Vec<FieldDef>>,
    enums: HashMap<String, Vec<VariantDef>>,
    offers: Vec<String>,
}

/// Lex, parse, and check `source` as a hosted program. The errors are the same
/// `LuxError`s `lux run` reports, with the hosted-program rules first: nothing
/// loose at the top level, no `main`, none of the built-ins that reach outside.
/// A call to a function that isn't there, or with the wrong number of values, is
/// refused here too, as `lux build` refuses it, rather than on the turn that
/// happens to reach it.
pub fn load(source: &str) -> Result<Program, Vec<LuxError>> {
    load_with(source, &[])
}

/// [`load`], with functions the host offers the program. Each offer is a lux
/// signature without a body or a return type, `func say(text: string, room:
/// string)`; its parameter types are lux's built-in types or types the program
/// declares. The program calls an offered function as it would its own, and each
/// call comes back to the host in [`CallResult::effects`] instead of running. A
/// program that declares a function under an offered name is refused, and so is
/// an offer that isn't a signature of that shape, since that is the host's own
/// mistake.
pub fn load_with(source: &str, offers: &[&str]) -> Result<Program, Vec<LuxError>> {
    let one = |e: LuxError| vec![e];
    let mut stmts = Vec::new();
    let mut names = Vec::new();
    for offer in offers {
        let func = parse_offer(offer).map_err(one)?;
        if let Stmt::Func { name, .. } = &func {
            names.push(name.clone());
        }
        stmts.push(func);
    }
    let program = lexer::lex(source).and_then(parser::parse).map_err(one)?;
    host_rules(&program).map_err(one)?;
    for s in &program {
        if let Stmt::Func { name, span, .. } = s
            && names.contains(name)
        {
            return Err(one(LuxError::new(
                format!(
                    "`{name}` is a function the host offers, so this program can't declare its own"
                ),
                *span,
            )
            .with_note("call it as it is; the host does the work once your function returns")
            .with_learn(
                "host",
                "a host may offer your program functions of its own",
            )));
        }
    }
    stmts.extend(program);
    check::check(&stmts).map_err(one)?;
    check::check_before_emit(&stmts).map_err(one)?;
    convert::type_check(&stmts).map_err(one)?;
    Ok(Program::new(stmts, names))
}

/// An offer as the host wrote it, parsed as a function with an empty body.
fn parse_offer(offer: &str) -> Result<Stmt, LuxError> {
    let bad = |why: &str| {
        value_error(
            format!("the host's offer `{offer}` {why}"),
            "an offer is a signature like `func say(text: string, room: string)`: no body, no return type",
        )
    };
    let mut stmts = lexer::lex(&format!("{offer} {{}}"))
        .and_then(parser::parse)
        .map_err(|e| bad(&format!("doesn't parse: {}", e.message)))?;
    match stmts.pop() {
        Some(Stmt::Func { ret: Some(_), .. }) => Err(bad(
            "has a return type, but an offered function hands nothing back",
        )),
        Some(func @ Stmt::Func { .. }) if stmts.is_empty() => Ok(func),
        _ => Err(bad("isn't one function signature")),
    }
}

impl Program {
    fn new(stmts: Vec<Stmt>, offers: Vec<String>) -> Program {
        let mut funcs = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        for s in &stmts {
            match s {
                Stmt::Func { name, .. } if offers.contains(name) => {}
                Stmt::Func {
                    name, params, ret, ..
                } => {
                    funcs.insert(
                        name.clone(),
                        FuncSig {
                            params: params.clone(),
                            ret: ret.clone(),
                        },
                    );
                }
                Stmt::Struct { name, fields, .. } => {
                    structs.insert(name.clone(), fields.clone());
                }
                Stmt::Enum { name, variants, .. } => {
                    enums.insert(name.clone(), variants.clone());
                }
                _ => {}
            }
        }
        Program {
            stmts: Arc::new(stmts),
            funcs,
            structs,
            enums,
            offers,
        }
    }

    /// The signature of a declared function. A function the host offered isn't
    /// one; see [`Program::offers`].
    pub fn func(&self, name: &str) -> Option<&FuncSig> {
        self.funcs.get(name)
    }

    /// The names of the functions the host offered when it loaded this program.
    pub fn offers(&self) -> &[String] {
        &self.offers
    }

    /// A declared struct or enum.
    pub fn type_decl(&self, name: &str) -> Option<TypeDecl<'_>> {
        if let Some(fields) = self.structs.get(name) {
            return Some(TypeDecl::Struct(fields));
        }
        self.enums.get(name).map(|v| TypeDecl::Enum(v))
    }

    /// Does `value` fit the type this program declares under `type_name`? The
    /// same check every argument to [`Instance::call`] gets. A host uses it to
    /// decide whether values saved under an older program carry over to a new
    /// one: a value still fits after a new enum case or a new function, and no
    /// longer fits after a field is added, removed, or retyped. The error names
    /// the field or element that doesn't fit.
    pub fn fits(&self, type_name: &str, value: &Value) -> Result<(), LuxError> {
        let ann = TypeAnn {
            kind: TypeKind::Named(type_name.to_string()),
            span: NOWHERE,
        };
        self.conform(&ann, value, type_name).map(|_| ())
    }

    /// Check a value against a type and hand back the value as the program
    /// declares it: struct and case fields in declared order, so the interpreter
    /// finds them where it expects to. `path` names the value in the error —
    /// `w.items[2]`, `World.doorOpen` — so a host knows what to fix.
    fn conform(&self, ann: &TypeAnn, v: &Value, path: &str) -> Result<Value, LuxError> {
        if let Value::Range(..) = v {
            return Err(value_error(
                format!("`{path}` is a range, which no declared type holds"),
                "a range only exists inside a `for`; pass the numbers it counts instead",
            ));
        }
        match &ann.kind {
            TypeKind::Named(n) => match n.as_str() {
                "int" | "float" | "string" | "bool" | "Unit" => {
                    let fits = matches!(
                        (n.as_str(), v),
                        ("int", Value::Int(_))
                            | ("float", Value::Float(_))
                            | ("string", Value::Str(_))
                            | ("bool", Value::Bool(_))
                            | ("Unit", Value::Unit)
                    );
                    if fits {
                        Ok(v.clone())
                    } else {
                        Err(mismatch(path, n, v))
                    }
                }
                _ => {
                    if let Some(fields) = self.structs.get(n) {
                        self.conform_struct(n, fields, v, path)
                    } else if let Some(variants) = self.enums.get(n) {
                        self.conform_enum(n, variants, v, path)
                    } else {
                        Err(value_error(
                            format!(
                                "`{path}` names the type `{n}`, which this program doesn't declare"
                            ),
                            "check the spelling against the program's struct and enum names",
                        ))
                    }
                }
            },
            TypeKind::Array(elem) => match v {
                Value::Array(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, it) in items.iter().enumerate() {
                        out.push(self.conform(elem, it, &format!("{path}[{i}]"))?);
                    }
                    Ok(Value::Array(out))
                }
                other => Err(mismatch(path, &ann.to_string(), other)),
            },
            TypeKind::Generic(name, args) => match v {
                Value::Enum {
                    enum_name,
                    variant,
                    fields,
                } if enum_name == name => {
                    let payload_ty = match (name.as_str(), variant.as_str()) {
                        ("Option", "none") if fields.is_empty() => return Ok(v.clone()),
                        ("Option", "some") | ("Result", "ok") => &args[0],
                        ("Result", "err") => &args[1],
                        _ => {
                            return Err(value_error(
                                format!(
                                    "`{path}` is `{name}.{variant}`, which isn't a case of `{name}`"
                                ),
                                "Option is some(x) or none; Result is ok(x) or err(e)",
                            ));
                        }
                    };
                    let [(_, payload)] = fields.as_slice() else {
                        return Err(value_error(
                            format!(
                                "`{path}` is `{variant}` carrying {} values, but it carries exactly one",
                                fields.len()
                            ),
                            "build it with Value::some, or Value::case_with and a single value",
                        ));
                    };
                    let inner = self.conform(payload_ty, payload, &format!("{path}.{variant}"))?;
                    Ok(Value::Enum {
                        enum_name: name.clone(),
                        variant: variant.clone(),
                        fields: vec![(PAYLOAD.to_string(), inner)],
                    })
                }
                other => Err(mismatch(path, &ann.to_string(), other)),
            },
        }
    }

    fn conform_struct(
        &self,
        name: &str,
        declared: &[FieldDef],
        v: &Value,
        path: &str,
    ) -> Result<Value, LuxError> {
        let Value::Struct { name: got, fields } = v else {
            return Err(mismatch(path, name, v));
        };
        if got != name {
            return Err(mismatch(path, name, v));
        }
        let built = self.conform_fields(declared, fields, path, &format!("struct `{name}`"))?;
        Ok(Value::Struct {
            name: name.to_string(),
            fields: built,
        })
    }

    fn conform_enum(
        &self,
        name: &str,
        variants: &[VariantDef],
        v: &Value,
        path: &str,
    ) -> Result<Value, LuxError> {
        let Value::Enum {
            enum_name,
            variant,
            fields,
        } = v
        else {
            return Err(mismatch(path, name, v));
        };
        if enum_name != name {
            return Err(mismatch(path, name, v));
        }
        let Some(vdef) = variants.iter().find(|vd| &vd.name == variant) else {
            let cases: Vec<&str> = variants.iter().map(|vd| vd.name.as_str()).collect();
            return Err(value_error(
                format!(
                    "`{path}` is `{name}.{variant}`, but enum `{name}` has no case `{variant}`"
                ),
                format!("cases are: {}", cases.join(", ")),
            ));
        };
        let built =
            self.conform_fields(&vdef.fields, fields, path, &format!("`{name}.{variant}`"))?;
        Ok(Value::Enum {
            enum_name: name.to_string(),
            variant: variant.to_string(),
            fields: built,
        })
    }

    /// Match provided labelled values against declared fields: every declared
    /// field present, nothing extra, each value fitting its type, and the result
    /// in declared order.
    fn conform_fields(
        &self,
        declared: &[FieldDef],
        provided: &[(String, Value)],
        path: &str,
        owner: &str,
    ) -> Result<Vec<(String, Value)>, LuxError> {
        for (k, _) in provided {
            if !declared.iter().any(|f| &f.name == k) {
                return Err(value_error(
                    format!("`{path}` has a field `{k}`, but {owner} declares no such field"),
                    "the program's declaration decides the fields; drop this one or rename it",
                ));
            }
        }
        let mut built = Vec::with_capacity(declared.len());
        for f in declared {
            let Some((_, value)) = provided.iter().find(|(k, _)| k == &f.name) else {
                return Err(value_error(
                    format!(
                        "`{path}` is missing the field `{}` that {owner} declares",
                        f.name
                    ),
                    format!("{owner} has a field `{}: {}`", f.name, f.ty),
                ));
            };
            built.push((
                f.name.clone(),
                self.conform(&f.ty, value, &format!("{path}.{}", f.name))?,
            ));
        }
        Ok(built)
    }
}

fn value_error(message: String, note: impl Into<String>) -> LuxError {
    LuxError::new(message, NOWHERE).with_note(note)
}

fn mismatch(path: &str, expected: &str, got: &Value) -> LuxError {
    value_error(
        format!("`{path}` should be {expected}, but got {}", value_type(got)),
        "the value a host passes has to match the type the program declares",
    )
}

/// The built-ins a hosted program may not call, each with why it's off and
/// what to do instead. This is a security rule, not a convenience: a host may
/// be running code written by someone else.
const SANDBOXED: &[(&str, &str, &str)] = &[
    (
        "readLine",
        "there is no keyboard or standard input to read from",
        "the host hands your function everything it needs as arguments — use those",
    ),
    (
        "input",
        "there is no keyboard or standard input to read from",
        "the host hands your function everything it needs as arguments — use those",
    ),
    (
        "args",
        "there is no command line",
        "the host hands your function everything it needs as arguments — use those",
    ),
    (
        "readFile",
        "a hosted program can't reach the file system",
        "hand the value back from your function and let the host decide what to keep",
    ),
    (
        "writeFile",
        "a hosted program can't reach the file system",
        "hand the value back from your function and let the host decide what to keep",
    ),
    (
        "run",
        "a hosted program can't start other programs",
        "hand the value back from your function and let the host decide what to do with it",
    ),
];

/// What a program has to be before a host will take it: declarations only, no
/// `main`, and none of the built-ins that reach outside the program.
fn host_rules(program: &[Stmt]) -> Result<(), LuxError> {
    for stmt in program {
        match stmt {
            Stmt::Func { name, span, .. } if name == "main" => {
                return Err(LuxError::new(
                    "`main` has no job here — the host decides which function runs, and when",
                    *span,
                )
                .with_note("give it the name the host calls it by; every function here is a place the host can start")
                .with_learn("main", "main is the entry point when the language picks it, not when a host does"));
            }
            Stmt::Func { .. } | Stmt::Struct { .. } | Stmt::Enum { .. } => {}
            other => {
                return Err(LuxError::new(
                    "this line would run on its own, but a hosted program only declares — the host decides when its functions run",
                    other.span(),
                )
                .with_note("move it into a `func` the host calls; the top level holds only `struct`, `enum`, and `func`")
                .with_learn("functions", "a function is a piece of work that waits to be called"));
            }
        }
    }
    each_call(program, &mut |name, span| {
        let Some((_, why, instead)) = SANDBOXED.iter().find(|(b, _, _)| *b == name) else {
            return Ok(());
        };
        Err(
            LuxError::new(format!("`{name}` isn't available here — {why}"), span)
                .with_note(*instead)
                .with_learn(
                    "io",
                    "the outside world is the host's to offer, and here it offers none",
                ),
        )
    })
}

type CallVisitor<'a> = dyn FnMut(&str, Span) -> Result<(), LuxError> + 'a;

/// Visit every call in a program, in source order.
fn each_call(stmts: &[Stmt], f: &mut CallVisitor<'_>) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Expr(value) => each_call_expr(value, f)?,
            Stmt::Var { value, .. } | Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    each_call_expr(v, f)?;
                }
            }
            Stmt::Assign { target, value, .. } => {
                each_call_expr(target, f)?;
                each_call_expr(value, f)?;
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                each_call_expr(cond, f)?;
                each_call(then_body, f)?;
                if let Some(e) = else_body {
                    each_call(e, f)?;
                }
            }
            Stmt::While { cond, body, .. } => {
                each_call_expr(cond, f)?;
                each_call(body, f)?;
            }
            Stmt::For { iter, body, .. } => {
                each_call_expr(iter, f)?;
                each_call(body, f)?;
            }
            Stmt::Func { body, .. } => each_call(body, f)?,
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }
    Ok(())
}

fn each_call_expr(e: &Expr, f: &mut CallVisitor<'_>) -> Result<(), LuxError> {
    match e {
        Expr::Call { name, args, span } => {
            f(name, *span)?;
            for a in args {
                each_call_expr(a, f)?;
            }
        }
        Expr::Array(items, _) => {
            for x in items {
                each_call_expr(x, f)?;
            }
        }
        Expr::Unary { rhs, .. } => each_call_expr(rhs, f)?,
        Expr::Binary { lhs, rhs, .. } => {
            each_call_expr(lhs, f)?;
            each_call_expr(rhs, f)?;
        }
        Expr::Index { base, index, .. } => {
            each_call_expr(base, f)?;
            each_call_expr(index, f)?;
        }
        Expr::Range { start, end, .. } => {
            each_call_expr(start, f)?;
            each_call_expr(end, f)?;
        }
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            for (_, v) in fields {
                each_call_expr(v, f)?;
            }
        }
        Expr::Field { base, .. } => each_call_expr(base, f)?,
        Expr::Match {
            scrutinee, arms, ..
        } => {
            each_call_expr(scrutinee, f)?;
            for arm in arms {
                each_call_expr(&arm.body, f)?;
            }
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Ident(..) => {}
    }
    Ok(())
}

/// A running interpreter for one [`Program`]. It lives on its own worker thread
/// with a stack sized for the depth limit, so a host calls it from any ordinary
/// thread and deep recursion ends in a lux error, never a crashed process. An
/// `Instance` holds only the channel to its worker, so it is `Send`, and a host
/// may keep as many as it likes, of the same program or different ones.
pub struct Instance {
    requests: Sender<Request>,
    program: Program,
}

struct Request {
    func: String,
    args: Vec<Value>,
    reply: Sender<CallResult>,
}

/// Start an interpreter for `program` under `limits`. The only way this fails is
/// the operating system refusing a thread.
pub fn instance(program: &Program, limits: Limits) -> std::io::Result<Instance> {
    let depth = limits.depth.unwrap_or(DEFAULT_DEPTH);
    let cfg = HostConfig {
        steps: limits.steps,
        depth,
        output_bytes: limits.output_bytes,
        value_bytes: limits.value_bytes,
        offers: program.offers.clone(),
    };
    let stmts = Arc::clone(&program.stmts);
    let (requests, inbox) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("lux host".to_string())
        .stack_size(depth.saturating_mul(STACK_PER_CALL))
        .spawn(move || {
            let mut interp = Interp::for_host(&stmts, cfg);
            for req in inbox {
                let outcome = interp.call_values(&req.func, req.args);
                let _ = req.reply.send(CallResult {
                    value: outcome.value,
                    output: outcome.output,
                    output_truncated: outcome.output_truncated,
                    steps_used: outcome.steps_used,
                    effects: outcome.effects,
                });
            }
        })?;
    Ok(Instance {
        requests,
        program: program.clone(),
    })
}

impl Instance {
    /// The program this instance runs.
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Call a declared function with values the host built. Each argument is
    /// checked against the parameter it fills before anything runs, so a value
    /// of the wrong shape comes back as a clear error rather than a confusing
    /// one from inside the body.
    pub fn call(&mut self, func: &str, args: Vec<Value>) -> CallResult {
        if self.program.offers.iter().any(|o| o == func) {
            return CallResult::failed(value_error(
                format!("`{func}` is a function the host offers, not one it can call"),
                "the program calls it, and the host reads the call back in CallResult::effects",
            ));
        }
        let Some(sig) = self.program.func(func) else {
            return CallResult::failed(value_error(
                format!("this program has no function `{func}`"),
                "the host can only call a `func` the program declares at its top level",
            ));
        };
        if args.len() != sig.params.len() {
            return CallResult::failed(value_error(
                format!(
                    "`{func}` takes {} but was called with {}",
                    count(sig.params.len(), "value"),
                    args.len()
                ),
                "pass one value per parameter, in the order the function declares them",
            ));
        }
        let mut conformed = Vec::with_capacity(args.len());
        for (p, a) in sig.params.iter().zip(&args) {
            match self.program.conform(&p.ty, a, &p.name) {
                Ok(v) => conformed.push(v),
                Err(e) => return CallResult::failed(e),
            }
        }
        let (reply, answer) = mpsc::channel();
        let request = Request {
            func: func.to_string(),
            args: conformed,
            reply,
        };
        if self.requests.send(request).is_err() {
            return CallResult::failed(stopped());
        }
        answer
            .recv()
            .unwrap_or_else(|_| CallResult::failed(stopped()))
    }
}

/// The error for an instance whose worker is gone. The worker only ends when
/// the instance is dropped or the interpreter panicked, so a host should never
/// meet this; if it does, a new instance is the way on.
fn stopped() -> LuxError {
    value_error(
        "this instance has stopped and can't take calls".to_string(),
        "start a new one with `host::instance`",
    )
}

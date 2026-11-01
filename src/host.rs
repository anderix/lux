//! Run a lux program from inside another program: load it once, call its
//! functions with values, read back what each call printed. A hosted program
//! holds only `struct`, `enum`, and `func`; `readLine`, `input`, `args`,
//! `readFile`, `writeFile`, and `run` are refused when it is loaded.
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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use crate::ast::{Expr, FieldDef, Param, Stmt, TypeAnn, TypeKind, VariantDef};
use crate::diagnostic::{LuxError, Span};
use crate::interpreter::{HostConfig, Interp, PAYLOAD, count, value_type};
use crate::{check, convert, lexer, parser};

pub use crate::interpreter::{OutputLine, Value};

/// How deep calls may nest when `Limits::depth` is `None`.
pub const DEFAULT_DEPTH: usize = 2_000;

const STACK_PER_CALL: usize = 128 << 10;

const NOWHERE: Span = Span { start: 0, end: 0 };

/// The limits one call runs under; none of them can be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Statements run, loop passes taken, and calls made before the call is stopped.
    pub steps: u64,
    /// How deep calls may nest; `DEFAULT_DEPTH` when `None`.
    pub depth: Option<usize>,
    /// Bytes of output kept; lines past it are dropped and flagged.
    pub output_bytes: usize,
    /// Bytes one value may hold, counting nested copies; a value grown past it stops the call.
    pub value_bytes: usize,
}

/// What one call produced.
#[derive(Debug)]
pub struct CallResult {
    /// The return value, or the error that stopped the call.
    pub value: Result<Value, LuxError>,
    /// Everything the call printed, in order.
    pub output: Vec<OutputLine>,
    /// True when output past `Limits::output_bytes` was dropped.
    pub output_truncated: bool,
    /// Steps the call took.
    pub steps_used: u64,
}

impl CallResult {
    fn failed(err: LuxError) -> CallResult {
        CallResult {
            value: Err(err),
            output: Vec::new(),
            output_truncated: false,
            steps_used: 0,
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

/// A program lexed, parsed, and checked as a hosted program.
#[derive(Debug, Clone)]
pub struct Program {
    stmts: Arc<Vec<Stmt>>,
    funcs: HashMap<String, FuncSig>,
    structs: HashMap<String, Vec<FieldDef>>,
    enums: HashMap<String, Vec<VariantDef>>,
}

/// Lex, parse, and check `source` as a hosted program.
pub fn load(source: &str) -> Result<Program, Vec<LuxError>> {
    let one = |e: LuxError| vec![e];
    let stmts = lexer::lex(source).and_then(parser::parse).map_err(one)?;
    host_rules(&stmts).map_err(one)?;
    check::check(&stmts).map_err(one)?;
    convert::type_check(&stmts).map_err(one)?;
    Ok(Program::new(stmts))
}

impl Program {
    fn new(stmts: Vec<Stmt>) -> Program {
        let mut funcs = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        for s in &stmts {
            match s {
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
        }
    }

    /// The signature of a declared function.
    pub fn func(&self, name: &str) -> Option<&FuncSig> {
        self.funcs.get(name)
    }

    /// A declared struct or enum.
    pub fn type_decl(&self, name: &str) -> Option<TypeDecl<'_>> {
        if let Some(fields) = self.structs.get(name) {
            return Some(TypeDecl::Struct(fields));
        }
        self.enums.get(name).map(|v| TypeDecl::Enum(v))
    }

    /// Whether `value` fits the type declared under `type_name`; the error names the field or element that doesn't.
    pub fn fits(&self, type_name: &str, value: &Value) -> Result<(), LuxError> {
        let ann = TypeAnn {
            kind: TypeKind::Named(type_name.to_string()),
            span: NOWHERE,
        };
        self.conform(&ann, value, type_name).map(|_| ())
    }

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

/// A running interpreter for one `Program`, on its own worker thread.
pub struct Instance {
    requests: Sender<Request>,
    program: Program,
}

struct Request {
    func: String,
    args: Vec<Value>,
    reply: Sender<CallResult>,
}

/// Start an interpreter for `program` under `limits`; fails only if the system refuses a thread.
pub fn instance(program: &Program, limits: Limits) -> std::io::Result<Instance> {
    let depth = limits.depth.unwrap_or(DEFAULT_DEPTH);
    let cfg = HostConfig {
        steps: limits.steps,
        depth,
        output_bytes: limits.output_bytes,
        value_bytes: limits.value_bytes,
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

    /// Call a declared function; each argument is checked against its parameter before anything runs.
    pub fn call(&mut self, func: &str, args: Vec<Value>) -> CallResult {
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

fn stopped() -> LuxError {
    value_error(
        "this instance has stopped and can't take calls".to_string(),
        "start a new one with `host::instance`",
    )
}

//! The interpreter: walk the ast and run it.
//!
//! This is a tree-walking interpreter. It keeps a stack of scopes (one per
//! block) holding the live bindings, plus a table of function definitions.
//! lux is statically typed by design, but has no separate type checker
//! yet — instead the interpreter enforces lux's no-coercion rule at the moment
//! of each operation, so `"5" + 3` fails with a clear error rather than
//! silently guessing. A real checker that catches these before the program
//! runs is a later milestone.

use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use crate::ast::*;
use crate::diagnostic::{LuxError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Array(Vec<Value>),
    /// A half-open range, the thing a `for i in 0..5` walks.
    Range(i64, i64),
    /// A struct value: a named record of labelled fields, in declared order.
    Struct {
        name: String,
        fields: Vec<(String, Value)>,
    },
    /// An enum value: one named case of an enum, carrying its labelled values.
    Enum {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Value)>,
    },
    /// The result of something done only for its effect, like `print`.
    Unit,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bool(_) => "bool",
            Value::Array(_) => "array",
            Value::Range(..) => "range",
            Value::Struct { .. } => "struct",
            Value::Enum { .. } => "enum",
            Value::Unit => "nothing",
        }
    }
}

/// Where a binding came from. Only a `var` can be assigned to; the rest hold
/// still, but each refuses a change in its own words. A parameter and a loop
/// variable have no `var` to reach for, so the note that fits a `let` — "use
/// var instead" — would send a learner into a wall, chasing syntax that isn't
/// there. Match keeps the `let` wording because the learner literally typed it.
#[derive(Clone, Copy, PartialEq)]
enum BindKind {
    Let,
    Var,
    Param,
    Loop,
    Match,
}

impl BindKind {
    fn mutable(self) -> bool {
        matches!(self, BindKind::Var)
    }
}

struct Binding {
    value: Value,
    kind: BindKind,
}

/// A user-defined function, stored once and shared (via `Rc`) so a call can
/// hold onto it while the interpreter keeps mutating its scopes — which is what
/// recursion needs.
struct FuncData {
    params: Vec<Param>,
    ret: Option<TypeAnn>,
    body: Vec<Stmt>,
}

/// A declared struct type: its fields, in declared order.
struct StructData {
    fields: Vec<FieldDef>,
}

/// A declared enum type: its cases, in declared order.
struct EnumData {
    variants: Vec<VariantDef>,
}

/// How a statement finished: either normally, or by hitting a `return`.
enum Flow {
    Normal,
    Return(Value),
}

struct Interp {
    scopes: Vec<HashMap<String, Binding>>,
    funcs: HashMap<String, Rc<FuncData>>,
    structs: HashMap<String, Rc<StructData>>,
    enums: HashMap<String, Rc<EnumData>>,
    /// What `args()` returns: the program's own command line, the program
    /// itself at index 0. Supplied by the caller so the interpreter shows the
    /// script's arguments, not `lux run`'s.
    program_args: Vec<String>,
    /// When set (by `lux trace`), narrate each executing line and the state it
    /// changes to stderr, leaving the program's own output on stdout untouched.
    trace: Option<Tracer>,
}

/// The line-by-line narrator behind `lux trace`. It holds the source so it can
/// reprint each executing line, and writes to stderr — so the program's own
/// output on stdout stays clean and can be watched, muted, or captured on its
/// own with a shell redirect.
struct Tracer {
    source: String,
}

/// The column the state note starts at, when the source line is shorter.
const TRACE_NOTE_COL: usize = 44;

impl Tracer {
    /// The 1-based line number and text of the line a span sits on.
    fn line_of(&self, span: Span) -> (usize, &str) {
        let at = span.start.min(self.source.len());
        let line_start = self.source[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = self.source[line_start..]
            .find('\n')
            .map(|i| line_start + i)
            .unwrap_or(self.source.len());
        let no = self.source[..line_start]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        (no, self.source[line_start..line_end].trim_end())
    }

    /// Emit one trace line: the line number, the source line (indentation kept,
    /// so nesting shows), and a note about what just happened — a new value, a
    /// loop variable, an answer read from input.
    fn step(&self, span: Span, note: &str) {
        // Push any pending program output to the screen first, so a line that
        // prints shows its trace line and its output in the order they happen —
        // even when the two streams are merged with a shell redirect.
        let _ = std::io::stdout().flush();
        let (no, text) = self.line_of(span);
        let left = format!("{:>4}  {}", no, text);
        if note.is_empty() {
            eprintln!("{left}");
            return;
        }
        let width = left.chars().count();
        if width >= TRACE_NOTE_COL {
            eprintln!("{left}  {note}");
        } else {
            eprintln!("{left}{}{note}", " ".repeat(TRACE_NOTE_COL - width));
        }
    }
}

/// Run a parsed program. `program_args` is the program's command line —
/// the script (or binary) at index 0, then anything the user passed after it.
pub fn run(program: &[Stmt], program_args: &[String]) -> Result<(), LuxError> {
    run_with(program, program_args, None)
}

/// Like `run`, but narrate each executing line and the state it changes to
/// stderr. The program itself runs identically — same stdout, same stdin — so a
/// traced program plays exactly as it would under `run`, with the trace riding
/// alongside on a separate stream.
pub fn run_traced(program: &[Stmt], program_args: &[String], source: &str) -> Result<(), LuxError> {
    run_with(
        program,
        program_args,
        Some(Tracer {
            source: source.to_string(),
        }),
    )
}

fn run_with(
    program: &[Stmt],
    program_args: &[String],
    trace: Option<Tracer>,
) -> Result<(), LuxError> {
    let mut interp = Interp {
        scopes: vec![HashMap::new()],
        funcs: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
        program_args: program_args.to_vec(),
        trace,
    };
    // Option and Result are built-in enums, registered before anything else so
    // they exist for type-checking and so a user can't redeclare their names.
    interp.register_builtin_types();
    // Collect every type and function up front, so a program can refer to them
    // before they appear in the file. Types come first because the validation
    // pass and the functions can mention them.
    interp.register_types(program)?;
    interp.validate_type_decls(program)?;
    interp.register_funcs(program)?;
    interp.exec_block(program)?;
    Ok(())
}

impl Interp {
    /// Register the built-in enums `Option` (`some`/`none`) and `Result`
    /// (`ok`/`err`). They are ordinary enum values under the hood, so `match`,
    /// exhaustiveness, and printing all work the same way they do for the
    /// enums a program declares itself. Their cases carry no declared fields
    /// here — the payload's type lives on each value, since these are generic.
    fn register_builtin_types(&mut self) {
        let variant = |name: &str| VariantDef {
            name: name.to_string(),
            fields: Vec::new(),
            span: Span::new(0, 0),
        };
        self.enums.insert(
            "Option".to_string(),
            Rc::new(EnumData {
                variants: vec![variant("some"), variant("none")],
            }),
        );
        self.enums.insert(
            "Result".to_string(),
            Rc::new(EnumData {
                variants: vec![variant("ok"), variant("err")],
            }),
        );
        // `Output` is the one built-in struct: what `run` hands back on success.
        // Registering it here reserves the name and lets a program spell it in a
        // type annotation, the way `Option` and `Result` are reserved above.
        let field = |name: &str, ty: &str| FieldDef {
            name: name.to_string(),
            ty: TypeAnn {
                kind: TypeKind::Named(ty.to_string()),
                span: Span::new(0, 0),
            },
            span: Span::new(0, 0),
        };
        self.structs.insert(
            "Output".to_string(),
            Rc::new(StructData {
                fields: vec![
                    field("status", "int"),
                    field("stdout", "string"),
                    field("stderr", "string"),
                ],
            }),
        );
    }

    /// Collect every top-level `struct` and `enum`, checking for name clashes.
    fn register_types(&mut self, program: &[Stmt]) -> Result<(), LuxError> {
        for s in program {
            match s {
                Stmt::Struct { name, fields, span } => {
                    if self.type_exists(name) {
                        return Err(already_defined(name, *span));
                    }
                    self.structs.insert(
                        name.clone(),
                        Rc::new(StructData {
                            fields: fields.clone(),
                        }),
                    );
                }
                Stmt::Enum {
                    name,
                    variants,
                    span,
                } => {
                    if self.type_exists(name) {
                        return Err(already_defined(name, *span));
                    }
                    self.enums.insert(
                        name.clone(),
                        Rc::new(EnumData {
                            variants: variants.clone(),
                        }),
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn type_exists(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }

    /// Now that every type is registered, confirm each struct field and enum
    /// case names a type that actually exists.
    fn validate_type_decls(&self, program: &[Stmt]) -> Result<(), LuxError> {
        for s in program {
            match s {
                Stmt::Struct { fields, .. } => {
                    for f in fields {
                        self.validate_type(&f.ty)?;
                    }
                }
                Stmt::Enum { variants, .. } => {
                    for v in variants {
                        for f in &v.fields {
                            self.validate_type(&f.ty)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Collect every top-level `func` up front, so a program can call a
    /// function before it appears in the file.
    fn register_funcs(&mut self, program: &[Stmt]) -> Result<(), LuxError> {
        for s in program {
            if let Stmt::Func {
                name,
                params,
                ret,
                body,
                span,
            } = s
            {
                if self.funcs.contains_key(name) {
                    return Err(LuxError::new(
                        format!("function `{}` is already defined", name),
                        *span,
                    ));
                }
                self.funcs.insert(
                    name.clone(),
                    Rc::new(FuncData {
                        params: params.clone(),
                        ret: ret.clone(),
                        body: body.clone(),
                    }),
                );
            }
        }
        Ok(())
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn current_has(&self, name: &str) -> bool {
        self.scopes.last().unwrap().contains_key(name)
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }

    fn declare(&mut self, name: String, value: Value, kind: BindKind) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name, Binding { value, kind });
    }

    // ----- statements -------------------------------------------------------

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Flow, LuxError> {
        for s in stmts {
            // A bare expression (usually print) is announced before it runs, so
            // its trace line comes ahead of the output it puts on screen. A value
            // binding narrates after, so its note can carry the value it landed
            // on. Compound statements (if/while/for) and input calls narrate from
            // inside their own handlers, where the per-iteration and per-read
            // state is in hand.
            let effect = matches!(s, Stmt::Expr(_));
            if effect {
                self.trace_leaf(s);
            }
            let flow = self.exec_stmt(s)?;
            if !effect {
                self.trace_leaf(s);
            }
            match flow {
                Flow::Normal => {}
                ret @ Flow::Return(_) => return Ok(ret),
            }
        }
        Ok(Flow::Normal)
    }

    /// If tracing, narrate a leaf statement: a `let`/`var`/`assign` with the
    /// value the named variable now holds, or a bare expression as a plain line
    /// (its effect shows on stdout). Anything else narrates elsewhere, so this
    /// quietly returns. The caller decides whether to call it before or after the
    /// statement runs.
    fn trace_leaf(&self, s: &Stmt) {
        let Some(t) = &self.trace else { return };
        match s {
            Stmt::Let { name, span, .. } | Stmt::Var { name, span, .. } => {
                if let Some(b) = self.lookup(name) {
                    t.step(*span, &format!("{} = {}", name, trace_render(&b.value)));
                }
            }
            // An assignment narrates the root binding it changed — after a
            // `w.doorOpen = true`, the whole `w` it now holds.
            Stmt::Assign { target, span, .. } => {
                if let Some(root) = target.place_root()
                    && let Some(b) = self.lookup(root)
                {
                    t.step(*span, &format!("{} = {}", root, trace_render(&b.value)));
                }
            }
            // A bare expression — usually a print — runs for its effect; its
            // output lands on stdout, so the trace just marks that the line ran.
            Stmt::Expr(e) => t.step(e.span(), ""),
            _ => {}
        }
    }

    /// If tracing, narrate a line with a given note. Used by the compound
    /// statements and input calls, which narrate at points a leaf binding can't.
    fn trace_at(&self, span: Span, note: &str) {
        if let Some(t) = &self.trace {
            t.step(span, note);
        }
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Flow, LuxError> {
        match stmt {
            Stmt::Let {
                name,
                ty,
                value,
                span,
            } => {
                let v = self.eval(value)?;
                // A Result is answered where it's produced, not stashed for later.
                if is_result(&v) {
                    return Err(result_not_stored(value.span()));
                }
                match ty {
                    Some(ann) => self.check_type(ann, &v)?,
                    // A bare `let x = none` still needs an annotation, but a call
                    // to a fallible built-in pins its own type — `let line =
                    // readLine()` is fine even on the `none` it returns at EOF.
                    None => {
                        if !call_pins_type(value, &self.funcs) {
                            ensure_determined(&v, value.span())?;
                        }
                    }
                }
                if self.current_has(name) {
                    return Err(LuxError::new(
                        format!("`{}` is already declared in this scope", name),
                        *span,
                    ));
                }
                self.declare(name.clone(), v, BindKind::Let);
                Ok(Flow::Normal)
            }

            Stmt::Var {
                name,
                ty,
                value,
                span,
            } => {
                let v = match value {
                    Some(e) => {
                        let v = self.eval(e)?;
                        if is_result(&v) {
                            return Err(result_not_stored(e.span()));
                        }
                        match ty {
                            Some(ann) => self.check_type(ann, &v)?,
                            None => {
                                if !call_pins_type(e, &self.funcs) {
                                    ensure_determined(&v, e.span())?;
                                }
                            }
                        }
                        v
                    }
                    None => {
                        let ann = ty.as_ref().ok_or_else(|| {
                            LuxError::new(format!("`{}` needs a type or a value", name), *span)
                                .with_note("write `var x: int` or `var x = 0`")
                                .with_learn("variables", "a let holds still, a var can change")
                        })?;
                        self.zero_value(ann)?
                    }
                };
                if self.current_has(name) {
                    return Err(LuxError::new(
                        format!("`{}` is already declared in this scope", name),
                        *span,
                    ));
                }
                self.declare(name.clone(), v, BindKind::Var);
                Ok(Flow::Normal)
            }

            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => {
                let new = self.eval(value)?;
                // Every place is rooted at a name; that binding must exist and be
                // a `var`, whether we're setting the whole thing or a field of it.
                let root = target.place_root().expect("the parser only builds a place");
                let binding = self.lookup(root).ok_or_else(|| {
                    LuxError::new(format!("`{}` is not defined", root), target.span())
                        .with_note("declare it first with let or var")
                        .with_learn("variables", "a name has to be made before it's used")
                })?;
                let kind = binding.kind;
                if !kind.mutable() {
                    let place = describe_place(target);
                    // The reason a place won't change depends on where its root came
                    // from. A `let` can become a `var`; a parameter and a loop
                    // variable can't, so pointing them at `var` would send a learner
                    // chasing syntax that doesn't exist. Each says the true thing.
                    let err = match kind {
                        BindKind::Param => LuxError::new(
                            format!("cannot change `{place}` — `{root}` is a parameter, and a parameter never changes"),
                            *span,
                        )
                        .with_note(format!(
                            "a parameter can't be a var; copy it into a local var first — `var copy = {root}` — and change that"
                        ))
                        .with_learn(
                            "variables",
                            "a parameter is a value handed in to read, not a place to store into",
                        ),
                        BindKind::Loop => LuxError::new(
                            format!("cannot change `{place}` — `{root}` is the loop's variable, and it holds still through each turn"),
                            *span,
                        )
                        .with_note(
                            "to build something up across a loop, change a var declared outside it instead",
                        )
                        .with_learn(
                            "variables",
                            "the loop hands you each item to read, not a place to store into",
                        ),
                        _ => LuxError::new(
                            format!("cannot change `{place}` — `{root}` was declared with let"),
                            *span,
                        )
                        .with_note("use `var` instead of `let` if it needs to change")
                        .with_learn(
                            "variables",
                            "a let holds still on purpose — that's what keeps it safe",
                        ),
                    };
                    return Err(err);
                }
                // Resolve any index expressions to concrete positions before the
                // mutable borrow, so evaluating them can't overlap the write.
                let path = self.resolve_path(target)?;
                let slot = navigate_mut(&mut self.lookup_mut(root).unwrap().value, &path, *span)?;
                let result = match op {
                    AssignOp::Set => {
                        if !same_type(slot, &new) {
                            return Err(LuxError::new(
                                format!(
                                    "`{}` is {} but you assigned {}",
                                    describe_place(target),
                                    value_type(slot),
                                    value_type(&new)
                                ),
                                *span,
                            )
                            .with_learn("variables", "a place keeps the type it started with"));
                        }
                        new
                    }
                    AssignOp::Add => append_or_add(slot.clone(), new, *span)?,
                    AssignOp::Sub => sub(slot, &new, *span)?,
                };
                *slot = result;
                Ok(Flow::Normal)
            }

            // Functions and types are registered up front; the declarations
            // themselves do nothing when execution reaches them.
            Stmt::Func { .. } | Stmt::Struct { .. } | Stmt::Enum { .. } => Ok(Flow::Normal),

            Stmt::Return { value, span } => {
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                self.trace_at(*span, &format!("→ {}", trace_render(&v)));
                Ok(Flow::Return(v))
            }

            Stmt::If {
                cond,
                then_body,
                else_body,
                span,
            } => {
                if self.eval_bool(cond)? {
                    self.trace_at(*span, "(yes)");
                    self.run_scoped(then_body)
                } else if let Some(eb) = else_body {
                    self.trace_at(*span, "(no)");
                    self.run_scoped(eb)
                } else {
                    self.trace_at(*span, "(no)");
                    Ok(Flow::Normal)
                }
            }

            Stmt::While { cond, body, span } => {
                while self.eval_bool(cond)? {
                    self.trace_at(*span, "");
                    match self.run_scoped(body)? {
                        Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                }
                Ok(Flow::Normal)
            }

            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                let iterable = self.eval(iter)?;
                match iterable {
                    Value::Array(items) => {
                        for item in items {
                            self.trace_at(*span, &format!("{} = {}", var, trace_render(&item)));
                            match self.run_loop_body(var, item, body)? {
                                Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                    }
                    Value::Range(lo, hi) => {
                        let mut i = lo;
                        while i < hi {
                            self.trace_at(
                                *span,
                                &format!("{} = {}", var, trace_render(&Value::Int(i))),
                            );
                            match self.run_loop_body(var, Value::Int(i), body)? {
                                Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                            i += 1;
                        }
                    }
                    other => {
                        return Err(LuxError::new(
                            format!("cannot loop over {}", value_type(&other)),
                            iter.span(),
                        )
                        .with_note("for ... in needs an array or a range like 0..10")
                        .with_learn("for", "for walks an array or counts a range like 0..10"));
                    }
                }
                Ok(Flow::Normal)
            }

            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::Normal)
            }
        }
    }

    /// Run a block in a fresh scope, always popping it even on error.
    fn run_scoped(&mut self, body: &[Stmt]) -> Result<Flow, LuxError> {
        self.push();
        let r = self.exec_block(body);
        self.pop();
        r
    }

    /// One pass of a `for` loop: bind the loop variable in a fresh scope and
    /// run the body. The loop variable is immutable, like Rust's and Swift's.
    fn run_loop_body(&mut self, var: &str, item: Value, body: &[Stmt]) -> Result<Flow, LuxError> {
        self.push();
        self.declare(var.to_string(), item, BindKind::Loop);
        let r = self.exec_block(body);
        self.pop();
        r
    }

    /// Flatten a place expression into a path of steps, evaluating each index to
    /// a concrete position as we go — so the write that follows takes its mutable
    /// borrow with nothing left to evaluate.
    fn resolve_path(&mut self, target: &Expr) -> Result<Vec<Step>, LuxError> {
        match target {
            Expr::Ident(..) => Ok(Vec::new()),
            Expr::Field { base, field, .. } => {
                let mut path = self.resolve_path(base)?;
                path.push(Step::Field(field.clone()));
                Ok(path)
            }
            Expr::Index { base, index, span } => {
                let mut path = self.resolve_path(base)?;
                let i = match self.eval(index)? {
                    Value::Int(n) if n >= 0 => n as usize,
                    Value::Int(n) => {
                        return Err(LuxError::new(format!("index {} is negative", n), *span)
                            .with_note("array positions start at 0"));
                    }
                    other => {
                        return Err(LuxError::new(
                            format!(
                                "an index has to be a whole number, not {}",
                                other.type_name()
                            ),
                            *span,
                        ));
                    }
                };
                path.push(Step::Index(i));
                Ok(path)
            }
            _ => unreachable!("place_root guarantees an Ident/Field/Index chain"),
        }
    }

    // ----- expressions ------------------------------------------------------

    fn eval(&mut self, e: &Expr) -> Result<Value, LuxError> {
        match e {
            Expr::Int(v, _) => Ok(Value::Int(*v)),
            Expr::Float(v, _) => Ok(Value::Float(*v)),
            Expr::Str(s, _) => Ok(Value::Str(s.clone())),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(b) => Ok(b.value.clone()),
                None if name == "none" => Ok(option_none()),
                None => Err(LuxError::new(format!("`{}` is not defined", name), *span)
                    .with_note("declare it with let or var before using it")
                    .with_learn("scope", "a name lives only inside the { } where it's made")),
            },
            Expr::Array(elems, _) => {
                let mut items = Vec::with_capacity(elems.len());
                for e in elems {
                    items.push(self.eval(e)?);
                }
                // Arrays are homogeneous: every element shares one type.
                if let Some(first) = items.first() {
                    for (v, e) in items.iter().zip(elems).skip(1) {
                        if !same_type(first, v) {
                            return Err(LuxError::new(
                                format!(
                                    "an array's elements must all be the same type, but found {} and {}",
                                    value_type(first),
                                    value_type(v)
                                ),
                                e.span(),
                            )
                            .with_learn("arrays", "an array holds many values of one type"));
                        }
                    }
                }
                Ok(Value::Array(items))
            }
            Expr::Unary { op, rhs, span } => {
                let v = self.eval(rhs)?;
                unary(*op, v, *span)
            }
            Expr::Binary { op, lhs, rhs, span } => {
                // && and || short-circuit, so evaluate the right side lazily.
                match op {
                    BinOp::And => {
                        if !self.eval_bool(lhs)? {
                            return Ok(Value::Bool(false));
                        }
                        Ok(Value::Bool(self.eval_bool(rhs)?))
                    }
                    BinOp::Or => {
                        if self.eval_bool(lhs)? {
                            return Ok(Value::Bool(true));
                        }
                        Ok(Value::Bool(self.eval_bool(rhs)?))
                    }
                    _ => {
                        let a = self.eval(lhs)?;
                        let b = self.eval(rhs)?;
                        binary_op(*op, &a, &b, *span)
                    }
                }
            }
            Expr::Index { base, index, span } => {
                let collection = self.eval(base)?;
                let idx = self.eval(index)?;
                let items = match collection {
                    Value::Array(items) => items,
                    other => {
                        return Err(LuxError::new(
                            format!(
                                "cannot index into {}; only arrays can be indexed",
                                value_type(&other)
                            ),
                            base.span(),
                        )
                        .with_learn("arrays", "a numbered row of values, all one type, from 0"));
                    }
                };
                let i = match idx {
                    Value::Int(i) => i,
                    other => {
                        return Err(LuxError::new(
                            format!(
                                "an array index must be an int, but this is {}",
                                value_type(&other)
                            ),
                            index.span(),
                        )
                        .with_learn(
                            "arrays",
                            "you reach an element by its position, counting from 0",
                        ));
                    }
                };
                if i < 0 || i as usize >= items.len() {
                    let note = if items.is_empty() {
                        "this array is empty".to_string()
                    } else {
                        format!("valid indices are 0 to {}", items.len() - 1)
                    };
                    return Err(LuxError::new(
                        format!(
                            "index {} is out of bounds for an array of length {}",
                            i,
                            items.len()
                        ),
                        *span,
                    )
                    .with_note(note)
                    .with_learn(
                        "arrays",
                        "the first element is 0, so the last is length minus 1",
                    ));
                }
                Ok(items[i as usize].clone())
            }
            Expr::Range { start, end, span } => {
                let lo = self.eval(start)?;
                let hi = self.eval(end)?;
                match (lo, hi) {
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Range(a, b)),
                    (a, b) => Err(LuxError::new(
                        format!(
                            "a range needs two ints, but got {} and {}",
                            value_type(&a),
                            value_type(&b)
                        ),
                        *span,
                    )
                    .with_note("write something like 0..10")
                    .with_learn("for", "a range like 0..10 counts, end not included")),
                }
            }
            Expr::Call { name, args, span } => self.call(name, args, *span),
            Expr::StructLit { name, fields, span } => self.eval_struct_lit(name, fields, *span),
            Expr::EnumLit {
                enum_name,
                variant,
                fields,
                span,
            } => self.construct_enum(enum_name, variant, fields, *span),
            Expr::Field { base, field, span } => self.eval_field(base, field, *span),
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.eval_match(scrutinee, arms, *span),
        }
    }

    /// Build a struct value: evaluate each declared field in order, checking
    /// that every field is supplied exactly once, with the right type.
    fn eval_struct_lit(
        &mut self,
        name: &str,
        provided: &[(String, Expr)],
        span: Span,
    ) -> Result<Value, LuxError> {
        let data = match self.structs.get(name) {
            Some(d) => Rc::clone(d),
            None => {
                return Err(LuxError::new(format!("unknown struct `{}`", name), span)
                    .with_note("define it with `struct`, or check the spelling")
                    .with_learn("structs", "a struct gathers a few values under one name"));
            }
        };
        // Reject fields that aren't part of this struct, blaming the field.
        for (k, e) in provided {
            if !data.fields.iter().any(|f| &f.name == k) {
                return Err(LuxError::new(
                    format!("struct `{}` has no field `{}`", name, k),
                    e.span(),
                )
                .with_learn("structs", "a struct's fields are fixed when you define it"));
            }
        }
        let mut built = Vec::with_capacity(data.fields.len());
        for f in &data.fields {
            let value_expr = match provided.iter().find(|(k, _)| k == &f.name) {
                Some((_, e)) => e,
                None => {
                    return Err(LuxError::new(
                        format!("missing field `{}` for struct `{}`", f.name, name),
                        span,
                    )
                    .with_note(format!(
                        "`{}` has a field `{}: {}`",
                        name,
                        f.name,
                        describe_type(&f.ty)
                    ))
                    .with_learn(
                        "structs",
                        "every field gets a value when you build a struct",
                    ));
                }
            };
            let v = self.eval(value_expr)?;
            // Storing a Result in a field is storing it — same rule as a `let`.
            if is_result(&v) {
                return Err(result_not_stored(value_expr.span()));
            }
            if !self.type_matches(&f.ty, &v) {
                return Err(LuxError::new(
                    format!(
                        "field `{}` of `{}` should be {}, but got {}",
                        f.name,
                        name,
                        describe_type(&f.ty),
                        value_type(&v)
                    ),
                    value_expr.span(),
                )
                .with_learn(
                    "structs",
                    "each field has a type, set when you define the struct",
                ));
            }
            built.push((f.name.clone(), v));
        }
        Ok(Value::Struct {
            name: name.to_string(),
            fields: built,
        })
    }

    /// Build an enum value: find the case, then match the supplied labelled
    /// values against the case's declared fields.
    fn construct_enum(
        &mut self,
        enum_name: &str,
        variant: &str,
        provided: &[(String, Expr)],
        span: Span,
    ) -> Result<Value, LuxError> {
        let data = match self.enums.get(enum_name) {
            Some(d) => Rc::clone(d),
            None => {
                return Err(LuxError::new(format!("unknown enum `{}`", enum_name), span)
                    .with_note("define it with `enum`, or check the spelling")
                    .with_learn("enums", "an enum is one of a fixed set of shapes"));
            }
        };
        let vdef = match data.variants.iter().find(|v| v.name == variant) {
            Some(v) => v,
            None => {
                return Err(LuxError::new(
                    format!("enum `{}` has no case `{}`", enum_name, variant),
                    span,
                )
                .with_note(format!("cases are: {}", variant_names(&data)))
                .with_learn(
                    "enums",
                    "an enum's cases are the fixed set of shapes it allows",
                ));
            }
        };
        if provided.len() != vdef.fields.len() {
            return Err(LuxError::new(
                format!(
                    "`{}.{}` carries {}, but you gave {}",
                    enum_name,
                    variant,
                    count(vdef.fields.len(), "value"),
                    provided.len()
                ),
                span,
            )
            .with_learn("enums", "each case can carry its own values"));
        }
        let mut built = Vec::with_capacity(vdef.fields.len());
        for f in &vdef.fields {
            let value_expr = match provided.iter().find(|(k, _)| k == &f.name) {
                Some((_, e)) => e,
                None => {
                    return Err(LuxError::new(
                        format!("missing value `{}` for `{}.{}`", f.name, enum_name, variant),
                        span,
                    )
                    .with_learn(
                        "enums",
                        "a case carries its values, named like a struct's fields",
                    ));
                }
            };
            let v = self.eval(value_expr)?;
            if !self.type_matches(&f.ty, &v) {
                return Err(LuxError::new(
                    format!(
                        "`{}` in `{}.{}` should be {}, but got {}",
                        f.name,
                        enum_name,
                        variant,
                        describe_type(&f.ty),
                        value_type(&v)
                    ),
                    value_expr.span(),
                )
                .with_learn("enums", "each value a case carries has a type"));
            }
            built.push((f.name.clone(), v));
        }
        Ok(Value::Enum {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
            fields: built,
        })
    }

    /// Read a struct field, or construct a payload-less enum case written as
    /// `Shape.dot`. The two look identical, so the enum table decides: if the
    /// thing before the dot is an enum name (and not a variable), it's a case.
    fn eval_field(&mut self, base: &Expr, field: &str, span: Span) -> Result<Value, LuxError> {
        if let Expr::Ident(n, nspan) = base
            && self.lookup(n).is_none()
        {
            if self.enums.contains_key(n) {
                return self.construct_enum(n, field, &[], span);
            }
            // `Name.field` where `Name` is neither a value nor an enum: most
            // likely a misspelled type or variable. Point at both fixes.
            return Err(LuxError::new(format!("`{}` is not defined", n), *nspan)
                .with_note("if it's an enum, declare it with `enum`; otherwise declare the value with let or var")
                .with_learn("variables", "a name has to be made before it's used"));
        }
        let v = self.eval(base)?;
        match v {
            Value::Struct { name, fields } => match fields.iter().find(|(k, _)| k == field) {
                Some((_, val)) => Ok(val.clone()),
                None => Err(LuxError::new(
                    format!("struct `{}` has no field `{}`", name, field),
                    span,
                )
                .with_learn("structs", "a struct only has the fields you gave it")),
            },
            other => Err(LuxError::new(
                format!(
                    "cannot read field `{}` of {}; only structs have fields",
                    field,
                    value_type(&other)
                ),
                base.span(),
            )
            .with_learn(
                "structs",
                "only a struct has named fields to read with a dot",
            )),
        }
    }

    /// Evaluate a `match`: check it covers its cases, then run the one arm whose
    /// pattern fits the scrutinee, binding any captured values.
    fn eval_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<Value, LuxError> {
        let v = self.eval(scrutinee)?;
        match &v {
            Value::Enum {
                enum_name,
                variant,
                fields,
            } => {
                let data = Rc::clone(
                    self.enums
                        .get(enum_name)
                        .expect("an enum value implies a registered enum"),
                );
                let has_wildcard = arms
                    .iter()
                    .any(|a| matches!(a.pattern, Pattern::Wildcard(_)));

                // Every arm of an enum match must name a real case (or be `_`).
                for a in arms {
                    match &a.pattern {
                        Pattern::Variant {
                            name, span: psp, ..
                        } => {
                            if !data.variants.iter().any(|vd| &vd.name == name) {
                                return Err(LuxError::new(
                                    format!("enum `{}` has no case `{}`", enum_name, name),
                                    *psp,
                                )
                                .with_note(format!("cases are: {}", variant_names(&data)))
                                .with_learn(
                                    "match",
                                    "every arm names one of the enum's real cases",
                                ));
                            }
                        }
                        Pattern::Wildcard(_) => {}
                        other => {
                            return Err(LuxError::new(
                                format!(
                                    "this matches on enum `{}`, so each arm must be a case name or `_`",
                                    enum_name
                                ),
                                other.span(),
                            )
                            .with_learn("match", "each arm is one case you handle"));
                        }
                    }
                }

                // Exhaustiveness: without a `_`, every case must be handled.
                if !has_wildcard {
                    let missing: Vec<String> = data
                        .variants
                        .iter()
                        .filter(|vd| {
                            !arms.iter().any(|a| {
                                matches!(&a.pattern, Pattern::Variant { name, .. } if name == &vd.name)
                            })
                        })
                        .map(|vd| vd.name.clone())
                        .collect();
                    if !missing.is_empty() {
                        return Err(LuxError::new(
                            format!("this match on `{}` doesn't handle every case", enum_name),
                            span,
                        )
                        .with_learn("match", "covering every case is what makes match safe")
                        .with_note(format!(
                            "add an arm for: {} (or a `_` catch-all)",
                            missing.join(", ")
                        )));
                    }
                }

                // Run the first arm that fits, top to bottom.
                for a in arms {
                    match &a.pattern {
                        Pattern::Variant {
                            name,
                            bindings,
                            span: psp,
                        } if name == variant => {
                            if bindings.len() != fields.len() {
                                return Err(LuxError::new(
                                    format!(
                                        "case `{}` carries {}, but the pattern captures {}",
                                        name,
                                        count(fields.len(), "value"),
                                        bindings.len()
                                    ),
                                    *psp,
                                )
                                .with_learn(
                                    "match",
                                    "a pattern binds the values its case carries",
                                ));
                            }
                            self.push();
                            for (b, (_, val)) in bindings.iter().zip(fields.iter()) {
                                self.declare(b.clone(), val.clone(), BindKind::Match);
                            }
                            let r = self.eval(&a.body);
                            self.pop();
                            return r;
                        }
                        Pattern::Wildcard(_) => return self.eval(&a.body),
                        _ => continue,
                    }
                }
                unreachable!("exhaustiveness guarantees a matching arm")
            }
            other => self.match_value(other, arms, scrutinee.span(), span),
        }
    }

    /// Match a plain value (int, string, bool) against literal patterns. These
    /// domains are open, so a `_` catch-all is required.
    fn match_value(
        &mut self,
        v: &Value,
        arms: &[MatchArm],
        scrutinee_span: Span,
        span: Span,
    ) -> Result<Value, LuxError> {
        if !matches!(v, Value::Int(_) | Value::Str(_) | Value::Bool(_)) {
            return Err(LuxError::new(
                format!(
                    "cannot match on {}; match works on enums, int, string, and bool",
                    value_type(v)
                ),
                scrutinee_span,
            )
            .with_learn(
                "match",
                "match takes apart an enum or a plain int, string, or bool",
            ));
        }
        // A value match needs a `_`, since int and string have endless values.
        // bool is the exception: `true` and `false` together cover everything.
        let has_wildcard = arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard(_)));
        let bool_exhaustive = matches!(v, Value::Bool(_))
            && arms
                .iter()
                .any(|a| matches!(a.pattern, Pattern::Bool(true, _)))
            && arms
                .iter()
                .any(|a| matches!(a.pattern, Pattern::Bool(false, _)));
        if !has_wildcard && !bool_exhaustive {
            return Err(LuxError::new(
                format!("this match on {} needs a `_` case", value_type(v)),
                span,
            )
            .with_note("matching a value (not an enum) can't be exhaustive, so add `_ => ...`")
            .with_learn(
                "match",
                "`_` is the catch-all that covers every other value",
            ));
        }
        for a in arms {
            let fits = match (&a.pattern, v) {
                (Pattern::Wildcard(_), _) => true,
                (Pattern::Int(n, _), Value::Int(m)) => n == m,
                (Pattern::Str(s, _), Value::Str(t)) => s == t,
                (Pattern::Bool(b, _), Value::Bool(c)) => b == c,
                (Pattern::Variant { span: psp, .. }, _) => {
                    return Err(LuxError::new(
                        format!("this is {}, not an enum, so it has no cases", value_type(v)),
                        *psp,
                    )
                    .with_learn("enums", "only an enum has named cases to match"));
                }
                _ => false,
            };
            if fits {
                return self.eval(&a.body);
            }
        }
        unreachable!("the required `_` arm guarantees a match")
    }

    fn eval_bool(&mut self, e: &Expr) -> Result<bool, LuxError> {
        match self.eval(e)? {
            Value::Bool(b) => Ok(b),
            other => Err(LuxError::new(
                format!(
                    "expected a true/false value, but this is {}",
                    named(other.type_name())
                ),
                e.span(),
            )
            .with_note("conditions and &&/|| operands must be bool")
            .with_learn("booleans", "if, while, and and/or all run on true or false")),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], span: Span) -> Result<Value, LuxError> {
        match name {
            "print" => {
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    parts.push(display(&self.eval(a)?));
                }
                println!("{}", parts.join(" "));
                Ok(Value::Unit)
            }
            "string" => {
                let v = self.one_arg(name, args, span)?;
                Ok(Value::Str(display(&v)))
            }
            "int" => {
                let v = self.one_arg(name, args, span)?;
                match v {
                    Value::Int(_) => Ok(v),
                    Value::Float(f) => Ok(Value::Int(f as i64)),
                    Value::Str(_) => Err(LuxError::new(
                        "int converts between numbers, not from text".to_string(),
                        span,
                    )
                    .with_note("to read a number from text use parseInt, which hands back an Option you match on")
                    .with_learn("conversions", "parseInt reads a number from text and gives back an Option")),
                    other => Err(LuxError::new(
                        format!("cannot convert {} to an int", named(other.type_name())),
                        span,
                    )),
                }
            }
            "float" => {
                let v = self.one_arg(name, args, span)?;
                match v {
                    Value::Float(_) => Ok(v),
                    Value::Int(n) => Ok(Value::Float(n as f64)),
                    Value::Str(_) => Err(LuxError::new(
                        "float converts between numbers, not from text".to_string(),
                        span,
                    )
                    .with_note("to read a number from text use parseFloat, which hands back an Option you match on")
                    .with_learn("conversions", "parseFloat reads a number from text and gives back an Option")),
                    other => Err(LuxError::new(
                        format!("cannot convert {} to a float", named(other.type_name())),
                        span,
                    )),
                }
            }
            "length" => {
                let v = self.one_arg(name, args, span)?;
                match v {
                    Value::Array(items) => Ok(Value::Int(items.len() as i64)),
                    // Count characters, not bytes, so length("café") is 4.
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    other => Err(LuxError::new(
                        format!(
                            "length expects an array or a string, but got {}",
                            value_type(&other)
                        ),
                        span,
                    )
                    .with_learn(
                        "arrays",
                        "length counts an array's items or a string's characters",
                    )),
                }
            }
            // ----- the outside world: files, arguments, the standard streams ---
            // Reading and writing can fail (a missing file, an unwritable path),
            // so they hand the failure back as a value — `Result` — instead of
            // crashing. The program decides what to do about it.
            "readFile" => {
                let path = self.one_str(name, args, span)?;
                match std::fs::read_to_string(&path) {
                    Ok(contents) => Ok(result_ok(Value::Str(contents))),
                    Err(e) => Ok(result_err(Value::Str(format!(
                        "could not read {}: {}",
                        path, e
                    )))),
                }
            }
            // Success here carries nothing — there's no value to hand back, only
            // the fact that it worked. That's `Result<Unit, string>`: you still
            // have to confirm it didn't fail, even when success is empty.
            "writeFile" => {
                let (path, contents) = self.two_str(name, args, span)?;
                match std::fs::write(&path, &contents) {
                    Ok(()) => Ok(result_ok(Value::Unit)),
                    Err(e) => Ok(result_err(Value::Str(format!(
                        "could not write {}: {}",
                        path, e
                    )))),
                }
            }
            // The command-line arguments, program name first — args()[0] is the
            // program itself, the way every shell hands it over.
            "args" => {
                self.no_args(name, args, span)?;
                let items = self.program_args.iter().cloned().map(Value::Str).collect();
                Ok(Value::Array(items))
            }
            // One line from standard input, with its newline removed. `none` at
            // end of input — so a loop reading until `none` works the same whether
            // it's a person typing or a file piped in.
            "readLine" => {
                self.no_args(name, args, span)?;
                let mut line = String::new();
                let result = match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => option_none(),
                    Ok(_) => {
                        let text = line.strip_suffix('\n').unwrap_or(&line);
                        let text = text.strip_suffix('\r').unwrap_or(text);
                        option_some(Value::Str(text.to_string()))
                    }
                };
                // The input seam: show the typed line becoming a value.
                self.trace_at(span, &format!("readLine() → {}", trace_render(&result)));
                Ok(result)
            }
            // The friendly front door to input, meant to be usable long before
            // Option and match. An optional prompt is shown first, on the same
            // line (no trailing newline), so the cursor waits right after the
            // question. Unlike `readLine`, the end of input is not a case you
            // have to handle here: it simply reads as an empty string. That is
            // the trade — reach for `readLine` when "they typed nothing" and
            // "the input ran out" must be told apart.
            "input" => {
                if args.len() > 1 {
                    return Err(LuxError::new(
                        format!(
                            "input takes an optional prompt, but got {} arguments",
                            args.len()
                        ),
                        span,
                    ));
                }
                if let Some(prompt) = args.first() {
                    print!("{}", display(&self.eval(prompt)?));
                    let _ = std::io::stdout().flush();
                }
                let mut line = String::new();
                let result = match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => Value::Str(String::new()),
                    Ok(_) => {
                        let text = line.strip_suffix('\n').unwrap_or(&line);
                        let text = text.strip_suffix('\r').unwrap_or(text);
                        Value::Str(text.to_string())
                    }
                };
                self.trace_at(span, &format!("input() → {}", trace_render(&result)));
                Ok(result)
            }
            // Like `print`, but to standard error — the stream for diagnostics,
            // kept separate so the real output on stdout stays clean for whatever
            // program reads it next.
            "eprint" => {
                let mut parts = Vec::with_capacity(args.len());
                for a in args {
                    parts.push(display(&self.eval(a)?));
                }
                eprintln!("{}", parts.join(" "));
                Ok(Value::Unit)
            }
            // Run another program and capture what it produced. Two layers of
            // truth: the `Result` says whether it *launched* (the program might
            // not exist), and the `status` inside says whether the command itself
            // *succeeded* (it can run fine and still report failure with a
            // non-zero code). The arguments are a list, never a shell string, so
            // there is no shell to inject into. The child's input is empty.
            "run" => {
                let (program, arg_list) = self.program_and_args(name, args, span)?;
                match std::process::Command::new(&program)
                    .args(&arg_list)
                    .stdin(std::process::Stdio::null())
                    .output()
                {
                    Ok(out) => {
                        let status = out.status.code().unwrap_or(-1) as i64;
                        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                        Ok(result_ok(output_value(status, stdout, stderr)))
                    }
                    Err(e) => Ok(result_err(Value::Str(format!(
                        "could not run {}: {}",
                        program, e
                    )))),
                }
            }
            // Reading a number from text can fail — the text might not be a
            // number — so unlike int/float these hand back an Option, never a
            // crash. `none` is the "that wasn't a number" answer.
            "parseInt" => match self.one_arg(name, args, span)? {
                Value::Str(s) => Ok(match s.trim().parse::<i64>() {
                    Ok(n) => option_some(Value::Int(n)),
                    Err(_) => option_none(),
                }),
                other => Err(LuxError::new(
                    format!("parseInt reads text, but got {}", named(other.type_name())),
                    span,
                )),
            },
            "parseFloat" => match self.one_arg(name, args, span)? {
                Value::Str(s) => Ok(match s.trim().parse::<f64>() {
                    Ok(f) => option_some(Value::Float(f)),
                    Err(_) => option_none(),
                }),
                other => Err(LuxError::new(
                    format!(
                        "parseFloat reads text, but got {}",
                        named(other.type_name())
                    ),
                    span,
                )),
            },
            // The built-in enum constructors. `none` has no value, so it's a
            // bare name handled in `eval`, not a call.
            "some" => Ok(option_some(self.one_arg(name, args, span)?)),
            "ok" => Ok(result_ok(self.one_arg(name, args, span)?)),
            "err" => Ok(result_err(self.one_arg(name, args, span)?)),
            _ => self.call_user(name, args, span),
        }
    }

    fn one_arg(&mut self, name: &str, args: &[Expr], span: Span) -> Result<Value, LuxError> {
        if args.len() != 1 {
            return Err(LuxError::new(
                format!("{} takes exactly one value, but got {}", name, args.len()),
                span,
            ));
        }
        self.eval(&args[0])
    }

    /// A built-in that takes exactly one string, like `readFile(path)`.
    fn one_str(&mut self, name: &str, args: &[Expr], span: Span) -> Result<String, LuxError> {
        match self.one_arg(name, args, span)? {
            Value::Str(s) => Ok(s),
            other => Err(LuxError::new(
                format!("{} expects a string, but got {}", name, value_type(&other)),
                span,
            )),
        }
    }

    /// A built-in that takes two strings, like `writeFile(path, contents)`.
    fn two_str(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<(String, String), LuxError> {
        if args.len() != 2 {
            return Err(LuxError::new(
                format!("{} takes exactly two values, but got {}", name, args.len()),
                span,
            ));
        }
        let first = self.eval(&args[0])?;
        let second = self.eval(&args[1])?;
        let path = match first {
            Value::Str(s) => s,
            other => {
                return Err(LuxError::new(
                    format!(
                        "{} expects the path as a string, but got {}",
                        name,
                        value_type(&other)
                    ),
                    span,
                ));
            }
        };
        let contents = match second {
            Value::Str(s) => s,
            other => {
                return Err(LuxError::new(
                    format!(
                        "{} expects the contents as a string, but got {}",
                        name,
                        value_type(&other)
                    ),
                    span,
                ));
            }
        };
        Ok((path, contents))
    }

    /// A built-in that takes no values, like `args()` or `readLine()`.
    fn no_args(&self, name: &str, args: &[Expr], span: Span) -> Result<(), LuxError> {
        if !args.is_empty() {
            return Err(LuxError::new(
                format!("{} takes no values, but got {}", name, args.len()),
                span,
            ));
        }
        Ok(())
    }

    /// `run`'s arguments: a program name and a list of string arguments. The
    /// arg-vector shape is deliberate — there is no shell string to inject into.
    fn program_and_args(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<(String, Vec<String>), LuxError> {
        if args.len() != 2 {
            return Err(LuxError::new(
                format!(
                    "{} takes a program name and a list of arguments, but got {}",
                    name,
                    args.len()
                ),
                span,
            ));
        }
        let program = match self.eval(&args[0])? {
            Value::Str(s) => s,
            other => {
                return Err(LuxError::new(
                    format!(
                        "{} expects the program name as a string, but got {}",
                        name,
                        value_type(&other)
                    ),
                    span,
                ));
            }
        };
        let arg_list = match self.eval(&args[1])? {
            Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    match it {
                        Value::Str(s) => out.push(s),
                        other => {
                            return Err(LuxError::new(
                                format!(
                                    "{} expects the arguments as a list of strings, but one was {}",
                                    name,
                                    value_type(&other)
                                ),
                                span,
                            ));
                        }
                    }
                }
                out
            }
            other => {
                return Err(LuxError::new(
                    format!(
                        "{} expects the arguments as a list of strings, but got {}",
                        name,
                        value_type(&other)
                    ),
                    span,
                ));
            }
        };
        Ok((program, arg_list))
    }

    /// Call a user-defined function. Arguments are checked against the declared
    /// parameter types (no coercion), the body runs in its own fresh scope —
    /// it sees its parameters and other functions, but not the caller's
    /// locals — and the returned value is checked against the declared return
    /// type.
    fn call_user(&mut self, name: &str, args: &[Expr], span: Span) -> Result<Value, LuxError> {
        let func = match self.funcs.get(name) {
            Some(f) => Rc::clone(f),
            None => {
                return Err(LuxError::new(format!("unknown function `{}`", name), span).with_note(
                    "define it with `func`, or use a built-in: print, eprint, string, int, float, length, readFile, writeFile, readLine, args, run",
                )
                .with_learn("functions", "a function takes values in and hands one result back"));
            }
        };

        if args.len() != func.params.len() {
            return Err(LuxError::new(
                format!(
                    "function `{}` expects {} but got {}",
                    name,
                    count(func.params.len(), "value"),
                    args.len()
                ),
                span,
            )
            .with_learn(
                "functions",
                "a function takes exactly the parameters it declares",
            ));
        }

        // Evaluate and type-check the arguments in the caller's scope.
        let mut frame = HashMap::new();
        for (param, arg) in func.params.iter().zip(args) {
            let v = self.eval(arg)?;
            self.validate_type(&param.ty)?;
            if !self.type_matches(&param.ty, &v) {
                return Err(LuxError::new(
                    format!(
                        "`{}` expects `{}` to be {}, but got {}",
                        name,
                        param.name,
                        describe_type(&param.ty),
                        value_type(&v)
                    ),
                    arg.span(),
                )
                .with_learn("functions", "each parameter has a type the call must match"));
            }
            frame.insert(
                param.name.clone(),
                Binding {
                    value: v,
                    kind: BindKind::Param,
                },
            );
        }

        // Run the body with a scope stack of just this call's frame, then put
        // the caller's scopes back — even if the body errored.
        let saved = std::mem::replace(&mut self.scopes, vec![frame]);
        let outcome = self.exec_block(&func.body);
        self.scopes = saved;
        let returned = match outcome? {
            Flow::Return(v) => v,
            Flow::Normal => Value::Unit,
        };

        match &func.ret {
            Some(ann) => {
                self.validate_type(ann)?;
                if matches!(returned, Value::Unit) {
                    return Err(LuxError::new(
                        format!(
                            "`{}` must return {}, but it ended without returning a value",
                            name,
                            describe_type(ann)
                        ),
                        span,
                    )
                    .with_learn("functions", "a `-> type` is a promise to hand that back"));
                }
                if !self.type_matches(ann, &returned) {
                    return Err(LuxError::new(
                        format!(
                            "`{}` should return {}, but returned {}",
                            name,
                            describe_type(ann),
                            value_type(&returned)
                        ),
                        span,
                    )
                    .with_learn("functions", "what comes back must match the `-> type`"));
                }
                Ok(returned)
            }
            None => {
                if !matches!(returned, Value::Unit) {
                    return Err(LuxError::new(
                        format!("`{}` has no return type, so it can't return a value", name),
                        span,
                    )
                    .with_note("add `-> type` to the signature if it should return something")
                    .with_learn(
                        "functions",
                        "no `-> type` means the function returns nothing",
                    ));
                }
                Ok(Value::Unit)
            }
        }
    }

    // ----- types: annotations, matching, and zero values --------------------

    /// Check that every name in an annotation is a real type: a built-in, or a
    /// struct or enum the program declares.
    fn validate_type(&self, ann: &TypeAnn) -> Result<(), LuxError> {
        match &ann.kind {
            TypeKind::Named(n) => {
                // Option and Result are real types, but they always need their
                // parameters — `Option` alone says nothing about what it holds.
                if matches!(n.as_str(), "Option" | "Result") {
                    let hint = if n == "Option" {
                        "write `Option<int>`"
                    } else {
                        "write `Result<int, string>`"
                    };
                    return Err(LuxError::new(
                        format!("`{}` needs a type in angle brackets", n),
                        ann.span,
                    )
                    .with_note(hint)
                    .with_learn(
                        "option",
                        "Option and Result say what they hold, like Option<int>",
                    ));
                }
                // `Unit` is the type of "nothing" — the success of a `writeFile`
                // that worked but has no value to return, as in `Result<Unit, string>`.
                if matches!(n.as_str(), "int" | "float" | "string" | "bool" | "Unit")
                    || self.type_exists(n)
                {
                    Ok(())
                } else {
                    Err(LuxError::new(format!("unknown type `{}`", n), ann.span).with_note(
                        "known types: int, float, string, bool, Unit, arrays like [int], and any struct or enum you define",
                    ))
                }
            }
            TypeKind::Array(elem) => self.validate_type(elem),
            TypeKind::Generic(name, args) => match name.as_str() {
                "Option" => {
                    if args.len() != 1 {
                        return Err(LuxError::new(
                            format!("`Option` takes one type, but got {}", args.len()),
                            ann.span,
                        )
                        .with_note("write `Option<int>`")
                        .with_learn(
                            "option",
                            "an Option holds one type — what's there when it's not none",
                        ));
                    }
                    self.validate_type(&args[0])
                }
                "Result" => {
                    if args.len() != 2 {
                        return Err(LuxError::new(
                            format!("`Result` takes two types, but got {}", args.len()),
                            ann.span,
                        )
                        .with_note(
                            "write `Result<int, string>` — the value type and the error type",
                        )
                        .with_learn(
                            "result",
                            "a Result holds two types: the value and the error",
                        ));
                    }
                    self.validate_type(&args[0])?;
                    self.validate_type(&args[1])
                }
                _ => Err(LuxError::new(
                    format!("`{}` is not a parameterized type", name),
                    ann.span,
                )
                .with_note("only Option and Result take a type in angle brackets, like Option<int>")
                .with_learn("option", "some(x) or none — a missing value with no null")),
            },
        }
    }

    /// Does a runtime value satisfy a type annotation? Assumes the annotation's
    /// names are already known (see `validate_type`). An empty array satisfies
    /// any array type, since it has no elements to disagree.
    fn type_matches(&self, ann: &TypeAnn, v: &Value) -> bool {
        match (&ann.kind, v) {
            (TypeKind::Named(n), Value::Struct { name, .. }) => n == name,
            (TypeKind::Named(n), Value::Enum { enum_name, .. }) => n == enum_name,
            // The value prints as "nothing"; the type that describes it is `Unit`.
            (TypeKind::Named(n), Value::Unit) => n == "Unit",
            (TypeKind::Named(n), _) => n == v.type_name(),
            (TypeKind::Array(elem), Value::Array(items)) => {
                items.iter().all(|it| self.type_matches(elem, it))
            }
            // A generic annotation matches a built-in enum value when the value's
            // case fits and the payload it carries matches the right parameter.
            // `none` carries nothing, so it satisfies any `Option<T>` — the same
            // way an empty array satisfies any array type.
            (
                TypeKind::Generic(name, args),
                Value::Enum {
                    enum_name,
                    variant,
                    fields,
                },
            ) if name == enum_name => match (name.as_str(), variant.as_str()) {
                ("Option", "none") => true,
                ("Option", "some") => self.payload_matches(&args[0], fields),
                ("Result", "ok") => self.payload_matches(&args[0], fields),
                ("Result", "err") => self.payload_matches(&args[1], fields),
                _ => false,
            },
            _ => false,
        }
    }

    /// Does a built-in enum's single carried value match the given type?
    fn payload_matches(&self, ann: &TypeAnn, fields: &[(String, Value)]) -> bool {
        match fields.first() {
            Some((_, v)) => self.type_matches(ann, v),
            None => false,
        }
    }

    /// Validate an annotation and confirm a value matches it. Used by `let`/`var`
    /// type annotations, where the annotation is the thing to blame.
    fn check_type(&self, ann: &TypeAnn, v: &Value) -> Result<(), LuxError> {
        self.validate_type(ann)?;
        if !self.type_matches(ann, v) {
            return Err(LuxError::new(
                format!(
                    "type mismatch: annotated `{}` but the value is {}",
                    describe_type(ann),
                    value_type(v)
                ),
                ann.span,
            ));
        }
        Ok(())
    }

    /// The starting value for a `var` declared with a type but no initializer.
    /// Structs and enums have no obvious zero, so they need an explicit value.
    fn zero_value(&self, ann: &TypeAnn) -> Result<Value, LuxError> {
        match &ann.kind {
            TypeKind::Named(n) => match n.as_str() {
                "int" => Ok(Value::Int(0)),
                "float" => Ok(Value::Float(0.0)),
                "string" => Ok(Value::Str(String::new())),
                "bool" => Ok(Value::Bool(false)),
                _ if self.type_exists(n) => Err(LuxError::new(
                    format!("a `var` of type `{}` needs a starting value", n),
                    ann.span,
                )
                .with_note(format!("write `var x = {}(...)`", n))),
                _ => Err(LuxError::new(format!("unknown type `{}`", n), ann.span)
                    .with_note("lux has int, float, string, bool, and arrays like [int]")),
            },
            TypeKind::Array(elem) => {
                self.validate_type(elem)?;
                Ok(Value::Array(Vec::new()))
            }
            TypeKind::Generic(name, _) => {
                self.validate_type(ann)?;
                if name == "Option" {
                    // The natural empty Option is `none`.
                    Ok(option_none())
                } else {
                    Err(LuxError::new(
                        format!(
                            "a `var` of type `{}` needs a starting value",
                            describe_type(ann)
                        ),
                        ann.span,
                    )
                    .with_note("a Result is either ok(...) or err(...) — there's no empty one")
                    .with_learn("result", "ok(x) or err(why) — failure as plain data"))
                }
            }
        }
    }
}

// ----- operators (free functions: pure value -> value) ----------------------

fn unary(op: UnOp, v: Value, span: Span) -> Result<Value, LuxError> {
    match (op, v) {
        (UnOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
        (UnOp::Neg, other) => Err(LuxError::new(
            format!("cannot negate {}", named(other.type_name())),
            span,
        )),
        (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnOp::Not, other) => Err(LuxError::new(
            format!("cannot apply ! to {}", named(other.type_name())),
            span,
        )
        .with_note("! works on bool values")
        .with_learn("booleans", "! flips true to false and back")),
    }
}

fn binary_op(op: BinOp, a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match op {
        BinOp::Add => add(a, b, span),
        BinOp::Sub => sub(a, b, span),
        BinOp::Mul => mul(a, b, span),
        BinOp::Div => div(a, b, span),
        BinOp::Mod => modulo(a, b, span),
        BinOp::Eq => equality(a, b, span, false),
        BinOp::Ne => equality(a, b, span, true),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => ordering(op, a, b, span),
        BinOp::And | BinOp::Or => unreachable!("&& and || are handled in eval"),
    }
}

/// A type name with its article, like "an int" or "a string", so error
/// messages read as proper English.
fn named(type_name: &str) -> String {
    let article = match type_name.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    format!("{} {}", article, type_name)
}

/// How a value's type reads in a message about types: scalars by name, arrays
/// as `[int]` so the element type shows. No article, so it composes next to a
/// `describe_type` annotation.
/// One step of a place path, with any index already evaluated to a position.
enum Step {
    Field(String),
    Index(usize),
}

/// Walk a resolved path into a value and hand back a mutable reference to the
/// slot it names, so an assignment can write straight into it — the value stays
/// where it lives (structs and arrays are held inline), so no copy is made.
fn navigate_mut<'a>(
    root: &'a mut Value,
    path: &[Step],
    span: Span,
) -> Result<&'a mut Value, LuxError> {
    let mut cur = root;
    for step in path {
        cur = match (cur, step) {
            (Value::Struct { fields, .. }, Step::Field(f)) => fields
                .iter_mut()
                .find(|(k, _)| k == f)
                .map(|(_, v)| v)
                .ok_or_else(|| LuxError::new(format!("there's no field `{}` here", f), span))?,
            (Value::Array(items), Step::Index(i)) => {
                let len = items.len();
                items.get_mut(*i).ok_or_else(|| {
                    LuxError::new(
                        format!("index {} is past the end of an array of {}", i, len),
                        span,
                    )
                    .with_note("an array position has to be one that already exists")
                })?
            }
            (other, Step::Field(f)) => {
                return Err(LuxError::new(
                    format!("`{}` isn't a field of {}", f, other.type_name()),
                    span,
                ));
            }
            (other, Step::Index(_)) => {
                return Err(LuxError::new(
                    format!("{} can't be indexed", other.type_name()),
                    span,
                ));
            }
        };
    }
    Ok(cur)
}

/// A readable name for a place, for error and trace text — `w.doorOpen`,
/// `items[…]` (the index isn't spelled out, only that there is one).
fn describe_place(e: &Expr) -> String {
    match e {
        Expr::Ident(n, _) => n.clone(),
        Expr::Field { base, field, .. } => format!("{}.{}", describe_place(base), field),
        Expr::Index { base, .. } => format!("{}[…]", describe_place(base)),
        _ => "this".to_string(),
    }
}

fn value_type(v: &Value) -> String {
    match v {
        Value::Array(items) => match items.first() {
            Some(first) => format!("[{}]", value_type(first)),
            None => "[]".to_string(),
        },
        Value::Struct { name, .. } => name.clone(),
        // For the built-in generics, show what's known about the parameters and
        // leave the unknown ones as `?`: `none` is `Option<?>`, `ok(5)` is
        // `Result<int, ?>`.
        Value::Enum {
            enum_name,
            variant,
            fields,
        } if enum_name == "Option" => match (variant.as_str(), fields.first()) {
            ("some", Some((_, v))) => format!("Option<{}>", value_type(v)),
            _ => "Option<?>".to_string(),
        },
        Value::Enum {
            enum_name,
            variant,
            fields,
        } if enum_name == "Result" => match (variant.as_str(), fields.first()) {
            ("ok", Some((_, v))) => format!("Result<{}, ?>", value_type(v)),
            ("err", Some((_, v))) => format!("Result<?, {}>", value_type(v)),
            _ => "Result".to_string(),
        },
        Value::Enum { enum_name, .. } => enum_name.clone(),
        other => other.type_name().to_string(),
    }
}

/// `+=` does two different jobs: it appends to an array, or adds two scalars.
fn append_or_add(current: Value, new: Value, span: Span) -> Result<Value, LuxError> {
    match current {
        Value::Array(mut items) => {
            if let Some(first) = items.first()
                && !same_type(first, &new)
            {
                return Err(LuxError::new(
                    format!(
                        "cannot add {} to an array of {}",
                        value_type(&new),
                        value_type(first)
                    ),
                    span,
                )
                .with_learn("arrays", "an array holds one type, so += has to match it"));
            }
            items.push(new);
            Ok(Value::Array(items))
        }
        scalar => add(&scalar, &new, span),
    }
}

fn add(a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
        (Value::Str(x), Value::Str(y)) => Ok(Value::Str(format!("{}{}", x, y))),
        _ => Err(mix_or_type_error("add", a, b, span)),
    }
}

fn sub(a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
        _ => Err(mix_or_type_error("subtract", a, b, span)),
    }
}

fn mul(a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
        _ => Err(mix_or_type_error("multiply", a, b, span)),
    }
}

fn div(a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => Err(LuxError::new("division by zero", span)),
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x / y)),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
        _ => Err(mix_or_type_error("divide", a, b, span)),
    }
}

fn modulo(a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => Err(LuxError::new("remainder by zero", span)),
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x % y)),
        _ => Err(LuxError::new(
            format!(
                "% needs two ints, but got {} and {}",
                named(a.type_name()),
                named(b.type_name())
            ),
            span,
        )),
    }
}

fn equality(a: &Value, b: &Value, span: Span, negate: bool) -> Result<Value, LuxError> {
    if !same_type(a, b) {
        return Err(LuxError::new(
            format!("cannot compare {} with {}", value_type(a), value_type(b)),
            span,
        )
        .with_note("both sides of == and != must be the same type"));
    }
    let eq = a == b;
    Ok(Value::Bool(if negate { !eq } else { eq }))
}

fn ordering(op: BinOp, a: &Value, b: &Value, span: Span) -> Result<Value, LuxError> {
    use std::cmp::Ordering;
    let ord: Ordering = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x
            .partial_cmp(y)
            .ok_or_else(|| LuxError::new("cannot compare with NaN", span))?,
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(_), Value::Bool(_)) => {
            return Err(LuxError::new("cannot order bool values with < or >", span)
                .with_note("use == or != to compare bools")
                .with_learn(
                    "booleans",
                    "true and false aren't ordered, only equal or not",
                ));
        }
        _ => {
            return Err(LuxError::new(
                format!(
                    "cannot compare {} with {}",
                    named(a.type_name()),
                    named(b.type_name())
                ),
                span,
            )
            .with_note("both sides must be the same type"));
        }
    };
    let result = match op {
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Gt => ord == Ordering::Greater,
        BinOp::Le => ord != Ordering::Greater,
        BinOp::Ge => ord != Ordering::Less,
        _ => unreachable!(),
    };
    Ok(Value::Bool(result))
}

/// Shared error for arithmetic: distinguishes "you mixed int and float" from a
/// plain type mismatch, because mixing is the common beginner mistake.
fn mix_or_type_error(verb: &str, a: &Value, b: &Value, span: Span) -> LuxError {
    let mixed = matches!(
        (a, b),
        (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_))
    );
    if mixed {
        LuxError::new("cannot mix int and float — convert one first", span)
            .with_note("wrap a value in float(...) or int(...)")
            .with_learn(
                "numbers",
                "there's a reason lux makes you say when a whole number becomes a fraction",
            )
    } else {
        // A string on either side of `+` is the classic "glue text to a number"
        // mistake, which the strings topic answers with `string(...)`; other
        // type mismatches are arithmetic, so they point at numbers.
        let (topic, lure) = if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
            (
                "strings",
                "lux never turns a number into text for you — you ask",
            )
        } else {
            (
                "numbers",
                "arithmetic needs both sides to be the same number type",
            )
        };
        LuxError::new(
            format!(
                "cannot {} {} and {}",
                verb,
                named(a.type_name()),
                named(b.type_name())
            ),
            span,
        )
        .with_learn(topic, lure)
    }
}

// ----- types: annotations, matching, and zero values ------------------------

/// Render a type annotation the way the source wrote it: `int`, `[int]`.
fn describe_type(ann: &TypeAnn) -> String {
    match &ann.kind {
        TypeKind::Named(n) => n.clone(),
        TypeKind::Array(elem) => format!("[{}]", describe_type(elem)),
        TypeKind::Generic(name, args) => {
            let inner: Vec<String> = args.iter().map(describe_type).collect();
            format!("{}<{}>", name, inner.join(", "))
        }
    }
}

/// Do two values share a type? For scalars this is just their type name; for
/// structs and enums it's the declared type name (a Point is not a Color); for
/// arrays it's the element type, with an empty array compatible with any.
fn same_type(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Struct { name: x, .. }, Value::Struct { name: y, .. }) => x == y,
        (
            Value::Enum {
                enum_name: x,
                variant: vx,
                fields: fx,
            },
            Value::Enum {
                enum_name: y,
                variant: vy,
                fields: fy,
            },
        ) => {
            if x != y {
                return false;
            }
            // For the built-in generics, two values share a type only when their
            // known payloads agree. A `none` knows no payload, so it fits any
            // Option; an `ok` and an `err` constrain different parameters, so
            // they never conflict.
            match x.as_str() {
                "Option" => payloads_compatible(fx, fy),
                "Result" if vx == vy => payloads_compatible(fx, fy),
                "Result" => true,
                _ => true,
            }
        }
        (Value::Array(x), Value::Array(y)) => match (x.first(), y.first()) {
            (Some(a), Some(b)) => same_type(a, b),
            _ => true,
        },
        _ => a.type_name() == b.type_name(),
    }
}

/// Do two built-in enum payloads agree? A missing payload (like `none`'s) is
/// compatible with anything, mirroring how an empty array fits any array type.
fn payloads_compatible(fx: &[(String, Value)], fy: &[(String, Value)]) -> bool {
    match (fx.first(), fy.first()) {
        (Some((_, a)), Some((_, b))) => same_type(a, b),
        _ => true,
    }
}

// ----- the built-in generics: Option and Result -----------------------------

/// The single field name carried inside a built-in enum value. It never shows
/// (these print positionally), and matching binds by position, so the name is
/// just internal bookkeeping.
const PAYLOAD: &str = "value";

fn option_none() -> Value {
    Value::Enum {
        enum_name: "Option".to_string(),
        variant: "none".to_string(),
        fields: Vec::new(),
    }
}

fn option_some(v: Value) -> Value {
    Value::Enum {
        enum_name: "Option".to_string(),
        variant: "some".to_string(),
        fields: vec![(PAYLOAD.to_string(), v)],
    }
}

fn result_ok(v: Value) -> Value {
    Value::Enum {
        enum_name: "Result".to_string(),
        variant: "ok".to_string(),
        fields: vec![(PAYLOAD.to_string(), v)],
    }
}

fn result_err(v: Value) -> Value {
    Value::Enum {
        enum_name: "Result".to_string(),
        variant: "err".to_string(),
        fields: vec![(PAYLOAD.to_string(), v)],
    }
}

/// The `Output` struct `run` hands back: the exit status and the two captured
/// streams, in the order the built-in struct declares its fields.
fn output_value(status: i64, stdout: String, stderr: String) -> Value {
    Value::Struct {
        name: "Output".to_string(),
        fields: vec![
            ("status".to_string(), Value::Int(status)),
            ("stdout".to_string(), Value::Str(stdout)),
            ("stderr".to_string(), Value::Str(stderr)),
        ],
    }
}

/// Reject a `let`/`var` with no annotation whose value can't pin its own type:
/// `none` could be any `Option`, an `ok`/`err` leaves Result's other half open.
/// lux can't infer what it won't run, so it asks for an annotation — the same
/// thing Rust and Swift do for a bare `None`/`nil`.
fn ensure_determined(v: &Value, span: Span) -> Result<(), LuxError> {
    if fully_determined(v) {
        Ok(())
    } else {
        Err(LuxError::new(
            format!(
                "can't tell what type this is — {} leaves it open",
                value_type(v)
            ),
            span,
        )
        .with_note("name the type, like `let x: Option<int> = none`")
        .with_learn(
            "option",
            "lux usually guesses the type, but an empty none needs you to say",
        ))
    }
}

/// Is a value's full type knowable from the value alone? Everything is, except
/// a built-in generic that hasn't pinned all its parameters.
fn fully_determined(v: &Value) -> bool {
    match v {
        Value::Enum {
            enum_name,
            variant,
            fields,
        } if enum_name == "Option" => {
            variant == "some" && fields.first().is_some_and(|(_, x)| fully_determined(x))
        }
        // A Result value is always just one side, so the other parameter is
        // never known from the value — it needs an annotation.
        Value::Enum { enum_name, .. } if enum_name == "Result" => false,
        // One determined element fixes a homogeneous array; an empty array
        // stays as loose as it has always been.
        Value::Array(items) => items.is_empty() || items.iter().any(fully_determined),
        _ => true,
    }
}

/// The built-ins whose return type is a fixed `Option<T>` or `Result<T, E>`. The
/// type is pinned by the signature even when the returned value can't show it — a
/// `none` from `readLine` is still `Option<string>` — so a `let`/`var` bound
/// straight to one of these calls needs no annotation, unlike a bare `none`/`err`
/// literal whose type genuinely is open. `input` is absent on purpose: it returns
/// a plain `string` (empty at end of input), so it never leaves a type unknown.
const TYPE_PINNING_BUILTINS: [&str; 6] = [
    "readLine",
    "readFile",
    "writeFile",
    "run",
    "parseInt",
    "parseFloat",
];

/// Does this expression's type come from a callee's signature rather than from
/// the value it produced? True for a direct call to one of the fallible built-ins
/// above, or to any user function — a declared return type pins the binding even
/// when the returned value can't, so `let r = half(4)` is as fine as `let n =
/// parseInt(x)`. `some(none)` and a bare `none` are still left to the
/// value-directed check, because their type really is open.
fn call_pins_type(expr: &Expr, funcs: &HashMap<String, Rc<FuncData>>) -> bool {
    matches!(expr, Expr::Call { name, .. }
        if TYPE_PINNING_BUILTINS.contains(&name.as_str()) || funcs.contains_key(name))
}

/// Is this value a `Result`? A `Result` is meant to be handled where it's made,
/// so it may not be stored in a variable (unlike an `Option`, which is a real
/// value everywhere). Storing errors as values to pass around is a Rust/Swift
/// habit that doesn't carry to Go — whose errors are handled at the call site —
/// so lux keeps the rule uniform, and stashing a Result becomes a lesson for when
/// you graduate to those languages.
fn is_result(v: &Value) -> bool {
    matches!(v, Value::Enum { enum_name, .. } if enum_name == "Result")
}

/// The error for trying to store a `Result` in a `let`/`var`.
fn result_not_stored(span: Span) -> LuxError {
    LuxError::new(
        "a Result can't be stored — handle it where it's produced".to_string(),
        span,
    )
    .with_note("match it right here — `match … { ok(let x) => …  err(let e) => … }` — or return it")
    .with_learn(
        "result",
        "a Result is answered where it's made: matched or returned, not kept in a variable",
    )
}

/// The comma-separated case names of an enum, for "did you mean" notes.
fn variant_names(data: &EnumData) -> String {
    data.variants
        .iter()
        .map(|v| v.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The error for declaring two types with the same name.
fn already_defined(name: &str, span: Span) -> LuxError {
    LuxError::new(format!("type `{}` is already defined", name), span)
}

/// "1 value" / "2 values" — small helper for argument-count errors.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{} {}", n, noun)
    } else {
        format!("{} {}s", n, noun)
    }
}

// ----- printing -------------------------------------------------------------

/// How a value prints. Floats always show a decimal point so 3.0 reads as a
/// float, not an int. Arrays print their elements comma-separated in brackets.
fn display(v: &Value) -> String {
    render(v, false)
}

/// Like `display`, but with strings quoted and escaped, so a traced value reads
/// unambiguously — `some("north")`, `["key", "torch"]` — where `print` shows the
/// bare text. Used only by `lux trace`.
fn trace_render(v: &Value) -> String {
    render(v, true)
}

/// Render a value to text. With `quote`, string leaves are shown quoted and
/// escaped; without, they're shown as their plain text, the way `print` does.
fn render(v: &Value, quote: bool) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => {
            if quote {
                format!("{s:?}")
            } else {
                s.clone()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(|x| render(x, quote)).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Range(lo, hi) => format!("{}..{}", lo, hi),
        Value::Struct { name, fields } => {
            format!("{}({})", name, render_fields(fields, quote))
        }
        // The built-in generics print the way they're written — `some(5)`,
        // `none`, `err("nope")` — not in the labelled `Enum.case(...)` form.
        Value::Enum {
            enum_name,
            variant,
            fields,
        } if enum_name == "Option" || enum_name == "Result" => match fields.first() {
            Some((_, payload)) => format!("{}({})", variant, render(payload, quote)),
            None => variant.clone(),
        },
        Value::Enum {
            enum_name,
            variant,
            fields,
        } => {
            if fields.is_empty() {
                format!("{}.{}", enum_name, variant)
            } else {
                format!(
                    "{}.{}({})",
                    enum_name,
                    variant,
                    render_fields(fields, quote)
                )
            }
        }
        Value::Unit => String::new(),
    }
}

fn format_float(f: f64) -> String {
    if f.is_finite() && f == f.trunc() {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

/// Render labelled fields as `name: value, name: value`, shared by structs and
/// enum cases so they print the way they were built.
fn render_fields(fields: &[(String, Value)], quote: bool) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}: {}", k, render(v, quote)))
        .collect::<Vec<_>>()
        .join(", ")
}

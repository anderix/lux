//! The interpreter: walk the ast and run it.
//!
//! This is a tree-walking interpreter. It keeps a stack of scopes (one per
//! block) holding the live bindings, plus a table of function definitions.
//! lux is statically typed by design, but v0.1 has no separate type checker
//! yet — instead the interpreter enforces lux's no-coercion rule at the moment
//! of each operation, so `"5" + 3` fails with a clear error rather than
//! silently guessing. A real checker that catches these before the program
//! runs is a later milestone.

use std::collections::HashMap;
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
            Value::Unit => "nothing",
        }
    }
}

struct Binding {
    value: Value,
    mutable: bool,
}

/// A user-defined function, stored once and shared (via `Rc`) so a call can
/// hold onto it while the interpreter keeps mutating its scopes — which is what
/// recursion needs.
struct FuncData {
    params: Vec<Param>,
    ret: Option<TypeAnn>,
    body: Vec<Stmt>,
}

/// How a statement finished: either normally, or by hitting a `return`.
enum Flow {
    Normal,
    Return(Value),
}

struct Interp {
    scopes: Vec<HashMap<String, Binding>>,
    funcs: HashMap<String, Rc<FuncData>>,
}

/// Run a parsed program.
pub fn run(program: &[Stmt]) -> Result<(), LuxError> {
    let mut interp = Interp {
        scopes: vec![HashMap::new()],
        funcs: HashMap::new(),
    };
    interp.register_funcs(program)?;
    interp.exec_block(program)?;
    Ok(())
}

impl Interp {
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

    fn declare(&mut self, name: String, value: Value, mutable: bool) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name, Binding { value, mutable });
    }

    // ----- statements -------------------------------------------------------

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Flow, LuxError> {
        for s in stmts {
            match self.exec_stmt(s)? {
                Flow::Normal => {}
                ret @ Flow::Return(_) => return Ok(ret),
            }
        }
        Ok(Flow::Normal)
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
                if let Some(ann) = ty {
                    check_type(ann, &v)?;
                }
                if self.current_has(name) {
                    return Err(LuxError::new(
                        format!("`{}` is already declared in this scope", name),
                        *span,
                    ));
                }
                self.declare(name.clone(), v, false);
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
                        if let Some(ann) = ty {
                            check_type(ann, &v)?;
                        }
                        v
                    }
                    None => {
                        let ann = ty.as_ref().ok_or_else(|| {
                            LuxError::new(format!("`{}` needs a type or a value", name), *span)
                                .with_note("write `var x: int` or `var x = 0`")
                        })?;
                        zero_value(ann)?
                    }
                };
                if self.current_has(name) {
                    return Err(LuxError::new(
                        format!("`{}` is already declared in this scope", name),
                        *span,
                    ));
                }
                self.declare(name.clone(), v, true);
                Ok(Flow::Normal)
            }

            Stmt::Assign {
                name,
                name_span,
                op,
                value,
                span,
            } => {
                let new = self.eval(value)?;
                let binding = self.lookup(name).ok_or_else(|| {
                    LuxError::new(format!("`{}` is not defined", name), *name_span)
                        .with_note("declare it first with let or var")
                })?;
                if !binding.mutable {
                    return Err(LuxError::new(
                        format!("cannot reassign `{}` — it was declared with let", name),
                        *span,
                    )
                    .with_note("use `var` instead of `let` if it needs to change"));
                }
                let current = binding.value.clone();
                let result = match op {
                    AssignOp::Set => {
                        if current.type_name() != new.type_name() {
                            return Err(LuxError::new(
                                format!(
                                    "`{}` is {} but you assigned {}",
                                    name,
                                    named(current.type_name()),
                                    named(new.type_name())
                                ),
                                *span,
                            ));
                        }
                        new
                    }
                    AssignOp::Add => append_or_add(current, new, *span)?,
                    AssignOp::Sub => sub(&current, &new, *span)?,
                };
                self.lookup_mut(name).unwrap().value = result;
                Ok(Flow::Normal)
            }

            // Functions are registered up front; the declaration itself does
            // nothing when execution reaches it.
            Stmt::Func { .. } => Ok(Flow::Normal),

            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(v))
            }

            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                if self.eval_bool(cond)? {
                    self.run_scoped(then_body)
                } else if let Some(eb) = else_body {
                    self.run_scoped(eb)
                } else {
                    Ok(Flow::Normal)
                }
            }

            Stmt::While { cond, body, .. } => {
                while self.eval_bool(cond)? {
                    match self.run_scoped(body)? {
                        Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                }
                Ok(Flow::Normal)
            }

            Stmt::For {
                var, iter, body, ..
            } => {
                let iterable = self.eval(iter)?;
                match iterable {
                    Value::Array(items) => {
                        for item in items {
                            match self.run_loop_body(var, item, body)? {
                                Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                    }
                    Value::Range(lo, hi) => {
                        let mut i = lo;
                        while i < hi {
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
                        .with_note("for ... in needs an array or a range like 0..10"));
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
    fn run_loop_body(
        &mut self,
        var: &str,
        item: Value,
        body: &[Stmt],
    ) -> Result<Flow, LuxError> {
        self.push();
        self.declare(var.to_string(), item, false);
        let r = self.exec_block(body);
        self.pop();
        r
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
                None => Err(LuxError::new(format!("`{}` is not defined", name), *span)
                    .with_note("declare it with let or var before using it")),
            },
            Expr::Array(elems, _) => {
                let mut items = Vec::with_capacity(elems.len());
                for e in elems {
                    items.push(self.eval(e)?);
                }
                // Arrays are homogeneous: every element shares one type.
                if let Some(first) = items.first() {
                    let want = first.type_name();
                    for (v, e) in items.iter().zip(elems).skip(1) {
                        if v.type_name() != want {
                            return Err(LuxError::new(
                                format!(
                                    "an array's elements must all be the same type, but found {} and {}",
                                    value_type(first),
                                    value_type(v)
                                ),
                                e.span(),
                            ));
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
                            format!("cannot index into {}; only arrays can be indexed", value_type(&other)),
                            base.span(),
                        ));
                    }
                };
                let i = match idx {
                    Value::Int(i) => i,
                    other => {
                        return Err(LuxError::new(
                            format!("an array index must be an int, but this is {}", value_type(&other)),
                            index.span(),
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
                    .with_note(note));
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
                    .with_note("write something like 0..10")),
                }
            }
            Expr::Call { name, args, span } => self.call(name, args, *span),
        }
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
            .with_note("conditions and &&/|| operands must be bool")),
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
                    Value::Str(s) => s.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                        LuxError::new(format!("cannot read \"{}\" as an int", s), span)
                    }),
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
                    Value::Str(s) => s.trim().parse::<f64>().map(Value::Float).map_err(|_| {
                        LuxError::new(format!("cannot read \"{}\" as a float", s), span)
                    }),
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
                        format!("length expects an array or a string, but got {}", value_type(&other)),
                        span,
                    )),
                }
            }
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
                    "define it with `func`, or use a built-in: print, string, int, float, length",
                ));
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
            ));
        }

        // Evaluate and type-check the arguments in the caller's scope.
        let mut frame = HashMap::new();
        for (param, arg) in func.params.iter().zip(args) {
            let v = self.eval(arg)?;
            validate_type(&param.ty)?;
            if !type_matches(&param.ty, &v) {
                return Err(LuxError::new(
                    format!(
                        "`{}` expects `{}` to be {}, but got {}",
                        name,
                        param.name,
                        describe_type(&param.ty),
                        value_type(&v)
                    ),
                    arg.span(),
                ));
            }
            frame.insert(
                param.name.clone(),
                Binding {
                    value: v,
                    mutable: false,
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
                validate_type(ann)?;
                if matches!(returned, Value::Unit) {
                    return Err(LuxError::new(
                        format!(
                            "`{}` must return {}, but it ended without returning a value",
                            name,
                            describe_type(ann)
                        ),
                        span,
                    ));
                }
                if !type_matches(ann, &returned) {
                    return Err(LuxError::new(
                        format!(
                            "`{}` should return {}, but returned {}",
                            name,
                            describe_type(ann),
                            value_type(&returned)
                        ),
                        span,
                    ));
                }
                Ok(returned)
            }
            None => {
                if !matches!(returned, Value::Unit) {
                    return Err(LuxError::new(
                        format!("`{}` has no return type, so it can't return a value", name),
                        span,
                    )
                    .with_note("add `-> type` to the signature if it should return something"));
                }
                Ok(Value::Unit)
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
        .with_note("! works on bool values")),
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
fn value_type(v: &Value) -> String {
    match v {
        Value::Array(items) => match items.first() {
            Some(first) => format!("[{}]", first.type_name()),
            None => "[]".to_string(),
        },
        other => other.type_name().to_string(),
    }
}

/// `+=` does two different jobs: it appends to an array, or adds two scalars.
fn append_or_add(current: Value, new: Value, span: Span) -> Result<Value, LuxError> {
    match current {
        Value::Array(mut items) => {
            if let Some(first) = items.first() {
                if first.type_name() != new.type_name() {
                    return Err(LuxError::new(
                        format!(
                            "cannot add {} to an array of {}",
                            value_type(&new),
                            first.type_name()
                        ),
                        span,
                    ));
                }
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
    if a.type_name() != b.type_name() {
        return Err(LuxError::new(
            format!(
                "cannot compare {} with {}",
                named(a.type_name()),
                named(b.type_name())
            ),
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
                .with_note("use == or != to compare bools"));
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
    } else {
        LuxError::new(
            format!(
                "cannot {} {} and {}",
                verb,
                named(a.type_name()),
                named(b.type_name())
            ),
            span,
        )
    }
}

// ----- types: annotations, matching, and zero values ------------------------

/// Render a type annotation the way the source wrote it: `int`, `[int]`.
fn describe_type(ann: &TypeAnn) -> String {
    match &ann.kind {
        TypeKind::Named(n) => n.clone(),
        TypeKind::Array(elem) => format!("[{}]", describe_type(elem)),
    }
}

/// Does a runtime value satisfy a type annotation? Assumes the annotation's
/// names are already known (see `validate_type`). An empty array satisfies any
/// array type, since it has no elements to disagree.
fn type_matches(ann: &TypeAnn, v: &Value) -> bool {
    match (&ann.kind, v) {
        (TypeKind::Named(n), _) => n == v.type_name(),
        (TypeKind::Array(elem), Value::Array(items)) => {
            items.iter().all(|it| type_matches(elem, it))
        }
        _ => false,
    }
}

/// Check that every name in an annotation is a real type.
fn validate_type(ann: &TypeAnn) -> Result<(), LuxError> {
    match &ann.kind {
        TypeKind::Named(n) => {
            if matches!(n.as_str(), "int" | "float" | "string" | "bool") {
                Ok(())
            } else {
                Err(LuxError::new(format!("unknown type `{}`", n), ann.span)
                    .with_note("v0.1 has int, float, string, bool, and arrays like [int]"))
            }
        }
        TypeKind::Array(elem) => validate_type(elem),
    }
}

/// Validate an annotation and confirm a value matches it. Used by `let`/`var`
/// type annotations, where the annotation is the thing to blame.
fn check_type(ann: &TypeAnn, v: &Value) -> Result<(), LuxError> {
    validate_type(ann)?;
    if !type_matches(ann, v) {
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
fn zero_value(ann: &TypeAnn) -> Result<Value, LuxError> {
    match &ann.kind {
        TypeKind::Named(n) => match n.as_str() {
            "int" => Ok(Value::Int(0)),
            "float" => Ok(Value::Float(0.0)),
            "string" => Ok(Value::Str(String::new())),
            "bool" => Ok(Value::Bool(false)),
            _ => Err(LuxError::new(format!("unknown type `{}`", n), ann.span)
                .with_note("v0.1 has int, float, string, bool, and arrays like [int]")),
        },
        TypeKind::Array(elem) => {
            validate_type(elem)?;
            Ok(Value::Array(Vec::new()))
        }
    }
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
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Range(lo, hi) => format!("{}..{}", lo, hi),
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

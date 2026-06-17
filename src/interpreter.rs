//! The interpreter: walk the ast and run it.
//!
//! This is a tree-walking interpreter. It keeps a stack of scopes (one per
//! block) holding the live bindings. lux is statically typed by design, but
//! v0.1 has no separate type checker yet — instead the interpreter enforces
//! lux's no-coercion rule at the moment of each operation, so `"5" + 3` fails
//! with a clear error rather than silently guessing. A real checker that
//! catches these before the program runs is a later milestone.

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::{LuxError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
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
            Value::Unit => "nothing",
        }
    }
}

struct Binding {
    value: Value,
    mutable: bool,
}

struct Interp {
    scopes: Vec<HashMap<String, Binding>>,
}

/// Run a parsed program.
pub fn run(program: &[Stmt]) -> Result<(), LuxError> {
    let mut interp = Interp {
        scopes: vec![HashMap::new()],
    };
    interp.exec_block(program)
}

impl Interp {
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

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<(), LuxError> {
        for s in stmts {
            self.exec_stmt(s)?;
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), LuxError> {
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
                Ok(())
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
                Ok(())
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
                    AssignOp::Add => add(&current, &new, *span)?,
                    AssignOp::Sub => sub(&current, &new, *span)?,
                };
                self.lookup_mut(name).unwrap().value = result;
                Ok(())
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
                    Ok(())
                }
            }

            Stmt::While { cond, body, .. } => {
                while self.eval_bool(cond)? {
                    self.run_scoped(body)?;
                }
                Ok(())
            }

            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(())
            }
        }
    }

    /// Run a block in a fresh scope, always popping it even on error.
    fn run_scoped(&mut self, body: &[Stmt]) -> Result<(), LuxError> {
        self.push();
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
            Expr::Call { name, args, span } => self.call(name, args, *span),
        }
    }

    fn eval_bool(&mut self, e: &Expr) -> Result<bool, LuxError> {
        match self.eval(e)? {
            Value::Bool(b) => Ok(b),
            other => Err(LuxError::new(
                format!("expected a true/false value, but this is {}", named(other.type_name())),
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
            _ => Err(LuxError::new(format!("unknown function `{}`", name), span)
                .with_note("v0.1 has print, string, int, and float; your own functions come next")),
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
        (Value::Int(_), Value::Int(0)) => {
            Err(LuxError::new("division by zero", span))
        }
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
            format!("cannot compare {} with {}", named(a.type_name()), named(b.type_name())),
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
                format!("cannot compare {} with {}", named(a.type_name()), named(b.type_name())),
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
            format!("cannot {} {} and {}", verb, named(a.type_name()), named(b.type_name())),
            span,
        )
    }
}

// ----- type annotations and printing ----------------------------------------

fn check_type(ann: &TypeAnn, v: &Value) -> Result<(), LuxError> {
    let known = matches!(ann.name.as_str(), "int" | "float" | "string" | "bool");
    if !known {
        return Err(LuxError::new(format!("unknown type `{}`", ann.name), ann.span)
            .with_note("v0.1 has int, float, string, and bool"));
    }
    if ann.name != v.type_name() {
        return Err(LuxError::new(
            format!(
                "type mismatch: annotated `{}` but the value is {}",
                ann.name,
                named(v.type_name())
            ),
            ann.span,
        ));
    }
    Ok(())
}

fn zero_value(ann: &TypeAnn) -> Result<Value, LuxError> {
    match ann.name.as_str() {
        "int" => Ok(Value::Int(0)),
        "float" => Ok(Value::Float(0.0)),
        "string" => Ok(Value::Str(String::new())),
        "bool" => Ok(Value::Bool(false)),
        _ => Err(LuxError::new(format!("unknown type `{}`", ann.name), ann.span)
            .with_note("v0.1 has int, float, string, and bool")),
    }
}

/// How a value prints. Floats always show a decimal point so 3.0 reads as a
/// float, not an int.
fn display(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
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

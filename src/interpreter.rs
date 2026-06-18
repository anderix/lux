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
}

/// Run a parsed program.
pub fn run(program: &[Stmt]) -> Result<(), LuxError> {
    let mut interp = Interp {
        scopes: vec![HashMap::new()],
        funcs: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
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
                match ty {
                    Some(ann) => self.check_type(ann, &v)?,
                    None => ensure_determined(&v, value.span())?,
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
                        match ty {
                            Some(ann) => self.check_type(ann, &v)?,
                            None => ensure_determined(&v, e.span())?,
                        }
                        v
                    }
                    None => {
                        let ann = ty.as_ref().ok_or_else(|| {
                            LuxError::new(format!("`{}` needs a type or a value", name), *span)
                                .with_note("write `var x: int` or `var x = 0`")
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
                    .with_note("use `var` instead of `let` if it needs to change")
                    .with_learn("variables"));
                }
                let current = binding.value.clone();
                let result = match op {
                    AssignOp::Set => {
                        if !same_type(&current, &new) {
                            return Err(LuxError::new(
                                format!(
                                    "`{}` is {} but you assigned {}",
                                    name,
                                    value_type(&current),
                                    value_type(&new)
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

            // Functions and types are registered up front; the declarations
            // themselves do nothing when execution reaches them.
            Stmt::Func { .. } | Stmt::Struct { .. } | Stmt::Enum { .. } => Ok(Flow::Normal),

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
                        .with_note("for ... in needs an array or a range like 0..10")
                        .with_learn("for"));
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
                None if name == "none" => Ok(option_none()),
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
                    for (v, e) in items.iter().zip(elems).skip(1) {
                        if !same_type(first, v) {
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
                        )
                        .with_learn("arrays"));
                    }
                };
                let i = match idx {
                    Value::Int(i) => i,
                    other => {
                        return Err(LuxError::new(
                            format!("an array index must be an int, but this is {}", value_type(&other)),
                            index.span(),
                        )
                        .with_learn("arrays"));
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
                    .with_learn("arrays"));
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
                    .with_note("define it with `struct`, or check the spelling"));
            }
        };
        // Reject fields that aren't part of this struct, blaming the field.
        for (k, e) in provided {
            if !data.fields.iter().any(|f| &f.name == k) {
                return Err(LuxError::new(
                    format!("struct `{}` has no field `{}`", name, k),
                    e.span(),
                ));
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
                    .with_note(format!("`{}` has a field `{}: {}`", name, f.name, describe_type(&f.ty))));
                }
            };
            let v = self.eval(value_expr)?;
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
                    .with_note("define it with `enum`, or check the spelling"));
            }
        };
        let vdef = match data.variants.iter().find(|v| v.name == variant) {
            Some(v) => v,
            None => {
                return Err(LuxError::new(
                    format!("enum `{}` has no case `{}`", enum_name, variant),
                    span,
                )
                .with_note(format!("cases are: {}", variant_names(&data))));
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
            ));
        }
        let mut built = Vec::with_capacity(vdef.fields.len());
        for f in &vdef.fields {
            let value_expr = match provided.iter().find(|(k, _)| k == &f.name) {
                Some((_, e)) => e,
                None => {
                    return Err(LuxError::new(
                        format!("missing value `{}` for `{}.{}`", f.name, enum_name, variant),
                        span,
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
                ));
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
        if let Expr::Ident(n, nspan) = base {
            if self.lookup(n).is_none() {
                if self.enums.contains_key(n) {
                    return self.construct_enum(n, field, &[], span);
                }
                // `Name.field` where `Name` is neither a value nor an enum: most
                // likely a misspelled type or variable. Point at both fixes.
                return Err(LuxError::new(format!("`{}` is not defined", n), *nspan)
                    .with_note("if it's an enum, declare it with `enum`; otherwise declare the value with let or var"));
            }
        }
        let v = self.eval(base)?;
        match v {
            Value::Struct { name, fields } => match fields.iter().find(|(k, _)| k == field) {
                Some((_, val)) => Ok(val.clone()),
                None => Err(LuxError::new(
                    format!("struct `{}` has no field `{}`", name, field),
                    span,
                )),
            },
            other => Err(LuxError::new(
                format!(
                    "cannot read field `{}` of {}; only structs have fields",
                    field,
                    value_type(&other)
                ),
                base.span(),
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
                        Pattern::Variant { name, span: psp, .. } => {
                            if !data.variants.iter().any(|vd| &vd.name == name) {
                                return Err(LuxError::new(
                                    format!("enum `{}` has no case `{}`", enum_name, name),
                                    *psp,
                                )
                                .with_note(format!("cases are: {}", variant_names(&data))));
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
                            ));
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
                        .with_learn("match")
                        .with_note(format!(
                            "add an arm for: {} (or a `_` catch-all)",
                            missing.join(", ")
                        )));
                    }
                }

                // Run the first arm that fits, top to bottom.
                for a in arms {
                    match &a.pattern {
                        Pattern::Variant { name, bindings, span: psp } if name == variant => {
                            if bindings.len() != fields.len() {
                                return Err(LuxError::new(
                                    format!(
                                        "case `{}` carries {}, but the pattern captures {}",
                                        name,
                                        count(fields.len(), "value"),
                                        bindings.len()
                                    ),
                                    *psp,
                                ));
                            }
                            self.push();
                            for (b, (_, val)) in bindings.iter().zip(fields.iter()) {
                                self.declare(b.clone(), val.clone(), false);
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
            ));
        }
        // A value match needs a `_`, since int and string have endless values.
        // bool is the exception: `true` and `false` together cover everything.
        let has_wildcard = arms.iter().any(|a| matches!(a.pattern, Pattern::Wildcard(_)));
        let bool_exhaustive = matches!(v, Value::Bool(_))
            && arms.iter().any(|a| matches!(a.pattern, Pattern::Bool(true, _)))
            && arms.iter().any(|a| matches!(a.pattern, Pattern::Bool(false, _)));
        if !has_wildcard && !bool_exhaustive {
            return Err(LuxError::new(
                format!("this match on {} needs a `_` case", value_type(v)),
                span,
            )
            .with_note("matching a value (not an enum) can't be exhaustive, so add `_ => ...`")
            .with_learn("match"));
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
                    ));
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
                self.validate_type(ann)?;
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
                if !self.type_matches(ann, &returned) {
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
                    .with_note(hint));
                }
                if matches!(n.as_str(), "int" | "float" | "string" | "bool") || self.type_exists(n)
                {
                    Ok(())
                } else {
                    Err(LuxError::new(format!("unknown type `{}`", n), ann.span).with_note(
                        "known types: int, float, string, bool, arrays like [int], and any struct or enum you define",
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
                        .with_note("write `Option<int>`"));
                    }
                    self.validate_type(&args[0])
                }
                "Result" => {
                    if args.len() != 2 {
                        return Err(LuxError::new(
                            format!("`Result` takes two types, but got {}", args.len()),
                            ann.span,
                        )
                        .with_note("write `Result<int, string>` — the value type and the error type"));
                    }
                    self.validate_type(&args[0])?;
                    self.validate_type(&args[1])
                }
                _ => Err(LuxError::new(
                    format!("`{}` is not a parameterized type", name),
                    ann.span,
                )
                .with_note("only Option and Result take type parameters in lux v0.1")),
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
            (TypeKind::Named(n), _) => n == v.type_name(),
            (TypeKind::Array(elem), Value::Array(items)) => {
                items.iter().all(|it| self.type_matches(elem, it))
            }
            // A generic annotation matches a built-in enum value when the value's
            // case fits and the payload it carries matches the right parameter.
            // `none` carries nothing, so it satisfies any `Option<T>` — the same
            // way an empty array satisfies any array type.
            (TypeKind::Generic(name, args), Value::Enum { enum_name, variant, fields })
                if name == enum_name =>
            {
                match (name.as_str(), variant.as_str()) {
                    ("Option", "none") => true,
                    ("Option", "some") => self.payload_matches(&args[0], fields),
                    ("Result", "ok") => self.payload_matches(&args[0], fields),
                    ("Result", "err") => self.payload_matches(&args[1], fields),
                    _ => false,
                }
            }
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
                    .with_note("v0.1 has int, float, string, bool, and arrays like [int]")),
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
                        format!("a `var` of type `{}` needs a starting value", describe_type(ann)),
                        ann.span,
                    )
                    .with_note("a Result is either ok(...) or err(...) — there's no empty one"))
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
            Some(first) => format!("[{}]", value_type(first)),
            None => "[]".to_string(),
        },
        Value::Struct { name, .. } => name.clone(),
        // For the built-in generics, show what's known about the parameters and
        // leave the unknown ones as `?`: `none` is `Option<?>`, `ok(5)` is
        // `Result<int, ?>`.
        Value::Enum { enum_name, variant, fields } if enum_name == "Option" => {
            match (variant.as_str(), fields.first()) {
                ("some", Some((_, v))) => format!("Option<{}>", value_type(v)),
                _ => "Option<?>".to_string(),
            }
        }
        Value::Enum { enum_name, variant, fields } if enum_name == "Result" => {
            match (variant.as_str(), fields.first()) {
                ("ok", Some((_, v))) => format!("Result<{}, ?>", value_type(v)),
                ("err", Some((_, v))) => format!("Result<?, {}>", value_type(v)),
                _ => "Result".to_string(),
            }
        }
        Value::Enum { enum_name, .. } => enum_name.clone(),
        other => other.type_name().to_string(),
    }
}

/// `+=` does two different jobs: it appends to an array, or adds two scalars.
fn append_or_add(current: Value, new: Value, span: Span) -> Result<Value, LuxError> {
    match current {
        Value::Array(mut items) => {
            if let Some(first) = items.first() {
                if !same_type(first, &new) {
                    return Err(LuxError::new(
                        format!(
                            "cannot add {} to an array of {}",
                            value_type(&new),
                            value_type(first)
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
            .with_learn("numbers")
    } else {
        // A string on either side of `+` is the classic "glue text to a number"
        // mistake, which the strings topic answers with `string(...)`; other
        // type mismatches are arithmetic, so they point at numbers.
        let topic = if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
            "strings"
        } else {
            "numbers"
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
        .with_learn(topic)
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
            Value::Enum { enum_name: x, variant: vx, fields: fx },
            Value::Enum { enum_name: y, variant: vy, fields: fy },
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

/// Reject a `let`/`var` with no annotation whose value can't pin its own type:
/// `none` could be any `Option`, an `ok`/`err` leaves Result's other half open.
/// lux can't infer what it won't run, so it asks for an annotation — the same
/// thing Rust and Swift do for a bare `None`/`nil`.
fn ensure_determined(v: &Value, span: Span) -> Result<(), LuxError> {
    if fully_determined(v) {
        Ok(())
    } else {
        Err(LuxError::new(
            format!("can't tell what type this is — {} leaves it open", value_type(v)),
            span,
        )
        .with_note("name the type, like `let x: Option<int> = none`"))
    }
}

/// Is a value's full type knowable from the value alone? Everything is, except
/// a built-in generic that hasn't pinned all its parameters.
fn fully_determined(v: &Value) -> bool {
    match v {
        Value::Enum { enum_name, variant, fields } if enum_name == "Option" => {
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
        Value::Struct { name, fields } => {
            format!("{}({})", name, display_fields(fields))
        }
        // The built-in generics print the way they're written — `some(5)`,
        // `none`, `err("nope")` — not in the labelled `Enum.case(...)` form.
        Value::Enum {
            enum_name,
            variant,
            fields,
        } if enum_name == "Option" || enum_name == "Result" => match fields.first() {
            Some((_, payload)) => format!("{}({})", variant, display(payload)),
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
                format!("{}.{}({})", enum_name, variant, display_fields(fields))
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
fn display_fields(fields: &[(String, Value)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}: {}", k, display(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

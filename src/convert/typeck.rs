//! A static type check that runs before any command.
//!
//! lux's interpreter checks types as it evaluates, so a type error only surfaces
//! on a line the program actually reaches — a mistake tucked inside `if false {
//! ... }` runs clean under `lux run`, then fails to compile the moment `lux
//! build` hands it to Rust. The three target languages are all statically typed,
//! so they see the whole program at once; the interpreter, walking only the live
//! path, did not. That is a parity gap: the same source was legal on one leg and
//! not on another.
//!
//! This pass closes it. It walks every expression, live path or not, and applies
//! the same concrete-type rules the interpreter applies at run time — same
//! wording, same `lux learn` trail — so a type error reads identically whether it
//! is caught here or there, and all four legs agree on what counts as a valid
//! program. It reuses the translator's [`Types`](super::Types) to infer an
//! expression's type, since that inference already backs every backend.
//!
//! The one rule it holds to without exception: it never rejects a program the
//! interpreter would accept. Wherever inference cannot pin a concrete type — a
//! bare `none`, a value whose type the pass cannot see — it stays silent and
//! leaves the call to the interpreter at run time and the target compiler at
//! build time. Catching a real mistake early is worth a great deal; refusing a
//! valid program is not worth anything, so the pass declines every uncertain
//! case. It adds no new type rules of its own: everything it enforces, some
//! executed path would already have caught.

use std::collections::HashMap;

use super::{Ty, Types, ty_from_ann};
use crate::ast::*;
use crate::diagnostic::LuxError;

/// Check a whole program's types before it runs or is emitted. Returns the first
/// type error found, or `Ok(())` if every concrete type lines up.
pub fn check(program: &[Stmt]) -> Result<(), LuxError> {
    let mut c = Checker {
        t: Types::new(program),
        ret: None,
        fname: String::new(),
    };
    // Top-level statements run in the global scope; each function body is checked
    // on its own, the way the interpreter runs a call against just its frame.
    c.walk(program)?;
    Ok(())
}

struct Checker {
    t: Types,
    /// The declared return type of the function currently being checked, if any,
    /// so a `return` inside it can be measured against the `-> type`.
    ret: Option<TypeAnn>,
    /// The name of that function, for the return-mismatch message.
    fname: String,
}

/// Whether an inferred type satisfies a written annotation. `Unsure` is the
/// answer whenever inference cannot see enough to decide — an empty array, a
/// bare `none`, an unknown payload — and it is treated exactly like `Yes`, so an
/// uncertain case is never reported.
#[derive(PartialEq)]
enum Fit {
    Yes,
    No,
    Unsure,
}

impl Checker {
    /// Walk a run of statements in the current scope, without opening a new one.
    /// The top level and a function body both use this: their scope is already
    /// set up by the caller.
    fn walk(&mut self, stmts: &[Stmt]) -> Result<(), LuxError> {
        for s in stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    /// Walk a nested block — the body of an `if`, `while`, `for`, or the arms of
    /// nothing in particular — in its own scope, so a name it declares does not
    /// leak to the code after it.
    fn scoped(&mut self, stmts: &[Stmt]) -> Result<(), LuxError> {
        self.t.push_scope();
        let r = self.walk(stmts);
        self.t.pop_scope();
        r
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), LuxError> {
        match s {
            Stmt::Let {
                name, ty, value, ..
            } => {
                self.expr(value)?;
                if let Some(ann) = ty {
                    self.check_annotation(ann, value)?;
                }
                let declared = match ty {
                    Some(ann) => ty_from_ann(ann),
                    None => self.t.type_of(value),
                };
                self.t.declare(name.clone(), declared);
                Ok(())
            }
            Stmt::Var {
                name, ty, value, ..
            } => {
                if let Some(v) = value {
                    self.expr(v)?;
                    if let Some(ann) = ty {
                        self.check_annotation(ann, v)?;
                    }
                }
                let declared = match (ty, value) {
                    (Some(ann), _) => ty_from_ann(ann),
                    (None, Some(v)) => self.t.type_of(v),
                    (None, None) => Ty::Unknown,
                };
                self.t.declare(name.clone(), declared);
                Ok(())
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => {
                self.expr(target)?;
                self.expr(value)?;
                let place = self.t.type_of(target);
                let val = self.t.type_of(value);
                match op {
                    AssignOp::Set => {
                        if same_ty(&place, &val) == Fit::No
                            && let (Some(p), Some(v)) = (value_name(&place), value_name(&val))
                        {
                            return Err(LuxError::new(
                                format!(
                                    "`{}` is {} but you assigned {}",
                                    describe_place(target),
                                    p,
                                    v
                                ),
                                *span,
                            )
                            .with_learn("variables", "a place keeps the type it started with"));
                        }
                        Ok(())
                    }
                    // `+=` appends when the place is an array, otherwise it adds.
                    AssignOp::Add => match &place {
                        Ty::Array(elem) => {
                            if same_ty(elem, &val) == Fit::No
                                && let (Some(e), Some(v)) = (value_name(elem), value_name(&val))
                            {
                                return Err(LuxError::new(
                                    format!("cannot add {} to an array of {}", v, e),
                                    *span,
                                )
                                .with_learn(
                                    "arrays",
                                    "an array holds one type, so += has to match it",
                                ));
                            }
                            Ok(())
                        }
                        _ => self.arithmetic(BinOp::Add, &place, &val, *span),
                    },
                    AssignOp::Sub => self.arithmetic(BinOp::Sub, &place, &val, *span),
                }
            }
            Stmt::Func {
                name,
                params,
                ret,
                body,
                ..
            } => self.check_func(name, params, ret, body),
            Stmt::Return { value, span } => {
                if let Some(e) = value {
                    self.expr(e)?;
                }
                // Only meaningful inside a function; a top-level `return` has no
                // signature to answer to.
                if self.fname.is_empty() {
                    return Ok(());
                }
                match (&self.ret, value) {
                    (Some(ann), Some(e)) => {
                        let ty = self.t.type_of(e);
                        if self.satisfies_ty(ann, &ty) == Fit::No
                            && let Some(got) = value_name(&ty)
                        {
                            return Err(LuxError::new(
                                format!(
                                    "`{}` should return {}, but returned {}",
                                    self.fname,
                                    describe_ann(ann),
                                    got
                                ),
                                *span,
                            )
                            .with_learn("functions", "what comes back must match the `-> type`"));
                        }
                        Ok(())
                    }
                    (None, Some(_)) => Err(LuxError::new(
                        format!(
                            "`{}` has no return type, so it can't return a value",
                            self.fname
                        ),
                        *span,
                    )
                    .with_learn(
                        "functions",
                        "add a `-> type` if it should hand something back",
                    )),
                    _ => Ok(()),
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.expr(cond)?;
                self.condition(cond)?;
                self.scoped(then_body)?;
                if let Some(eb) = else_body {
                    self.scoped(eb)?;
                }
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond)?;
                self.condition(cond)?;
                self.scoped(body)
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                self.expr(iter)?;
                let it = self.t.type_of(iter);
                let elem = match &it {
                    Ty::Array(e) => Some((**e).clone()),
                    Ty::Range => Some(Ty::Int),
                    Ty::Unknown => None,
                    other => {
                        if let Some(shown) = value_name(other) {
                            return Err(LuxError::new(
                                format!("cannot loop over {}", shown),
                                iter.span(),
                            )
                            .with_note("for ... in needs an array or a range like 0..10")
                            .with_learn("for", "for walks an array or counts a range like 0..10"));
                        }
                        None
                    }
                };
                self.t.push_scope();
                self.t.declare(var.clone(), elem.unwrap_or(Ty::Unknown));
                let r = self.walk(body);
                self.t.pop_scope();
                r
            }
            // Declarations carry no runtime type check of their own; their names
            // are already in the shared environment.
            Stmt::Struct { .. } | Stmt::Enum { .. } => Ok(()),
            Stmt::Expr(e) => self.expr(e),
        }
    }

    /// Check a function body against its signature, in a scope holding only its
    /// parameters — the same isolation the interpreter gives a call.
    fn check_func(
        &mut self,
        name: &str,
        params: &[Param],
        ret: &Option<TypeAnn>,
        body: &[Stmt],
    ) -> Result<(), LuxError> {
        let mut frame = HashMap::new();
        for p in params {
            frame.insert(p.name.clone(), ty_from_ann(&p.ty));
        }
        let saved_scopes = std::mem::replace(&mut self.t.scopes, vec![frame]);
        let saved_ret = self.ret.take();
        let saved_fname = std::mem::replace(&mut self.fname, name.to_string());
        self.ret = ret.clone();

        let r = self.walk(body);

        self.t.scopes = saved_scopes;
        self.ret = saved_ret;
        self.fname = saved_fname;
        r
    }

    /// Walk an expression, checking every concrete-type rule it can reach. The
    /// walk is bottom-up: sub-expressions first, then the rule at this node.
    fn expr(&mut self, e: &Expr) -> Result<(), LuxError> {
        match e {
            Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Ident(..) => {
                Ok(())
            }
            Expr::Array(els, _) => {
                for el in els {
                    self.expr(el)?;
                }
                Ok(())
            }
            Expr::Unary { rhs, .. } => self.expr(rhs),
            Expr::Binary { op, lhs, rhs, span } => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                let l = self.t.type_of(lhs);
                let r = self.t.type_of(rhs);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        self.arithmetic(*op, &l, &r, *span)
                    }
                    // `&&` and `||` run on bool, the same as any condition does.
                    BinOp::And | BinOp::Or => {
                        self.condition(lhs)?;
                        self.condition(rhs)
                    }
                    _ => Ok(()),
                }
            }
            Expr::Index { base, index, .. } => {
                self.expr(base)?;
                self.expr(index)
            }
            Expr::Range { start, end, .. } => {
                self.expr(start)?;
                self.expr(end)
            }
            Expr::Call { name, args, .. } => {
                for a in args {
                    self.expr(a)?;
                }
                self.check_call(name, args)
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v)?;
                }
                Ok(())
            }
            Expr::EnumLit { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v)?;
                }
                Ok(())
            }
            Expr::Field { base, field, .. } => {
                self.expr(base)?;
                self.check_field(base, field)
            }
            Expr::Match {
                scrutinee, arms, ..
            } => self.check_match(scrutinee, arms, e.span()),
        }
    }

    /// The arithmetic rule for `+ - * / %`, shared by binary expressions and the
    /// scalar case of `+=`/`-=`. Fires only when both sides are known scalars; a
    /// valid string concatenation, an array, or anything the pass cannot pin is
    /// left alone.
    fn arithmetic(
        &self,
        op: BinOp,
        l: &Ty,
        r: &Ty,
        span: crate::diagnostic::Span,
    ) -> Result<(), LuxError> {
        let (Some(ln), Some(rn)) = (scalar_name(l), scalar_name(r)) else {
            return Ok(());
        };
        // The combinations the interpreter accepts.
        let ok = match op {
            BinOp::Add => (l == r) && matches!(l, Ty::Int | Ty::Float | Ty::Str),
            BinOp::Sub | BinOp::Mul | BinOp::Div => {
                matches!((l, r), (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float))
            }
            BinOp::Mod => matches!((l, r), (Ty::Int, Ty::Int)),
            _ => true,
        };
        if ok {
            return Ok(());
        }
        if op == BinOp::Mod {
            return Err(LuxError::new(
                format!("% needs two ints, but got {} and {}", named(ln), named(rn)),
                span,
            ));
        }
        let mixed = matches!((l, r), (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int));
        if mixed {
            return Err(
                LuxError::new("cannot mix int and float — convert one first", span)
                    .with_note("wrap a value in float(...) or int(...)")
                    .with_learn(
                        "numbers",
                        "there's a reason lux makes you say when a whole number becomes a fraction",
                    ),
            );
        }
        let verb = match op {
            BinOp::Add => "add",
            BinOp::Sub => "subtract",
            BinOp::Mul => "multiply",
            BinOp::Div => "divide",
            _ => unreachable!(),
        };
        let (topic, lure) = if matches!(l, Ty::Str) || matches!(r, Ty::Str) {
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
        Err(LuxError::new(
            format!("cannot {} {} and {}", verb, named(ln), named(rn)),
            span,
        )
        .with_learn(topic, lure))
    }

    /// A condition — an `if`/`while` test, or a `&&`/`||` operand — must be bool.
    fn condition(&self, e: &Expr) -> Result<(), LuxError> {
        let ty = self.t.type_of(e);
        if matches!(ty, Ty::Bool) {
            return Ok(());
        }
        if let Some(shown) = self.runtime_name(&ty) {
            return Err(LuxError::new(
                format!("expected a true/false value, but this is {}", named(&shown)),
                e.span(),
            )
            .with_note("conditions and &&/|| operands must be bool")
            .with_learn("booleans", "if, while, and and/or all run on true or false"));
        }
        Ok(())
    }

    /// A call to a user-defined function: every argument whose type is known must
    /// match the parameter it is passed to. Built-ins and unknown names are left
    /// to their own run-time checks; argument count is already checked earlier.
    fn check_call(&self, name: &str, args: &[Expr]) -> Result<(), LuxError> {
        let Some((params, _)) = self.t.env.funcs.get(name) else {
            return Ok(());
        };
        if params.len() != args.len() {
            return Ok(());
        }
        for (param, arg) in params.iter().zip(args) {
            let ty = self.t.type_of(arg);
            if self.satisfies_ty(&param.ty, &ty) == Fit::No
                && let Some(got) = value_name(&ty)
            {
                return Err(LuxError::new(
                    format!(
                        "`{}` expects `{}` to be {}, but got {}",
                        name,
                        param.name,
                        describe_ann(&param.ty),
                        got
                    ),
                    arg.span(),
                )
                .with_learn("functions", "each parameter has a type the call must match"));
            }
        }
        Ok(())
    }

    /// Reading a field with a dot: the base has to be a struct that owns the
    /// field. A known struct missing the field, or a scalar that has no fields at
    /// all, is caught; anything the pass cannot resolve is left to run time.
    fn check_field(&self, base: &Expr, field: &str) -> Result<(), LuxError> {
        let bt = self.t.type_of(base);
        match &bt {
            Ty::User(n) => {
                if let Some(fields) = self.t.env.structs.get(n)
                    && !fields.iter().any(|f| f.name == field)
                {
                    return Err(LuxError::new(
                        format!("struct `{}` has no field `{}`", n, field),
                        base.span(),
                    )
                    .with_learn("structs", "a struct only has the fields you gave it"));
                }
                Ok(())
            }
            _ => {
                if let Some(shown) = value_name(&bt) {
                    return Err(LuxError::new(
                        format!(
                            "cannot read field `{}` of {}; only structs have fields",
                            field, shown
                        ),
                        base.span(),
                    )
                    .with_learn(
                        "structs",
                        "only a struct has named fields to read with a dot",
                    ));
                }
                Ok(())
            }
        }
    }

    /// A `match` on a known enum, with no `_`, must handle every case. Arm bodies
    /// are then walked with each pattern's captured values in scope, so a mistake
    /// inside an arm is checked too. Everything else about a `match` — matching a
    /// plain int/string/bool, an unreachable case name — is left to run time.
    fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: crate::diagnostic::Span,
    ) -> Result<(), LuxError> {
        self.expr(scrutinee)?;
        let sty = self.t.type_of(scrutinee);

        if let Ty::User(n) = &sty
            && let Some(variants) = self.t.env.enums.get(n).cloned()
        {
            let has_wildcard = arms
                .iter()
                .any(|a| matches!(a.pattern, Pattern::Wildcard(_)));
            if !has_wildcard {
                let missing: Vec<String> = variants
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
                        format!("this match on `{}` doesn't handle every case", n),
                        span,
                    )
                    .with_learn("match", "covering every case is what makes match safe")
                    .with_note(format!(
                        "add an arm for: {} (or a `_` catch-all)",
                        missing.join(", ")
                    )));
                }
            }
            // Walk each arm body with its bindings typed from the enum's fields.
            for a in arms {
                self.t.push_scope();
                if let Pattern::Variant { name, bindings, .. } = &a.pattern
                    && let Some(vd) = variants.iter().find(|v| &v.name == name)
                    && vd.fields.len() == bindings.len()
                {
                    for (b, f) in bindings.iter().zip(&vd.fields) {
                        self.t.declare(b.clone(), ty_from_ann(&f.ty));
                    }
                }
                let r = self.expr(&a.body);
                self.t.pop_scope();
                r?;
            }
            return Ok(());
        }

        // Scrutinee type unknown, or not an enum: check the arm bodies plainly.
        for a in arms {
            self.expr(&a.body)?;
        }
        Ok(())
    }

    /// Confirm a value expression matches a written annotation, for a `let`/`var`
    /// that spells its type. The annotation is the thing to blame.
    fn check_annotation(&self, ann: &TypeAnn, value: &Expr) -> Result<(), LuxError> {
        let ty = self.t.type_of(value);
        if self.satisfies_ty(ann, &ty) == Fit::No
            && let Some(got) = value_name(&ty)
        {
            return Err(LuxError::new(
                format!(
                    "type mismatch: annotated `{}` but the value is {}",
                    describe_ann(ann),
                    got
                ),
                ann.span,
            ));
        }
        Ok(())
    }

    /// Does an inferred type satisfy a written annotation? `Unsure` wherever a
    /// part of either side is unknown, so a partly-seen value is never rejected.
    /// Mirrors the interpreter's `type_matches`, which decides the same question
    /// against a runtime value.
    fn satisfies_ty(&self, ann: &TypeAnn, ty: &Ty) -> Fit {
        if matches!(ty, Ty::Unknown) {
            return Fit::Unsure;
        }
        match &ann.kind {
            TypeKind::Named(n) => {
                let want = match n.as_str() {
                    "int" => Ty::Int,
                    "float" => Ty::Float,
                    "string" => Ty::Str,
                    "bool" => Ty::Bool,
                    "Unit" => Ty::Unit,
                    other => Ty::User(other.to_string()),
                };
                if *ty == want { Fit::Yes } else { Fit::No }
            }
            TypeKind::Array(elem) => match ty {
                // An empty array satisfies any array type — its element type is
                // unknown, so there is nothing to disagree with.
                Ty::Array(inner) if matches!(**inner, Ty::Unknown) => Fit::Yes,
                Ty::Array(inner) => self.satisfies_ty(elem, inner),
                _ => Fit::No,
            },
            TypeKind::Generic(name, args) => match (name.as_str(), ty) {
                ("Option", Ty::Option(inner)) => {
                    if matches!(**inner, Ty::Unknown) {
                        Fit::Yes
                    } else {
                        self.satisfies_ty(&args[0], inner)
                    }
                }
                ("Result", Ty::Result(a, b)) => {
                    // A Result value is only ever ok or err, and only the present
                    // side carries a type; the other reads as unknown. So a
                    // definite mismatch on a known side is a No, but an unknown
                    // side never turns into one.
                    let side = |want: &TypeAnn, got: &Ty| {
                        if matches!(got, Ty::Unknown) {
                            Fit::Unsure
                        } else {
                            self.satisfies_ty(want, got)
                        }
                    };
                    match (side(&args[0], a), side(&args[1], b)) {
                        (Fit::No, _) | (_, Fit::No) => Fit::No,
                        (Fit::Yes, Fit::Yes) => Fit::Yes,
                        _ => Fit::Unsure,
                    }
                }
                _ => Fit::Unsure,
            },
        }
    }

    /// A type's name the way `Value::type_name` would render it at run time —
    /// used where the interpreter's message reads `named(v.type_name())`. `None`
    /// wherever the run-time name cannot be pinned from the type alone.
    fn runtime_name(&self, ty: &Ty) -> Option<String> {
        Some(
            match ty {
                Ty::Int => "int",
                Ty::Float => "float",
                Ty::Str => "string",
                Ty::Bool => "bool",
                Ty::Array(_) => "array",
                Ty::Range => "range",
                Ty::Unit => "nothing",
                // A built-in generic is an enum value at run time.
                Ty::Option(_) | Ty::Result(..) => "enum",
                Ty::User(n) => {
                    if self.t.env.structs.contains_key(n) {
                        "struct"
                    } else if self.t.env.enums.contains_key(n) {
                        "enum"
                    } else {
                        return None;
                    }
                }
                Ty::Unknown => return None,
            }
            .to_string(),
        )
    }
}

/// The scalar name of a type, or `None` if it is not a scalar. Matches the
/// interpreter's `type_name` for the four scalar kinds.
fn scalar_name(ty: &Ty) -> Option<&'static str> {
    match ty {
        Ty::Int => Some("int"),
        Ty::Float => Some("float"),
        Ty::Str => Some("string"),
        Ty::Bool => Some("bool"),
        _ => None,
    }
}

/// A type rendered the way the interpreter's `value_type` renders a value's type
/// in a message: scalars by name, an array as `[elem]`, a user type by its name.
/// `None` wherever a part is unknown, so a message is only built from a fully
/// pinned type.
fn value_name(ty: &Ty) -> Option<String> {
    Some(match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::Str => "string".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Array(inner) => format!("[{}]", value_name(inner)?),
        Ty::User(n) => n.clone(),
        Ty::Option(inner) => format!("Option<{}>", value_name(inner)?),
        Ty::Result(a, b) => format!("Result<{}, {}>", value_name(a)?, value_name(b)?),
        Ty::Range => "range".to_string(),
        Ty::Unit => "nothing".to_string(),
        Ty::Unknown => return None,
    })
}

/// Prefix a type name with the right article, as the interpreter's `named` does:
/// `an int`, `a string`.
fn named(type_name: &str) -> String {
    let article = match type_name.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    format!("{} {}", article, type_name)
}

/// Render an annotation the way it was written — `int`, `[int]`, `Option<int>` —
/// matching the interpreter's `describe_type`.
fn describe_ann(ann: &TypeAnn) -> String {
    match &ann.kind {
        TypeKind::Named(n) => n.clone(),
        TypeKind::Array(elem) => format!("[{}]", describe_ann(elem)),
        TypeKind::Generic(name, args) => {
            let inner: Vec<String> = args.iter().map(describe_ann).collect();
            format!("{}<{}>", name, inner.join(", "))
        }
    }
}

/// Are two inferred types confidently the same, confidently different, or too
/// uncertain to say? Used for an assignment, where a place keeps its type.
fn same_ty(a: &Ty, b: &Ty) -> Fit {
    match (a, b) {
        (Ty::Unknown, _) | (_, Ty::Unknown) => Fit::Unsure,
        (Ty::Array(x), Ty::Array(y)) => same_ty(x, y),
        (Ty::Option(x), Ty::Option(y)) => same_ty(x, y),
        (Ty::Result(x1, x2), Ty::Result(y1, y2)) => match (same_ty(x1, y1), same_ty(x2, y2)) {
            (Fit::No, _) | (_, Fit::No) => Fit::No,
            (Fit::Yes, Fit::Yes) => Fit::Yes,
            _ => Fit::Unsure,
        },
        _ if a == b => Fit::Yes,
        _ => Fit::No,
    }
}

/// A readable name for an assignment target, for the mismatch message —
/// `count`, `w.doorOpen`, `items[…]`. Mirrors the interpreter's `describe_place`.
fn describe_place(e: &Expr) -> String {
    match e {
        Expr::Ident(n, _) => n.clone(),
        Expr::Field { base, field, .. } => format!("{}.{}", describe_place(base), field),
        Expr::Index { base, .. } => format!("{}[…]", describe_place(base)),
        _ => "this".to_string(),
    }
}

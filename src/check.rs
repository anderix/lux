//! Checks a program has to pass before it runs or is translated, regardless of
//! command. These are the rules that hold at the level of a declaration — true
//! whatever the program then does — so they belong here rather than in the
//! interpreter (which would catch them only on the `lux run` path) or a backend
//! (only on `lux convert`/`lux build`). `load` runs them once, up front.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, Param, Stmt};
use crate::diagnostic::LuxError;
use crate::interpreter::{BUILTINS, count, describe_place, nearest_name};

/// Run every whole-program check, returning the first failure. Kept to rules that
/// are true of a declaration on its own — no evaluation, no type inference — so a
/// pass here means nothing about whether the program is otherwise correct.
pub fn check(program: &[Stmt]) -> Result<(), LuxError> {
    reserved_names(program)
}

/// The checks `lux run` makes that `lux convert` and `lux build` must make too,
/// before emitting anything — so a broken program meets a lux error in its own
/// words rather than rustc's, about a file the learner never wrote and in a
/// language they haven't started (#29). `lux build` is the graduation step, and its
/// pitch is that the good diagnostics come with you; without this, the errors
/// switch off at exactly that moment, and hardest for the lux-specific rules the
/// target compilers describe worst.
///
/// Only the checks decidable from the program's structure, with no type inference:
/// a call to a function that isn't there or is passed the wrong number of values,
/// and a write through a parameter — the rule the issue leads with, and one no
/// target language phrases in lux's terms. The type-directed rules (mixing `int`
/// and `float`, storing a `Result`, a return type that doesn't match) are left to
/// the target compiler for now: a static answer to them would lean on inference
/// that assumes a well-formed program, and refusing a valid one is worse than the
/// wall of rustc it was meant to prevent.
pub fn check_before_emit(program: &[Stmt]) -> Result<(), LuxError> {
    let funcs: HashMap<&str, usize> = program
        .iter()
        .filter_map(|s| match s {
            Stmt::Func { name, params, .. } => Some((name.as_str(), params.len())),
            _ => None,
        })
        .collect();
    // A struct or enum name used call-style (`Point(...)`) isn't an unknown
    // function — it's construction the emitter handles — so it's known here too.
    let types: HashSet<&str> = program
        .iter()
        .filter_map(|s| match s {
            Stmt::Struct { name, .. } | Stmt::Enum { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    check_calls(program, &funcs, &types)?;
    check_param_writes(program)?;
    Ok(())
}

/// A top-level function can't be named `main`. lux runs a program from its first
/// line, so `main` earns a learner nothing — and every backend generates its own
/// `main` as the entry point, so a user one collides with it and won't build on
/// Rust or Go, after running fine interpreted (#37). A learner arriving from C,
/// Java, Go, or Rust reaches for `main` first of all, so this is the collision
/// most worth catching, and catching it early with a reason beats a linker error
/// three steps later. Only the top level collides: a `func main` nested inside
/// another function is a local, and the emitters keep it local.
fn reserved_names(program: &[Stmt]) -> Result<(), LuxError> {
    for stmt in program {
        if let Stmt::Func { name, span, .. } = stmt
            && name == "main"
        {
            return Err(LuxError::new("lux has no `main` — it runs your program from the top", *span)
                .with_note(
                    "name this function for what it does and call it yourself, the way you call any other",
                )
                .with_learn(
                    "functions",
                    "lux starts at the first line of the file; there's no entry point to declare",
                ));
        }
    }
    Ok(())
}

// ----- calls: unknown function, wrong argument count -----------------------

/// Walk every call in the program, reporting the first that names a function that
/// isn't there or passes the wrong number of values. The messages are the
/// interpreter's own, so the same mistake reads the same whether it's run or built.
fn check_calls(
    stmts: &[Stmt],
    funcs: &HashMap<&str, usize>,
    types: &HashSet<&str>,
) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } => check_call_expr(value, funcs, types)?,
            Stmt::Var { value, .. } => {
                if let Some(v) = value {
                    check_call_expr(v, funcs, types)?;
                }
            }
            Stmt::Assign { target, value, .. } => {
                check_call_expr(target, funcs, types)?;
                check_call_expr(value, funcs, types)?;
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    check_call_expr(v, funcs, types)?;
                }
            }
            Stmt::Expr(e) => check_call_expr(e, funcs, types)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                check_call_expr(cond, funcs, types)?;
                check_calls(then_body, funcs, types)?;
                if let Some(e) = else_body {
                    check_calls(e, funcs, types)?;
                }
            }
            Stmt::While { cond, body, .. } => {
                check_call_expr(cond, funcs, types)?;
                check_calls(body, funcs, types)?;
            }
            Stmt::For { iter, body, .. } => {
                check_call_expr(iter, funcs, types)?;
                check_calls(body, funcs, types)?;
            }
            Stmt::Func { body, .. } => check_calls(body, funcs, types)?,
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }
    Ok(())
}

fn check_call_expr(
    e: &Expr,
    funcs: &HashMap<&str, usize>,
    types: &HashSet<&str>,
) -> Result<(), LuxError> {
    match e {
        Expr::Call { name, args, span } => {
            check_one_call(name, args.len(), *span, funcs, types)?;
            for a in args {
                check_call_expr(a, funcs, types)?;
            }
        }
        Expr::Array(items, _) => {
            for x in items {
                check_call_expr(x, funcs, types)?;
            }
        }
        Expr::Unary { rhs, .. } => check_call_expr(rhs, funcs, types)?,
        Expr::Binary { lhs, rhs, .. } => {
            check_call_expr(lhs, funcs, types)?;
            check_call_expr(rhs, funcs, types)?;
        }
        Expr::Index { base, index, .. } => {
            check_call_expr(base, funcs, types)?;
            check_call_expr(index, funcs, types)?;
        }
        Expr::Range { start, end, .. } => {
            check_call_expr(start, funcs, types)?;
            check_call_expr(end, funcs, types)?;
        }
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            for (_, v) in fields {
                check_call_expr(v, funcs, types)?;
            }
        }
        Expr::Field { base, .. } => check_call_expr(base, funcs, types)?,
        Expr::Match {
            scrutinee, arms, ..
        } => {
            check_call_expr(scrutinee, funcs, types)?;
            for arm in arms {
                check_call_expr(&arm.body, funcs, types)?;
            }
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Ident(..) => {}
    }
    Ok(())
}

/// The name-level checks for a single call. Built-ins and the `some`/`ok`/`err`
/// constructors are known and carry their own arity rules (checked at runtime with
/// their own messages), so they pass here; a struct or enum name used call-style is
/// construction, not a function. What's left is a user function — checked for its
/// argument count — or a name that names nothing, reported with the same
/// did-you-mean the interpreter gives.
fn check_one_call(
    name: &str,
    argc: usize,
    span: crate::diagnostic::Span,
    funcs: &HashMap<&str, usize>,
    types: &HashSet<&str>,
) -> Result<(), LuxError> {
    if BUILTINS.contains(&name) || matches!(name, "some" | "ok" | "err") {
        return Ok(());
    }
    if let Some(&arity) = funcs.get(name) {
        if argc != arity {
            return Err(LuxError::new(
                format!(
                    "function `{}` expects {} but got {}",
                    name,
                    count(arity, "value"),
                    argc
                ),
                span,
            )
            .with_learn(
                "functions",
                "a function takes exactly the parameters it declares",
            ));
        }
        return Ok(());
    }
    if types.contains(name) {
        return Ok(());
    }
    let err = LuxError::new(format!("unknown function `{}`", name), span);
    let candidates = BUILTINS.iter().copied().chain(funcs.keys().copied());
    let err = match nearest_name(name, candidates) {
        Some(near) => err.with_note(format!("did you mean `{}`?", near)),
        None => err.with_note(format!(
            "define it with `func`, or use a built-in: {}",
            BUILTINS.join(", ")
        )),
    };
    Err(err.with_learn(
        "functions",
        "a function takes values in and hands one result back",
    ))
}

// ----- writing through a parameter -----------------------------------------

/// Report the first write through a parameter, in any function. A parameter is a
/// value handed in to read, not a place to store into — a rule lux checks and the
/// target languages have no clean way to phrase, so it's exactly the kind of error
/// worth catching here (#29). Each function is checked against its own parameters.
fn check_param_writes(stmts: &[Stmt]) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Func { params, body, .. } => {
                check_fn_param_writes(params, body)?;
                // Nested functions have their own parameters, checked in turn.
                check_param_writes(body)?;
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                check_param_writes(then_body)?;
                if let Some(e) = else_body {
                    check_param_writes(e)?;
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => check_param_writes(body)?,
            _ => {}
        }
    }
    Ok(())
}

/// Check one function's body for a write to one of its parameters. A parameter that
/// the body rebinds with a `var` of the same name can be assigned legally through
/// that local, so it's excluded — conservatively, if the name is rebound anywhere,
/// since a `let`, a loop variable, or a match capture is itself immutable and a
/// write to it is already an error the interpreter reports its own way. That keeps
/// this from ever refusing a valid program; at worst it leaves a rarer case to the
/// interpreter or the target compiler, never flags one that isn't there.
fn check_fn_param_writes(params: &[Param], body: &[Stmt]) -> Result<(), LuxError> {
    let mut rebound: HashSet<&str> = HashSet::new();
    collect_var_names(body, &mut rebound);
    let protected: HashSet<&str> = params
        .iter()
        .map(|p| p.name.as_str())
        .filter(|n| !rebound.contains(n))
        .collect();
    if protected.is_empty() {
        return Ok(());
    }
    find_param_write(body, &protected)
}

/// Collect every `var` binding name in a subtree — the only bindings that make a
/// same-named write legal. Descends through blocks and nested functions; over-
/// counting is safe, since a name in this set is only ever removed from checking.
fn collect_var_names<'a>(stmts: &'a [Stmt], out: &mut HashSet<&'a str>) {
    for stmt in stmts {
        match stmt {
            Stmt::Var { name, .. } => {
                out.insert(name.as_str());
            }
            Stmt::Func { body, .. } | Stmt::While { body, .. } | Stmt::For { body, .. } => {
                collect_var_names(body, out)
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_var_names(then_body, out);
                if let Some(e) = else_body {
                    collect_var_names(e, out);
                }
            }
            _ => {}
        }
    }
}

/// Walk a function body for an assignment whose place is rooted at a protected
/// parameter, stopping at nested-function boundaries — a write inside a nested
/// function is against that function's own scope, not this one's parameters.
fn find_param_write(stmts: &[Stmt], protected: &HashSet<&str>) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { target, span, .. } => {
                if let Some(root) = target.place_root()
                    && protected.contains(root)
                {
                    let place = describe_place(target);
                    return Err(LuxError::new(
                        format!(
                            "cannot change `{place}` — `{root}` is a parameter, and a parameter never changes"
                        ),
                        *span,
                    )
                    .with_note(format!(
                        "a parameter can't be a var; copy it into a local var first — `var copy = {root}` — and change that"
                    ))
                    .with_learn(
                        "variables",
                        "a parameter is a value handed in to read, not a place to store into",
                    ));
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                find_param_write(then_body, protected)?;
                if let Some(e) = else_body {
                    find_param_write(e, protected)?;
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => find_param_write(body, protected)?,
            // A nested function's writes are its own scope's business.
            Stmt::Func { .. } => {}
            _ => {}
        }
    }
    Ok(())
}

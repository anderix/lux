//! Checks a program has to pass before it runs or is translated, regardless of
//! command. These are the rules that hold at the level of a declaration — true
//! whatever the program then does — so they belong here rather than in the
//! interpreter (which would catch them only on the `lux run` path) or a backend
//! (only on `lux convert`/`lux build`). `load` runs them once, up front.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, Param, Pattern, Stmt, TypeAnn, TypeKind};
use crate::convert::{Ty, Types};
use crate::diagnostic::{LuxError, Span};
use crate::interpreter::{
    BUILTINS, count, describe_place, nearest_name, result_not_parameter, result_not_printed,
    result_not_stored,
};

/// Run every whole-program check, returning the first failure. Kept to rules that
/// are true of a declaration on its own — no evaluation, no type inference — so a
/// pass here means nothing about whether the program is otherwise correct.
pub fn check(program: &[Stmt]) -> Result<(), LuxError> {
    entry_point(program)?;
    // A name you declare has to be new where you make it: not a built-in, not already
    // the name of a function or type, and not still in scope from an enclosing block.
    // One rule instead of the several de-facto behaviours that shadowing used to leave
    // lying on a learner's path.
    check_names(program)?;
    // A `Result` parameter is the store-a-Result rule seen at a binding, and it's
    // syntactic — the annotation says `Result<…>` — so it's refused on every path,
    // making `lux run` agree with the targets rather than accepting what Go can't
    // emit (#42).
    reject_result_parameter(program)?;
    // An empty array literal bound without a type annotation: the same declaration
    // rule an empty `none` already meets — name what it holds — settled here so all
    // four legs agree it's illegal until annotated (#66).
    reject_untyped_empty_array(program)
}

// ----- reserved names and shadowing ----------------------------------------

/// If `name` is a built-in — a function, a value form, or a type — say which kind,
/// so the error can name what the learner just bumped into (and, the hope goes,
/// send them to look it up). Built-in names are reserved the way keywords are: a
/// program can't rebind one and quietly change what a later `length(...)` or `none`
/// means.
fn reserved_kind(name: &str) -> Option<&'static str> {
    // `int`/`float`/`string` are also conversion built-ins, but a learner naming
    // one means the type, so name that role first.
    if matches!(
        name,
        "int" | "float" | "string" | "bool" | "Option" | "Result" | "Unit"
    ) {
        Some("a built-in type")
    } else if BUILTINS.contains(&name) {
        Some("a built-in function")
    } else if matches!(name, "some" | "none" | "ok" | "err") {
        Some("a built-in value")
    } else {
        None
    }
}

/// The whole-program naming rule: every declared name must be new where it is
/// introduced — not a built-in, and not still in scope from an enclosing block. A
/// function or type name is checked against the built-ins here; the body is then
/// walked with a stack of block scopes so each binding is checked against the
/// variables still visible around it.
fn check_names(program: &[Stmt]) -> Result<(), LuxError> {
    let mut globals: HashSet<&str> = HashSet::new();
    for s in program {
        let (name, span) = match s {
            Stmt::Func { name, span, .. }
            | Stmt::Struct { name, span, .. }
            | Stmt::Enum { name, span, .. } => (name.as_str(), *span),
            _ => continue,
        };
        if let Some(kind) = reserved_kind(name) {
            return Err(reserved_error(name, kind, span));
        }
        globals.insert(name);
    }
    let mut scopes: Vec<HashSet<String>> = vec![HashSet::new()];
    walk_stmts_names(program, &globals, &mut scopes)
}

fn reserved_error(name: &str, kind: &str, span: Span) -> LuxError {
    LuxError::new(
        format!("`{}` is {}, so it can't be a name you declare", name, kind),
        span,
    )
    .with_note("built-in names are reserved the way keywords are — choose another name")
}

/// Introduce a name at a declaration, refusing it if it collides with a built-in, a
/// global function or type, or a name still in scope. A variable can't take a
/// function's name even from a different scope: the targets resolve the two as one
/// name, so a program that shadows a function and then calls it runs interpreted but
/// won't build. One name, one meaning. On success the name is added to the innermost
/// block so later siblings see it.
fn declare_name(
    name: &str,
    span: Span,
    globals: &HashSet<&str>,
    scopes: &mut [HashSet<String>],
) -> Result<(), LuxError> {
    // `_` is the discard binding — "ignore this", not a name. Any number of them
    // coexist, so a nested `for _ in ...` never clashes with an outer one.
    if name == "_" {
        return Ok(());
    }
    if let Some(kind) = reserved_kind(name) {
        return Err(reserved_error(name, kind, span));
    }
    if globals.contains(name) {
        return Err(LuxError::new(
            format!("`{}` is already the name of a function or type", name),
            span,
        )
        .with_note("a name is a value or something you call, not both — choose another")
        .with_learn("scope", "a name means one thing wherever it can be seen"));
    }
    if scopes.last().is_some_and(|c| c.contains(name)) {
        return Err(LuxError::new(
            format!("`{}` is already declared in this scope", name),
            span,
        )
        .with_learn("scope", "a name lives only inside the { } where it's made"));
    }
    if scopes.iter().any(|s| s.contains(name)) {
        return Err(LuxError::new(
            format!("`{}` is already in scope from an enclosing block", name),
            span,
        )
        .with_note(
            "the outer name is still visible here, so a new one would shadow it — choose another",
        )
        .with_learn("scope", "a name means one thing wherever it can be seen"));
    }
    if let Some(current) = scopes.last_mut() {
        current.insert(name.to_string());
    }
    Ok(())
}

fn walk_stmts_names(
    stmts: &[Stmt],
    globals: &HashSet<&str>,
    scopes: &mut Vec<HashSet<String>>,
) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                name, value, span, ..
            } => {
                walk_expr_names(value, globals, scopes)?;
                declare_name(name, *span, globals, scopes)?;
            }
            Stmt::Var {
                name, value, span, ..
            } => {
                if let Some(v) = value {
                    walk_expr_names(v, globals, scopes)?;
                }
                declare_name(name, *span, globals, scopes)?;
            }
            Stmt::Func { params, body, .. } => {
                // A function is its own world: it can't see top-level variables, so
                // its parameters and locals start from an empty scope. Built-ins and
                // global names still apply.
                let mut fn_scopes: Vec<HashSet<String>> = vec![HashSet::new()];
                for p in params {
                    declare_name(&p.name, p.span, globals, &mut fn_scopes)?;
                }
                walk_stmts_names(body, globals, &mut fn_scopes)?;
            }
            Stmt::Assign { target, value, .. } => {
                walk_expr_names(target, globals, scopes)?;
                walk_expr_names(value, globals, scopes)?;
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    walk_expr_names(v, globals, scopes)?;
                }
            }
            Stmt::Expr(e) => walk_expr_names(e, globals, scopes)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                walk_expr_names(cond, globals, scopes)?;
                scopes.push(HashSet::new());
                walk_stmts_names(then_body, globals, scopes)?;
                scopes.pop();
                if let Some(e) = else_body {
                    scopes.push(HashSet::new());
                    walk_stmts_names(e, globals, scopes)?;
                    scopes.pop();
                }
            }
            Stmt::While { cond, body, .. } => {
                walk_expr_names(cond, globals, scopes)?;
                scopes.push(HashSet::new());
                walk_stmts_names(body, globals, scopes)?;
                scopes.pop();
            }
            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                walk_expr_names(iter, globals, scopes)?;
                scopes.push(HashSet::new());
                declare_name(var, *span, globals, scopes)?;
                walk_stmts_names(body, globals, scopes)?;
                scopes.pop();
            }
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }
    Ok(())
}

/// Expressions introduce names only through a `match` arm's captures, but they have
/// to be walked so a `match` nested anywhere — a call argument, a branch of a
/// binary — has its captures checked in the right scope.
fn walk_expr_names(
    e: &Expr,
    globals: &HashSet<&str>,
    scopes: &mut Vec<HashSet<String>>,
) -> Result<(), LuxError> {
    match e {
        Expr::Match {
            scrutinee, arms, ..
        } => {
            walk_expr_names(scrutinee, globals, scopes)?;
            for arm in arms {
                scopes.push(HashSet::new());
                if let Pattern::Variant { bindings, span, .. } = &arm.pattern {
                    for b in bindings {
                        declare_name(b, *span, globals, scopes)?;
                    }
                }
                walk_expr_names(&arm.body, globals, scopes)?;
                scopes.pop();
            }
        }
        Expr::Array(items, _) => {
            for x in items {
                walk_expr_names(x, globals, scopes)?;
            }
        }
        Expr::Unary { rhs, .. } => walk_expr_names(rhs, globals, scopes)?,
        Expr::Binary { lhs, rhs, .. } => {
            walk_expr_names(lhs, globals, scopes)?;
            walk_expr_names(rhs, globals, scopes)?;
        }
        Expr::Index { base, index, .. } => {
            walk_expr_names(base, globals, scopes)?;
            walk_expr_names(index, globals, scopes)?;
        }
        Expr::Range { start, end, .. } => {
            walk_expr_names(start, globals, scopes)?;
            walk_expr_names(end, globals, scopes)?;
        }
        Expr::Call { args, .. } => {
            for a in args {
                walk_expr_names(a, globals, scopes)?;
            }
        }
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            for (_, v) in fields {
                walk_expr_names(v, globals, scopes)?;
            }
        }
        Expr::Field { base, .. } => walk_expr_names(base, globals, scopes)?,
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Ident(..) => {}
    }
    Ok(())
}

/// The checks `lux run` makes that `lux convert` and `lux build` must make too,
/// before emitting anything — so a broken program meets a lux error in its own
/// words rather than rustc's, about a file the learner never wrote and in a
/// language they haven't started (#29). `lux build` is the graduation step, and its
/// pitch is that the good diagnostics come with you; without this, the errors
/// switch off at exactly that moment, and hardest for the lux-specific rules the
/// target compilers describe worst.
///
/// Mostly the checks decidable from the program's structure, with no type
/// inference: a call to a function that isn't there or is passed the wrong number
/// of values, and a write through a parameter — the rule the issue leads with, and
/// one no target language phrases in lux's terms. The one type-directed rule that
/// belongs here is a stored `Result` (#39) — see `reject_result_flow` for why it's
/// safe where the others aren't. The remaining type-directed rules (mixing `int`
/// and `float`, a return type that doesn't match) are left to the target compiler
/// for now: a static answer to them would lean on inference that assumes a
/// well-formed program, and refusing a valid one is worse than the wall of rustc it
/// was meant to prevent.
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
    // The store-a-Result rule is the one type-directed check that moves across to
    // convert/build (#39): unlike mixing int and float or a wrong return type, it
    // isn't caught by the target compiler — the program builds and prints `Ok(…)`
    // on Rust and `success(…)` on Swift, the one value lux says can never be
    // printed. It also can't false-positive: a `Result` is never storable, so
    // there's no valid program to turn away. It's the rule that keeps one source
    // crossing three targets, so it's enforced where the value is produced.
    reject_result_flow(program, &Types::new(program))?;
    Ok(())
}

/// The rules that make a top-level `func main` the program's entry point. lux
/// doesn't need one — with no `main`, the file is the program and runs top to
/// bottom, the starting gift a beginner never has to earn. But `main` is the shape
/// the C-family languages require, so lux accepts it too, as the last lesson before
/// leaving: define it and lux runs it for you, exactly as Rust, Go, Java, and C do
/// (Swift, like lux, lets the file be the program — it has no entry-point `main`).
/// That "runs it for you" is one idea seen from three sides, and each side is
/// a rule here, each phrased to teach it. `main` takes no values and returns nothing
/// (it is where a program starts, not a function whose result is used); nothing else
/// runs beside it at the top level (once you name the start, there is nowhere for
/// loose code to go); and you don't call it yourself (the language does — that is
/// what an entry point is). All three hold only for a *top-level* `main`: a `func
/// main` nested inside another function is an ordinary local, and the emitters keep
/// it local. Enforced on every path, since auto-run is a `lux run` behavior as much
/// as a build one — and so the rules, like the good errors, come with you to the
/// target compiler instead of switching off at graduation.
fn entry_point(program: &[Stmt]) -> Result<(), LuxError> {
    let main = program.iter().find_map(|s| match s {
        Stmt::Func {
            name,
            params,
            ret,
            span,
            ..
        } if name == "main" => Some((params.as_slice(), ret.is_some(), *span)),
        _ => None,
    });
    let Some((params, has_ret, main_span)) = main else {
        return Ok(());
    };
    // A value handed to `main` would have nowhere to come from — the language calls
    // it, not another line of the program.
    if let Some(first) = params.first() {
        return Err(LuxError::new(
            "`main` takes no values — it is only where your program starts",
            first.span,
        )
        .with_note(
            "read what your program needs inside main, with `args()` and `readLine`, the way any function reads its input",
        )
        .with_learn(
            "main",
            "main is the place lux begins, not a function you hand values to",
        ));
    }
    // Nothing waits on `main`'s result: the top level runs it, and the top level has
    // no caller of its own.
    if has_ret {
        return Err(LuxError::new(
            "`main` returns nothing — it is only where your program starts",
            main_span,
        )
        .with_note(
            "drop the `-> ...`; main runs top to bottom and there is no caller waiting for a value",
        )
        .with_learn(
            "main",
            "main is the place lux begins, not a function whose result gets used",
        ));
    }
    // Naming the start leaves the top level for definitions only — loose code has
    // nowhere left to run.
    for stmt in program {
        if !matches!(
            stmt,
            Stmt::Func { .. } | Stmt::Struct { .. } | Stmt::Enum { .. }
        ) {
            return Err(LuxError::new(
                "nothing runs beside `main` at the top level — it is where your program starts",
                stmt_span(stmt),
            )
            .with_note("move this line into main; once you name the start, the top level holds only your definitions")
            .with_learn(
                "main",
                "main gathers your program's work into the one place it now begins",
            ));
        }
    }
    // The language runs `main`; a program that calls it as well would run it twice —
    // and no target language lets you call `main` at all.
    reject_main_call(program)
}

/// Report the first place the program calls `main` itself. The interpreter runs
/// `main` for you, so a hand-written call would re-enter it; Rust, Go, and Swift
/// forbid calling `main` outright, so catching it here keeps the same program from
/// running interpreted only to fail at the target compiler.
fn reject_main_call(stmts: &[Stmt]) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Expr(value) => find_main_call(value)?,
            Stmt::Var { value, .. } | Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    find_main_call(v)?;
                }
            }
            Stmt::Assign { target, value, .. } => {
                find_main_call(target)?;
                find_main_call(value)?;
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                find_main_call(cond)?;
                reject_main_call(then_body)?;
                if let Some(e) = else_body {
                    reject_main_call(e)?;
                }
            }
            Stmt::While { cond, body, .. } => {
                find_main_call(cond)?;
                reject_main_call(body)?;
            }
            Stmt::For { iter, body, .. } => {
                find_main_call(iter)?;
                reject_main_call(body)?;
            }
            Stmt::Func { body, .. } => reject_main_call(body)?,
            Stmt::Struct { .. } | Stmt::Enum { .. } => {}
        }
    }
    Ok(())
}

fn find_main_call(e: &Expr) -> Result<(), LuxError> {
    match e {
        Expr::Call { name, args, span } => {
            if name == "main" {
                return Err(LuxError::new(
                    "you don't call `main` yourself — lux runs it for you",
                    *span,
                )
                .with_note(
                    "delete this call; defining `main` is enough, and your program starts there",
                )
                .with_learn(
                    "main",
                    "main is the entry point — the one function the language calls, not you",
                ));
            }
            for a in args {
                find_main_call(a)?;
            }
        }
        Expr::Array(items, _) => {
            for x in items {
                find_main_call(x)?;
            }
        }
        Expr::Unary { rhs, .. } => find_main_call(rhs)?,
        Expr::Binary { lhs, rhs, .. } => {
            find_main_call(lhs)?;
            find_main_call(rhs)?;
        }
        Expr::Index { base, index, .. } => {
            find_main_call(base)?;
            find_main_call(index)?;
        }
        Expr::Range { start, end, .. } => {
            find_main_call(start)?;
            find_main_call(end)?;
        }
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            for (_, v) in fields {
                find_main_call(v)?;
            }
        }
        Expr::Field { base, .. } => find_main_call(base)?,
        Expr::Match {
            scrutinee, arms, ..
        } => {
            find_main_call(scrutinee)?;
            for arm in arms {
                find_main_call(&arm.body)?;
            }
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Ident(..) => {}
    }
    Ok(())
}

/// An empty array literal bound without a type annotation leaves its element type
/// open, and the four legs each settle it differently — the interpreter waits to see
/// what's appended, Swift and Rust refuse it, Go infers `[]any` and fails wherever
/// the variable later meets a typed position (#66). lux closes the divergence the
/// way it already closes an empty `none`: name what it holds. This is a declaration
/// rule — the binding is under-determined on its own, whatever a later line appends —
/// so it belongs here and holds on every path, and all four legs agree the program is
/// illegal until it carries the annotation the arrays card already teaches.
fn reject_untyped_empty_array(stmts: &[Stmt]) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                ty: None, value, ..
            } => reject_empty_array_value(value)?,
            Stmt::Var {
                ty: None,
                value: Some(value),
                ..
            } => reject_empty_array_value(value)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                reject_untyped_empty_array(then_body)?;
                if let Some(e) = else_body {
                    reject_untyped_empty_array(e)?;
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Func { body, .. } => {
                reject_untyped_empty_array(body)?
            }
            _ => {}
        }
    }
    Ok(())
}

/// Refuse a binding whose value is the bare `[]` literal, pointing at it with the
/// same words an empty `none` meets, so the two under-determined literals read as
/// one rule rather than two accidents.
fn reject_empty_array_value(value: &Expr) -> Result<(), LuxError> {
    if let Expr::Array(items, span) = value
        && items.is_empty()
    {
        return Err(LuxError::new(
            "can't tell what type this is — an empty array leaves it open",
            *span,
        )
        .with_note("name the type, like `let xs: [int] = []`")
        .with_learn(
            "arrays",
            "lux usually guesses the type, but an empty array needs you to say",
        ));
    }
    Ok(())
}

/// The source span of any statement, for pointing an error at the line it names.
fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Let { span, .. }
        | Stmt::Var { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::Func { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::Struct { span, .. }
        | Stmt::Enum { span, .. }
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. } => *span,
        Stmt::Expr(e) => e.span(),
    }
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

// ----- the Result rule: not a parameter, not stored, not printed -------------

/// Report the first `Result`-typed parameter, in any function. A `Result` is
/// handled where it's produced, so it can't be handed in as a value — the same
/// rule the `let` case enforces, checked here from the annotation alone (#42).
fn reject_result_parameter(stmts: &[Stmt]) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Func { params, body, .. } => {
                for p in params {
                    if is_result_ann(&p.ty) {
                        return Err(result_not_parameter(p.span));
                    }
                }
                reject_result_parameter(body)?;
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                reject_result_parameter(then_body)?;
                if let Some(e) = else_body {
                    reject_result_parameter(e)?;
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => reject_result_parameter(body)?,
            _ => {}
        }
    }
    Ok(())
}

fn is_result_ann(a: &TypeAnn) -> bool {
    matches!(&a.kind, TypeKind::Generic(name, _) if name == "Result")
}

/// Refuse a `Result` stored in a binding or handed to `print`/`eprint`, the same
/// two places the interpreter refuses it at runtime — so `lux convert` and `lux
/// build` reject it before emitting rather than compiling a program `lux run`
/// won't accept (#39). Returning a `Result` is fine — that's handing it to the
/// caller — so a `return` value is not a stored one.
fn reject_result_flow(stmts: &[Stmt], types: &Types) -> Result<(), LuxError> {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } => stored_check(value, types)?,
            Stmt::Var { value: Some(v), .. } => stored_check(v, types)?,
            Stmt::Assign { target, value, .. } => {
                stored_check(value, types)?;
                print_check(target, types)?;
            }
            Stmt::Return { value: Some(v), .. } => print_check(v, types)?,
            Stmt::Expr(e) => print_check(e, types)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                print_check(cond, types)?;
                reject_result_flow(then_body, types)?;
                if let Some(e) = else_body {
                    reject_result_flow(e, types)?;
                }
            }
            Stmt::While { cond, body, .. } => {
                print_check(cond, types)?;
                reject_result_flow(body, types)?;
            }
            Stmt::For { iter, body, .. } => {
                print_check(iter, types)?;
                reject_result_flow(body, types)?;
            }
            Stmt::Func { body, .. } => reject_result_flow(body, types)?,
            _ => {}
        }
    }
    Ok(())
}

/// A value flowing into a binding: refuse it if its type is a `Result`. The
/// binding position also carries any `print` nested in the value.
fn stored_check(value: &Expr, types: &Types) -> Result<(), LuxError> {
    if matches!(types.type_of(value), Ty::Result(..)) {
        return Err(result_not_stored(value.span()));
    }
    print_check(value, types)
}

/// Walk an expression for a `print`/`eprint` whose argument is a `Result`.
fn print_check(e: &Expr, types: &Types) -> Result<(), LuxError> {
    if let Expr::Call { name, args, .. } = e {
        if matches!(name.as_str(), "print" | "eprint") {
            for a in args {
                if matches!(types.type_of(a), Ty::Result(..)) {
                    return Err(result_not_printed(a.span()));
                }
            }
        }
        for a in args {
            print_check(a, types)?;
        }
        return Ok(());
    }
    match e {
        Expr::Array(items, _) => {
            for x in items {
                print_check(x, types)?;
            }
        }
        Expr::Unary { rhs, .. } => print_check(rhs, types)?,
        Expr::Binary { lhs, rhs, .. } => {
            print_check(lhs, types)?;
            print_check(rhs, types)?;
        }
        Expr::Index { base, index, .. } => {
            print_check(base, types)?;
            print_check(index, types)?;
        }
        Expr::Range { start, end, .. } => {
            print_check(start, types)?;
            print_check(end, types)?;
        }
        Expr::StructLit { fields, .. } | Expr::EnumLit { fields, .. } => {
            for (_, v) in fields {
                print_check(v, types)?;
            }
        }
        Expr::Field { base, .. } => print_check(base, types)?,
        Expr::Match {
            scrutinee, arms, ..
        } => {
            print_check(scrutinee, types)?;
            for arm in arms {
                print_check(&arm.body, types)?;
            }
        }
        _ => {}
    }
    Ok(())
}

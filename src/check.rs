//! Checks a program has to pass before it runs or is translated, regardless of
//! command. These are the rules that hold at the level of a declaration — true
//! whatever the program then does — so they belong here rather than in the
//! interpreter (which would catch them only on the `lux run` path) or a backend
//! (only on `lux convert`/`lux build`). `load` runs them once, up front.

use crate::ast::Stmt;
use crate::diagnostic::LuxError;

/// Run every whole-program check, returning the first failure. Kept to rules that
/// are true of a declaration on its own — no evaluation, no type inference — so a
/// pass here means nothing about whether the program is otherwise correct.
pub fn check(program: &[Stmt]) -> Result<(), LuxError> {
    reserved_names(program)
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

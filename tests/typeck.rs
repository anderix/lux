//! The static type check. lux's interpreter only checks a line it actually runs,
//! so a type error inside `if false { ... }` used to run clean under `lux run`
//! and then fail to compile under `lux build` — the same source legal on one leg
//! and not another. The pass in `convert::typeck` closes that gap: it applies the
//! interpreter's concrete-type rules to every path, before any command, in the
//! interpreter's own words.
//!
//! These tests hold it to two promises. Each rule, placed in a dead branch a
//! learner's program would never reach, is now caught — and caught with the exact
//! message, note, and `lux learn` trail the interpreter gives on a live path
//! (`caught_reads_like_the_runtime`). And the same well-formed constructs, live
//! and dead, still run (`valid_constructs_in_a_dead_branch_still_run`): the pass
//! never turns away a program the interpreter would have accepted. The parity test
//! ties the legs together — one bad program, rejected the same way whether run,
//! converted, or built.

use std::process::{Command, Stdio};

/// Run a program expected to be rejected, and hand back its stderr.
fn err_of(tag: &str, src: &str) -> String {
    let path = std::env::temp_dir().join(format!("lux-tc-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!out.status.success(), "`{src}` should be rejected, but ran");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Run a program expected to be accepted — the guard that the pass doesn't turn
/// away a valid program along with the invalid ones.
fn runs_ok(tag: &str, src: &str) {
    let path = std::env::temp_dir().join(format!("lux-tc-ok-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "`{src}` should run, but was rejected:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Assert a rejection carries every one of the given fragments — the message, its
/// note, and its trail — so a static catch reads exactly like the runtime one.
fn reads_like(tag: &str, src: &str, fragments: &[&str]) {
    let err = err_of(tag, src);
    for f in fragments {
        assert!(err.contains(f), "expected `{f}` in:\n{err}");
    }
}

/// The `error:` line a leg prints for a program, or empty if it was accepted.
fn error_line(leg: &[&str], path: &std::path::Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .args(leg)
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find(|l| l.starts_with("error:"))
        .unwrap_or("")
        .to_string()
}

// ----- each rule, in a dead branch, reads like the runtime error --------------

#[test]
fn adding_a_string_and_a_number_is_caught() {
    reads_like(
        "add",
        "if false {\n  let x = \"a\" + 1\n  print(x)\n}\nprint(\"ok\")\n",
        &[
            "cannot add a string and an int",
            "lux learn strings",
            "lux never turns a number into text for you",
        ],
    );
}

#[test]
fn mixing_int_and_float_is_caught() {
    reads_like(
        "mix",
        "if false { print(7 / 2.0) }\nprint(\"ok\")\n",
        &[
            "cannot mix int and float — convert one first",
            "wrap a value in float(...) or int(...)",
            "lux learn numbers",
        ],
    );
}

#[test]
fn a_non_bool_condition_is_caught() {
    reads_like(
        "cond",
        "if false {\n  if 5 { print(\"x\") }\n}\nprint(\"ok\")\n",
        &[
            "expected a true/false value, but this is an int",
            "conditions and &&/|| operands must be bool",
            "lux learn booleans",
        ],
    );
}

#[test]
fn looping_over_a_non_array_is_caught() {
    reads_like(
        "for",
        "if false {\n  for x in 5 { print(x) }\n}\nprint(\"ok\")\n",
        &[
            "cannot loop over int",
            "for ... in needs an array or a range",
            "lux learn for",
        ],
    );
}

#[test]
fn a_wrong_argument_type_is_caught() {
    reads_like(
        "arg",
        "func sq(n: int) -> int { return n * n }\nif false { print(sq(\"hi\")) }\nprint(\"ok\")\n",
        &[
            "`sq` expects `n` to be int, but got string",
            "lux learn functions",
            "each parameter has a type the call must match",
        ],
    );
}

#[test]
fn appending_the_wrong_type_to_an_array_is_caught() {
    reads_like(
        "append",
        "var xs: [int] = []\nif false { xs += \"nope\" }\nprint(\"ok\")\n",
        &[
            "cannot add string to an array of int",
            "lux learn arrays",
            "an array holds one type, so += has to match it",
        ],
    );
}

#[test]
fn assigning_the_wrong_type_to_a_var_is_caught() {
    reads_like(
        "assign",
        "var n = 0\nif false { n = \"text\" }\nprint(\"ok\")\n",
        &[
            "`n` is int but you assigned string",
            "lux learn variables",
            "a place keeps the type it started with",
        ],
    );
}

#[test]
fn an_annotation_that_disagrees_with_its_value_is_caught() {
    reads_like(
        "annot",
        "if false {\n  let n: int = \"text\"\n  print(n)\n}\nprint(\"ok\")\n",
        &["type mismatch: annotated `int` but the value is string"],
    );
}

#[test]
fn returning_the_wrong_type_is_caught() {
    reads_like(
        "ret",
        "func f() -> int {\n  return \"text\"\n}\nprint(\"ok\")\n",
        &[
            "`f` should return int, but returned string",
            "lux learn functions",
            "what comes back must match the `-> type`",
        ],
    );
}

#[test]
fn a_match_that_misses_a_case_is_caught() {
    reads_like(
        "match",
        "enum Color { red, green, blue }\nif false {\n  let c = Color.red\n  let s = match c {\n    red => \"r\"\n    green => \"g\"\n  }\n  print(s)\n}\nprint(\"ok\")\n",
        &[
            "this match on `Color` doesn't handle every case",
            "add an arm for: blue (or a `_` catch-all)",
            "lux learn match",
        ],
    );
}

#[test]
fn reading_a_field_a_struct_lacks_is_caught() {
    reads_like(
        "field",
        "struct Point { x: int, y: int }\nif false {\n  let p = Point(x: 1, y: 2)\n  print(p.z)\n}\nprint(\"ok\")\n",
        &[
            "struct `Point` has no field `z`",
            "lux learn structs",
            "a struct only has the fields you gave it",
        ],
    );
}

// ----- the two promises -------------------------------------------------------

#[test]
fn valid_constructs_in_a_dead_branch_still_run() {
    // Every construct the pass judges, used correctly, live and inside a branch
    // that never runs. None of it may be turned away.
    runs_ok(
        "valid",
        "enum Color { red, green, blue }\n\
         struct Point { x: int, y: int }\n\
         func sq(n: int) -> int { return n * n }\n\
         let p = Point(x: 1, y: 2)\n\
         var xs: [int] = []\n\
         xs += 5\n\
         let c = Color.red\n\
         let name = match c {\n  red => \"r\"\n  _ => \"other\"\n}\n\
         if false {\n\
           print(sq(p.x) + p.y)\n\
           for i in 0..3 { print(i) }\n\
           print(name)\n\
         }\n\
         print(\"ok\")\n",
    );
}

#[test]
fn empty_containers_and_none_are_never_rejected() {
    // The uncertain cases the pass must decline to judge: an empty array satisfies
    // any element type, a bare `none` any Option, an `ok`/`err` the other side.
    runs_ok(
        "uncertain",
        "func take(xs: [int], o: Option<string>) -> int { return length(xs) }\n\
         let n = take([], none)\n\
         print(n)\n",
    );
}

#[test]
fn every_leg_rejects_the_same_program_the_same_way() {
    // The parity the pass exists to guarantee: one type error, in a dead branch,
    // rejected identically whether the program is run, converted, or built —
    // where before, `run` accepted it and only the target compiler complained.
    let path = std::env::temp_dir().join(format!("lux-tc-parity-{}.lux", std::process::id()));
    std::fs::write(
        &path,
        "func sq(n: int) -> int { return n * n }\nif false { print(sq(\"hi\")) }\nprint(\"ok\")\n",
    )
    .unwrap();

    let run = error_line(&["run"], &path);
    let convert = error_line(&["convert", "rust"], &path);
    let build = error_line(&["build"], &path);
    let _ = std::fs::remove_file(&path);

    assert!(run.contains("`sq` expects `n` to be int"), "run: {run}");
    assert_eq!(run, convert, "run and convert must reject the same way");
    assert_eq!(run, build, "run and build must reject the same way");
}

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

// ----- the rest of the interpreter's type rules, on every path ----------------

#[test]
fn comparing_two_different_types_is_caught() {
    reads_like(
        "eq",
        "if false { print(1 == \"a\") }\nprint(\"ok\")\n",
        &[
            "cannot compare int with string",
            "both sides of == and != must be the same type",
        ],
    );
}

#[test]
fn ordering_two_different_types_is_caught() {
    reads_like(
        "ord",
        "if false { print(1 < \"a\") }\nprint(\"ok\")\n",
        &[
            "cannot compare an int with a string",
            "both sides must be the same type",
        ],
    );
}

#[test]
fn ordering_two_bools_is_caught() {
    reads_like(
        "ordbool",
        "if false { print(true < false) }\nprint(\"ok\")\n",
        &[
            "cannot order bool values with < or >",
            "use == or != to compare bools",
            "lux learn booleans",
        ],
    );
}

#[test]
fn not_on_a_non_bool_is_caught() {
    reads_like(
        "not",
        "if false { print(!5) }\nprint(\"ok\")\n",
        &[
            "cannot apply ! to an int",
            "! works on bool values",
            "lux learn booleans",
        ],
    );
}

#[test]
fn negating_a_non_number_is_caught() {
    reads_like(
        "neg",
        "if false { print(-\"x\") }\nprint(\"ok\")\n",
        &["cannot negate a string"],
    );
}

#[test]
fn indexing_a_non_array_is_caught() {
    reads_like(
        "idxbase",
        "if false {\n  let n = 5\n  print(n[0])\n}\nprint(\"ok\")\n",
        &[
            "cannot index into int; only arrays can be indexed",
            "lux learn arrays",
        ],
    );
}

#[test]
fn a_non_int_index_is_caught() {
    reads_like(
        "idxnum",
        "if false {\n  let xs = [1, 2, 3]\n  print(xs[\"a\"])\n}\nprint(\"ok\")\n",
        &[
            "an array index must be an int, but this is string",
            "lux learn arrays",
        ],
    );
}

#[test]
fn a_range_over_non_ints_is_caught() {
    reads_like(
        "range",
        "if false {\n  for i in 0..\"x\" { print(i) }\n}\nprint(\"ok\")\n",
        &[
            "a range needs two ints, but got int and string",
            "write something like 0..10",
            "lux learn for",
        ],
    );
}

#[test]
fn a_struct_built_with_an_unknown_field_is_caught() {
    reads_like(
        "sfield",
        "struct Point { x: int, y: int }\nif false { let p = Point(x: 1, z: 2) }\nprint(\"ok\")\n",
        &["struct `Point` has no field `z`", "lux learn structs"],
    );
}

#[test]
fn a_struct_built_missing_a_field_is_caught() {
    reads_like(
        "smiss",
        "struct Point { x: int, y: int }\nif false { let p = Point(x: 1) }\nprint(\"ok\")\n",
        &[
            "missing field `y` for struct `Point`",
            "`Point` has a field `y: int`",
            "lux learn structs",
        ],
    );
}

#[test]
fn a_struct_field_of_the_wrong_type_is_caught() {
    reads_like(
        "stype",
        "struct Point { x: int, y: int }\nif false { let p = Point(x: 1, y: \"no\") }\nprint(\"ok\")\n",
        &[
            "field `y` of `Point` should be int, but got string",
            "lux learn structs",
        ],
    );
}

#[test]
fn an_enum_case_that_does_not_exist_is_caught() {
    reads_like(
        "ecase",
        "enum Shape { dot, circle(r: int) }\nif false { let s = Shape.square(a: 2) }\nprint(\"ok\")\n",
        &[
            "enum `Shape` has no case `square`",
            "cases are: dot, circle",
            "lux learn enums",
        ],
    );
}

#[test]
fn an_enum_payload_of_the_wrong_type_is_caught() {
    reads_like(
        "etype",
        "enum Shape { dot, circle(r: int) }\nif false { let s = Shape.circle(r: \"no\") }\nprint(\"ok\")\n",
        &[
            "`r` in `Shape.circle` should be int, but got string",
            "lux learn enums",
        ],
    );
}

#[test]
fn a_payload_less_case_that_does_not_exist_is_caught() {
    reads_like(
        "eaccess",
        "enum Color { red, green, blue }\nif false { let c = Color.purple }\nprint(\"ok\")\n",
        &[
            "enum `Color` has no case `purple`",
            "cases are: red, green, blue",
        ],
    );
}

#[test]
fn matching_on_something_unmatchable_is_caught() {
    reads_like(
        "mstruct",
        "struct Point { x: int, y: int }\nif false {\n  let p = Point(x: 1, y: 2)\n  let r = match p { _ => 0 }\n}\nprint(\"ok\")\n",
        &[
            "cannot match on Point; match works on enums, int, string, and bool",
            "lux learn match",
        ],
    );
}

#[test]
fn a_value_match_without_a_wildcard_is_caught() {
    reads_like(
        "mscalar",
        "if false { let r = match 5 { 1 => \"a\" } }\nprint(\"ok\")\n",
        &[
            "this match on int needs a `_` case",
            "matching a value (not an enum) can't be exhaustive",
            "lux learn match",
        ],
    );
}

#[test]
fn a_case_pattern_on_a_scalar_is_caught() {
    reads_like(
        "mvariant",
        "if false { let r = match 5 { red => \"a\"  _ => \"b\" } }\nprint(\"ok\")\n",
        &[
            "this is int, not an enum, so it has no cases",
            "lux learn enums",
        ],
    );
}

#[test]
fn a_function_that_can_run_off_its_end_is_caught() {
    reads_like(
        "fallthru",
        "func f(n: int) -> int {\n    if n > 0 { return 1 }\n}\nprint(\"ok\")\n",
        &[
            "`f` must return int, but it ended without returning a value",
            "lux learn functions",
        ],
    );
}

#[test]
fn a_bare_return_where_a_value_is_promised_is_caught() {
    reads_like(
        "bareret",
        "func f() -> int {\n    return\n}\nprint(\"ok\")\n",
        &["`f` must return int, but it ended without returning a value"],
    );
}

#[test]
fn the_valid_near_misses_of_every_rule_still_run() {
    // Each construct the extended rules judge, used correctly: same-type compares,
    // an ordered string compare, an Option match, a struct and an enum built right,
    // and functions that return on every path — through both arms of an `if`, and
    // via `return match`. None may be turned away.
    runs_ok(
        "nearmiss",
        "enum Shape { dot, circle(r: int) }\n\
         struct Point { x: int, y: int }\n\
         func area(s: Shape) -> int {\n  return match s {\n    dot => 0\n    circle(let r) => r * r\n  }\n}\n\
         func sign(n: int) -> string {\n  if n < 0 { return \"neg\" } else { return \"nonneg\" }\n}\n\
         let p = Point(x: 3, y: 4)\n\
         print(p.x == 3)\n\
         print(\"a\" < \"b\")\n\
         print(area(Shape.dot))\n\
         print(area(Shape.circle(r: 5)))\n\
         print(sign(-2))\n\
         print(match some(7) { some(let v) => v  none => -1 })\n\
         for i in 0..3 { print(i) }\n",
    );
}

// ----- built-in argument types, on every path (#70) ---------------------------

#[test]
fn a_wrong_argument_type_to_a_builtin_is_caught() {
    // The string and conversion built-ins, and the file/process seams, each read
    // like the interpreter — same message, same trail — from a dead branch.
    reads_like(
        "blen",
        "if false { print(length(5)) }\nprint(\"ok\")\n",
        &[
            "length expects an array or a string, but got int",
            "lux learn arrays",
            "length counts an array's items or a string's characters",
        ],
    );
    reads_like(
        "bcontains",
        "if false { print(contains(\"a\", 5)) }\nprint(\"ok\")\n",
        &["contains expects argument 2 to be a string, but got int"],
    );
    reads_like(
        "bparse",
        "if false { print(parseInt(5)) }\nprint(\"ok\")\n",
        &["parseInt reads text, but got an int"],
    );
    reads_like(
        "bint",
        "if false { print(int(\"x\")) }\nprint(\"ok\")\n",
        &[
            "int converts between numbers, not from text",
            "lux learn conversions",
            "parseInt reads a number from text and gives back an Option",
        ],
    );
    reads_like(
        "bfloatother",
        "if false { print(float([1])) }\nprint(\"ok\")\n",
        &["cannot convert an array to a float"],
    );
    // A bare call, not `print(readFile(5))`: `readFile` hands back a `Result`, so
    // printing it trips the Result-can't-be-printed rule first (as it does under the
    // interpreter) — the bare form isolates the argument-type check being tested here.
    reads_like(
        "breadfile",
        "if false { readFile(5) }\nprint(\"ok\")\n",
        &["readFile expects a string, but got int"],
    );
}

#[test]
fn int_of_a_string_is_refused_before_any_swift_is_emitted() {
    // Refusing the call also removes a wrong Swift program: `int("x")` compiled to
    // Swift's failable `Int("x")`, which prints `nil` — a value lux has no concept
    // of. Convert must now refuse it, on the same footing as run, not emit it.
    let path = std::env::temp_dir().join(format!("lux-tc-intstr-{}.lux", std::process::id()));
    std::fs::write(&path, "print(int(\"x\"))\n").unwrap();
    let run = error_line(&["run"], &path);
    let swift = error_line(&["convert", "swift"], &path);
    let _ = std::fs::remove_file(&path);
    assert!(
        run.contains("int converts between numbers, not from text"),
        "run: {run}"
    );
    assert_eq!(run, swift, "run and convert swift must refuse the same way");
}

#[test]
fn well_typed_builtin_calls_still_run() {
    // The other side of the rule: every built-in used correctly is left alone.
    runs_ok(
        "bok",
        "print(length([1, 2, 3]))\n\
         print(length(\"cafe\"))\n\
         print(contains(\"hello\", \"ell\"))\n\
         print(replace(\"a-b\", \"-\", \"+\"))\n\
         print(split(\"a,b\", \",\"))\n\
         print(int(3.7))\n\
         print(float(5))\n\
         print(parseInt(\"42\"))\n",
    );
}

// ----- duplicate declarations, on every path (#71) ----------------------------

#[test]
fn two_declarations_of_the_same_name_are_caught() {
    reads_like(
        "dupstruct",
        "struct S { x: int }\nstruct S { y: int }\nprint(\"done\")\n",
        &["type `S` is already defined"],
    );
    reads_like(
        "dupenum",
        "enum E { a }\nenum E { b }\nprint(\"done\")\n",
        &["type `E` is already defined"],
    );
    reads_like(
        "dupfunc",
        "func f() -> int { return 1 }\nfunc f() -> int { return 2 }\nprint(\"done\")\n",
        &["function `f` is already defined"],
    );
}

#[test]
fn a_duplicate_declaration_is_refused_on_every_leg() {
    // Before, `run` refused a second `struct S` and the three converts emitted
    // both, leaving the target compiler to reject the duplicate in its own words.
    let path = std::env::temp_dir().join(format!("lux-tc-dup-{}.lux", std::process::id()));
    std::fs::write(
        &path,
        "struct S { x: int }\nstruct S { y: int }\nprint(\"done\")\n",
    )
    .unwrap();
    let run = error_line(&["run"], &path);
    let convert = error_line(&["convert", "go"], &path);
    let build = error_line(&["build"], &path);
    let _ = std::fs::remove_file(&path);
    assert!(run.contains("type `S` is already defined"), "run: {run}");
    assert_eq!(run, convert, "run and convert must refuse the same way");
    assert_eq!(run, build, "run and build must refuse the same way");
}

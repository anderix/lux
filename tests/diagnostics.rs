//! Diagnostics that have to meet the errors-are-trails bar, since a learner meets
//! them early: an empty struct, a construction field that forgot its label, a
//! `return` inside a match arm, and recursion that never reaches a base case. Each
//! should name the cause and point somewhere useful, not fall through to a raw
//! "expected a value" — or, for runaway recursion, a stack overflow that aborts
//! with no message at all.

use std::process::{Command, Stdio};

/// Run a program expected to be rejected, and hand back its stderr.
fn err_of(tag: &str, src: &str) -> String {
    let path = std::env::temp_dir().join(format!("lux-diag-{}-{}.lux", std::process::id(), tag));
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

#[test]
fn an_empty_struct_is_refused_with_a_pointer_to_enums() {
    // A struct with no fields can't be built (`Name()` reads as a call), so it's
    // caught at the declaration, and the message points at the enum, which is how
    // you name a value that carries no data.
    let err = err_of("emptystruct", "struct Empty { }\nlet e = Empty()\n");
    assert!(
        err.contains("needs at least one field"),
        "should name the rule, got:\n{err}"
    );
    assert!(
        err.contains("enum"),
        "should point at the enum alternative, got:\n{err}"
    );
}

#[test]
fn a_construction_field_without_a_label_names_the_fix() {
    // The commonest slip building an enum case: forgetting the label.
    let err = err_of(
        "nolabel",
        "enum Color { red  named(label: string) }\nprint(Color.named(\"teal\"))\n",
    );
    assert!(
        err.contains("needs a label"),
        "should name the missing label, got:\n{err}"
    );
    assert!(
        !err.contains("expected a field name"),
        "should not fall through to the raw parser error, got:\n{err}"
    );
}

#[test]
fn a_return_inside_a_match_arm_explains_arms_are_values() {
    let err = err_of(
        "retarm",
        "func f(n: int) -> int {\n    match n { 1 => return 10  _ => print(\"no\") }\n    return 0\n}\nprint(f(1))\n",
    );
    assert!(
        err.contains("match arm is a value") && err.contains("`return`"),
        "should explain arms are expressions, got:\n{err}"
    );
}

/// Recursion with no reachable base case — the classic beginner mistake, and the
/// one place the interpreter used to show its own host language: a raw stack
/// overflow, `SIGABRT`, exit 134, and not one word about the program that was run.
/// It now stops itself at a depth limit and reports an ordinary lux error that
/// names the function and points at the base case, exiting 1 like every other.
#[test]
fn runaway_recursion_reports_a_lux_error_instead_of_aborting() {
    let src = "func fact(n: int) -> int {\n    return n * fact(n - 1)\n}\nprint(fact(5))\n";
    let path = std::env::temp_dir().join(format!("lux-diag-{}-runaway.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);

    // Exit 1 (an ordinary lux error), not 134 (SIGABRT) or any other signal death.
    assert_eq!(
        out.status.code(),
        Some(1),
        "should exit 1 like every lux error"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("`fact`") && err.contains("base case"),
        "should name the function and point at the base case, got:\n{err}"
    );
    assert!(
        !err.contains("stack overflow") && !err.contains("aborting"),
        "should not leak the host runtime's overflow abort, got:\n{err}"
    );
}

/// The depth limit sits well above any real recursion, so a correct, terminating
/// program that simply goes deep still runs — the interpreter clears thousands of
/// frames on the large stack it runs on, converging with the compiled targets
/// rather than refusing what they accept.
#[test]
fn deep_but_terminating_recursion_still_runs() {
    let src = "func d(n: int) -> int {\n    if n == 0 { return 0 }\n    return 1 + d(n - 1)\n}\nprint(d(5000))\n";
    let path = std::env::temp_dir().join(format!("lux-diag-{}-deep.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "d(5000) should run to completion");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5000\n");
}

/// The "unknown function" note names the built-ins a mistyped call might have
/// meant, and it's the one place a stuck learner is told what exists — so a
/// built-in missing from it reads as a built-in that doesn't exist. It rendered a
/// hand-written list that had drifted three names behind the real set; it now
/// renders the single source, so every working built-in appears, `input`,
/// `parseInt`, and `parseFloat` included.
#[test]
fn the_unknown_function_note_lists_every_builtin() {
    let err = err_of("unknownfn", "print(contains(\"ab\", \"a\"))\n");
    for name in [
        "print",
        "eprint",
        "string",
        "int",
        "float",
        "length",
        "input",
        "readLine",
        "readFile",
        "writeFile",
        "args",
        "run",
        "parseInt",
        "parseFloat",
    ] {
        assert!(
            err.contains(name),
            "the note should list the `{name}` built-in, got:\n{err}"
        );
    }
}

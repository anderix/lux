//! Regression tests for the fix that lets a bare `let`/`var` bind a fallible
//! built-in without an annotation.
//!
//! `readLine`, `readFile`, `writeFile`, `run`, `parseInt`, and `parseFloat`
//! return `Option`/`Result`, and the built-in's signature pins the type even when
//! the value can't — a `none` from `readLine` is still `Option<string>`. So
//! `let line = readLine()` is fine even on the `none` it returns at end of input.
//! A bare `none`/`err` literal, whose type really is open, must still be
//! annotated. The failure variant is the one that used to crash, so it is tested
//! for every built-in; these drive the built binary the way a learner hits it.

use std::io::Write;
use std::process::{Command, Stdio};

/// Write `src` to a uniquely named temp .lux file and return its path.
fn write_lux(tag: &str, src: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("lux-binding-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    path
}

/// Run `src` with `stdin` fed in; return (succeeded, stdout, stderr).
fn run(tag: &str, src: &str, stdin: &str) -> (bool, String, String) {
    let path = write_lux(tag, src);
    let mut child = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&path);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A declaration binds cleanly: the program reaches the marker print with no
/// determinacy error.
fn binds(tag: &str, decl: &str, stdin: &str) {
    let src = format!("{decl}\nprint(\"bound\")\n");
    let (ok, stdout, stderr) = run(tag, &src, stdin);
    assert!(ok, "`{decl}` should bind without an annotation:\n{stderr}");
    assert!(
        stdout.contains("bound"),
        "`{decl}` never reached the print:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn parse_int_binds_on_both_variants() {
    binds("parseint-some", "let n = parseInt(\"17\")", "");
    binds("parseint-none", "let n = parseInt(\"nope\")", "");
}

#[test]
fn parse_float_binds_on_both_variants() {
    binds("parsefloat-some", "let f = parseFloat(\"1.5\")", "");
    binds("parsefloat-none", "let f = parseFloat(\"nope\")", "");
}

#[test]
fn read_line_binds_on_both_variants() {
    binds("readline-some", "let line = readLine()", "hello\n");
    binds("readline-none", "let line = readLine()", ""); // EOF returns none
}

#[test]
fn read_file_binds_on_the_error_path() {
    binds("readfile-err", "let r = readFile(\"/no/such/file\")", "");
}

#[test]
fn write_file_binds_on_the_error_path() {
    binds(
        "writefile-err",
        "let w = writeFile(\"/no/such/dir/f\", \"x\")",
        "",
    );
}

#[test]
fn run_binds_on_both_variants() {
    // The error path: a command that can't launch. The success path runs the lux
    // binary itself, which is guaranteed to exist, so both sides of the `Result`
    // are covered — and a `Result` value never pins its own type, so before the
    // fix even the success path needed an annotation.
    binds(
        "run-err",
        "let r = run(\"lux-no-such-command-xyzzy\", [])",
        "",
    );
    let bin = env!("CARGO_BIN_EXE_lux")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    binds(
        "run-ok",
        &format!("let r = run(\"{bin}\", [\"--version\"])"),
        "",
    );
}

#[test]
fn var_form_also_binds_a_fallible_builtin() {
    binds("var-parseint", "var n = parseInt(\"nope\")", "");
}

#[test]
fn user_functions_pin_the_type_the_same_way_builtins_do() {
    // A declared return type pins the binding whether the callee is a built-in or
    // a function the program wrote — so a bare `let` needs no annotation even on
    // the none/err path, just as for `parseInt`.
    binds(
        "userfn-option",
        "func find() -> Option<int> {\n    return none\n}\nlet o = find()",
        "",
    );
    binds(
        "userfn-result",
        "func attempt() -> Result<int, string> {\n    return err(\"no\")\n}\nlet r = attempt()",
        "",
    );
}

#[test]
fn a_bare_none_still_needs_an_annotation() {
    let (ok, _stdout, stderr) = run("bare-none", "let x = none\nprint(\"bound\")\n", "");
    assert!(!ok, "a bare `let x = none` must still be rejected");
    assert!(
        stderr.contains("leaves it open"),
        "expected the determinacy error, got:\n{stderr}"
    );
}

//! Bindings for fallible calls, and the line lux draws between Option and Result.
//!
//! A built-in or function that returns `Option<T>` pins its type from the
//! signature, so a bare `let n = parseInt(x)` needs no annotation even on the
//! `none` path — built-ins and user functions alike. A `Result`, though, is a
//! question you answer where you ask it: it may be matched inline or returned,
//! but never stored, a Rust/Swift habit that doesn't carry to Go. And a bare
//! `none` literal still needs its annotation. These drive the built binary the
//! way a learner hits it.

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

/// A program reaches its marker print with no error.
fn ok_reaches_end(tag: &str, body: &str, stdin: &str) {
    let src = format!("{body}\nprint(\"reached\")\n");
    let (ok, stdout, stderr) = run(tag, &src, stdin);
    assert!(ok, "`{body}` should run:\n{stderr}");
    assert!(
        stdout.contains("reached"),
        "`{body}` never reached the end:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// A bare `let`/`var` binding reaches its marker (binds cleanly, no annotation).
fn binds(tag: &str, decl: &str, stdin: &str) {
    ok_reaches_end(tag, decl, stdin);
}

/// A declaration is rejected, with an error containing `needle`.
fn rejected(tag: &str, decl: &str, needle: &str) {
    let src = format!("{decl}\nprint(\"reached\")\n");
    let (ok, stdout, stderr) = run(tag, &src, "");
    assert!(!ok, "`{decl}` should be rejected, but ran:\n{stdout}");
    assert!(
        stderr.contains(needle),
        "`{decl}` gave the wrong error (wanted `{needle}`):\n{stderr}"
    );
}

// --- Option: a real value, storable without an annotation ------------------

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
fn var_form_also_binds_an_option() {
    binds("var-parseint", "var n = parseInt(\"nope\")", "");
}

#[test]
fn user_option_functions_bind_without_annotation() {
    // A declared return type pins the binding for a user function too, so a bare
    // `let` needs no annotation even on the `none` path.
    binds(
        "userfn-option",
        "func find() -> Option<int> {\n    return none\n}\nlet o = find()",
        "",
    );
}

#[test]
fn a_bare_none_still_needs_an_annotation() {
    let (ok, _stdout, stderr) = run("bare-none", "let x = none\nprint(\"reached\")\n", "");
    assert!(!ok, "a bare `let x = none` must still be rejected");
    assert!(
        stderr.contains("leaves it open"),
        "expected the determinacy error, got:\n{stderr}"
    );
}

// --- Result: handled where it's produced, never stored ---------------------

const HALF: &str = "func half(n: int) -> Result<int, string> {\n    if n % 2 == 0 { return ok(n / 2) }\n    return err(\"odd\")\n}";

#[test]
fn a_result_cannot_be_stored() {
    // Built-ins that return Result, and a user function that does — none may be
    // stashed in a binding.
    rejected(
        "store-readfile",
        "let r = readFile(\"/no/such\")",
        "can't be stored",
    );
    rejected(
        "store-run",
        "let r = run(\"lux-nope-xyzzy\", [])",
        "can't be stored",
    );
    rejected(
        "store-userfn",
        &format!("{HALF}\nlet r = half(4)"),
        "can't be stored",
    );
    // An annotation doesn't buy it back — storing is the thing that's disallowed.
    rejected(
        "store-annotated",
        &format!("{HALF}\nlet r: Result<int, string> = half(4)"),
        "can't be stored",
    );
    // Nor does hiding it in a struct field, which is storing it just the same.
    rejected(
        "store-struct-field",
        &format!("struct Box {{\n    r: Result<int, string>\n}}\n{HALF}\nlet b = Box(r: half(4))"),
        "can't be stored",
    );
}

#[test]
fn a_result_can_be_matched_inline_or_returned() {
    // Matched right where it's produced.
    ok_reaches_end(
        "match-inline",
        &format!("{HALF}\nmatch half(4) {{ ok(let v) => print(v)  err(let e) => print(e) }}"),
        "",
    );
    // Or handed straight back up — a Result return is a passthrough, not storage.
    ok_reaches_end(
        "return-passthrough",
        &format!(
            "{HALF}\nfunc g(n: int) -> Result<int, string> {{ return half(n) }}\nmatch g(6) {{ ok(let v) => print(v)  err(let e) => print(e) }}"
        ),
        "",
    );
}

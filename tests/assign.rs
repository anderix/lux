//! Assigning through a place — a struct field or an array element — when the
//! root is a `var`. The interpreter is the reference for the behaviour and for
//! the diagnostics a learner meets: a `let` refuses with a named rule, and the
//! left of an assignment has to be a place, not a value.

use std::process::{Command, Stdio};

fn run(tag: &str, src: &str) -> (bool, String, String) {
    let path = std::env::temp_dir().join(format!("lux-assign-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&path);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_var_field_and_element_can_be_assigned() {
    let src = "\
struct World { doorOpen: bool  items: [string] }
var w = World(doorOpen: false, items: [\"key\"])
w.doorOpen = true
w.items[0] = \"lantern\"
w.items += \"torch\"
print(w.doorOpen)
print(w.items[0])
print(w.items[1])
";
    let (ok, out, err) = run("place", src);
    assert!(ok, "assigning through a var place should run:\n{err}");
    assert_eq!(out, "true\nlantern\ntorch\n");
}

#[test]
fn assigning_through_a_let_is_refused_with_a_named_rule() {
    let src = "\
struct World { doorOpen: bool }
let w = World(doorOpen: false)
w.doorOpen = true
";
    let (ok, _out, err) = run("letref", src);
    assert!(!ok, "a let place must be refused");
    assert!(
        err.contains("cannot change `w.doorOpen`") && err.contains("declared with let"),
        "the refusal should name the place and the rule, got:\n{err}"
    );
}

#[test]
fn assignment_copies_rather_than_shares() {
    // lux structs are values: a copy assigned into another var is independent.
    let src = "\
struct World { doorOpen: bool }
var w = World(doorOpen: false)
var a = w
a.doorOpen = true
print(w.doorOpen)
print(a.doorOpen)
";
    let (ok, out, err) = run("copy", src);
    assert!(ok, "should run:\n{err}");
    assert_eq!(
        out, "false\ntrue\n",
        "mutating the copy must leave the source alone"
    );
}

#[test]
fn the_left_of_an_assignment_must_be_a_place() {
    let (ok, _out, err) = run("nonplace", "let x = 5\nx + 1 = 3\n");
    assert!(!ok, "assigning to a non-place must be refused");
    assert!(
        err.contains("has to be a place"),
        "expected the place diagnostic, got:\n{err}"
    );
}

#[test]
fn a_parameter_is_refused_without_pointing_at_var() {
    // A parameter is immutable, but it can't be made a `var` — the syntax isn't
    // there — so the refusal must name it as a parameter and point at the
    // copy-into-a-local idiom, not at `let`/`var`.
    let src = "\
func firstToZero(xs: [int]) -> [int] {
    xs[0] = 0
    return xs
}
print(firstToZero([1, 2, 3]))
";
    let (ok, _out, err) = run("param", src);
    assert!(!ok, "assigning through a parameter must be refused");
    assert!(
        err.contains("`xs` is a parameter") && err.contains("copy it into a local var"),
        "a parameter refusal must name it and point at the copy idiom, got:\n{err}"
    );
    assert!(
        !err.contains("declared with let"),
        "a parameter is not a let, so the let wording must not appear:\n{err}"
    );
}

#[test]
fn a_loop_variable_is_refused_without_pointing_at_var() {
    let src = "\
struct Counter { n: int }
var cs = [Counter(n: 0)]
for c in cs {
    c.n = 1
}
";
    let (ok, _out, err) = run("loopvar", src);
    assert!(!ok, "assigning through a loop variable must be refused");
    assert!(
        err.contains("`c` is the loop's variable"),
        "a loop-variable refusal must name it as the loop's, got:\n{err}"
    );
    assert!(
        !err.contains("declared with let"),
        "a loop variable is not a let, so the let wording must not appear:\n{err}"
    );
}

#[test]
fn copying_a_parameter_into_a_local_var_works() {
    // The idiom the parameter refusal points at must actually run.
    let src = "\
func firstToZero(values: [int]) -> [int] {
    var xs = values
    xs[0] = 0
    return xs
}
print(firstToZero([1, 2, 3]))
";
    let (ok, out, err) = run("paramcopy", src);
    assert!(ok, "the copy-into-a-local idiom should run:\n{err}");
    assert_eq!(out, "[0, 2, 3]\n");
}

#[test]
fn a_field_keeps_its_type() {
    let (ok, _out, err) = run(
        "fieldty",
        "struct W { n: int }\nvar w = W(n: 1)\nw.n = \"hi\"\n",
    );
    assert!(!ok, "a type-mismatched field assignment must be refused");
    assert!(
        err.contains("`w.n` is int but you assigned string"),
        "expected the field type error, got:\n{err}"
    );
}

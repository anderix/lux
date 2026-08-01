//! Parser-level diagnostics that have to meet the errors-are-trails bar, since a
//! learner meets them early: an empty struct, a construction field that forgot its
//! label, and a `return` inside a match arm. Each should name the cause and point
//! somewhere useful, not fall through to a raw "expected a value".

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

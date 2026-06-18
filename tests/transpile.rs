//! The Rust backend, end to end: every example must translate to Rust that
//! the real compiler accepts. This is the transpiler's load-bearing test —
//! it's cheap to assert "it parsed", but only rustc can confirm the output is
//! actually valid Rust.

use std::process::Command;

use lux::{convert, lexer, parser};

const EXAMPLES: &[&str] = &["hello", "functions", "types", "option", "tour"];

fn to_rust(name: &str) -> String {
    let path = format!("{}/examples/{}.lux", env!("CARGO_MANIFEST_DIR"), name);
    let source = std::fs::read_to_string(&path).expect("read example");
    let tokens = lexer::lex(&source).expect("lex");
    let program = parser::parse(tokens).expect("parse");
    convert::to_rust(&program)
}

fn rustc_available() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn examples_translate_to_compilable_rust() {
    if !rustc_available() {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let tmp = std::env::temp_dir();
    for name in EXAMPLES {
        let rust = to_rust(name);
        let rs = tmp.join(format!("lux_transpile_{}.rs", name));
        std::fs::write(&rs, &rust).expect("write generated rust");
        let bin = tmp.join(format!("lux_transpile_{}.bin", name));
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run rustc");
        assert!(
            out.status.success(),
            "{}.lux did not compile:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn naming_follows_rust_idiom() {
    let rust = to_rust("tour");
    // camelCase functions become snake_case.
    assert!(rust.contains("fn first_even("), "expected snake_case function");
    // lowercase enum cases become PascalCase tuple variants.
    assert!(rust.contains("Shape::Circle("), "expected PascalCase variant");
    // top-level statements are wrapped in a main.
    assert!(rust.contains("fn main()"), "expected a main");
}

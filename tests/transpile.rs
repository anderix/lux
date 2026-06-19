//! The transpiler backends, end to end: every example must translate to source
//! that the real target compiler accepts. It's cheap to assert "it parsed", but
//! only `rustc`, `swiftc`, and `go` can confirm the output is valid in their
//! languages. Each compiler check is skipped when that toolchain isn't present,
//! so the suite stays green on a machine that only has some of them.

use std::path::Path;
use std::process::Command;

use lux::{convert, lexer, parser};

const EXAMPLES: &[&str] = &["hello", "functions", "types", "option", "tour", "io", "shell"];

fn parse(name: &str) -> Vec<lux::ast::Stmt> {
    let path = format!("{}/examples/{}.lux", env!("CARGO_MANIFEST_DIR"), name);
    let source = std::fs::read_to_string(&path).expect("read example");
    let tokens = lexer::lex(&source).expect("lex");
    parser::parse(tokens).expect("parse")
}

fn tool_available(cmd: &str, version_arg: &str) -> bool {
    Command::new(cmd)
        .arg(version_arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// --- Rust ------------------------------------------------------------------

#[test]
fn rust_examples_compile() {
    if !tool_available("rustc", "--version") {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let tmp = std::env::temp_dir();
    for name in EXAMPLES {
        let rust = convert::to_rust(&parse(name));
        let rs = tmp.join(format!("lux_rs_{}.rs", name));
        std::fs::write(&rs, &rust).expect("write rust");
        let bin = tmp.join(format!("lux_rs_{}.bin", name));
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run rustc");
        assert!(
            out.status.success(),
            "{}.lux did not compile as Rust:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn rust_naming_is_idiomatic() {
    let rust = convert::to_rust(&parse("tour"));
    assert!(rust.contains("fn first_even("), "camelCase becomes snake_case");
    assert!(rust.contains("Shape::Circle("), "lowercase cases become PascalCase");
    assert!(rust.contains("fn main()"), "top level wraps in a main");
}

// --- Swift -----------------------------------------------------------------

#[test]
fn swift_examples_compile() {
    if !tool_available("swiftc", "--version") {
        eprintln!("skipping: swiftc not on PATH");
        return;
    }
    let tmp = std::env::temp_dir();
    for name in EXAMPLES {
        let swift = convert::to_swift(&parse(name));
        let src = tmp.join(format!("lux_sw_{}.swift", name));
        std::fs::write(&src, &swift).expect("write swift");
        let bin = tmp.join(format!("lux_sw_{}.bin", name));
        let out = Command::new("swiftc")
            .arg(&src)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success(),
            "{}.lux did not compile as Swift:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
        // The backend's bar is warning-clean output, so no diagnostics at all.
        assert!(
            out.stderr.is_empty(),
            "{}.lux produced Swift warnings:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn swift_idioms() {
    let swift = convert::to_swift(&parse("tour"));
    // Underscore labels keep calls positional like lux's.
    assert!(swift.contains("func factorial(_ n: Int)"), "positional labels");
    // Swift keeps lux's labeled enum cases.
    assert!(swift.contains("case circle(radius: Double)"), "labeled cases");
    // Optional is native.
    assert!(swift.contains("-> Int?"), "Option becomes Optional");

    // A string-carrying Result pulls in the retroactive Error conformance.
    let opt = convert::to_swift(&parse("option"));
    assert!(
        opt.contains("extension String: @retroactive Error {}"),
        "string Result conforms String to Error"
    );
    assert!(opt.contains("Result<Int, String>"), "native Result");
}

// --- Go --------------------------------------------------------------------

#[test]
fn go_examples_compile() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let tmp = std::env::temp_dir();
    let cache = tmp.join("lux_go_cache");
    for name in EXAMPLES {
        let go = convert::to_go(&parse(name));
        let dir = tmp.join(format!("lux_go_{}", name));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
        std::fs::write(dir.join("main.go"), &go).expect("write go");
        let out = Command::new("go")
            .arg("build")
            .arg("-o")
            .arg(dir.join("bin"))
            .current_dir(&dir)
            .env("GOCACHE", &cache)
            .output()
            .expect("run go build");
        assert!(
            out.status.success(),
            "{}.lux did not compile as Go:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn go_idioms() {
    let go = convert::to_go(&parse("tour"));
    // An enum becomes a marker interface plus a struct per case.
    assert!(go.contains("type Shape interface{ isShape() }"), "marker interface");
    assert!(go.contains("type ShapeCircle struct {"), "per-case struct");
    assert!(go.contains("switch v := s.(type)"), "type switch on the enum");
    // Option is a pointer; the func wrapping is a main.
    assert!(go.contains("*int"), "Option becomes a pointer");
    assert!(go.contains("func main()"), "top level wraps in a main");

    // Result is Go's (value, error) pair.
    let opt = convert::to_go(&parse("option"));
    assert!(opt.contains("(int, error)"), "Result becomes value, error");
    assert!(opt.contains("errors.New("), "err carries a reason");
}

// --- structure shared by all three ----------------------------------------

#[test]
fn every_backend_emits_nonempty() {
    for name in EXAMPLES {
        let program = parse(name);
        for (lang, src) in [
            ("rust", convert::to_rust(&program)),
            ("swift", convert::to_swift(&program)),
            ("go", convert::to_go(&program)),
        ] {
            assert!(!src.trim().is_empty(), "{} backend emitted nothing for {}", lang, name);
        }
    }
    let _ = Path::new("");
}

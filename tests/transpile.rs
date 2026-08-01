//! The transpiler backends, end to end: every example must translate to source
//! that the real target compiler accepts. It's cheap to assert "it parsed", but
//! only `rustc`, `swiftc`, and `go` can confirm the output is valid in their
//! languages. Each compiler check is skipped when that toolchain isn't present,
//! so the suite stays green on a machine that only has some of them.

use std::path::Path;
use std::process::Command;

use lux::{convert, lexer, parser};

const EXAMPLES: &[&str] = &[
    "hello",
    "functions",
    "types",
    "option",
    "conversions",
    "tour",
    "io",
    "shell",
];

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
    assert!(
        rust.contains("fn first_even("),
        "camelCase becomes snake_case"
    );
    assert!(
        rust.contains("Shape::Circle("),
        "lowercase cases become PascalCase"
    );
    assert!(rust.contains("fn main()"), "top level wraps in a main");
}

/// Two Rust codegen footguns: a loop variable that collides with a Rust keyword
/// (`gen`) must be mangled identically where it's declared and where it's read,
/// and `length(x) < n` must parenthesize the cast so Rust doesn't read `i64 < n`
/// as generic arguments. Both must compile — and warning-clean, since the parens
/// are needed exactly here and nowhere else.
#[test]
fn rust_keyword_loop_var_and_cast_before_less_than_compile_clean() {
    if !tool_available("rustc", "--version") {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let src = r#"
var count = 0
for gen in 0..3 {
    if length([1, 2, 3]) < gen {
        count += 1
    }
}
print(count)
"#;
    let rust = convert::to_rust(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    let tmp = std::env::temp_dir();
    let rs = tmp.join("lux_rs_keyword_cast.rs");
    std::fs::write(&rs, &rust).expect("write rust");
    let out = Command::new("rustc")
        .arg(&rs)
        .arg("-o")
        .arg(tmp.join("lux_rs_keyword_cast.bin"))
        .output()
        .expect("run rustc");
    assert!(
        out.status.success(),
        "keyword loop var / cast-before-< did not compile as Rust:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "should compile warning-clean, got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    assert!(
        swift.contains("func factorial(_ n: Int)"),
        "positional labels"
    );
    // Swift keeps lux's labeled enum cases.
    assert!(
        swift.contains("case circle(radius: Double)"),
        "labeled cases"
    );
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
    assert!(
        go.contains("type Shape interface{ isShape() }"),
        "marker interface"
    );
    assert!(go.contains("type ShapeCircle struct {"), "per-case struct");
    assert!(
        go.contains("switch v := s.(type)"),
        "type switch on the enum"
    );
    // Option is a pointer; the func wrapping is a main.
    assert!(go.contains("*int"), "Option becomes a pointer");
    assert!(go.contains("func main()"), "top level wraps in a main");

    // Result is Go's (value, error) pair.
    let opt = convert::to_go(&parse("option"));
    assert!(opt.contains("(int, error)"), "Result becomes value, error");
    assert!(opt.contains("errors.New("), "err carries a reason");
}

/// Go rejects an unused local where Rust and Swift only warn, so a match arm that
/// binds a payload and ignores it must not emit the binding. This covers all
/// three lowerings: a user enum (both arms ignore — so even the type-switch guard
/// must be dropped — and one-of-two), an Option, and a Result on both sides.
#[test]
fn go_match_arms_that_ignore_their_binding_compile() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
enum Shape {
    circle(radius: float)
    square(side: float)
}
func name(s: Shape) -> string {
    return match s {
        circle(let r) => "round"
        square(let a) => "boxy"
    }
}
func area(s: Shape) -> float {
    return match s {
        circle(let r) => 3.14159 * r * r
        square(let a) => 0.0
    }
}
func maybe() -> Option<int> {
    return some(5)
}
func attempt() -> Result<int, string> {
    return ok(1)
}
print(name(Shape.circle(radius: 2.0)))
print(area(Shape.square(side: 3.0)))
match maybe() {
    some(let x) => print("present")
    none => print("absent")
}
match attempt() {
    ok(let v) => print("good")
    err(let e) => print("bad")
}
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);

    let tmp = std::env::temp_dir().join("lux_go_match_ignore");
    std::fs::create_dir_all(&tmp).expect("mkdir");
    std::fs::write(tmp.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
    std::fs::write(tmp.join("main.go"), &go).expect("write go");
    let out = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(tmp.join("bin"))
        .current_dir(&tmp)
        .env("GOCACHE", std::env::temp_dir().join("lux_go_cache"))
        .output()
        .expect("run go build");
    assert!(
        out.status.success(),
        "match arms that ignore their binding did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The Go backend opens a type switch with a scratch subject and lowers a Result
/// through a scratch error name. When a match arm binds a variable with that same
/// name — `full(let v, …)` or `err(let err)` — the scratch must step aside, or Go
/// rejects `v := v.field` / `err := err.Error()` with "no new variables on left
/// side of :=". Covers the enum type-switch subject and both Result scratch sites.
#[test]
fn go_match_binding_that_shadows_the_scratch_name_compiles() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
enum Box {
    empty
    full(value: int, label: string)
}
func describe(b: Box) -> string {
    return match b {
        empty => "empty"
        full(let v, let name) => name + "=" + string(v)
    }
}
func fail() -> Result<int, string> {
    return err("boom")
}
func ok_val() -> Result<int, string> {
    return ok(3)
}
print(describe(Box.full(value: 7, label: "seven")))
match fail() {
    ok(let v) => print(string(v))
    err(let err) => print(err)
}
match ok_val() {
    ok(let err) => print(string(err))
    err(let e) => print(e)
}
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);

    let tmp = std::env::temp_dir().join("lux_go_scratch_shadow");
    std::fs::create_dir_all(&tmp).expect("mkdir");
    std::fs::write(tmp.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
    std::fs::write(tmp.join("main.go"), &go).expect("write go");
    let out = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(tmp.join("bin"))
        .current_dir(&tmp)
        .env("GOCACHE", std::env::temp_dir().join("lux_go_cache"))
        .output()
        .expect("run go build");
    assert!(
        out.status.success(),
        "a match binding that shadows the emitter's scratch name did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// An empty array literal carries no element to infer from, so its type must come
/// from the annotation or field it's assigned into — otherwise Go gets `[]any{}`
/// where a typed slice is required. Covers both the annotated binding and the
/// struct-literal field.
#[test]
fn go_empty_arrays_take_their_declared_element_type() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
struct Bag {
    items: [int]
    tags: [string]
}
func empties() -> [int] {
    var out: [int] = []
    out += 1
    return out
}
let b = Bag(items: [], tags: [])
print(length(empties()))
print(length(b.items))
print(length(b.tags))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    // The annotation and field types must reach the literal, not Go's `[]any{}`.
    assert!(go.contains("out := []int{}"), "annotated binding:\n{go}");
    assert!(
        go.contains("items: []int{}") && go.contains("tags: []string{}"),
        "struct-literal fields:\n{go}"
    );

    let tmp = std::env::temp_dir().join("lux_go_empty_arrays");
    std::fs::create_dir_all(&tmp).expect("mkdir");
    std::fs::write(tmp.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
    std::fs::write(tmp.join("main.go"), &go).expect("write go");
    let out = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(tmp.join("bin"))
        .current_dir(&tmp)
        .env("GOCACHE", std::env::temp_dir().join("lux_go_cache"))
        .output()
        .expect("run go build");
    assert!(
        out.status.success(),
        "empty annotated arrays did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
            assert!(
                !src.trim().is_empty(),
                "{} backend emitted nothing for {}",
                lang,
                name
            );
        }
    }
    let _ = Path::new("");
}

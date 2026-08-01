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
    // The crawl starter world — the program every learner is handed, so it must
    // convert and compile warning-clean on all three backends. It exercises a
    // value moved into an array/struct field (Rust) and an `Option` of an enum
    // (Go), the two seams closed in 0.14.3.
    "keep",
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

/// Swift emits enum cases as the bare lowercase name lux wrote, so a case named
/// after a Swift keyword — `nil` is the textbook empty-list case — must be
/// backtick-quoted at every site: the declaration, a match pattern, and
/// construction. Go and Rust qualify their cases and never hit this.
#[test]
fn swift_keyword_named_enum_case_compiles() {
    let src = r#"
enum List {
    nil
    cons(head: int, tail: List)
}
func sum(l: List) -> int {
    return match l {
        nil => 0
        cons(let h, let t) => h + sum(t)
    }
}
let xs = List.cons(head: 1, tail: List.cons(head: 2, tail: List.nil))
print(sum(xs))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let swift = convert::to_swift(&program);
    assert!(
        swift.contains("case `nil`"),
        "declaration is backtick-quoted"
    );
    assert!(swift.contains(".`nil`"), "pattern and construction quoted");

    if tool_available("swiftc", "--version") {
        let tmp = std::env::temp_dir();
        let sw = tmp.join("lux_kwcase.swift");
        std::fs::write(&sw, &swift).expect("write swift");
        let bin = tmp.join("lux_kwcase_sw");
        let out = Command::new("swiftc")
            .arg(&sw)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success(),
            "keyword-named enum case did not compile as Swift:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run swift bin");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "3");
    }
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

// --- Recursive data --------------------------------------------------------

/// A recursive enum runs interpreted; every target has to spell the indirection
/// its own way — a `Box` in Rust, `indirect` in Swift, the interface encoding in
/// Go. This compiles the same tree on each available backend and checks the
/// answer, so the three stay in step with the interpreter.
#[test]
fn recursive_enum_compiles_and_runs_on_every_backend() {
    let src = r#"
enum Tree {
    leaf
    node(left: Tree, value: int, right: Tree)
}
func sum(t: Tree) -> int {
    return match t {
        leaf => 0
        node(let l, let v, let r) => sum(l) + v + sum(r)
    }
}
let t = Tree.node(left: Tree.node(left: Tree.leaf, value: 1, right: Tree.leaf), value: 2, right: Tree.leaf)
print(sum(t))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let tmp = std::env::temp_dir();

    // Rust: the recursive field is boxed, and reads deref through it.
    let rust = convert::to_rust(&program);
    assert!(
        rust.contains("Box<Tree>"),
        "recursive field should be boxed"
    );
    if tool_available("rustc", "--version") {
        let rs = tmp.join("lux_rec.rs");
        std::fs::write(&rs, &rust).expect("write rust");
        let bin = tmp.join("lux_rec_rs");
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-O")
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run rustc");
        assert!(
            out.status.success(),
            "recursive enum did not compile as Rust:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run rust bin");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "3");
    }

    // Swift: the enum is marked `indirect`, which boxes for us.
    let swift = convert::to_swift(&program);
    assert!(
        swift.contains("indirect enum Tree"),
        "recursive enum should be marked indirect"
    );
    if tool_available("swiftc", "--version") {
        let sw = tmp.join("lux_rec.swift");
        std::fs::write(&sw, &swift).expect("write swift");
        let bin = tmp.join("lux_rec_sw");
        let out = Command::new("swiftc")
            .arg(&sw)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success(),
            "recursive enum did not compile as Swift:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run swift bin");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "3");
    }

    // Go: the interface encoding already carries the indirection.
    if tool_available("go", "version") {
        let go = convert::to_go(&program);
        let dir = tmp.join("lux_go_rec");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
        std::fs::write(dir.join("main.go"), &go).expect("write go");
        let bin = dir.join("bin");
        let out = Command::new("go")
            .arg("build")
            .arg("-o")
            .arg(&bin)
            .current_dir(&dir)
            .env("GOCACHE", tmp.join("lux_go_cache"))
            .output()
            .expect("run go build");
        assert!(
            out.status.success(),
            "recursive enum did not compile as Go:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run go bin");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "3");
    }
}

/// A binding a match arm never reads becomes `_`, so Rust and Swift compile
/// warning-clean (Go already had to drop it — an unused local is a hard error
/// there). This matches a tree and, in the `node` arm, ignores both subtrees to
/// return the value, so `l` and `r` must be elided on every backend.
#[test]
fn unread_match_bindings_are_dropped_on_every_backend() {
    let src = r#"
enum Tree {
    leaf
    node(left: Tree, value: int, right: Tree)
}
func rootvalue(t: Tree) -> int {
    return match t {
        leaf => -1
        node(let l, let v, let r) => v
    }
}
let t = Tree.node(left: Tree.leaf, value: 42, right: Tree.leaf)
print(rootvalue(t))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let tmp = std::env::temp_dir();

    if tool_available("rustc", "--version") {
        let rust = convert::to_rust(&program);
        let rs = tmp.join("lux_unread.rs");
        std::fs::write(&rs, &rust).expect("write rust");
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(tmp.join("lux_unread_rs"))
            .output()
            .expect("run rustc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "an ignored binding should compile warning-clean as Rust, got:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    if tool_available("swiftc", "--version") {
        let swift = convert::to_swift(&program);
        let sw = tmp.join("lux_unread.swift");
        std::fs::write(&sw, &swift).expect("write swift");
        let out = Command::new("swiftc")
            .arg(&sw)
            .arg("-o")
            .arg(tmp.join("lux_unread_sw"))
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "an ignored binding should compile warning-clean as Swift, got:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Assigning through a place — a struct field or an array element — is emitted by
/// every backend, and all three preserve lux's value semantics: `var a = w` copies,
/// so mutating `a` leaves `w` alone. Compiles warning-clean (the `var w` that's
/// only read binds immutably) and prints the same on each.
#[test]
fn field_and_element_assignment_matches_on_every_backend() {
    let src = r#"
struct World {
    doorOpen: bool
    items: [string]
}
var w = World(doorOpen: false, items: ["key"])
w.doorOpen = true
w.items[0] = "lantern"
w.items += "torch"
var copy = w
copy.doorOpen = false
print(w.doorOpen)
print(copy.doorOpen)
print(w.items[0])
print(w.items[1])
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let expected = "true\nfalse\nlantern\ntorch\n";
    let tmp = std::env::temp_dir();

    if tool_available("rustc", "--version") {
        let rust = convert::to_rust(&program);
        let rs = tmp.join("lux_assign.rs");
        std::fs::write(&rs, &rust).expect("write rust");
        let bin = tmp.join("lux_assign_rs");
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run rustc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "place assignment should compile warning-clean as Rust:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run rust bin");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "rust output"
        );
    }
    if tool_available("swiftc", "--version") {
        let swift = convert::to_swift(&program);
        let sw = tmp.join("lux_assign.swift");
        std::fs::write(&sw, &swift).expect("write swift");
        let bin = tmp.join("lux_assign_sw");
        let out = Command::new("swiftc")
            .arg(&sw)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "place assignment should compile warning-clean as Swift:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run swift bin");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "swift output"
        );
    }
    if tool_available("go", "version") {
        let go = convert::to_go(&program);
        let dir = tmp.join("lux_go_assign");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
        std::fs::write(dir.join("main.go"), &go).expect("write go");
        let bin = dir.join("bin");
        let out = Command::new("go")
            .arg("build")
            .arg("-o")
            .arg(&bin)
            .current_dir(&dir)
            .env("GOCACHE", tmp.join("lux_go_cache"))
            .output()
            .expect("run go build");
        assert!(
            out.status.success(),
            "place assignment did not compile as Go:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run go bin");
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected, "go output");
    }
}

/// An enum lowers to a Go interface, which is already nil-able, so `Option<enum>`
/// must emit the bare interface (`nil` = none), not `*Interface` — a pointer to
/// an interface that nothing satisfies. Covers the type, `some`/`none`
/// construction, the match binding, and `Result` over an enum's error path.
#[test]
fn go_option_of_an_enum_compiles() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
enum Room {
    hall
    cellar
}
func exit(r: Room, dir: string) -> Option<Room> {
    return match r {
        hall => match dir { "east" => some(Room.cellar)  _ => none }
        cellar => none
    }
}
func enter(dir: string) -> Result<Room, string> {
    if dir == "in" { return ok(Room.hall) }
    return err("no room")
}
match exit(Room.hall, "east") {
    some(let dest) => match dest { hall => print("hall")  cellar => print("cellar") }
    none => print("nowhere")
}
match enter("in") {
    ok(let r) => print("entered")
    err(let e) => print(e)
}
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    // The Option return is the bare interface, not a pointer to one.
    assert!(
        go.contains("-> Option<Room>") == false && !go.contains("*Room"),
        "Option<enum> should drop the pointer:\n{}",
        go
    );
    let dir = std::env::temp_dir().join("lux_go_opt_enum");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
    std::fs::write(dir.join("main.go"), &go).expect("write go");
    let out = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(dir.join("bin"))
        .current_dir(&dir)
        .env("GOCACHE", std::env::temp_dir().join("lux_go_cache"))
        .output()
        .expect("run go build");
    assert!(
        out.status.success(),
        "Option/Result of an enum did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Rust moves a non-Copy value stored into a container, so a value pushed into an
/// array, put in a struct field, or handed to an enum/`some`/`ok` constructor and
/// then read again must be cloned at the move site — otherwise the later read is a
/// use-after-move. Compiles warning-clean, since the clones are exactly needed.
#[test]
fn rust_value_moved_into_a_container_then_read_compiles() {
    if !tool_available("rustc", "--version") {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let src = r#"
struct Boxed { label: string }
enum Tagged { wrap(a: string, b: string) }
func take(pack: [string], thing: string) -> [string] {
    var p = pack
    p += thing
    print("You take the " + thing + ".")
    return p
}
func viaStruct(s: string) -> string {
    let b = Boxed(label: s)
    print("made " + s)
    return b.label
}
func viaEnum(s: string) -> string {
    let t = Tagged.wrap(a: s, b: s)
    print("wrapped " + s)
    return match t { wrap(let x, let y) => x + y }
}
func viaSome(s: string) -> Option<string> {
    let o = some(s)
    print("wrapped " + s)
    return o
}
print(take([], "key"))
print(viaStruct("a"))
print(viaEnum("b"))
match viaSome("c") { some(let v) => print(v)  none => print("none") }
"#;
    let rust = convert::to_rust(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    let rs = std::env::temp_dir().join("lux_rs_move_container.rs");
    std::fs::write(&rs, &rust).expect("write rust");
    let out = Command::new("rustc")
        .arg(&rs)
        .arg("-o")
        .arg(std::env::temp_dir().join("lux_rs_move_container_bin"))
        .output()
        .expect("run rustc");
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "a value moved into a container then read should compile warning-clean as Rust:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A nested type switch must not reuse the scratch name an enclosing one holds.
/// Here the outer `node` arm binds `v` and the inner match on the left subtree
/// returns it, so the inner switch subject has to step past both the outer
/// binding `v` and the outer subject `v_` — or Go shadows `v` and returns the
/// wrong value (or won't compile).
#[test]
fn go_nested_type_switch_does_not_shadow_an_outer_binding() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
enum Tree {
    leaf
    node(left: Tree, value: int, right: Tree)
}
func leftval(t: Tree) -> int {
    return match t {
        leaf => 0
        node(let l, let v, let r) => match l {
            leaf => v
            node(let ll, let lv, let lr) => lv
        }
    }
}
let t = Tree.node(left: Tree.node(left: Tree.leaf, value: 7, right: Tree.leaf), value: 9, right: Tree.leaf)
print(leftval(t))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    let dir = std::env::temp_dir().join("lux_go_nested_scratch");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("write go.mod");
    std::fs::write(dir.join("main.go"), &go).expect("write go");
    let bin = dir.join("bin");
    let out = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&bin)
        .current_dir(&dir)
        .env("GOCACHE", std::env::temp_dir().join("lux_go_cache"))
        .output()
        .expect("run go build");
    assert!(
        out.status.success(),
        "a nested type switch that shadows an outer binding did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().expect("run go bin");
    // The left child is a leaf-flanked node whose value is 7; leftval returns it.
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "7");
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

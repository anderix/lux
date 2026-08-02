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

/// Build `src` on every backend whose compiler is present, run the result, and
/// assert it prints `expected`. Rust and Swift are held to warning-clean output
/// (empty stderr) as well as a correct answer, since the backends' bar is source
/// a learner can read without a warning about code they didn't write; Go has no
/// warning tier for these cases (an unused local or a type mismatch is a hard
/// error there), so success plus the right output is its bar. `tag` names the
/// temp files, so parallel tests don't collide. This is the spine of a
/// cross-backend behaviour test — a new case is a single call.
fn assert_prints_everywhere(src: &str, tag: &str, expected: &str) {
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let tmp = std::env::temp_dir();

    if tool_available("rustc", "--version") {
        let rust = convert::to_rust(&program);
        let rs = tmp.join(format!("lux_{tag}.rs"));
        std::fs::write(&rs, &rust).expect("write rust");
        let bin = tmp.join(format!("lux_{tag}_rs"));
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run rustc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "{tag}: should compile warning-clean as Rust:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run rust bin");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "{tag}: rust output"
        );
    }
    if tool_available("swiftc", "--version") {
        let swift = convert::to_swift(&program);
        let sw = tmp.join(format!("lux_{tag}.swift"));
        std::fs::write(&sw, &swift).expect("write swift");
        let bin = tmp.join(format!("lux_{tag}_sw"));
        let out = Command::new("swiftc")
            .arg(&sw)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("run swiftc");
        assert!(
            out.status.success() && out.stderr.is_empty(),
            "{tag}: should compile warning-clean as Swift:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run swift bin");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "{tag}: swift output"
        );
    }
    if tool_available("go", "version") {
        let go = convert::to_go(&program);
        let dir = tmp.join(format!("lux_go_{tag}"));
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
            "{tag}: did not compile as Go:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run go bin");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "{tag}: go output"
        );
    }
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

/// #2 made a self-referential enum compile everywhere; mutual recursion is the
/// same need one step out — the shape any AST takes past a single type, an `Expr`
/// that holds a `Fn` that holds an `Expr`. The interpreter and Go carried it
/// already (Go's enum is an interface, already a pointer); Rust wanted a `Box` and
/// Swift an `indirect` that the self-reference-only check never placed. The pass
/// now follows the enum graph, so a field whose type cycles back gets the
/// indirection wherever the cycle runs — and both edges of the two-enum cycle do,
/// while a non-recursive enum elsewhere in the corpus stays plain. (#17)
#[test]
fn mutually_recursive_enums_compile_and_run_on_every_backend() {
    let src = r#"
enum Expr {
    lit(v: int)
    call(f: Fn)
}
enum Fn {
    negate(arg: Expr)
    zero
}
func evalE(e: Expr) -> int {
    return match e {
        lit(let v) => v
        call(let f) => evalF(f)
    }
}
func evalF(f: Fn) -> int {
    return match f {
        negate(let a) => 0 - evalE(a)
        zero => 0
    }
}
print(evalE(Expr.call(f: Fn.negate(arg: Expr.lit(v: 7)))))
print(evalE(Expr.lit(v: 3)))
print(evalF(Fn.zero))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    // Rust boxes each field that re-enters the cycle; Swift marks both enums indirect.
    let rust = convert::to_rust(&program);
    assert!(
        rust.contains("Call(Box<Fn>)") && rust.contains("Negate(Box<Expr>)"),
        "both cycle edges should be boxed, got:\n{rust}"
    );
    let swift = convert::to_swift(&program);
    assert!(
        swift.contains("indirect enum Expr") && swift.contains("indirect enum Fn"),
        "both enums in the cycle should be indirect, got:\n{swift}"
    );
    assert_prints_everywhere(src, "mutualenum", "-7\n3\n0\n");
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
        !go.contains("-> Option<Room>") && !go.contains("*Room"),
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
    // Compiling isn't enough: the risk is a silent miscompile where the arm's
    // `err` binding shadows the emitter's scratch and prints the wrong value. Run
    // it and check the reason string and the ok value both come through.
    let run = Command::new(tmp.join("bin")).output().expect("run go bin");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "seven=7\nboom\n3\n",
        "scratch-shadowing arm should still print the right values"
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

/// A range whose end falls below its start is empty everywhere: the interpreter,
/// Rust, and Go all iterate zero times. Swift's `..<` traps on out-of-order
/// bounds, so the backend emits `stride(from:to:by:)`, which is empty instead of
/// fatal. The bound here goes negative on the last call — exactly a shrinking
/// inner loop over an emptying row — so a wrong emission crashes rather than
/// misprints, which the run step catches.
#[test]
fn a_reversed_range_iterates_zero_times_on_every_backend() {
    let src = r#"
func count(upto: int) -> int {
    var seen = 0
    for i in 0..upto {
        seen += 1
    }
    return seen
}
print(count(3))
print(count(0))
print(count(-1))
"#;
    // Swift stride yields an Int, so a body that reads the variable still works.
    let swift = convert::to_swift(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        swift.contains("stride(from: 0, to: upto, by: 1)"),
        "a Swift range loop should emit stride, got:\n{swift}"
    );
    assert_prints_everywhere(src, "revrange", "3\n0\n0\n");
}

/// A loop variable the body never reads is emitted as `_`, so Rust and Swift come
/// out warning-clean — the same elision the match arms already do. Go is
/// unaffected: its counted loop reads the variable in its own condition, so it
/// never warned. `assert_prints_everywhere` enforces the warning-clean bar, and
/// the structural check pins the elision itself.
#[test]
fn an_unread_loop_variable_is_dropped_on_rust_and_swift() {
    let src = r#"
var seen = 0
for i in 0..3 {
    seen += 1
}
print(seen)
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let rust = convert::to_rust(&program);
    let swift = convert::to_swift(&program);
    assert!(
        rust.contains("for _ in 0..3"),
        "Rust should drop the unread loop variable, got:\n{rust}"
    );
    assert!(
        swift.contains("for _ in stride(from: 0, to: 3, by: 1)"),
        "Swift should drop the unread loop variable, got:\n{swift}"
    );
    assert_prints_everywhere(src, "unusedloop", "3\n");
}

/// A loop that discards its variable — `for _ in 0..n`, the natural "do this n
/// times" — can't lower `_` into the three slots of Go's C-style `for`: `_ := 0`,
/// `_ < n` and `_++` are each invalid. Go gets a throwaway name instead; Rust and
/// Swift emit a plain `_`. It's the very spelling lux's own emitter now writes for
/// an unread loop variable, so the Go backend has to accept it back. (#18)
#[test]
fn a_discarded_loop_variable_compiles_on_every_backend() {
    let src = r#"
var s = ""
for _ in 0..3 {
    s = s + "*"
}
print(s)
"#;
    let go = convert::to_go(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        !go.contains("for _ :="),
        "Go can't read `_` in a C-for's condition; it needs a throwaway name, got:\n{go}"
    );
    assert_prints_everywhere(src, "discardloop", "***\n");
}

/// The array sibling of the loop above: `for _ in xs` discards the element. Go's
/// range form has no counter to reuse, so `for _, _ := range xs` has no new name
/// on the left and won't compile; it lowers to the bare `for range xs`, which
/// iterates without binding. Rust and Swift already emit a plain `_`.
#[test]
fn a_discarded_array_loop_variable_compiles_on_every_backend() {
    let src = r#"
let xs = [10, 20, 30]
var n = 0
for _ in xs {
    n = n + 1
}
print(n)
"#;
    let go = convert::to_go(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        go.contains("for range xs") && !go.contains("for _, _ := range"),
        "Go should iterate without binding, got:\n{go}"
    );
    assert_prints_everywhere(src, "discardarray", "3\n");
}

/// `none` names the empty `Option`, but a program that binds it as an ordinary
/// variable means the local — the same shadowing every other built-in name already
/// allows. The declaration always respected the scope; the use site used to reach
/// the built-in first and emit `None`/`nil`, compiling nowhere. Both an int (the
/// common case) and a non-Copy value (which also needs lux's value-semantics copy)
/// are covered. (#19)
#[test]
fn a_variable_named_none_shadows_the_builtin_on_every_backend() {
    assert_prints_everywhere("let none = 5\nprint(none + 1)\n", "noneint", "6\n");
    let src = r#"
var none = [1, 2, 3]
let copy = none
none += 4
print(copy)
print(none)
"#;
    assert_prints_everywhere(src, "nonearr", "[1, 2, 3]\n[1, 2, 3, 4]\n");
}

/// Reading one cell out of a grid of strings and handing it back is the accessor
/// every grid program writes. Rust can't move a `String` out of a `Vec` index
/// (E0507), so a returned index of a non-Copy element is cloned — the same copy a
/// binding or a call argument already gets. Over `[[int]]` it compiled anyway,
/// because an int element is Copy, which is what made it easy to miss. The
/// behavioural check is the real guard: without the clone, Rust won't compile. (#20)
#[test]
fn returning_an_indexed_string_compiles_on_every_backend() {
    let src = r#"
func at(g: [[string]], r: int, c: int) -> string {
    let row = g[r]
    return row[c]
}
let board = [["a", "b"], ["c", "d"]]
print(at(board, 1, 0))
"#;
    let rust = convert::to_rust(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        rust.contains("return row[(c) as usize].clone();"),
        "Rust should clone an indexed String returned by value, got:\n{rust}"
    );
    assert_prints_everywhere(src, "indexret", "c\n");
}

/// Not a wrong answer — a cost, and so the kind a diff never catches. lux evaluates
/// a range's bound once, but Go's C-for re-checks its condition every pass, so a
/// bound that's a call landed inside the loop and re-ran each iteration — quietly
/// turning an O(n²) grid walk cubic when the call deep-copies the grid. The bound
/// is hoisted to a variable evaluated once; only a literal, which can't change,
/// stays in the condition. (#21)
#[test]
fn go_hoists_a_computed_range_bound_out_of_the_loop() {
    let src = r#"
func rows(m: [[int]]) -> int {
    return length(m)
}
func total(m: [[int]]) -> int {
    var sum = 0
    for i in 0..rows(m) {
        let row = m[i]
        for v in row {
            sum += v
        }
    }
    return sum
}
print(total([[1, 2], [3, 4]]))
"#;
    let go = convert::to_go(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    // The bound is computed once, before the loop...
    assert!(
        go.contains("__end0 := rows("),
        "Go should hoist the computed range bound, got:\n{go}"
    );
    // ...and the loop's condition just reads it, never re-calling rows().
    assert!(
        go.contains("; i < __end0;"),
        "the loop condition should read the hoisted bound, got:\n{go}"
    );
    assert_prints_everywhere(src, "hoistbound", "10\n");
}

/// Printing an array of scalars reads the same on every backend — the common
/// `print(xs)` line after a sort. Go used to defer to `fmt`, which prints a slice
/// space-separated (`[1 2 3]`); it now renders lux's way, with commas.
#[test]
fn an_int_array_prints_with_commas_on_every_backend() {
    assert_prints_everywhere("print([3, 1, 2])\n", "intarray", "[3, 1, 2]\n");
}

/// Go's array rendering matches the interpreter down the line: commas, strings
/// shown as their bare text (the way `print` shows a scalar string), and nesting
/// rendered the same all the way down — not `fmt`'s space-separated default. The
/// expected strings here are exactly what `lux run` prints.
#[test]
fn go_renders_arrays_the_way_lux_does() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
print([1, 2, 3])
print(["a", "b"])
print([[1, 2], [3]])
let xs = [1, 2, 3]
print("xs is", xs)
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    let dir = std::env::temp_dir().join("lux_go_showlist");
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
        "array printing did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().expect("run go bin");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "[1, 2, 3]\n[a, b]\n[[1, 2], [3]]\nxs is [1, 2, 3]\n",
        "go array rendering should match lux run"
    );
}

/// Printing a struct or an enum in Go reads the way lux renders it —
/// `P(x: 1, y: 2)`, `Shape.circle(radius: 5)`, `Shape.dot` — not `fmt`'s `{1 2}`,
/// `{5}`, `{}`. The generated `luxShow` recurses, so a struct that holds an array
/// and an enum renders all the way down. Expected strings are exactly `lux run`.
#[test]
fn go_renders_structs_and_enums_the_way_lux_does() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let src = r#"
struct P { x: int  y: int }
enum Shape { circle(radius: int)  dot }
struct Box { items: [int]  shape: Shape }
print(P(x: 1, y: 2))
print(Shape.circle(radius: 5))
print(Shape.dot)
print([P(x: 1, y: 2), P(x: 3, y: 4)])
print(Box(items: [1, 2], shape: Shape.dot))
"#;
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    let dir = std::env::temp_dir().join("lux_go_luxshow");
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
        "struct/enum printing did not compile as Go:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().expect("run go bin");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "P(x: 1, y: 2)\nShape.circle(radius: 5)\nShape.dot\n[P(x: 1, y: 2), P(x: 3, y: 4)]\nBox(items: [1, 2], shape: Shape.dot)\n",
        "go struct/enum rendering should match lux run"
    );
}

/// An empty array literal carries no element to infer a type from, so Go needs
/// the type from the position it lands in: a parameter (`total([])`) or a return
/// (`empty => []` where the function returns `[int]`). 0.14.0 typed an annotated
/// binding's empty array; these are the two positions it didn't reach. The other
/// backends never had the problem — this pins that all four now agree.
#[test]
fn an_empty_array_literal_is_typed_at_arg_and_return() {
    let src = r#"
enum Tree { empty  node(v: int) }
func total(xs: [int]) -> int {
    var n = 0
    for x in xs { n += x }
    return n
}
func inorder(t: Tree) -> [int] {
    return match t {
        empty => []
        node(let v) => [v]
    }
}
print(total([]))
print(total([1, 2, 3]))
print(inorder(Tree.empty))
print(inorder(Tree.node(v: 7)))
"#;
    assert_prints_everywhere(src, "emptyarg", "0\n6\n[]\n[7]\n");
}

/// Forwarding a failure out of a `match` arm — `err(why) => err(why)` — is the
/// shape the "handle a Result where it's produced" rule pushes every program
/// toward, so it must be the best-supported one. In Go it emitted a single value
/// where the `(value, error)` lowering wants two; a returning arm now goes
/// through the same return path a top-level `return err(why)` does. The failure
/// here travels up through two levels of nested match, the evaluator pattern.
#[test]
fn an_error_forwarded_from_a_match_arm_lowers_on_every_backend() {
    let src = r#"
func half(n: int) -> Result<int, string> {
    if n % 2 != 0 { return err(string(n) + " is odd") }
    return ok(n / 2)
}
func quarter(n: int) -> Result<int, string> {
    return match half(n) {
        err(let why) => err(why)
        ok(let h)    => half(h)
    }
}
match quarter(8) { ok(let v) => print("ok", v)  err(let e) => print("err", e) }
match quarter(5) { ok(let v) => print("ok", v)  err(let e) => print("err", e) }
match quarter(6) { ok(let v) => print("ok", v)  err(let e) => print("err", e) }
"#;
    assert_prints_everywhere(src, "errforward", "ok 2\nerr 5 is odd\nerr 3 is odd\n");
}

/// A `var` initialised from one enum case must be typed as the enum, not that
/// case. In Go the enum is an interface; `:=` would infer the concrete case
/// struct, so reassigning a different case wouldn't compile — the ordinary way to
/// accumulate an enum value, `var out = List.nil` then `out = push(out, x)` in a
/// loop. The backend now pins the interface type. Rust and Swift never had the
/// problem; this holds all three to the same behaviour.
#[test]
fn a_var_of_an_enum_case_takes_the_enum_type_on_every_backend() {
    let src = r#"
enum Colour { red  blue }
enum List { nil  cons(head: int, tail: List) }
func name(c: Colour) -> string {
    return match c { red => "red"  blue => "blue" }
}
func push(l: List, x: int) -> List {
    return List.cons(head: x, tail: l)
}
func size(l: List) -> int {
    return match l { nil => 0  cons(let h, let t) => 1 + size(t) }
}
var c = Colour.red
print(name(c))
c = Colour.blue
print(name(c))
var out = List.nil
for x in [1, 2, 3] { out = push(out, x) }
print(size(out))
"#;
    // Go pins the interface type on the binding, not the case struct.
    let go = convert::to_go(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        go.contains("var c Colour =") && go.contains("var out List ="),
        "a Go enum binding should be typed as the enum interface, got:\n{go}"
    );
    assert_prints_everywhere(src, "enumvar", "red\nblue\n3\n");
}

/// lux arrays are values: copying one and mutating the copy leaves the original
/// alone. A Go slice is a reference, so `var xs = input` aliased the caller's row
/// and an in-place sort reached back through it — the flex corpus's bubble sort
/// mutating a source it promised to leave untouched. The Go backend now copies an
/// array bound from a place; Rust already cloned at its bind sites, Swift's arrays
/// are values. This holds all three to lux's semantics: the source row survives a
/// sort, and a `let` copy doesn't see a later write to what it was copied from.
#[test]
fn an_array_copy_is_independent_on_every_backend() {
    let src = r#"
func swapFirstTwo(input: [int]) -> [int] {
    var xs = input
    let hold = xs[0]
    xs[0] = xs[1]
    xs[1] = hold
    return xs
}
let row = [2, 1]
print(swapFirstTwo(row))
print(row)
var a = [1, 2, 3]
let b = a
a[0] = 99
print(b)
print(a)
"#;
    assert_prints_everywhere(src, "arraycopy", "[1, 2]\n[2, 1]\n[1, 2, 3]\n[99, 2, 3]\n");
}

/// Value semantics reach through a struct: a struct that holds an array, copied
/// and then its array mutated, leaves the original untouched — and so does a
/// board of grids two levels deep, and a value handed to a function and back. A Go
/// struct copies by value but shares its slice fields underneath, so the backend
/// deep-copies a slice-bearing value wherever it flows into a new place — the same
/// points Rust clones at. Rust and Swift already had this; all three now agree.
#[test]
fn a_struct_holding_an_array_copies_deeply_on_every_backend() {
    let src = r#"
struct Grid { cells: [int] }
struct Board { grids: [Grid] }
func passthrough(g: Grid) -> Grid { return g }
var g = Grid(cells: [1, 2])
var h = passthrough(g)
h.cells[0] = 99
print(g.cells)
print(h.cells)
var board = Board(grids: [Grid(cells: [1]), Grid(cells: [2])])
var b2 = board
b2.grids[0].cells[0] = 77
print(board.grids[0].cells)
print(b2.grids[0].cells)
"#;
    assert_prints_everywhere(src, "structcopy", "[1, 2]\n[99, 2]\n[1]\n[77]\n");
}

/// Printing a struct, an enum case (with or without a payload), an array of
/// structs, and a recursive tree reads the same on every backend and matches the
/// interpreter — `P(x: 1, y: 2)`, `Shape.circle(radius: 5)`, `Shape.dot`,
/// `Tree.node(left: Tree.leaf, …)`. Each backend renders through its own `luxShow`
/// (a Go type switch, a Rust trait, a Swift protocol); this holds all four to the
/// same reading. Rust and Swift stay warning-clean.
#[test]
fn compound_values_print_the_same_on_every_backend() {
    let src = r#"
struct P { x: int  y: int }
enum Shape { circle(radius: int)  dot }
struct Bag { items: [int]  shape: Shape }
enum Tree { leaf  node(left: Tree, value: int, right: Tree) }
print(P(x: 1, y: 2))
print(Shape.circle(radius: 5))
print(Shape.dot)
print([P(x: 1, y: 2), P(x: 3, y: 4)])
print(Bag(items: [1, 2], shape: Shape.dot))
print(Tree.node(left: Tree.leaf, value: 7, right: Tree.node(left: Tree.leaf, value: 9, right: Tree.leaf)))
"#;
    let expected = "P(x: 1, y: 2)\n\
        Shape.circle(radius: 5)\n\
        Shape.dot\n\
        [P(x: 1, y: 2), P(x: 3, y: 4)]\n\
        Bag(items: [1, 2], shape: Shape.dot)\n\
        Tree.node(left: Tree.leaf, value: 7, right: Tree.node(left: Tree.leaf, value: 9, right: Tree.leaf))\n";
    assert_prints_everywhere(src, "compound", expected);
}

/// Printing an `Option` reads `some(v)` / `none` on every backend. A typed one —
/// a variable or a call result — carries its element type; a bare `some`/`none`
/// literal in print position does not, and used to be a compile error in Rust and
/// Swift and `<nil>` in Go. The backend now pins the type at the print site.
#[test]
fn options_print_the_same_on_every_backend() {
    let src = r#"
func find(xs: [int], t: int) -> Option<int> {
    for x in xs { if x == t { return some(x) } }
    return none
}
print(find([1, 2, 3], 2))
print(find([1, 2, 3], 9))
print(some(7))
print(some("north"))
print(none)
"#;
    assert_prints_everywhere(
        src,
        "options",
        "some(2)\nnone\nsome(7)\nsome(north)\nnone\n",
    );
}

/// Matching one enum case and defaulting the rest — `match it { potion(let a) => …
/// _ => … }` — reads the same on every backend. Go used to drop the `_` arm from
/// its type switch and `panic("unreachable")` on every case the match didn't name;
/// the wildcard now lowers to the switch's `default`. Everyday code the corpus
/// missed by always matching exhaustively.
#[test]
fn an_enum_match_with_a_wildcard_covers_the_rest_on_every_backend() {
    let src = r#"
enum E { a  b  c }
func name(e: E) -> string {
    return match e {
        a => "a"
        _ => "other"
    }
}
print(name(E.a))
print(name(E.b))
print(name(E.c))
"#;
    assert_prints_everywhere(src, "wildcard", "a\nother\nother\n");
}

/// A `_` arm stands in for the unnamed case of an `Option` or `Result` match too —
/// `match o { some(let v) => …  _ => … }`, `match r { ok(let v) => …  _ => … }`.
/// Go built each of these by finding arms by name (`some`/`none`, `ok`/`err`) and
/// left the else branch empty when the other side was a wildcard, so a returning
/// match compiled to a missing return. The wildcard now fills the missing side.
#[test]
fn a_wildcard_covers_option_and_result_matches_on_every_backend() {
    let src = r#"
func opt(o: Option<int>) -> string {
    return match o { some(let v) => "some"  _ => "none" }
}
func res(n: int) -> string {
    return match half(n) { ok(let v) => "ok"  _ => "err" }
}
func half(n: int) -> Result<int, string> {
    if n % 2 == 0 { return ok(n / 2) }
    return err("odd")
}
print(opt(some(1)))
print(opt(none))
print(res(4))
print(res(3))
"#;
    assert_prints_everywhere(src, "optreswild", "some\nnone\nok\nerr\n");
}

/// Accumulating an `Option` across a loop — `var result: Option<int> = none` then
/// `result = match … { some(let v) => some(v)  none => next }` — works on every
/// backend. Two edges met here: a bare `none` with a type annotation (Go needs the
/// declared type, not an untyped `nil`), and a match used as a value whose first
/// arm reads a binding, so its type has to come from a sibling arm that's concrete.
#[test]
fn an_option_accumulated_across_a_loop_works_on_every_backend() {
    let src = r#"
enum Item { coin(value: int)  other }
func coinValue(it: Item) -> Option<int> {
    return match it { coin(let v) => some(v)  _ => none }
}
func firstCoin(bag: [Item]) -> Option<int> {
    var result: Option<int> = none
    for it in bag {
        result = match result {
            some(let v) => some(v)
            none => coinValue(it)
        }
    }
    return result
}
print(firstCoin([Item.other, Item.coin(value: 7), Item.coin(value: 9)]))
print(firstCoin([Item.other]))
"#;
    assert_prints_everywhere(src, "optaccum", "some(7)\nnone\n");
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

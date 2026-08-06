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
        // Compile the way `lux build` does — overflow checks off, so integer
        // arithmetic wraps like the interpreter and the other targets (#35).
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-C")
            .arg("overflow-checks=off")
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

/// A top-level `func main` is the entry point: lux runs it, and each backend maps it
/// straight onto its own `main` (Swift, which runs top-level code like lux, gets one
/// `main()` call to start it) with no wrapper. The interpreter and all three targets
/// must produce the same output — the graduation "hello world" understood in full.
#[test]
fn a_top_level_main_runs_itself_everywhere() {
    let src = "func greet(name: string) {\n    print(\"hello,\", name)\n}\n\nfunc main() {\n    greet(\"world\")\n    print(\"from main\")\n}\n";
    let expected = "hello, world\nfrom main\n";

    // The interpreter auto-runs main with no explicit call.
    let path = std::env::temp_dir().join(format!("lux_mainrun_{}.lux", std::process::id()));
    std::fs::write(&path, src).expect("write lux");
    let run = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .output()
        .expect("run lux");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected,
        "interpreter should auto-run main"
    );

    // And every backend agrees, from its own idiomatic `main`.
    assert_prints_everywhere(src, "mainrun", expected);
}

/// A failed `readFile` and a failed `run` name what was attempted, in lux's own
/// shape `could not read/run <path>: <reason>`, on every backend — the path
/// especially, so a program that touches several files says which one failed
/// (#43). And a missing program takes the `err` arm, not a silent `ok` with a
/// wrapper's status (#48). The reason text differs by platform, so this checks the
/// shape and the branch, not an exact string.
#[test]
fn io_errors_name_the_path_on_every_backend() {
    let src = "print(match readFile(\"no_such_file_here.txt\") {\n    ok(let t) => \"ok\"\n    err(let e) => e\n})\nprint(match run(\"no_such_prog_here\", []) {\n    ok(let o) => \"OK-\" + string(o.status)\n    err(let w) => w\n})\n";
    for backend in ["rust", "go", "swift"] {
        if let Some(out) = build_run("ioerr", backend, src) {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                stdout.contains("could not read no_such_file_here.txt"),
                "{backend}: readFile error should name the path, got:\n{stdout}"
            );
            assert!(
                stdout.contains("could not run no_such_prog_here") && !stdout.contains("OK-"),
                "{backend}: a missing program should take the err arm and name it, got:\n{stdout}"
            );
        }
    }
}

/// Floats print positionally at every magnitude, never in exponent notation —
/// the only form lux can read back, since it has no exponent literal (#47) — and
/// `inf`/`-inf`/`NaN` render the same three ways everywhere (#52). The interpreter
/// is the reference, and all three targets now match it exactly.
#[test]
fn floats_render_the_same_way_everywhere() {
    assert_prints_everywhere(
        "print(0.00001)\nprint(0.0000001)\nprint(1000000.0)\nprint(123456789.5)\n",
        "fltpos",
        "0.00001\n0.0000001\n1000000.0\n123456789.5\n",
    );
    // A non-finite value normalises the same way on each leg, and int() saturates
    // rather than trapping (Swift) or going undefined (Go).
    let src = "func over(a: float, b: float) -> float {\n    return a / b\n}\nprint(over(1.0, 0.0), over(-1.0, 0.0), over(0.0, 0.0))\nprint(int(over(1.0, 0.0)))\n";
    assert_prints_everywhere(src, "fltinf", "inf -inf NaN\n9223372036854775807\n");
}

/// The `help:` trail — the line that names the rule and its `lux learn` topic — is
/// a constant string, so it survives to the compiled targets rather than being
/// dropped with the source location a binary can't carry (#40). It's the line that
/// does the teaching, so keeping it is what keeps a built program's errors trails
/// rather than pointers.
#[test]
fn the_help_trail_survives_to_the_compiled_targets() {
    let src = "let xs = [1, 2, 3]\nprint(xs[9])\n";
    for backend in ["rust", "go", "swift"] {
        if let Some(out) = build_run("helptrail", backend, src) {
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("help: `lux learn arrays`"),
                "{backend}: the help trail should survive to the binary, got:\n{stderr}"
            );
        }
    }
}

/// A named loop variable the body never reads compiles on Go — it drops to `for
/// range xs`, since Go rejects a `for _, v` whose `v` is unused, and naming the
/// item you walk over is the ordinary spelling even when you only count (#44).
#[test]
fn go_compiles_a_named_but_unused_loop_variable() {
    let src = "let xs = [1, 2, 3]\nvar c = 0\nfor item in xs {\n    c += 1\n}\nprint(c)\n";
    if let Some(out) = build_run("unusedloop", "go", src) {
        assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
    }
}

/// A ragged grid written as a literal — an empty inner array beside a non-empty one
/// — keeps its element type on every backend, rather than degrading to `[]any` on
/// Go and failing to compile (#45).
#[test]
fn a_nested_empty_array_literal_compiles_everywhere() {
    assert_prints_everywhere(
        "let week: [[int]] = [[], [9, 10], [14], [], [11]]\nprint(week)\n",
        "ragged",
        "[[], [9, 10], [14], [], [11]]\n",
    );
}

/// `parseInt`/`parseFloat` accept surrounding whitespace on every backend, the way
/// the interpreter's `.trim()` does — a number read from a column-aligned file or a
/// paste has spaces around it, and Swift used to return `none` for it (#41).
#[test]
fn parse_trims_surrounding_whitespace_everywhere() {
    let src = "print(match parseInt(\" 42\") { some(let n) => string(n)  none => \"none\" })\nprint(match parseFloat(\"2.5 \") { some(let f) => string(f)  none => \"none\" })\n";
    for backend in ["rust", "go", "swift"] {
        if let Some(out) = build_run("parsetrim", backend, src) {
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "42\n2.5\n",
                "{backend}: parse should trim surrounding whitespace"
            );
        }
    }
}

/// When stdout and stderr are merged — a pipe, `2>&1`, `tee` — a warning stays with
/// the output it follows rather than jumping to the top. Swift block-buffers stdout
/// off a terminal and writes stderr through, so without a flush the `eprint` lines
/// all landed first (#51); the fix flushes stdout ahead of each one.
#[test]
fn swift_interleaves_stdout_and_stderr_when_merged() {
    if !tool_available("swiftc", "--version") {
        eprintln!("skipping: swiftc not on PATH");
        return;
    }
    let src = "print(\"out 1\")\neprint(\"err 1\")\nprint(\"out 2\")\neprint(\"err 2\")\n";
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let tmp = std::env::temp_dir();
    let sw = tmp.join("lux_streams.swift");
    std::fs::write(&sw, convert::to_swift(&program)).expect("write swift");
    let bin = tmp.join("lux_streams_sw");
    let c = Command::new("swiftc")
        .arg(&sw)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("swiftc");
    assert!(
        c.status.success(),
        "swift compile:\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    // Merge the streams the way a pipe does, and check program order is preserved.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{} 2>&1", bin.display()))
        .output()
        .expect("run merged");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "out 1\nerr 1\nout 2\nerr 2\n",
        "a warning should stay with the line it follows when the streams are merged"
    );
}

/// `string()` of a compound renders it the same way `print` does — through
/// `luxShow` — rather than the host's default: Rust and Swift wouldn't build a
/// `string(struct)`, and Go gave `{1 2}`, a different string that then flows on into
/// whatever the program saves or compares (#54). The scalar cases are unchanged.
#[test]
fn string_of_a_compound_matches_print_everywhere() {
    let src = "struct P {\n    x: int\n    y: int\n}\nenum E {\n    a\n    b(v: int)\n}\nprint(string(P(x: 1, y: 2)))\nprint(string(E.a))\nprint(string(E.b(v: 7)))\nprint(string([1, 2, 3]))\n";
    assert_prints_everywhere(
        src,
        "strcompound",
        "P(x: 1, y: 2)\nE.a\nE.b(v: 7)\n[1, 2, 3]\n",
    );
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
        rust.contains("return (*lux_index(&row, c)).clone();"),
        "Rust should clone the bounds-checked indexed String returned by value, got:\n{rust}"
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

/// Value semantics reach into an array *literal* too: a place stored as an
/// element — a struct that holds an array (A), or a bare array (D) — is deep-copied
/// as the literal is built, so mutating the source afterwards can't reach into the
/// stored copy. Go used to copy on the struct-field path (B, C) but not the
/// array-literal path, and silently shared the inner slice (#61). All four legs
/// must print 1 for every case; a leak prints 42.
#[test]
fn a_place_stored_in_an_array_literal_is_copied_on_every_backend() {
    let src = r#"
struct Bag { items: [int] }
struct Holder { bag: Bag }
var s1 = Bag(items: [1])
var arr = [s1]
s1.items[0] = 42
print(arr[0].items[0])
var s2 = Bag(items: [1])
var h = Holder(bag: s2)
s2.items[0] = 42
print(h.bag.items[0])
var raw = [1]
var h2 = Bag(items: raw)
raw[0] = 42
print(h2.items[0])
var raw2 = [1]
var nested = [raw2]
raw2[0] = 42
print(nested[0][0])
"#;
    assert_all_four(src, "arraylitcopy", "1\n1\n1\n1\n");
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

/// `==`/`!=` on a compound value answers the same on every backend and matches the
/// interpreter: two arrays by contents, a struct that holds one, two cases of an
/// enum, and an `Option` of an array. Go's own `==` won't build for a slice or a
/// struct holding one, and it compared an `Option` (a pointer) by address — so
/// `some(a) == some(a)` silently came back false; both are routed through a
/// generated `luxEqual` now (#58). The `some(p) == some(q)` line is the one that
/// used to compile and lie.
#[test]
fn compound_equality_matches_on_every_backend() {
    let src = r#"
enum E { a(x: int)  b }
struct B { xs: [int] }
print([1, 2] == [1, 2])
print([1, 2] == [1, 3])
print([1, 2] != [1, 3])
print(B(xs: [1]) == B(xs: [1]))
print(B(xs: [1]) == B(xs: [2]))
print(E.a(x: 1) == E.a(x: 1))
print(E.a(x: 1) == E.b)
print(E.a(x: 1) != E.b)
let p = [1, 2]
let q = [1, 2]
print(some(p) == some(q))
print(some(p) == some(p))
"#;
    assert_all_four(
        src,
        "compoundeq",
        "true\nfalse\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\ntrue\n",
    );
}

/// Comparing an `Option` against the bare `none` literal still works after that
/// routing: `none` emits as an untyped nil that `luxEqual` can't compare, so a
/// comparison against the literal stays native `== nil` on Go, while a comparison
/// between two `Option` *variables* goes through `luxEqual`. Both must agree with
/// the interpreter (#58).
#[test]
fn comparing_an_option_against_none_still_works() {
    let src = r#"
let m: Option<int> = some(5)
let n: Option<int> = none
print(m == none)
print(n == none)
print(m == n)
print(m != none)
"#;
    assert_all_four(src, "opteqnone", "false\ntrue\nfalse\ntrue\n");
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

/// Binding a bare `some(x)` without an annotation — the ordinary way a learner
/// first reaches for an optional — reads the same on every backend. The element
/// type is known from the argument, but Swift emitted `let a = .some(5)`, which
/// it can't resolve without a contextual type, so it alone refused a program the
/// other three ran. The Swift backend now carries the inferred element type onto
/// the binding (`let a: Int? = .some(5)`), the way it already did for a source
/// annotation (#67). The unannotated `none` case stays its own rule: its element
/// type really is open, so it still asks the learner to say what it holds (#66).
#[test]
fn a_bare_some_binding_prints_the_same_on_every_backend() {
    let src = r#"
let a = some(5)
let b = some("north")
print(a)
print(b)
"#;
    assert_prints_everywhere(src, "baresome", "some(5)\nsome(north)\n");
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

/// A struct named and then listed in an array literal — `let rect = [origin, …]`
/// — is cloned into the array, so a later `print(origin)` still reads it. Rust
/// moves a non-`Copy` value into a `vec!` element, so without the clone every
/// move site already places, naming the corners of a shape and then reading one
/// back wouldn't compile (#30). The array literal was the one move position the
/// 0.15.0 clone set missed; the other backends copy by nature.
#[test]
fn a_named_struct_in_an_array_literal_is_cloned_on_every_backend() {
    let src = r#"
struct Point { x: int  y: int }
let origin = Point(x: 0, y: 0)
let far = Point(x: 4, y: 3)
let rect = [origin, Point(x: 4, y: 0), far, Point(x: 0, y: 3)]
print(origin)
print(far)
print(rect)
"#;
    assert_prints_everywhere(
        src,
        "arraylitclone",
        "Point(x: 0, y: 0)\nPoint(x: 4, y: 3)\n[Point(x: 0, y: 0), Point(x: 4, y: 0), Point(x: 4, y: 3), Point(x: 0, y: 3)]\n",
    );
}

/// Adding to an array while a loop walks it — the natural first version of a
/// queue or a flood fill — is a snapshot everywhere: the loop sees the row as it
/// was when it began. The interpreter, Swift's copy-on-write, and Go's range over
/// a snapshot all give that; Rust's `.iter().cloned()` held the array borrowed
/// for the whole loop, so the append wouldn't compile. Iterating a clone releases
/// the borrow and keeps the same snapshot semantics (#36).
#[test]
fn mutating_an_array_while_looping_over_it_works_on_every_backend() {
    let src = r#"
var xs = [1, 2, 3]
for x in xs {
    if x == 1 {
        xs += 99
    }
    print("saw", x)
}
print("after", xs)
"#;
    assert_prints_everywhere(
        src,
        "mutwhileloop",
        "saw 1\nsaw 2\nsaw 3\nafter [1, 2, 3, 99]\n",
    );
}

/// A whole float keeps its decimal point on every backend — `88.0`, not `88` — so
/// the int/float distinction lux enforces at every arithmetic survives into the
/// output. Go's `fmt` drops it, printing a `float` holding 88.0 identically to an
/// `int` holding 88; a float now renders through a helper that matches lux, in a
/// scalar, inside an array, and through `string()` (#31).
#[test]
fn a_whole_float_prints_with_its_decimal_on_every_backend() {
    let src = r#"
let a = 88.0
print(a)
print(1.5)
print([1.0, 2.5, 3.0])
print(string(2.0))
"#;
    assert_prints_everywhere(src, "wholefloat", "88.0\n1.5\n[1.0, 2.5, 3.0]\n2.0\n");
}

/// `int()` of a float literal is the most natural way to show what truncation
/// does, and it must compile. Go refuses converting a constant float to an int —
/// `int(3.9)` loses precision at compile time — so a float goes through a helper
/// that reaches the conversion as a runtime value (#32). A float variable and a
/// negative literal take the same path.
#[test]
fn int_of_a_float_literal_compiles_on_every_backend() {
    let src = r#"
let x = 3.9
print(int(x))
print(int(3.9))
print(int(-3.9))
"#;
    assert_prints_everywhere(src, "intoffloat", "3\n3\n-3\n");
}

/// An `Option` binding that supplies its own type — `let a: Option<int> = some(5)`
/// — keeps that type on every backend. Swift's `.some(5)` needs a contextual type
/// to resolve, and the emitter dropped the annotation the program had already
/// written, leaving nothing to infer from (#33). A `var`, a present value, and the
/// empty `none` all land typed.
#[test]
fn an_annotated_option_binding_keeps_its_type_on_every_backend() {
    let src = r#"
let a: Option<int> = some(5)
print(a)
var b: Option<int> = some(7)
print(b)
let c: Option<int> = none
print(c)
"#;
    assert_prints_everywhere(src, "annotsome", "some(5)\nsome(7)\nnone\n");
}

/// A read-only array parameter is passed without a per-call copy — the accessor
/// pattern (`cols(m)` asking a grid its width every pass of a loop) that ran an
/// order slower than it needed to (#28). A scalar-returning function can't leak
/// its parameter's backing, so Go passes the slice as-is and Rust borrows it.
/// Correctness has to survive the change: a callee that copies the parameter into
/// a local and mutates the local must leave the caller's own array untouched,
/// since lux forbids writing through a parameter and the copy happens inside.
#[test]
fn a_read_only_array_parameter_is_passed_without_a_copy() {
    let src = r#"
func width(m: [[int]]) -> int {
    return length(m[0])
}
func sumWith(xs: [int]) -> int {
    var ys = xs
    ys[0] = 999
    var s = 0
    for y in ys { s += y }
    return s
}
var grid = [[1, 2, 3], [4, 5, 6]]
var row = [10, 20, 30]
print(width(grid))
print(sumWith(row))
print(grid)
print(row)
"#;
    // sumWith mutates its own copy of xs, so row is untouched: it sums 999+20+30.
    assert_prints_everywhere(
        src,
        "roparam",
        "3\n1049\n[[1, 2, 3], [4, 5, 6]]\n[10, 20, 30]\n",
    );

    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let go = convert::to_go(&program);
    assert!(
        go.contains("width(grid)") && go.contains("sumWith(row)"),
        "Go should pass a read-only array argument as-is, not deep-copied:\n{go}"
    );
    let rust = convert::to_rust(&program);
    assert!(
        rust.contains("fn width(m: &Vec<Vec<i64>>)") && rust.contains("width(&grid)"),
        "Rust should borrow a read-only array parameter:\n{rust}"
    );
}

/// Compile `src` on one backend and run the result, handing back the process
/// output — used where the program is meant to fail at runtime, so the streams and
/// exit status matter rather than a clean print. Returns `None` when the toolchain
/// isn't present. Panics if the program doesn't compile, since that's a backend
/// bug, not the runtime behaviour under test.
fn build_run(tag: &str, backend: &str, src: &str) -> Option<std::process::Output> {
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let tmp = std::env::temp_dir();
    match backend {
        "rust" if tool_available("rustc", "--version") => {
            let rs = tmp.join(format!("lux_{tag}.rs"));
            std::fs::write(&rs, convert::to_rust(&program)).expect("write rust");
            let bin = tmp.join(format!("lux_{tag}_rs"));
            let c = Command::new("rustc")
                .arg(&rs)
                .arg("-C")
                .arg("overflow-checks=off")
                .arg("-o")
                .arg(&bin)
                .output()
                .expect("rustc");
            assert!(
                c.status.success(),
                "{tag}: rust compile:\n{}",
                String::from_utf8_lossy(&c.stderr)
            );
            Some(Command::new(&bin).output().expect("run"))
        }
        "swift" if tool_available("swiftc", "--version") => {
            let sw = tmp.join(format!("lux_{tag}.swift"));
            std::fs::write(&sw, convert::to_swift(&program)).expect("write swift");
            let bin = tmp.join(format!("lux_{tag}_sw"));
            let c = Command::new("swiftc")
                .arg(&sw)
                .arg("-o")
                .arg(&bin)
                .output()
                .expect("swiftc");
            assert!(
                c.status.success(),
                "{tag}: swift compile:\n{}",
                String::from_utf8_lossy(&c.stderr)
            );
            Some(Command::new(&bin).output().expect("run"))
        }
        "go" if tool_available("go", "version") => {
            let dir = tmp.join(format!("lux_{tag}_go"));
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join("go.mod"), "module luxtest\n\ngo 1.21\n").expect("go.mod");
            std::fs::write(dir.join("main.go"), convert::to_go(&program)).expect("write go");
            let bin = dir.join("bin");
            let c = Command::new("go")
                .arg("build")
                .arg("-o")
                .arg(&bin)
                .current_dir(&dir)
                .env("GOCACHE", tmp.join("lux_go_cache"))
                .output()
                .expect("go build");
            assert!(
                c.status.success(),
                "{tag}: go compile:\n{}",
                String::from_utf8_lossy(&c.stderr)
            );
            Some(Command::new(&bin).output().expect("run"))
        }
        _ => None,
    }
}

/// Dividing by zero is one of the two runtime mistakes a beginner actually makes,
/// and after `lux build` it used to surface as the host runtime showing through —
/// a Rust panic trace, a Go goroutine dump, a Swift register dump. Each backend now
/// guards integer `/` and `%`, so a zero divisor reports `division by zero` and
/// exits, the way the interpreter does, and the output printed before it survives
/// (#34). The three targets already detected the zero; only the message was wrong.
#[test]
fn dividing_by_zero_reports_a_lux_error_on_every_backend() {
    let src = "func divide(a: int, b: int) -> int {\n    return a / b\n}\nprint(\"before\")\nprint(divide(10, 0))\n";
    for backend in ["rust", "swift", "go"] {
        let Some(out) = build_run("divzero", backend, src) else {
            continue;
        };
        assert!(
            !out.status.success(),
            "{backend}: dividing by zero should fail, not succeed"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("division by zero"),
            "{backend}: should name the fault in lux's words, got:\n{err}"
        );
        assert!(
            !err.contains("panic")
                && !err.contains("Illegal instruction")
                && !err.contains("goroutine"),
            "{backend}: should not leak the host runtime's crash, got:\n{err}"
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("before"),
            "{backend}: output printed before the fault should survive"
        );
    }
}

/// A user type named `LuxShow` collided with the trait (Rust) or protocol (Swift)
/// the emitter injects for compound printing, so it wouldn't build on either — the
/// reserved set wasn't reserved (#37). The generated name now steps aside to
/// `LuxShow_`, so the user keeps the name and the value still prints lux's way.
#[test]
fn a_user_type_named_luxshow_does_not_collide_with_the_printer() {
    let src =
        "struct LuxShow { x: int }\nlet s = LuxShow(x: 7)\nprint(s)\nprint([s, LuxShow(x: 9)])\n";
    assert_prints_everywhere(
        src,
        "userluxshow",
        "LuxShow(x: 7)\n[LuxShow(x: 7), LuxShow(x: 9)]\n",
    );
    let program = parser::parse(lexer::lex(src).expect("lex")).expect("parse");
    let rust = convert::to_rust(&program);
    assert!(
        rust.contains("trait LuxShow_") && rust.contains("struct LuxShow {"),
        "Rust should rename its trait and keep the user's struct:\n{rust}"
    );
    let swift = convert::to_swift(&program);
    assert!(
        swift.contains("protocol LuxShow_") && swift.contains("struct LuxShow:"),
        "Swift should rename its protocol and keep the user's struct:\n{swift}"
    );
}

/// Integer overflow wraps on every backend, so all four agree — where before Swift
/// trapped, the rest wrapped, and `lux run` disagreed with `lux build` over an
/// optimization flag nobody chose (#35). Doubling past the top of a 64-bit int
/// wraps to the smallest, and once more to zero; the interpreter, Go (native),
/// Rust (compiled with overflow checks off), and Swift (masking operators) all land
/// on the same bytes. Overflow is remote for a learner, so keeping the four in step
/// beats trapping a case they'll almost never reach.
#[test]
fn integer_overflow_wraps_the_same_on_every_backend() {
    let src = r#"
var x = 1
var i = 0
while i < 63 {
    x = x * 2
    i += 1
}
print(x)
print(x * 2)
print(x + x)
"#;
    assert_prints_everywhere(src, "overflow", "-9223372036854775808\n0\n0\n");
    // Swift reaches for its masking operators rather than trapping.
    let swift = convert::to_swift(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        swift.contains("x &* 2") && swift.contains("x &+ x"),
        "Swift should use wrapping operators for integer arithmetic:\n{swift}"
    );
}

/// A large integer literal defaults to `i32` in Rust, so one past that range — 3
/// billion, ordinary in a real file — overflowed the default type at compile time
/// when it landed in an expression rather than an annotated binding. It now carries
/// an `i64` suffix where it needs one, so the four agree; small literals stay bare.
#[test]
fn a_large_integer_literal_compiles_on_every_backend() {
    let src = "print(3000000000)\nprint(3000000000 * 2)\nlet big = 5000000000\nprint(big - 1)\n";
    assert_prints_everywhere(src, "bigint", "3000000000\n6000000000\n4999999999\n");
    let rust = convert::to_rust(&parser::parse(lexer::lex(src).expect("lex")).expect("parse"));
    assert!(
        rust.contains("3000000000i64") && !rust.contains("2i64"),
        "a literal past i32 takes an i64 suffix; a small one stays bare:\n{rust}"
    );
}

/// Going past the end of an array is the most common beginner runtime error, and
/// the interpreter's message for it is the richest of the runtime family — it names
/// the index, the length, and the valid range. After `lux build` it used to be lost
/// to a Rust panic, a Go trace, or a Swift register dump; each backend now
/// bounds-checks an index and reports in lux's words, and the output printed before
/// it survives (#38).
#[test]
fn an_out_of_bounds_index_reports_a_lux_error_on_every_backend() {
    let src = "func pick(xs: [int], i: int) -> int {\n    return xs[i]\n}\nprint(\"before\")\nprint(pick([1, 2, 3], 10))\n";
    for backend in ["rust", "swift", "go"] {
        let Some(out) = build_run("oob", backend, src) else {
            continue;
        };
        assert!(
            !out.status.success(),
            "{backend}: an out-of-bounds read should fail, not succeed"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("index 10 is out of bounds for an array of length 3")
                && err.contains("valid indices are 0 to 2"),
            "{backend}: should name the index, length, and valid range, got:\n{err}"
        );
        assert!(
            !err.contains("panic")
                && !err.contains("Illegal instruction")
                && !err.contains("goroutine"),
            "{backend}: should not leak the host runtime's crash, got:\n{err}"
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("before"),
            "{backend}: output printed before the fault should survive"
        );
    }
}

/// The bounds check must not disturb ordinary indexing: a nested read and a nested
/// write both still work, and value semantics hold, on every backend (#38).
#[test]
fn nested_array_read_and_write_still_work_on_every_backend() {
    let src = "var grid = [[1, 2, 3], [4, 5, 6]]\nprint(grid[0][1])\ngrid[1][2] = 99\nprint(grid[1][2])\nprint(grid)\n";
    assert_prints_everywhere(src, "gridrw", "2\n99\n[[1, 2, 3], [4, 5, 99]]\n");
}

// --- String functions: contains, replace, split ------------------------------
// Pinned before implementation (flex, 2026-08-04) — expected values measured
// against rustc/go/swiftc, so these fix the semantics rather than ratify whatever
// the first implementation happens to do. See ~/notes/lux_string_functions.md.

/// The interpreter is the reference, so a behaviour pin has to include it — the
/// three backends agreeing with each other and not with `lux run` is the exact
/// failure this catches. `assert_prints_everywhere` covers the compiled legs; this
/// adds the fourth and keeps a case to one call.
fn assert_all_four(src: &str, tag: &str, expected: &str) {
    let path = std::env::temp_dir().join(format!("lux_{tag}_{}.lux", std::process::id()));
    std::fs::write(&path, src).expect("write lux");
    let run = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run lux");
    let _ = std::fs::remove_file(&path);
    assert!(
        run.status.success(),
        "{tag}: interpreter should run it:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected,
        "{tag}: interpreter output"
    );
    assert_prints_everywhere(src, tag, expected);
}

/// `contains` answers the same question on all four legs. Two of these are the
/// footgun on purpose: `contains("sunday", "sun")` is true, because it asks about a
/// substring and not about a word — a learner writing a guessing game will meet that
/// on their own, so it is pinned rather than left to be discovered.
#[test]
fn contains_answers_the_same_everywhere() {
    let src = "print(contains(\"hello world\", \"world\"))\n\
               print(contains(\"hello world\", \"xyz\"))\n\
               print(contains(\"sunday\", \"sun\"))\n\
               print(contains(\"yesterday\", \"yes\"))\n\
               print(contains(\"caf\u{e9}\", \"\u{e9}\"))\n\
               print(contains(\"ABC\", \"abc\"))\n";
    assert_all_four(src, "contains", "true\nfalse\ntrue\ntrue\ntrue\nfalse\n");
}

/// `replace` changes every occurrence, scanning left to right and never
/// overlapping — `replace("aaa", "aa", "b")` is `ba`, not `bb` and not `ab`. An
/// empty replacement is deletion and is deliberately allowed; only an empty
/// *pattern* is refused.
#[test]
fn replace_answers_the_same_everywhere() {
    let src = "print(replace(\"hello\", \"l\", \"L\"))\n\
               print(replace(\"aaa\", \"aa\", \"b\"))\n\
               print(replace(\"hello\", \"l\", \"\"))\n\
               print(replace(\"hello\", \"z\", \"Q\"))\n\
               print(replace(\"caf\u{e9}\", \"\u{e9}\", \"e\"))\n";
    assert_all_four(src, "replace", "heLLo\nba\nheo\nhello\ncafe\n");
}

/// `split` keeps empty fields, including leading and trailing ones, so the field
/// count is stable and a learner can trust position. Each field is bracketed in the
/// output because an empty field is otherwise a blank line, and a pin that reads as
/// whitespace is a pin nobody can check.
#[test]
fn split_keeps_every_field_everywhere() {
    let src = "for w in split(\"a,b,c\", \",\") {\n    print(\"[\" + w + \"]\")\n}\n\
               for w in split(\"a,,b\", \",\") {\n    print(\"[\" + w + \"]\")\n}\n\
               for w in split(\"a,\", \",\") {\n    print(\"[\" + w + \"]\")\n}\n\
               for w in split(\",a\", \",\") {\n    print(\"[\" + w + \"]\")\n}\n\
               for w in split(\"a::b::c\", \"::\") {\n    print(\"[\" + w + \"]\")\n}\n";
    assert_all_four(
        src,
        "splitfields",
        "[a]\n[b]\n[c]\n\
         [a]\n[]\n[b]\n\
         [a]\n[]\n\
         []\n[a]\n\
         [a]\n[b]\n[c]\n",
    );
}

/// The two degenerate subjects both yield one field rather than none: splitting the
/// empty string gives one empty field, and a separator that never occurs gives the
/// whole string back. Both are the shape a `for` loop over the result depends on.
#[test]
fn split_of_a_degenerate_subject_still_has_one_field() {
    let src = "print(length(split(\"\", \",\")))\n\
               print(length(split(\"abc\", \"x\")))\n\
               print(length(split(\"a,b,c\", \",\")))\n";
    assert_all_four(src, "splitlen", "1\n1\n3\n");
}

/// The one place the three targets disagree, closed by refusing it rather than by
/// picking a winner. Left alone, `split(s, "")` is three different answers — Rust
/// yields 13 fields for "hello world" with leading and trailing empties, Go yields
/// 11 runes, Swift yields the whole string — and `replace(s, "", to)` interleaves on
/// Rust and Go but is a no-op on Swift, and `contains(s, "")` is true on Rust and Go
/// and false on Swift. All three spellings are refused, which also catches what
/// actually causes them: a variable that was accidentally empty.
///
/// The bar is the one the divide-by-zero and out-of-bounds work set — lux's own
/// words, a non-zero exit, and no host runtime showing through.
#[test]
fn an_empty_pattern_is_refused_on_every_backend() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "emptyneedle",
            "print(\"before\")\nprint(contains(\"hello\", \"\"))\n",
            "search text is empty",
        ),
        (
            "emptyfrom",
            "print(\"before\")\nprint(replace(\"hello\", \"\", \"-\"))\n",
            "text to replace is empty",
        ),
        (
            "emptysep",
            "print(\"before\")\nprint(length(split(\"hello\", \"\")))\n",
            "separator is empty",
        ),
    ];

    for (tag, src, clause) in cases {
        // The interpreter defines the message; the backends have to carry it.
        let path = std::env::temp_dir().join(format!("lux_{tag}_{}.lux", std::process::id()));
        std::fs::write(&path, src).expect("write lux");
        let run = Command::new(env!("CARGO_BIN_EXE_lux"))
            .arg("run")
            .arg(&path)
            .output()
            .expect("run lux");
        let _ = std::fs::remove_file(&path);
        assert!(!run.status.success(), "{tag}: interpreter should refuse it");
        assert!(
            String::from_utf8_lossy(&run.stderr).contains(clause),
            "{tag}: interpreter should say `{clause}`, got:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );

        for backend in ["rust", "swift", "go"] {
            let Some(out) = build_run(tag, backend, src) else {
                continue;
            };
            assert!(
                !out.status.success(),
                "{backend}/{tag}: an empty pattern should fail, not succeed"
            );
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(
                err.contains(clause),
                "{backend}/{tag}: should name the fault in lux's words, got:\n{err}"
            );
            assert!(
                !err.contains("panic")
                    && !err.contains("Illegal instruction")
                    && !err.contains("goroutine"),
                "{backend}/{tag}: should not leak the host runtime's crash, got:\n{err}"
            );
            assert!(
                String::from_utf8_lossy(&out.stdout).contains("before"),
                "{backend}/{tag}: output printed before the fault should survive"
            );
        }
    }
}

/// A lux string is a sequence of Unicode scalars, and these three match at that
/// level — the same level `length` counts at and `==` compares at. This is the pin
/// that stops Swift drifting back to Foundation: `range(of:)`,
/// `replacingOccurrences(of:with:)`, and `components(separatedBy:)` all match on
/// graphemes with canonical equivalence, so they would answer five of these
/// differently. `==` already made this choice — the Swift backend emits
/// `unicodeScalars.elementsEqual` rather than String `==` for exactly this reason —
/// and these three have to make it the same way.
///
/// `cafe` + combining acute and `caf` + precomposed é look identical and are not the
/// same string in lux; two of the cases below are the two directions of that, and
/// the third reaches inside the pair to the plain `e`, which Foundation will not do.
/// The last splits a ZWJ family emoji, which is one grapheme and five scalars.
#[test]
fn string_functions_match_scalars_not_graphemes() {
    let src = "print(contains(\"cafe\u{301}\", \"\u{e9}\"))\n\
               print(contains(\"caf\u{e9}\", \"e\u{301}\"))\n\
               print(contains(\"cafe\u{301}\", \"e\"))\n\
               print(replace(\"cafe\u{301}\", \"e\", \"E\"))\n\
               print(length(split(\"\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}\", \"\u{200D}\")))\n";
    assert_all_four(src, "scalarmatch", "false\nfalse\ntrue\ncafE\u{301}\n3\n");
}

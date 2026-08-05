//! Diagnostics that have to meet the errors-are-trails bar, since a learner meets
//! them early: an empty struct, a construction field that forgot its label, a
//! `return` inside a match arm, and recursion that never reaches a base case. Each
//! should name the cause and point somewhere useful, not fall through to a raw
//! "expected a value" — or, for runaway recursion, a stack overflow that aborts
//! with no message at all.

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

/// Recursion with no reachable base case — the classic beginner mistake, and the
/// one place the interpreter used to show its own host language: a raw stack
/// overflow, `SIGABRT`, exit 134, and not one word about the program that was run.
/// It now stops itself at a depth limit and reports an ordinary lux error, exiting
/// 1 like every other. Depth alone can't prove a missing base case, so the message
/// names the limit and offers both readings — a missing base case, or a program
/// that honestly nests this deep, which `lux build` will run past (#26) — rather
/// than diagnosing a bug that might not be there.
#[test]
fn runaway_recursion_reports_a_lux_error_instead_of_aborting() {
    let src = "func fact(n: int) -> int {\n    return n * fact(n - 1)\n}\nprint(fact(5))\n";
    let path = std::env::temp_dir().join(format!("lux-diag-{}-runaway.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);

    // Exit 1 (an ordinary lux error), not 134 (SIGABRT) or any other signal death.
    assert_eq!(
        out.status.code(),
        Some(1),
        "should exit 1 like every lux error"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("`fact`") && err.contains("limit"),
        "should name the function and the limit it reached, got:\n{err}"
    );
    // Both readings: the likely missing base case, and the escape hatch for a
    // program that really is this deep.
    assert!(
        err.contains("base case") && err.contains("lux build"),
        "should offer both a missing base case and `lux build` for a deep one, got:\n{err}"
    );
    assert!(
        !err.contains("stack overflow") && !err.contains("aborting"),
        "should not leak the host runtime's overflow abort, got:\n{err}"
    );
}

/// The depth limit sits well past any ordinary recursion — a recursive walk over
/// 20,000 items clears it — so a correct, terminating program that simply goes deep
/// still runs, converging with the compiled targets rather than refusing what they
/// accept. 0.15.1 set the limit at 10,000, low enough to reach by accident on a
/// real file; it now runs a walk this deep to completion (#26).
#[test]
fn deep_but_terminating_recursion_still_runs() {
    let src = "func d(n: int) -> int {\n    if n == 0 { return 0 }\n    return 1 + d(n - 1)\n}\nprint(d(20000))\n";
    let path = std::env::temp_dir().join(format!("lux-diag-{}-deep.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "d(20000) should run to completion");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "20000\n");
}

/// The "unknown function" note names the built-ins a mistyped call might have
/// meant, and it's the one place a stuck learner is told what exists — so a
/// built-in missing from it reads as a built-in that doesn't exist. It rendered a
/// hand-written list that had drifted three names behind the real set; it now
/// renders the single source, so every working built-in appears, `input`,
/// `parseInt`, and `parseFloat` included.
#[test]
fn the_unknown_function_note_lists_every_builtin() {
    let err = err_of("unknownfn", "print(sparkle(\"ab\", \"a\"))\n");
    for name in [
        "print",
        "eprint",
        "string",
        "int",
        "float",
        "length",
        "contains",
        "replace",
        "split",
        "input",
        "readLine",
        "readFile",
        "writeFile",
        "args",
        "run",
        "parseInt",
        "parseFloat",
    ] {
        assert!(
            err.contains(name),
            "the note should list the `{name}` built-in, got:\n{err}"
        );
    }
}

/// A near miss — a typo or a case slip — is redirected to the name that was meant,
/// which is the one thing a stuck learner needs at that moment: `parseint` reaches
/// `parseInt`, and a mistyped user function reaches its own real name. A short name
/// that only coincidentally lands near a built-in is left to the list instead, so
/// the suggestion never guesses at a name that isn't there.
#[test]
fn a_near_miss_call_suggests_the_name_meant() {
    let err = err_of("typo", "print(parseint(\"5\"))\n");
    assert!(
        err.contains("did you mean `parseInt`?"),
        "a case slip should suggest parseInt, got:\n{err}"
    );

    let err = err_of(
        "userfn",
        "func evalExpr(n: int) -> int {\n    return n\n}\nprint(evalexpr(5))\n",
    );
    assert!(
        err.contains("did you mean `evalExpr`?"),
        "should suggest the user's own function, got:\n{err}"
    );

    // `sum` is two edits from `run`, but too short to be a confident guess: the
    // note should name the built-ins, not reach for one.
    let err = err_of("shorttypo", "print(sum(1, 2))\n");
    assert!(
        !err.contains("did you mean"),
        "should not guess for a short coincidental match, got:\n{err}"
    );
    assert!(
        err.contains("parseInt"),
        "should fall back to the built-in list, got:\n{err}"
    );
}

/// A top-level `func main` is lux's entry point — the graduation shape every other
/// language requires — and lux runs it for you. That "runs it for you" is one idea
/// with three edges, each a rule with its own teaching error: main takes no values,
/// main returns nothing, and nothing else runs beside it at the top level. The
/// learner arriving from C or Java who reaches for `main` first meets these, not a
/// refusal.
#[test]
fn main_that_shares_the_top_level_is_refused() {
    let err = err_of(
        "mainmix",
        "print(\"loose\")\nfunc main() {\n    print(\"hi\")\n}\n",
    );
    assert!(
        err.contains("nothing runs beside `main`") && err.contains("where your program starts"),
        "should say main owns the top level, got:\n{err}"
    );
    // The same rule holds when converting — the checks come with the student to the
    // target compiler, they don't switch off at graduation.
    let conv = convert_err(
        "cmainmix",
        "rust",
        "print(1)\nfunc main() {\n    print(2)\n}\n",
    );
    assert!(
        conv.contains("nothing runs beside `main`"),
        "convert should enforce the entry-point rule too, got:\n{conv}"
    );
}

#[test]
fn main_with_a_parameter_is_refused() {
    let err = err_of("mainparam", "func main(x: int) {\n    print(x)\n}\n");
    assert!(
        err.contains("`main` takes no values"),
        "should say main takes no values, got:\n{err}"
    );
}

#[test]
fn main_with_a_return_type_is_refused() {
    let err = err_of("mainret", "func main() -> int {\n    return 0\n}\n");
    assert!(
        err.contains("`main` returns nothing"),
        "should say main returns nothing, got:\n{err}"
    );
}

#[test]
fn calling_main_by_hand_is_refused() {
    let err = err_of(
        "maincall",
        "func main() {\n    print(\"hi\")\n    main()\n}\n",
    );
    assert!(
        err.contains("you don't call `main` yourself"),
        "should say lux runs main for you, got:\n{err}"
    );
}

/// Run a program through `lux convert` and hand back its stderr, asserting it was
/// refused. `lux convert` and `lux build` used to skip every check `lux run` makes,
/// so a broken program met rustc instead of a lux error (#29); these pin that the
/// structural checks now run before anything is emitted.
fn convert_err(tag: &str, lang: &str, src: &str) -> String {
    let path = std::env::temp_dir().join(format!("lux-conv-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("convert")
        .arg(lang)
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        !out.status.success(),
        "`{src}` should be refused by convert, but was emitted"
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The write-through-a-parameter rule is lux's own and no target language phrases
/// it in lux's terms, so it's the one the issue leads with: `lux convert` now
/// refuses it with the very message `lux run` gives, rather than translating it and
/// leaving rustc to complain about a `mut` the learner never wrote.
#[test]
fn convert_refuses_a_write_through_a_parameter_like_run_does() {
    let src = "func poke(xs: [int]) -> int {\n    xs[0] = 99\n    return xs[0]\n}\nprint(poke([1, 2, 3]))\n";
    let conv = convert_err("cparam", "rust", src);
    assert!(
        conv.contains("`xs` is a parameter, and a parameter never changes"),
        "convert should refuse the parameter write in lux's words, got:\n{conv}"
    );
    // The same error `lux run` produces — the point of the fix is that they match.
    let run = err_of("rparam", src);
    assert!(
        run.contains("`xs` is a parameter, and a parameter never changes"),
        "run and convert should report the same rule, got run:\n{run}"
    );
}

/// A stored `Result` is the one type-directed rule that convert/build enforce,
/// because the target compiler doesn't catch it: the program builds and prints
/// `Ok(...)`/`success(...)`, the value lux says can never be printed (#39). The
/// same message `lux run` gives now fires before anything is emitted.
#[test]
fn convert_refuses_a_stored_result_like_run_does() {
    let src = "let r = readFile(\"x.txt\")\nprint(r)\n";
    let conv = convert_err("cstored", "rust", src);
    assert!(
        conv.contains("a Result can't be stored"),
        "convert should refuse a stored Result, got:\n{conv}"
    );
    let run = err_of("rstored", src);
    assert!(
        run.contains("a Result can't be stored"),
        "run and convert should report the same rule, got run:\n{run}"
    );
}

/// A `Result` parameter is the same rule at a binding, and it has no Go type to
/// lower to, so it's refused on every path — including `lux run`, which used to
/// accept it (#42).
#[test]
fn a_result_parameter_is_refused_everywhere() {
    let src = "func f(r: Result<string, string>) -> string {\n    return match r { ok(let t) => t  err(let e) => e }\n}\nprint(f(readFile(\"x.txt\")))\n";
    let run = err_of("rresultparam", src);
    assert!(
        run.contains("a Result can't be a parameter"),
        "run should refuse a Result parameter, got:\n{run}"
    );
    let conv = convert_err("cresultparam", "go", src);
    assert!(
        conv.contains("a Result can't be a parameter"),
        "convert should refuse a Result parameter, got:\n{conv}"
    );
}

/// An empty array literal bound without a type is refused, in the same words an
/// empty `none` meets, and — like the naming and Result-parameter rules — on every
/// path, so `lux run`, `lux convert` and `lux build` all agree it's illegal rather
/// than each inferring a different element type (#66).
#[test]
fn an_untyped_empty_array_is_refused_everywhere() {
    let src = "var xs = []\nxs += 1\nprint(xs)\n";
    let run = err_of("runemptyarr", src);
    assert!(
        run.contains("an empty array leaves it open"),
        "run should refuse an untyped empty array, got:\n{run}"
    );
    let conv = convert_err("cemptyarr", "swift", src);
    assert!(
        conv.contains("an empty array leaves it open"),
        "convert should refuse an untyped empty array, got:\n{conv}"
    );
    // The nested case — an accumulator declared inside a loop — is the ordinary way
    // the mistake is written, so it must be reached by the walk too.
    let nested = err_of(
        "runemptyarrnested",
        "var pending = [\"a\"]\nwhile length(pending) > 0 {\n    var next = []\n    for item in pending {\n        next += item\n    }\n    pending = next\n}\n",
    );
    assert!(
        nested.contains("an empty array leaves it open"),
        "the rule should reach a binding nested in a loop, got:\n{nested}"
    );
}

#[test]
fn convert_refuses_an_unknown_function() {
    let conv = convert_err("cunknown", "go", "print(frobnicate(3))\n");
    assert!(
        conv.contains("unknown function `frobnicate`"),
        "convert should name the unknown function, got:\n{conv}"
    );
}

#[test]
fn convert_refuses_the_wrong_argument_count() {
    let conv = convert_err(
        "cargc",
        "swift",
        "func add(a: int, b: int) -> int {\n    return a + b\n}\nprint(add(1))\n",
    );
    assert!(
        conv.contains("`add` expects 2 values but got 1"),
        "convert should catch the arity mismatch, got:\n{conv}"
    );
}

/// A `var` that rebinds a parameter's name is refused — one name means one thing in
/// a scope — so the copy-into-a-mutable-local idiom takes a fresh name instead (see
/// `firstToZero`'s `var xs = values` in the assign tests). Refused on the convert
/// path too, since the naming rule runs before any command.
#[test]
fn a_var_shadowing_a_parameter_is_refused() {
    let src = "func f(x: int) -> int {\n    var x = 5\n    x = 6\n    return x\n}\nprint(f(1))\n";
    let path = std::env::temp_dir().join(format!("lux-conv-{}-shadow.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("convert")
        .arg("rust")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        !out.status.success(),
        "a var shadowing a parameter must be refused"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already declared in this scope"),
        "should name the collision, got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ----- one naming rule: reserved built-ins, no shadowing -------------------

/// A built-in function name can't be a name you declare — `func length` used to be
/// silent dead code (the built-in won), which is exactly the confusion the rule ends.
#[test]
fn a_built_in_function_name_cannot_be_declared() {
    let err = err_of(
        "resfn",
        "func length(xs: [int]) -> int {\n    return 0\n}\nprint(0)\n",
    );
    assert!(
        err.contains("`length` is a built-in function"),
        "got:\n{err}"
    );
    let err = err_of("ressplit", "let split = 3\nprint(split)\n");
    assert!(
        err.contains("`split` is a built-in function"),
        "got:\n{err}"
    );
}

/// A built-in type name is reserved too, as a variable or a type — `struct int`
/// would otherwise collide with the `int()` conversion.
#[test]
fn a_built_in_type_name_cannot_be_declared() {
    let err = err_of("restype", "let int = 5\nprint(int)\n");
    assert!(err.contains("`int` is a built-in type"), "got:\n{err}");
    let err = err_of("resstruct", "struct string {\n    n: int\n}\nprint(0)\n");
    assert!(err.contains("`string` is a built-in type"), "got:\n{err}");
}

/// #19 made `let none = 5` resolve to the local; the naming rule refuses the binding
/// instead, so `none` always means the empty `Option`.
#[test]
fn the_empty_option_none_cannot_be_a_variable() {
    let err = err_of("resnone", "let none = 5\nprint(none)\n");
    assert!(err.contains("`none` is a built-in value"), "got:\n{err}");
}

/// A binding can't shadow one still in scope from an enclosing block — a nested
/// block or a loop variable that repeats an outer name. Same-scope was always an
/// error; this closes the gap that made the nested version a silent shadow.
#[test]
fn a_binding_cannot_shadow_an_enclosing_one() {
    let err = err_of(
        "shadowblock",
        "let x = 1\nif true {\n    let x = 2\n    print(x)\n}\n",
    );
    assert!(err.contains("`x` is already in scope"), "got:\n{err}");
    let err = err_of("shadowloop", "var i = 9\nfor i in 0..2 {\n}\nprint(i)\n");
    assert!(err.contains("`i` is already in scope"), "got:\n{err}");
}

/// A name is a value or something you call, never both — a variable can't take a
/// function's name.
#[test]
fn a_name_is_a_value_or_a_function_not_both() {
    // A variable can't take a function's name, even from another scope: the targets
    // resolve the two as one name, so shadowing a function and then calling it runs
    // interpreted but won't build. Refused up front, one name to one meaning.
    let err = err_of(
        "valfn",
        "func f() -> int {\n    return 1\n}\nfunc g() -> int {\n    var f = 5\n    return f\n}\nprint(g())\n",
    );
    assert!(
        err.contains("`f` is already the name of a function"),
        "got:\n{err}"
    );
}

/// The rule forbids shadowing, not reuse: two separate loops and a parameter that
/// repeats a top-level name are never visible at once, so all of it runs. Guards
/// against the rule over-reaching into ordinary reuse.
#[test]
fn reusing_a_name_in_sibling_scopes_is_fine() {
    let path = std::env::temp_dir().join(format!("lux-okname-{}.lux", std::process::id()));
    std::fs::write(
        &path,
        "for i in 0..2 {\n}\nfor i in 0..2 {\n}\nfor _ in 0..2 {\n    for _ in 0..1 {\n    }\n}\nlet x = 1\nfunc f(x: int) -> int {\n    return x + 1\n}\nprint(f(x))\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "sibling reuse must run, stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

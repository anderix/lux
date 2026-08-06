//! The built-in `lux learn` material is only honest if every example in it is
//! real lux. These tests parse the topics straight out of the embedded doc and
//! put each example through the interpreter and all three backends, and check
//! that the navigation graph — guided lessons, and any cross-references — only
//! points at topics that actually exist.

use std::process::{Command, Stdio};

use lux::{convert, interpreter, learn, lexer, parser};

fn program(src: &str) -> Vec<lux::ast::Stmt> {
    let tokens = lexer::lex(src).expect("a learn example should lex");
    parser::parse(tokens).expect("a learn example should parse")
}

fn tool_available(cmd: &str, version_arg: &str) -> bool {
    Command::new(cmd)
        .arg(version_arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn every_topic_runs() {
    // Run through the binary with an explicitly empty stdin, not the in-process
    // interpreter against the ambient one: the `input` topic's example reads stdin,
    // so with a terminal or an open pipe on fd 0 the run blocks forever with no
    // output — a silent hang in the suite every contributor runs first (#53). An
    // empty stdin gives `input()` immediate EOF, the way `< /dev/null` always did.
    let tmp = std::env::temp_dir();
    for t in learn::topics() {
        let path = tmp.join(format!("lux_topic_{}_{}.lux", std::process::id(), t.id));
        std::fs::write(&path, &t.example).expect("write example");
        let out = Command::new(env!("CARGO_BIN_EXE_lux"))
            .arg("run")
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .expect("run lux");
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "`{}` example does not run:\n{}",
            t.id,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn every_topic_converts() {
    for t in learn::topics() {
        let prog = program(&t.example);
        for (lang, src) in [
            ("rust", convert::to_rust(&prog)),
            ("swift", convert::to_swift(&prog)),
            ("go", convert::to_go(&prog)),
        ] {
            assert!(
                !src.trim().is_empty(),
                "`{}` produced no {} source",
                t.id,
                lang
            );
        }
    }
}

/// Compile `src` for `lang` and run it with an empty stdin, returning its stdout. A
/// compile failure fails the test by name — this is where "did not compile as Rust /
/// Go / Swift" is caught, Swift included, the one backend the teaching material was
/// never checked against before.
fn build_and_run(lang: &str, src: &str, id: &str) -> String {
    let tmp = std::env::temp_dir();
    let bin = match lang {
        "rust" => {
            let rs = tmp.join(format!("lux_learn_{id}.rs"));
            let bin = tmp.join(format!("lux_learn_{id}_rs"));
            std::fs::write(&rs, src).expect("write rust");
            let out = Command::new("rustc")
                .arg(&rs)
                .arg("-o")
                .arg(&bin)
                .output()
                .expect("run rustc");
            assert!(
                out.status.success(),
                "`{id}` did not compile as Rust:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            bin
        }
        // Go treats an unused local as a hard error, so building every example also
        // enforces that each one uses what it binds — which is what makes "try it"
        // show output.
        "go" => {
            let dir = tmp.join(format!("lux_learn_go_{id}"));
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join("go.mod"), "module luxlearn\n\ngo 1.21\n").expect("go.mod");
            std::fs::write(dir.join("main.go"), src).expect("write go");
            let bin = dir.join("bin");
            let out = Command::new("go")
                .arg("build")
                .arg("-o")
                .arg(&bin)
                .current_dir(&dir)
                .env("GOCACHE", tmp.join("lux_learn_go_cache"))
                .output()
                .expect("run go build");
            assert!(
                out.status.success(),
                "`{id}` did not compile as Go:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            bin
        }
        "swift" => {
            let sw = tmp.join(format!("lux_learn_{id}.swift"));
            let bin = tmp.join(format!("lux_learn_{id}_sw"));
            std::fs::write(&sw, src).expect("write swift");
            let out = Command::new("swiftc")
                .arg(&sw)
                .arg("-o")
                .arg(&bin)
                .output()
                .expect("run swiftc");
            assert!(
                out.status.success(),
                "`{id}` did not compile as Swift:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            bin
        }
        _ => unreachable!("unknown backend {lang}"),
    };
    let out = Command::new(&bin)
        .stdin(Stdio::null())
        .output()
        .expect("run compiled example");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run one lux program on the interpreter, then on each present backend, and hold
/// them all to the interpreter's output. The shared body of the agreement sweep,
/// called once per card example and once per lux block on a more page.
fn agree_on_every_backend(id: &str, src: &str, backends: &[&str], tmp: &std::path::Path) {
    // The interpreter is the reference every backend must match.
    let path = tmp.join(format!("lux_agree_{}_{}.lux", std::process::id(), id));
    std::fs::write(&path, src).expect("write lux");
    let run = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .expect("run lux");
    let _ = std::fs::remove_file(&path);
    assert!(
        run.status.success(),
        "`{id}` does not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let want = String::from_utf8_lossy(&run.stdout).into_owned();

    for lang in backends {
        let out = match *lang {
            "rust" => convert::to_rust(&program(src)),
            "go" => convert::to_go(&program(src)),
            "swift" => convert::to_swift(&program(src)),
            _ => unreachable!(),
        };
        let got = build_and_run(lang, &out, &format!("{id}_{lang}"));
        assert_eq!(
            got, want,
            "`{id}` prints differently on {lang} than the interpreter"
        );
    }
}

/// Every lux program in the teaching material prints the same on all four legs —
/// the card example each topic leads with, and every lux code block on its earned
/// more page. Each was only ever built and thrown away, so a program that *ran*
/// differently on a target passed unseen — which is how the `io` card's readFile
/// reason came to differ three ways (#62). The card sweep caught that; the more
/// pages went a level deeper, unchecked, until this reached them too (#68). The
/// `main` topic's more page deliberately shows the same hello-world in Rust, Go,
/// and Swift, so only blocks tagged `lux` (or untagged) are run as lux. Swift is
/// compiled here too — the backend the teaching material was never checked against.
/// Stdin is empty, since the `input` card and two more blocks read a line (#53).
#[test]
fn every_topic_agrees_on_every_backend() {
    let backends: Vec<&str> = [
        ("rust", "rustc", "--version"),
        ("go", "go", "version"),
        ("swift", "swiftc", "--version"),
    ]
    .into_iter()
    .filter(|(_, cmd, ver)| tool_available(cmd, ver))
    .map(|(lang, _, _)| lang)
    .collect();
    let tmp = std::env::temp_dir();
    for t in learn::topics() {
        agree_on_every_backend(&t.id, &t.example, &backends, &tmp);

        // The deeper level: every lux block on the more page, held to the same bar.
        if let Some(more) = &t.more {
            let mut n = 0;
            for block in &more.body {
                if let learn::Block::Code { lang, body } = block {
                    // A block tagged for a target language — the `main` page's
                    // hello-world trio — is not a lux program, so leave it alone.
                    let is_lux = lang.as_deref().is_none_or(|l| l == "lux");
                    if is_lux {
                        agree_on_every_backend(
                            &format!("{}_more{}", t.id, n),
                            body,
                            &backends,
                            &tmp,
                        );
                        n += 1;
                    }
                }
            }
        }
    }
}

#[test]
fn errors_point_at_real_topics() {
    // Each program makes a specific mistake, and the diagnostic should send the
    // learner to the topic that explains it. Several of these mirror the `try:`
    // experiments a topic suggests — the loop closes both ways.
    let cases: &[(&str, &str, &str)] = &[
        ("reassign a let", "let pi = 3.14\npi = 3.0\n", "variables"),
        ("mix int and float", "print(7 / 2.0)\n", "numbers"),
        (
            "glue a string to an int",
            "print(\"Score: \" + 42)\n",
            "strings",
        ),
        (
            "index past the end",
            "let xs = [1, 2, 3]\nprint(xs[10])\n",
            "arrays",
        ),
        (
            "loop over a non-array",
            "for x in 5 {\n print(x)\n}\n",
            "for",
        ),
        (
            "non-exhaustive match",
            "enum Shape {\n circle(radius: float)\n square(side: float)\n}\n\
             func area(s: Shape) -> float {\n return match s {\n circle(let r) => r\n }\n}\n\
             print(area(Shape.circle(radius: 1.0)))\n",
            "match",
        ),
        (
            "read a name out of its scope",
            "func loud(w: string) -> string {\n let banged = w + \"!\"\n return banged\n}\n\
             print(loud(\"hi\"))\nprint(banged)\n",
            "scope",
        ),
        ("assign to an undeclared name", "nope = 5\n", "variables"),
        ("a non-bool condition", "if 5 {\n print(1)\n}\n", "booleans"),
        ("call an unknown function", "foo()\n", "functions"),
    ];

    let topic_ids: Vec<String> = learn::topics().into_iter().map(|t| t.id).collect();
    for (label, src, expected) in cases {
        let err = interpreter::run(&program(src), &[])
            .expect_err(&format!("`{}` should be an error", label));
        assert_eq!(
            err.learn.map(|(topic, _)| topic),
            Some(*expected),
            "`{}` should point at `{}`, got {:?}",
            label,
            expected,
            err.learn
        );
        assert!(
            topic_ids.iter().any(|t| t == expected),
            "`{}` points at `{}`, which is not a real topic",
            label,
            expected
        );
    }
}

#[test]
fn navigation_only_points_at_real_topics() {
    let ids: Vec<String> = learn::topics().into_iter().map(|t| t.id).collect();
    let exists = |id: &str| ids.iter().any(|t| t == id);

    // Every guided-lesson member is a real topic.
    for (lesson, members) in learn::paths() {
        for id in *members {
            assert!(
                exists(id),
                "lesson `{}` lists missing topic `{}`",
                lesson,
                id
            );
        }
    }

    // Every topic belongs to exactly one lesson, so none is unreachable.
    for id in &ids {
        let count = learn::paths()
            .iter()
            .filter(|(_, m)| m.contains(&id.as_str()))
            .count();
        assert_eq!(
            count, 1,
            "topic `{}` should be in exactly one lesson, found {}",
            id, count
        );
    }

    // Any `see:` cross-reference on a more page resolves to a real topic.
    for t in learn::topics() {
        if let Some(more) = &t.more {
            for s in &more.see {
                assert!(
                    exists(&s.id),
                    "topic `{}` cross-references missing `{}`",
                    t.id,
                    s.id
                );
            }
        }
    }
}

#[test]
fn every_more_page_has_prose() {
    // A more page is earned, so it's optional — but when present it must say
    // something, or the card's pointer leads nowhere.
    for t in learn::topics() {
        if let Some(more) = &t.more {
            let has_prose = more.body.iter().any(|b| match b {
                learn::Block::Prose(p) => !p.trim().is_empty(),
                learn::Block::Code { .. } => false,
            });
            assert!(has_prose, "`{}` has a more page with no prose", t.id);
        }
    }
}

/// A card is parsed as one concept paragraph, one example fence, and a `try` hint;
/// anything an author writes after that first fence — a second code block, a second
/// paragraph — is silently dropped from `lux learn <topic>`. So a card region may
/// hold at most one fenced block, or the card is quietly showing less than it says.
/// (The `main` card was authored with two and lost its `func main` example until
/// this caught it; the deeper prose belongs on the `more` page, which does render
/// several blocks.)
#[test]
fn no_card_hides_content_after_its_first_fence() {
    const DOC: &str = include_str!("../learn-lux.md");
    let region = DOC.split("<!-- learn:end -->").next().unwrap_or(DOC);
    for chunk in region.split("<!-- topic:").skip(1) {
        let id = chunk.split("-->").next().unwrap_or("").trim();
        // The card is everything before the `more` page begins.
        let card = chunk.split("<!-- more -->").next().unwrap_or(chunk);
        let fences = card
            .lines()
            .filter(|l| l.trim_start().starts_with("```"))
            .count();
        assert!(
            fences <= 2,
            "card `{id}` has {fences} fence lines (>1 code block); content after the \
             first fence is dropped from the card — move it to the more page"
        );
    }
}

#[test]
fn basics_names_real_topics() {
    // The skeleton page is furniture, not a topic. It covers only the universal
    // shapes (not enums/match/option/result), and every topic it does name by id
    // must be a real one.
    let basics = learn::basics();
    assert!(!basics.trim().is_empty(), "basics page is empty");
    let ids: Vec<String> = learn::topics().into_iter().map(|t| t.id).collect();
    let shapes = [
        "variables",
        "numbers",
        "booleans",
        "strings",
        "arrays",
        "structs",
        "if",
        "while",
        "for",
        "functions",
        "scope",
    ];
    for id in shapes {
        assert!(
            ids.iter().any(|t| t == id),
            "skeleton names `{}`, not a real topic",
            id
        );
        assert!(
            basics.contains(id),
            "skeleton should name the `{}` shape",
            id
        );
    }
}

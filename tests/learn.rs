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

#[test]
fn every_topic_compiles_as_rust() {
    if !tool_available("rustc", "--version") {
        eprintln!("skipping: rustc not on PATH");
        return;
    }
    let tmp = std::env::temp_dir();
    for t in learn::topics() {
        let rust = convert::to_rust(&program(&t.example));
        let rs = tmp.join(format!("lux_learn_{}.rs", t.id));
        std::fs::write(&rs, &rust).expect("write rust");
        let out = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(tmp.join(format!("lux_learn_{}.bin", t.id)))
            .output()
            .expect("run rustc");
        assert!(
            out.status.success(),
            "`{}` did not compile as Rust:\n{}",
            t.id,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn every_topic_compiles_as_go() {
    if !tool_available("go", "version") {
        eprintln!("skipping: go not on PATH");
        return;
    }
    // Go treats an unused local as a hard error, so this also enforces that every
    // example actually uses what it binds — which is what makes "try it" show output.
    let tmp = std::env::temp_dir();
    let cache = tmp.join("lux_learn_go_cache");
    for t in learn::topics() {
        let go = convert::to_go(&program(&t.example));
        let dir = tmp.join(format!("lux_learn_go_{}", t.id));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("go.mod"), "module luxlearn\n\ngo 1.21\n").expect("write go.mod");
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
            "`{}` did not compile as Go:\n{}",
            t.id,
            String::from_utf8_lossy(&out.stderr)
        );
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
                learn::Block::Code(_) => false,
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

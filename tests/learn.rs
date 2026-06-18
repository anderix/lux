//! The built-in `lux learn` material is only honest if every example in it is
//! real lux. These tests parse the topics straight out of the embedded doc and
//! put each example through the interpreter and all three backends, and check
//! that the navigation graph — guided lessons, and any cross-references — only
//! points at topics that actually exist.

use std::process::Command;

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
    for t in learn::topics() {
        let prog = program(&t.example);
        assert!(
            interpreter::run(&prog).is_ok(),
            "`{}` example does not run under the interpreter",
            t.id
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
fn navigation_only_points_at_real_topics() {
    let ids: Vec<String> = learn::topics().into_iter().map(|t| t.id).collect();
    let exists = |id: &str| ids.iter().any(|t| t == id);

    // Every guided-lesson member is a real topic.
    for (lesson, members) in learn::paths() {
        for id in *members {
            assert!(exists(id), "lesson `{}` lists missing topic `{}`", lesson, id);
        }
    }

    // Every topic belongs to exactly one lesson, so none is unreachable.
    for id in &ids {
        let count = learn::paths().iter().filter(|(_, m)| m.contains(&id.as_str())).count();
        assert_eq!(count, 1, "topic `{}` should be in exactly one lesson, found {}", id, count);
    }

    // Any `see:` cross-reference resolves to a real topic.
    for t in learn::topics() {
        if let Some(learn::Footer::See(refs)) = &t.footer {
            for r in refs {
                assert!(exists(r), "topic `{}` cross-references missing `{}`", t.id, r);
            }
        }
    }
}

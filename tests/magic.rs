//! `lux magic` is only honest if every spell is real lux that works. These tests
//! parse the spells straight out of the embedded doc and put each one through the
//! interpreter and all three backends. Every spell is written to terminate under
//! empty input — so an input-reading spell finishes here instead of hanging — and
//! `every_spell_runs` proves exactly that by running it with stdin closed.

use std::path::Path;
use std::process::{Command, Stdio};

use lux::{convert, lexer, magic, parser};

const LUX_BIN: &str = env!("CARGO_BIN_EXE_lux");

fn program(src: &str) -> Vec<lux::ast::Stmt> {
    let tokens = lexer::lex(src).expect("a spell should lex");
    parser::parse(tokens).expect("a spell should parse")
}

fn write_spell(ext: &str, id: &str, src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("lux-magic");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.{}", id, ext));
    std::fs::write(&path, src).expect("write spell");
    path
}

fn tool_available(cmd: &str, version_arg: &str) -> bool {
    Command::new(cmd)
        .arg(version_arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn every_spell_has_a_trail() {
    for s in magic::spells() {
        assert!(
            !s.trail.is_empty(),
            "spell `{}` has no trail back into lux learn",
            s.id
        );
    }
}

#[test]
fn every_spell_parses() {
    for s in magic::spells() {
        let _ = program(&s.example);
    }
}

/// Run each spell through the real binary with stdin closed: it must finish (no
/// hang on `readLine`) and exit cleanly. This is the run check and the
/// terminates-under-empty-input check in one.
#[test]
fn every_spell_runs() {
    for s in magic::spells() {
        let path = write_spell("lux", &s.id, &s.example);
        let out = Command::new(LUX_BIN)
            .arg("run")
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .expect("run lux");
        assert!(
            out.status.success(),
            "spell `{}` did not run cleanly with empty input:\n{}",
            s.id,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn every_spell_converts() {
    for s in magic::spells() {
        let prog = program(&s.example);
        for (lang, src) in [
            ("rust", convert::to_rust(&prog)),
            ("swift", convert::to_swift(&prog)),
            ("go", convert::to_go(&prog)),
        ] {
            assert!(!src.trim().is_empty(), "spell `{}` produced no {}", s.id, lang);
        }
    }
}

#[test]
fn every_spell_trail_points_at_real_topics() {
    let ids: Vec<String> = lux::learn::topics().into_iter().map(|t| t.id).collect();
    for s in magic::spells() {
        for topic in &s.trail {
            assert!(
                ids.iter().any(|t| t == topic),
                "spell `{}` trails to `{}`, which is not a real lux learn topic",
                s.id,
                topic
            );
        }
    }
}

#[test]
fn every_spell_compiles_as_rust() {
    if !tool_available("rustc", "--version") {
        return;
    }
    for s in magic::spells() {
        let src = write_spell("rs", &s.id, &convert::to_rust(&program(&s.example)));
        let out = std::env::temp_dir().join("lux-magic").join(&s.id);
        let status = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run rustc");
        assert!(
            status.status.success(),
            "spell `{}` did not compile as Rust:\n{}",
            s.id,
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

#[test]
fn every_spell_compiles_as_go() {
    if !tool_available("go", "version") {
        return;
    }
    for s in magic::spells() {
        let src = write_spell("go", &s.id, &convert::to_go(&program(&s.example)));
        let out = Command::new("go")
            .arg("build")
            .arg("-o")
            .arg(Path::new("/dev/null"))
            .arg(&src)
            .output()
            .expect("run go build");
        assert!(
            out.status.success(),
            "spell `{}` did not compile as Go:\n{}",
            s.id,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

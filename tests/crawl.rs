//! `lux crawl` scaffolds a world into a folder — but `--help`/`-h` must explain
//! the command, never scaffold a folder literally named `--help`. These drive the
//! built binary in a scratch directory so the filesystem effect is observable.

use std::process::Command;

fn lux() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lux"))
}

/// A unique empty scratch directory to run `lux crawl` inside.
fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lux-crawl-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn crawl_help_explains_and_scaffolds_nothing() {
    for flag in ["--help", "-h"] {
        let dir = scratch(flag.trim_start_matches('-'));
        let out = lux()
            .arg("crawl")
            .arg(flag)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "`lux crawl {flag}` should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("usage:") && stdout.contains("lux crawl"),
            "`lux crawl {flag}` should print usage, got:\n{stdout}"
        );
        // The bug was scaffolding into a folder named after the flag.
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert!(
            entries.is_empty(),
            "`lux crawl {flag}` must not create anything, found {} entries",
            entries.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn crawl_scaffolds_a_world() {
    let dir = scratch("scaffold");
    let out = lux().arg("crawl").current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "`lux crawl` should exit 0");
    assert!(
        dir.join("crawl").join("world.lux").exists(),
        "`lux crawl` should write crawl/world.lux"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn crawl_takes_a_name() {
    let dir = scratch("named");
    let out = lux()
        .arg("crawl")
        .arg("dungeon")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "`lux crawl dungeon` should exit 0");
    assert!(
        dir.join("dungeon").join("world.lux").exists(),
        "`lux crawl dungeon` should write dungeon/world.lux"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

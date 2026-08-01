//! `lux crawl` scaffolds a world into a folder. These drive the built binary in a
//! scratch directory so the filesystem effect is observable. (That `--help` never
//! scaffolds a folder is covered for every command in help.rs.)

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

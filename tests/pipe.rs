//! A program whose consumer stops reading early — `prog | head`, `prog | less`
//! then quit, `prog | grep -q` — must die quietly on SIGPIPE the way every Unix
//! tool does, not panic with a rustc backtrace about a broken pipe (#57). Rust's
//! runtime sets SIGPIPE to SIG_IGN, so without the fix the write EPIPEs and
//! `println!` panics (exit 101) with the least lux-like output the tool produces.
//!
//! The flex corpus structurally can't catch this — its harness never closes the
//! pipe early — so the guard lives here.

#![cfg(unix)]

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

#[test]
fn a_program_whose_reader_closes_early_dies_by_sigpipe_not_panic() {
    // Enough output that the pipe fills and the child is still writing when the
    // reader goes away — the smallest reliable reproducer from the issue.
    let src = "for i in 0..100000 {\n    print(i)\n}\n";
    let path = std::env::temp_dir().join(format!("lux-pipe-{}.lux", std::process::id()));
    std::fs::write(&path, src).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn lux");

    // Read a little, then close the read end. The child's next write meets a broken
    // pipe; with SIGPIPE at its default disposition that's a clean signal death.
    let mut out = child.stdout.take().unwrap();
    let _ = out.read(&mut [0u8; 8]);
    drop(out);

    // Draining stderr to EOF waits for the child to exit; it must be empty of any panic.
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).ok();
    let status = child.wait().expect("wait");
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        status.signal(),
        Some(13),
        "should die by SIGPIPE (13), not exit {:?} (a panic); stderr:\n{}",
        status.code(),
        err
    );
    assert!(
        !err.contains("panic"),
        "a rustc panic must never reach the learner on a broken pipe; stderr:\n{}",
        err
    );
}

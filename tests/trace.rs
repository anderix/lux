//! `lux trace` narrates execution to stderr while leaving the program's own
//! stdout untouched. These tests drive the built binary the way a user would,
//! so they cover the whole path: the command, the tracer, and the two streams.

use std::io::Write;
use std::process::{Command, Stdio};

fn lux() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lux"))
}

/// Write `src` to a uniquely named temp .lux file and return its path.
fn write_lux(tag: &str, src: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("lux-trace-{}-{}.lux", std::process::id(), tag));
    std::fs::write(&path, src).unwrap();
    path
}

#[test]
fn trace_narrates_a_loop_accumulating_on_stderr() {
    let path = write_lux(
        "sum",
        "let n = 4\nvar total = 0\nfor i in 1..n {\n    total = total + i\n}\nprint(total)\n",
    );
    let out = lux()
        .arg("trace")
        .arg(&path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let trace = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // The loop variable climbs and the total accumulates, all on stderr.
    for needle in [
        "i = 1",
        "i = 2",
        "i = 3",
        "total = 1",
        "total = 3",
        "total = 6",
    ] {
        assert!(trace.contains(needle), "trace missing `{needle}`:\n{trace}");
    }
    // The program's own output is just the number, alone, on stdout.
    assert_eq!(
        stdout.trim(),
        "6",
        "program output should be 6, got {stdout:?}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn trace_quotes_the_value_read_from_input_and_keeps_the_streams_apart() {
    let path = write_lux("input", "let answer = input(\"name? \")\nprint(answer)\n");
    let mut child = lux()
        .arg("trace")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"north\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let trace = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // The input seam, with the string quoted so it reads as text that came in.
    assert!(
        trace.contains("input() → \"north\""),
        "trace should show the quoted input seam:\n{trace}"
    );
    // The program's own voice stays on stdout, and the trace never leaks into it.
    assert!(stdout.contains("north"), "program should print the answer");
    assert!(
        !stdout.contains("input() →"),
        "trace must not appear on stdout:\n{stdout}"
    );
    let _ = std::fs::remove_file(&path);
}

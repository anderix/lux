//! Every subcommand answers `--help` (and `-h`) the way the top level does:
//! it prints the command's usage to stdout, exits 0, and does nothing else. The
//! regression this guards is a flag being mistaken for an argument — `lux crawl
//! --help` once scaffolded a folder named `--help`.

use std::process::Command;

fn lux() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lux"))
}

const SUBCOMMANDS: &[&str] = &[
    "run", "trace", "crawl", "build", "convert", "learn", "magic", "editors", "update",
];

#[test]
fn every_subcommand_answers_help() {
    for &cmd in SUBCOMMANDS {
        for flag in ["--help", "-h"] {
            // Run in a fresh empty dir so a stray scaffold would be visible.
            let dir = std::env::temp_dir().join(format!(
                "lux-help-{}-{}-{}",
                std::process::id(),
                cmd,
                flag.trim_start_matches('-')
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            let out = lux().arg(cmd).arg(flag).current_dir(&dir).output().unwrap();

            assert!(
                out.status.success(),
                "`lux {cmd} {flag}` should exit 0, got {:?}",
                out.status.code()
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                stdout.contains(&format!("lux {cmd}")) && stdout.contains("usage"),
                "`lux {cmd} {flag}` should print its usage, got:\n{stdout}"
            );
            let created: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
            assert!(
                created.is_empty(),
                "`lux {cmd} {flag}` must not create anything, found {} entries",
                created.len()
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

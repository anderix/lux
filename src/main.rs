//! The `lux` command-line tool.
//!
//! `lux run` interprets a program directly. `lux convert <rust|swift|go>`
//! translates it to that language's source, `lux build` runs the Rust
//! translation through `rustc` to a native binary, and `lux learn` prints the
//! language's own built-in reference.

use std::io::Write;
use std::path::Path;
use std::process::{exit, Command, Stdio};

use lux::{convert, diagnostic, interpreter, learn, lexer, parser};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--version") | Some("-V") => println!("lux {}", VERSION),
        Some("--help") | Some("-h") => print_usage(),
        Some("run") => run_cmd(&args[2..]),
        Some("build") => build_cmd(&args[2..]),
        Some("convert") => convert_cmd(&args[2..]),
        Some("learn") => learn_cmd(&args[2..]),
        Some(other) => {
            eprintln!("unknown command `{}`\n", other);
            print_usage();
            exit(1);
        }
        None => {
            print_usage();
            exit(1);
        }
    }
}

fn run_cmd(rest: &[String]) {
    let Some(path) = rest.first() else {
        eprintln!("usage: lux run <file.lux>");
        exit(1);
    };
    let (source, program) = load(path);
    // The program's own command line: the script at index 0, then its arguments.
    if let Err(err) = interpreter::run(&program, rest) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
}

fn convert_cmd(rest: &[String]) {
    let (lang, path) = match rest {
        [lang, path, ..] => (lang.as_str(), path.as_str()),
        _ => {
            eprintln!("usage: lux convert <rust|swift|go> <file.lux>");
            exit(1);
        }
    };
    let (_, program) = load(path);
    let out = match lang {
        "rust" => convert::to_rust(&program),
        "swift" => convert::to_swift(&program),
        // The emitter produces valid Go; gofmt canonicalises its spacing, which
        // is the one thing reproducing go/printer by hand isn't worth.
        "go" => gofmt(convert::to_go(&program)),
        other => {
            eprintln!("`lux convert` speaks rust, swift, and go, not `{}`.", other);
            exit(1);
        }
    };
    print!("{}", out);
}

fn learn_cmd(rest: &[String]) {
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    // A trailing `more` — `lux learn enums more` — asks for the topic's deeper page.
    let (target, more) = match words.split_last() {
        Some((&"more", head)) if !head.is_empty() => (head, true),
        _ => (words.as_slice(), false),
    };
    match target.first().copied() {
        None => print!("{}", learn::menu()),
        Some("tour") => print!("{}", learn::tour()),
        Some("basics") => print!("{}", learn::basics()),
        Some(topic) => {
            let rendered = if more {
                learn::topic_more(topic)
            } else {
                learn::lookup(topic)
            };
            match rendered {
                Some(text) => print!("{}", text),
                None => {
                    eprintln!("there's no lesson or topic called `{}`.\n", topic);
                    print!("{}", learn::menu());
                    exit(1);
                }
            }
        }
    }
}

/// Run generated Go through `gofmt`, falling back to the raw source if gofmt
/// isn't installed — the output is valid either way, just less tidy without it.
fn gofmt(src: String) -> String {
    let spawned = Command::new("gofmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(_) => return src,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(src.as_bytes()).is_err() {
            return src;
        }
        // Drop stdin to close the pipe before reading gofmt's output.
    }
    match child.wait_with_output() {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout).unwrap_or(src),
        _ => src,
    }
}

fn build_cmd(rest: &[String]) {
    let Some(path) = rest.first() else {
        eprintln!("usage: lux build <file.lux>");
        exit(1);
    };
    let (_, program) = load(path);
    let rust = convert::to_rust(&program);

    // Write the generated Rust beside a stem-named binary, hand it to rustc.
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    let rs_path = std::env::temp_dir().join(format!("{}.rs", stem));
    if let Err(e) = std::fs::write(&rs_path, &rust) {
        eprintln!("cannot write generated Rust to {}: {}", rs_path.display(), e);
        exit(1);
    }
    let status = Command::new("rustc")
        .arg(&rs_path)
        .arg("-o")
        .arg(stem)
        .status();
    match status {
        Ok(s) if s.success() => println!("built ./{}", stem),
        Ok(_) => exit(1),
        Err(e) => {
            eprintln!("could not run rustc: {} (is it installed?)", e);
            exit(1);
        }
    }
}

/// Read, lex, and parse a source file, reporting and exiting on any error.
fn load(path: &str) -> (String, Vec<lux::ast::Stmt>) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            exit(1);
        }
    };
    let program = lexer::lex(&source)
        .and_then(parser::parse)
        .unwrap_or_else(|err| {
            diagnostic::report(path, &source, &err);
            exit(1);
        });
    (source, program)
}

fn print_usage() {
    println!("lux {} — a small language for learning to program", VERSION);
    println!();
    println!("usage:");
    println!("  lux run <file.lux>            run a program");
    println!("  lux build <file.lux>          compile to a native binary via Rust");
    println!("  lux convert <lang> <file.lux> translate to rust, swift, or go source");
    println!("  lux learn [topic] [more]      read the language, built in");
    println!();
    println!("  -V, --version                 print version");
    println!("  -h, --help                    print this help");
}

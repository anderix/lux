//! The `lux` command-line tool.
//!
//! v0.1 implements `lux run`. `lux build` (native binary via Rust) and
//! `lux convert rust|swift|go` (the transpiler) are wired into the command
//! surface now and filled in over the next milestones.

use std::process::exit;

use lux::{diagnostic, interpreter, lexer, parser};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--version") | Some("-V") => println!("lux {}", VERSION),
        Some("--help") | Some("-h") => print_usage(),
        Some("run") => run_cmd(&args[2..]),
        Some("build") => {
            eprintln!("`lux build` arrives once the Rust transpiler lands (milestone 5).");
            exit(1);
        }
        Some("convert") => {
            eprintln!("`lux convert` arrives once the transpiler lands (milestone 5+).");
            exit(1);
        }
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

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            exit(1);
        }
    };

    if let Err(err) = execute(&source) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
}

fn execute(source: &str) -> Result<(), diagnostic::LuxError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    interpreter::run(&program)?;
    Ok(())
}

fn print_usage() {
    println!("lux {} — a small language for learning to program", VERSION);
    println!();
    println!("usage:");
    println!("  lux run <file.lux>            run a program");
    println!("  lux build <file.lux>          compile to a native binary (coming soon)");
    println!("  lux convert <lang> <file>     translate to rust, swift, or go (coming soon)");
    println!();
    println!("  -V, --version                 print version");
    println!("  -h, --help                    print this help");
}

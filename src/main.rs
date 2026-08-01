//! The `lux` command-line tool.
//!
//! `lux run` interprets a program directly. `lux convert <rust|swift|go>`
//! translates it to that language's source, `lux build` runs the Rust
//! translation through `rustc` to a native binary, and `lux learn` prints the
//! language's own built-in reference.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio, exit};

use lux::{convert, diagnostic, editors, interpreter, learn, lexer, magic, parser};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// The stable front door for install and update: a short redirect on anderix.com
// to the release installer. `lux update` and the docs both point here, so there
// is one canonical way to get the latest lux. Unix only — the Windows installer
// is PowerShell, reached below.
#[cfg(unix)]
const INSTALL_URL: &str = "https://anderix.com/lux/install";

// The Windows front door: the PowerShell twin of INSTALL_URL, a short redirect on
// anderix.com to the repo's install.ps1 wrapper over the latest release.
#[cfg(windows)]
const INSTALL_PS_URL: &str = "https://anderix.com/lux/install.ps1";

// The starter crawl, scaffolded by `lux crawl`. The world is the example file
// itself, so the thing you play and the thing the tests run can never drift.
const STARTER_WORLD: &str = include_str!("../examples/keep.lux");
const STARTER_SCROLL: &str = include_str!("../examples/crawl-readme.txt");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--version") | Some("-V") => println!("lux {}", VERSION),
        Some("--help") | Some("-h") => print_usage(),
        Some("run") => run_cmd(&args[2..]),
        Some("trace") => trace_cmd(&args[2..]),
        Some("crawl") => crawl_cmd(&args[2..]),
        Some("build") => build_cmd(&args[2..]),
        Some("convert") => convert_cmd(&args[2..]),
        Some("learn") => learn_cmd(&args[2..]),
        Some("magic") => magic_cmd(&args[2..]),
        Some("editors") => editors_cmd(&args[2..]),
        Some("update") => update_cmd(&args[2..]),
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

/// `lux trace`: run a program while narrating each line and the state it
/// changes to stderr. The program's own output stays on stdout, so the two
/// streams can be watched together, or split with a redirect — play the crawl
/// clean on screen and capture the trace with `2> trace.log` to read after.
fn trace_cmd(rest: &[String]) {
    let Some(path) = rest.first() else {
        eprintln!("usage: lux trace <file.lux>");
        exit(1);
    };
    let (source, program) = load(path);
    eprintln!(
        "tracing {} — each line as it runs, with the state it changes on the right",
        path
    );
    eprintln!("(your program's own output is on stdout; this trace is on stderr)");
    eprintln!();
    if let Err(err) = interpreter::run_traced(&program, rest, &source) {
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
        Some("beyond") => print!("{}", learn::beyond()),
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

/// `lux magic`: with no argument, the spells on offer; with one, that spell —
/// a working shape and its trail into `lux learn`.
fn magic_cmd(rest: &[String]) {
    match rest.first().map(String::as_str) {
        None => print!("{}", magic::menu()),
        Some(name) => match magic::lookup(name) {
            Some(text) => print!("{}", text),
            None => {
                eprintln!("there's no spell called `{}`.\n", name);
                print!("{}", magic::menu());
                exit(1);
            }
        },
    }
}

/// `lux editors`: with no argument, report which editors are here and whether
/// lux highlighting is installed; `lux editors install` writes it for each.
fn editors_cmd(rest: &[String]) {
    match rest.first().map(String::as_str) {
        None => println!("{}", editors::report()),
        Some("install") => println!("{}", editors::install()),
        Some(other) => {
            eprintln!("`lux editors` knows `install`, not `{}`.\n", other);
            println!("{}", editors::report());
            exit(1);
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

fn crawl_usage() {
    println!("lux crawl — drop a small text-adventure world in front of you, as plain");
    println!("lux you can open, play, and change");
    println!();
    println!("usage:");
    println!("  lux crawl           scaffold a new crawl in ./crawl/");
    println!("  lux crawl <name>    scaffold it in ./<name>/ instead");
    println!();
    println!("Then: `lux run <name>/world.lux` to play, or open world.lux to edit it.");
    println!("New to building one? `lux learn crawl` walks through how a world is made.");
}

/// Scaffold a playable, editable text adventure into the current directory.
/// The whole world is plain lux the player can open and change — `lux crawl` is
/// just the thing that drops a fresh copy in front of them.
fn crawl_cmd(rest: &[String]) {
    // `--help` is a request to explain, not a folder to scaffold into — without
    // this it would drop a crawl in ./--help/.
    if matches!(
        rest.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        crawl_usage();
        return;
    }
    let dir = rest.first().map(String::as_str).unwrap_or("crawl");
    let path = Path::new(dir);
    let world = path.join("world.lux");

    // Running `lux crawl` over a crawl you already started reports where it is
    // rather than overwriting it — the world may be full of your own changes.
    if path.exists() {
        if world.exists() {
            println!("There's already a crawl in ./{}/.", dir);
            println!();
            println!("  play it:     lux run {}/world.lux", dir);
            println!("  edit it:     open {}/world.lux in your editor", dir);
            println!(
                "  start over:  delete the ./{}/ folder, then run `lux crawl` again",
                dir
            );
        } else {
            eprintln!(
                "./{}/ already exists but isn't a crawl (no world.lux). \
                 Try a different name: `lux crawl <name>`.",
                dir
            );
            exit(1);
        }
        return;
    }

    if let Err(e) = std::fs::create_dir_all(path) {
        eprintln!("cannot create ./{}/: {}", dir, e);
        exit(1);
    }
    for (name, contents) in [
        ("world.lux", STARTER_WORLD),
        ("read-me-first.txt", STARTER_SCROLL),
    ] {
        let file = path.join(name);
        if let Err(e) = std::fs::write(&file, contents) {
            eprintln!("cannot write {}: {}", file.display(), e);
            exit(1);
        }
    }

    println!(
        "A new crawl is waiting in ./{}/. Step inside it first:",
        dir
    );
    println!();
    println!("  cd {}", dir);
    println!();
    println!("  read first:  read-me-first.txt");
    println!("  play it:     lux run world.lux");
    println!("  the world:   world.lux  — open it; every room is yours to change");
    println!();
    println!("New to building one? `lux learn crawl` walks through how a world is made.");
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
        eprintln!(
            "cannot write generated Rust to {}: {}",
            rs_path.display(),
            e
        );
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

/// `lux update`: fetch and install the latest release by re-running the same
/// stable installer the docs print. cargo-dist installs into a user-owned
/// directory (~/.cargo/bin), so this needs no sudo — and must not use it, or it
/// would prompt for a password it does not need and could leave root-owned files
/// where the user's should be. On Unix a running binary can be replaced in place,
/// so lux can update itself while it runs.
fn update_cmd(rest: &[String]) {
    if !rest.is_empty() {
        eprintln!("usage: lux update");
        exit(1);
    }

    // On Windows a running lux.exe can't overwrite its own file, and lux carries no
    // self-replace machinery by design (zero dependencies). So instead of failing
    // mid-swap, hand back the one-line PowerShell installer to run in a fresh
    // terminal, where lux isn't the running process. `irm` is built into every
    // PowerShell, so there's nothing to check for first.
    #[cfg(windows)]
    {
        println!("On Windows, update lux by running the installer in a new terminal:");
        println!("  irm {} | iex", INSTALL_PS_URL);
        return;
    }

    #[cfg(unix)]
    {
        // curl is the one tool the installer leans on. If it is missing, hand back
        // the manual command rather than a cryptic pipe failure.
        let has_curl = Command::new("curl")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !has_curl {
            eprintln!("lux update needs curl, which isn't on your PATH.");
            eprintln!("Update by hand with:");
            eprintln!("  curl -LsSf {} | sh", INSTALL_URL);
            exit(1);
        }

        println!("Updating lux to the latest release...");
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("curl -LsSf {} | sh", INSTALL_URL))
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("Done. Run `lux --version` to see what you're on.");
                // Point at editor highlighting without touching a single config file
                // here — writing is `lux editors install`'s job, not update's.
                if let Some(tip) = editors::nudge() {
                    println!("{}", tip);
                }
            }
            Ok(_) => {
                eprintln!("the update didn't finish. You can run it by hand:");
                eprintln!("  curl -LsSf {} | sh", INSTALL_URL);
                exit(1);
            }
            Err(e) => {
                eprintln!("could not start the update: {}", e);
                exit(1);
            }
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
    println!("  lux trace <file.lux>          run it, narrating each line and what changes");
    println!("  lux crawl [name]              start a text adventure you can open and change");
    println!("  lux build <file.lux>          compile to a native binary via Rust");
    println!("  lux convert <lang> <file.lux> translate to rust, swift, or go source");
    println!("  lux learn [topic] [more]      read the language, built in");
    println!("  lux magic [spell]             working shapes for what you want to do now");
    println!("  lux editors [install]         syntax highlighting for your editors");
    println!("  lux update                    update lux to the latest release");
    println!();
    println!("  -V, --version                 print version");
    println!("  -h, --help                    print this help");
}

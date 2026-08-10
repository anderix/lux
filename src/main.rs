//! The `lux` command-line tool.
//!
//! `lux run` interprets a program directly. `lux convert <rust|swift|go>`
//! translates it to that language's source, `lux build` runs the Rust
//! translation through `rustc` to a native binary, and `lux learn` prints the
//! language's own built-in reference.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio, exit};

use lux::{check, convert, diagnostic, editors, interpreter, learn, lexer, magic, parser};

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

/// Restore the default `SIGPIPE` disposition on Unix. Rust's runtime sets it to
/// `SIG_IGN`, so a write to a closed pipe returns `EPIPE` and `println!` panics with
/// a rustc backtrace — the least lux-like output the tool can produce, and what a
/// learner sees the moment they type `lux run world.lux | head`. With the default
/// restored, the process dies quietly on the signal (exit 141) the way `seq | head`
/// and every other Unix tool does, matching the Go and Swift translations (#57).
/// The disposition is process-wide, so it also covers the interpreter's worker
/// thread. Declared here rather than depending on the `libc` crate.
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // signal(SIGPIPE = 13, SIG_DFL = 0)
    unsafe {
        signal(13, 0);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

fn main() {
    reset_sigpipe();
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
    if wants_help(rest) {
        sub_usage("run");
        return;
    }
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
    if wants_help(rest) {
        sub_usage("trace");
        return;
    }
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
    if wants_help(rest) {
        sub_usage("convert");
        return;
    }
    let (lang, path) = match rest {
        [lang, path, ..] => (lang.as_str(), path.as_str()),
        _ => {
            eprintln!("usage: lux convert <rust|swift|go> <file.lux>");
            exit(1);
        }
    };
    let (source, program) = load(path);
    // Refuse a broken program with lux's own error before emitting, so the learner
    // never meets rustc about a file they didn't write (#29).
    if let Err(err) = check::check_before_emit(&program) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
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
    if wants_help(rest) {
        sub_usage("learn");
        return;
    }
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
    if wants_help(rest) {
        sub_usage("magic");
        return;
    }
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
/// lux highlighting is installed; `lux editors highlighting` writes it for each.
fn editors_cmd(rest: &[String]) {
    if wants_help(rest) {
        sub_usage("editors");
        return;
    }
    match rest.first().map(String::as_str) {
        None => println!("{}", editors::report()),
        Some("highlighting") => println!("{}", editors::install()),
        Some(other) => {
            eprintln!("`lux editors` knows `highlighting`, not `{}`.\n", other);
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
        let wrote = stdin.write_all(src.as_bytes());
        // Close the pipe (drop the writer) before reading gofmt's output.
        drop(stdin);
        if wrote.is_err() {
            return src;
        }
    }
    match child.wait_with_output() {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout).unwrap_or(src),
        _ => src,
    }
}

/// Did the user ask a command to explain itself — `lux <cmd> --help`? Every
/// subcommand checks this first, so all of them answer `--help` the way the
/// top-level `lux --help` does, rather than treating the flag as an argument.
fn wants_help(rest: &[String]) -> bool {
    matches!(
        rest.first().map(String::as_str),
        Some("--help") | Some("-h")
    )
}

/// Per-command help, kept together so every command's `--help` reads the same.
fn sub_usage(cmd: &str) {
    match cmd {
        "run" => {
            println!("lux run — run a program");
            println!();
            println!("usage: lux run <file.lux>");
        }
        "trace" => {
            println!("lux trace — run a program, narrating each line and the state it changes");
            println!();
            println!("usage: lux trace <file.lux>");
            println!();
            println!("The narration goes to stderr and the program's own output to stdout, so");
            println!(
                "`lux trace world.lux 2> trace.log` plays clean and saves the trace to read after."
            );
        }
        "crawl" => {
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
        "build" => {
            println!("lux build — compile a program to a native binary through Rust");
            println!();
            println!("usage: lux build <file.lux>");
            println!();
            println!("Writes ./<name> beside you; needs rustc installed.");
            println!();
            println!("`lux run` is where a program is watched as it runs: it stops runaway");
            println!("recursion with an error, where a built binary hits the machine's own stack");
            println!("limit instead — a hang or a crash with no lux message. Run a program with");
            println!("`lux run` while you're still finding its bugs; build it once it works.");
        }
        "convert" => {
            println!("lux convert — translate a program to another language's source");
            println!();
            println!("usage: lux convert <rust|swift|go> <file.lux>");
            println!();
            println!("Prints the translation to stdout; redirect it to a file to keep it.");
        }
        "learn" => {
            println!("lux learn — read the language, built in");
            println!();
            println!("usage:");
            println!("  lux learn                list the lessons and topics");
            println!("  lux learn <topic>        read one");
            println!("  lux learn <topic> more   its deeper page");
        }
        "magic" => {
            println!("lux magic — working shapes for what you want to do now");
            println!();
            println!("usage:");
            println!("  lux magic            list the spells");
            println!("  lux magic <spell>    show one");
        }
        "editors" => {
            println!("lux editors — syntax highlighting for your editors");
            println!();
            println!("usage:");
            println!(
                "  lux editors               report which editors are here and what's installed"
            );
            println!("  lux editors highlighting  write highlighting for each one found");
        }
        "update" => {
            println!("lux update — update lux to the latest release");
            println!();
            println!("usage: lux update");
        }
        _ => print_usage(),
    }
}

/// Scaffold a playable, editable text adventure into the current directory.
/// The whole world is plain lux the player can open and change — `lux crawl` is
/// just the thing that drops a fresh copy in front of them.
fn crawl_cmd(rest: &[String]) {
    // `--help` is a request to explain, not a folder to scaffold into — without
    // this it would drop a crawl in ./--help/.
    if wants_help(rest) {
        sub_usage("crawl");
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
    if wants_help(rest) {
        sub_usage("build");
        return;
    }
    let Some(path) = rest.first() else {
        eprintln!("usage: lux build <file.lux>");
        exit(1);
    };
    let (source, program) = load(path);
    // Same pre-emit checks as `lux convert`: a broken program is refused in lux's
    // words here, not by rustc after it's translated (#29).
    if let Err(err) = check::check_before_emit(&program) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
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
    // Compile with overflow checks off, so integer arithmetic wraps the way `lux
    // run` and the other targets do — otherwise a debug rustc would trap on an
    // overflow the interpreter wraps, and `lux build` would disagree with `lux run`
    // over an optimization flag nobody chose (#35).
    let status = Command::new("rustc")
        .arg(&rs_path)
        .arg("-C")
        .arg("overflow-checks=off")
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

/// Where the running lux came from.
#[derive(Debug, PartialEq)]
enum Channel {
    Homebrew,
    WinGet,
    /// The cargo-dist installer behind the vanity URLs, established by a receipt
    /// that covers the running binary rather than assumed from a path.
    Installer,
    /// The installer manages a lux, but not this one. Carries the prefix it does
    /// manage, so the message can name both.
    Shadowed(PathBuf),
    /// Nothing claims this binary: eget, a hand-built copy, a distro package, a
    /// file someone dropped on their PATH.
    Unmanaged,
}

/// Pull a string field out of the install receipt without a JSON parser.
///
/// One field is wanted from a file cargo-dist wrote, so a dependency would be a
/// steep price. Handles the escapes that can appear in a path — `\\` on Windows,
/// and the `\"` and `\/` a JSON writer is allowed to emit — and stops at the
/// closing quote.
fn json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = text.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = text[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            _ => out.push(c),
        }
    }
    None
}

/// Where the cargo-dist installer says it put lux, if it left a receipt.
///
/// The installer writes `luxc-receipt.json` under `$XDG_CONFIG_HOME/luxc`
/// (falling back to `~/.config/luxc`) on Unix and `%LOCALAPPDATA%\luxc` on
/// Windows, recording the prefix it installed into. That file is *positive*
/// evidence of provenance, which a path never was — it says lux came from the
/// installer, and where the installer put it.
fn receipt_prefix() -> Option<PathBuf> {
    #[cfg(windows)]
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("luxc");
    #[cfg(not(windows))]
    let dir = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("luxc"),
        _ => PathBuf::from(std::env::var_os("HOME")?)
            .join(".config")
            .join("luxc"),
    };
    let text = std::fs::read_to_string(dir.join("luxc-receipt.json")).ok()?;
    json_string_field(&text, "install_prefix").map(PathBuf::from)
}

/// Classify an already-resolved executable path against an already-read receipt.
///
/// Split out from `current_channel` so it can be tested against paths and
/// receipts this machine does not have. Both package managers are matched on
/// every platform rather than behind `cfg`, so the Windows branch is covered by
/// the suite on a Linux runner too.
///
/// Match on the *resolved* path. Homebrew links `bin/lux` into its prefix while
/// the real file lives under `Cellar`, and `current_exe` follows symlinks on
/// Linux and macOS, so a Cellar path is what arrives here — the link is never
/// what gets classified.
///
/// The package managers are checked first and win outright. A machine can hold a
/// stale receipt from an earlier installer run alongside a lux that now comes
/// from Homebrew, and the path is the better evidence in that case because it
/// describes the binary actually running.
fn channel_of(exe: &Path, receipt: Option<&Path>) -> Channel {
    let path = normalise_path(exe);

    // Apple silicon, Intel macOS, and Linuxbrew respectively. `/usr/local` is
    // qualified with `Cellar` — unlike the other two, that prefix is shared with
    // everything else installed by hand, and claiming all of it would send a
    // hand-built lux to a Homebrew that never had it.
    if path.starts_with("/opt/homebrew/")
        || path.starts_with("/usr/local/cellar/")
        || path.starts_with("/home/linuxbrew/.linuxbrew/")
    {
        return Channel::Homebrew;
    }

    // Covers both the package directory and the shim in `Links`.
    if path.contains("/microsoft/winget/") {
        return Channel::WinGet;
    }

    // Compared against the recorded prefix rather than a reconstructed `bin`
    // directory: cargo-dist has several install layouts, and the binary sits
    // under the prefix in all of them.
    match receipt {
        Some(prefix) if is_under(exe, prefix) => Channel::Installer,
        Some(prefix) => Channel::Shadowed(prefix.to_path_buf()),
        None => Channel::Unmanaged,
    }
}

/// Flatten a path to text for comparison: separators unified, case folded, and
/// Windows' verbatim `\\?\` prefix dropped.
///
/// Compared as text rather than with `Path::starts_with` so the Windows forms are
/// exercised by the suite on a Linux runner, where a backslash is an ordinary
/// character and a whole Windows path would otherwise read as one component.
/// `canonicalize` returns verbatim paths on Windows while a receipt records a
/// plain one, so the prefix has to come off or the two never meet. Folding case
/// is for Windows, where paths are case-insensitive; on Unix it can only merge
/// two paths differing solely in case, which in practice name the same install.
fn normalise_path(p: &Path) -> String {
    let text = p.to_string_lossy().replace('\\', "/").to_lowercase();
    text.strip_prefix("//?/")
        .map(str::to_string)
        .unwrap_or(text)
}

/// Whether the running binary sits under the prefix the receipt recorded.
fn is_under(exe: &Path, prefix: &Path) -> bool {
    let exe = normalise_path(exe);
    let prefix = normalise_path(prefix);
    let prefix = prefix.trim_end_matches('/');
    // The trailing separator is what keeps `/home/sam/.cargo` from claiming a
    // binary that lives in `/home/sam/.cargofoo`.
    exe == prefix || exe.starts_with(&format!("{}/", prefix))
}

/// The one-line command that installs the latest release on this platform.
#[cfg(unix)]
fn installer_command() -> String {
    format!("curl -LsSf {} | sh", INSTALL_URL)
}

#[cfg(windows)]
fn installer_command() -> String {
    format!("irm {} | iex", INSTALL_PS_URL)
}

/// `lux update`: fetch and install the latest release by re-running the same
/// stable installer the docs print. cargo-dist installs into a user-owned
/// directory (~/.cargo/bin), so this needs no sudo — and must not use it, or it
/// would prompt for a password it does not need and could leave root-owned files
/// where the user's should be. On Unix a running binary can be replaced in place,
/// so lux can update itself while it runs.
///
/// When lux arrived any other way, that installer is the wrong tool: it would
/// write a second copy into `~/.cargo/bin` that nothing else knows about, leaving
/// PATH order to decide which one answers `lux`. Nothing would look wrong — the
/// update reports success and `lux --version` reports the old version. So the
/// installer runs only on proof that lux came from it, and every other case is
/// handed the step that updates the copy it actually has. Refusing to guess is
/// the point: an unrecognised channel used to inherit the bug silently.
fn update_cmd(rest: &[String]) {
    if wants_help(rest) {
        sub_usage("update");
        return;
    }
    if !rest.is_empty() {
        eprintln!("usage: lux update");
        exit(1);
    }

    // An unreadable path means nothing can be established, and behaving as lux
    // always has beats refusing on a technicality.
    let exe = std::env::current_exe()
        .ok()
        .map(|e| e.canonicalize().unwrap_or(e));
    let channel = match &exe {
        Some(path) => channel_of(path, receipt_prefix().as_deref()),
        None => Channel::Installer,
    };

    match channel {
        Channel::Homebrew => {
            println!("This lux came from Homebrew, so Homebrew is what should replace it:");
            println!("  brew upgrade anderix/tap/luxc");
            return;
        }
        Channel::WinGet => {
            println!("This lux came from WinGet, so WinGet is what should replace it:");
            println!("  winget upgrade Anderix.luxc");
            return;
        }
        Channel::Shadowed(managed) => {
            println!(
                "The lux you are running is at {}.",
                exe.as_deref().unwrap_or(Path::new("?")).display()
            );
            println!(
                "The installer manages a different one, under {}.",
                managed.display()
            );
            println!("Updating would refresh that copy and leave this one behind, and whichever");
            println!("comes first on your PATH is the one that answers `lux`. To update it:");
            println!("  {}", installer_command());
            return;
        }
        Channel::Unmanaged => {
            println!(
                "lux is at {}, and nothing here says how it got there.",
                exe.as_deref().unwrap_or(Path::new("?")).display()
            );
            println!("If a package manager or a tool like eget put it there, update it the same");
            println!("way. Installing over the top would leave a second lux somewhere else, and");
            println!("PATH order would decide which one answers. To install the latest release:");
            println!("  {}", installer_command());
            return;
        }
        Channel::Installer => {}
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
                // here — writing is `lux editors highlighting`'s job, not update's.
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
    // Whole-program checks that hold whatever the command — refusing a name the
    // emitters reserve, before it runs fine interpreted and fails to build.
    if let Err(err) = check::check(&program) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
    // Static type check: the same concrete-type rules the interpreter enforces at
    // run time, applied to every path up front, so `run`, `convert`, and `build`
    // all agree on what counts as a valid program — whichever leg reaches it.
    if let Err(err) = convert::type_check(&program) {
        diagnostic::report(path, &source, &err);
        exit(1);
    }
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
    println!("  lux editors [highlighting]    syntax highlighting for your editors");
    println!("  lux update                    update lux to the latest release");
    println!();
    println!("  -V, --version                 print version");
    println!("  -h, --help                    print this help");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three prefixes Homebrew actually uses, and the symlink detail that
    /// makes this worth testing: what reaches `channel_of` is the Cellar path the
    /// link resolves to, not the `bin/lux` a learner sees on their PATH.
    ///
    /// Note the shape of that path — `Cellar/<formula>/<version>/bin/<binary>`.
    /// The formula is `luxc`, matching the crate name on crates.io, because
    /// `lux` in homebrew-core is an unrelated video downloader. The binary it
    /// installs is `lux`, which is the only name a learner ever types.
    #[test]
    fn a_homebrew_lux_is_recognised_on_every_prefix() {
        for path in [
            "/opt/homebrew/Cellar/luxc/0.19.12/bin/lux",
            "/usr/local/Cellar/luxc/0.19.12/bin/lux",
            "/home/linuxbrew/.linuxbrew/Cellar/luxc/0.19.12/bin/lux",
        ] {
            assert_eq!(
                channel_of(Path::new(path), None),
                Channel::Homebrew,
                "{}",
                path
            );
        }
    }

    /// A machine can carry a receipt from an earlier installer run while the lux
    /// on its PATH now comes from Homebrew. The path describes the binary that is
    /// actually running, so it wins.
    #[test]
    fn a_stale_receipt_does_not_override_a_homebrew_path() {
        assert_eq!(
            channel_of(
                Path::new("/opt/homebrew/Cellar/luxc/0.19.12/bin/lux"),
                Some(Path::new("/home/sam/.cargo")),
            ),
            Channel::Homebrew
        );
    }

    /// WinGet's package directory and its shim directory, in the casing Windows
    /// reports and in the verbatim form `canonicalize` hands back.
    #[test]
    fn a_winget_lux_is_recognised_whatever_the_casing() {
        for path in [
            r"C:\Users\sam\AppData\Local\Microsoft\WinGet\Packages\Anderix.luxc_abc123\lux.exe",
            r"c:\users\sam\appdata\local\microsoft\winget\links\lux.exe",
            r"\\?\C:\Users\sam\AppData\Local\Microsoft\WinGet\Packages\Anderix.luxc_abc123\lux.exe",
        ] {
            assert_eq!(
                channel_of(Path::new(path), None),
                Channel::WinGet,
                "{}",
                path
            );
        }
    }

    /// A receipt covering the running binary is the only thing that authorises
    /// re-running the installer. The prefix is the parent of `bin`, not `bin`
    /// itself, because that is what cargo-dist records for its default layout.
    #[test]
    fn a_receipt_covering_this_binary_authorises_the_installer() {
        for (exe, prefix) in [
            ("/home/sam/.cargo/bin/lux", "/home/sam/.cargo"),
            ("/Users/sam/.cargo/bin/lux", "/Users/sam/.cargo"),
            (r"C:\Users\sam\.cargo\bin\lux.exe", r"C:\Users\sam\.cargo"),
            // A flat layout, where the prefix is the directory holding the binary.
            ("/home/sam/.local/bin/lux", "/home/sam/.local/bin"),
        ] {
            assert_eq!(
                channel_of(Path::new(exe), Some(Path::new(prefix))),
                Channel::Installer,
                "{}",
                exe
            );
        }
    }

    /// The installer manages a lux, but not the one running. Updating would
    /// refresh the other copy and leave this one stale, which is the failure the
    /// receipt exists to catch.
    #[test]
    fn a_receipt_pointing_elsewhere_reports_a_shadowed_install() {
        assert_eq!(
            channel_of(
                Path::new("/home/sam/bin/lux"),
                Some(Path::new("/home/sam/.cargo")),
            ),
            Channel::Shadowed(PathBuf::from("/home/sam/.cargo"))
        );
    }

    /// No receipt means no evidence. eget drops a binary wherever the user points
    /// it — the working directory by default — so there is no path to match on,
    /// and guessing "installer" is what used to create the second copy.
    #[test]
    fn a_binary_with_no_receipt_is_unmanaged() {
        for path in [
            "/home/sam/.local/bin/lux",
            "/usr/local/bin/lux",
            "/usr/bin/lux",
            "/opt/lux/bin/lux",
            "/home/sam/projects/lux",
        ] {
            assert_eq!(
                channel_of(Path::new(path), None),
                Channel::Unmanaged,
                "{}",
                path
            );
        }
    }

    /// The receipt is JSON written by another tool, so the one field lux reads
    /// has to survive the escapes a JSON writer is allowed to use — Windows paths
    /// carry doubled backslashes.
    #[test]
    fn the_receipt_field_survives_json_escaping() {
        let unix = r#"{"install_layout":"cargo-home","install_prefix":"/home/sam/.cargo","version":"0.19.13"}"#;
        assert_eq!(
            json_string_field(unix, "install_prefix").as_deref(),
            Some("/home/sam/.cargo")
        );

        let windows = r#"{"install_prefix":"C:\\Users\\sam\\.cargo","version":"0.19.13"}"#;
        assert_eq!(
            json_string_field(windows, "install_prefix").as_deref(),
            Some(r"C:\Users\sam\.cargo")
        );

        assert_eq!(
            json_string_field(r#"{"version":"1"}"#, "install_prefix"),
            None
        );
    }
}

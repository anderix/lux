# lux

lux is a small language built to be a great first language and then to be
outgrown. Every feature is the simplest version of an idea you meet again in Rust,
Swift, and Go, so the idea carries over when you move on to one of those — you
relearn the syntax, not the concept. What you take away is a working model of how
a computer holds a value, makes a decision, and admits that an answer might not
be there — and that is worth having whether or not you write another line of
code. The few hard ideas lux leaves out — ownership, classes, goroutines — are
the ideas those bigger languages are built around, and they land better once you
have written something that needs them.

The whole language is one file — read [learn-lux.md](learn-lux.md): it is the
reference, the tutorial, and the test corpus all at once, and the suite runs
every example in it. The same material is built into the binary, so once lux is
installed you can read it in your terminal with `lux learn`.

## Installing

On macOS or Linux, install a prebuilt `lux` with one command — no Rust toolchain
needed:

```
curl -LsSf https://anderix.com/lux/install | sh
```

On Windows, the same in PowerShell:

```
irm https://anderix.com/lux/install.ps1 | iex
```

Those URLs are stable front doors to the latest release: running one again updates
in place, and once lux is installed `lux update` does the same from the binary
itself. To remove it again:

```
curl -LsSf https://anderix.com/lux/uninstall | sh
```

If you already use Homebrew, lux is in a tap:

```
brew install anderix/tap/luxc
```

The formula is named `luxc`, matching the crate on crates.io, because `lux` was
already taken in both places. The command it installs is `lux`, and that is the
only name you type afterwards. On a Homebrew install `lux update` prints the
`brew upgrade` line rather than updating in place, so the package manager stays
the one thing that decides what is on your machine. Remove it with
`brew uninstall anderix/tap/luxc`.

If you already have Rust, you can install from crates.io instead:

```
cargo install luxc
```

Remove that build with `cargo uninstall luxc`.

## Running a program

```
lux run examples/tour.lux
```

`lux run` interprets a program directly. `lux trace <file.lux>` runs it the same
way but narrates each line and the state it changes, so you can watch a program
work step by step — the narration goes to stderr, so the program's own output
stays clean and can be captured on its own. `lux convert <rust|swift|go>
<file.lux>` prints your program as real source in that language, and `lux build
<file.lux>` runs the Rust translation through `rustc` to a native binary.

## Playing a crawl

```
lux crawl
```

`lux crawl` writes a small, playable text adventure into a `crawl/` folder and
tells you how to play it. The whole world is the `world.lux` it leaves you — open
it, and the rooms, doors, and the torch in the cellar are all there in plain lux,
yours to change. `lux learn crawl` walks through how one is built.

## Learning the language

The reference travels inside the binary. `lux learn` opens a menu of short
topics and guided lessons. Each topic is a one-screen card — `lux learn match`
prints the idea, a runnable example, and an experiment to try; add `more` for
the deeper why, the universal name for the concept, and where it goes in Rust,
Swift, and Go. `lux learn basics` lays out the handful of shapes every
procedural language shares, so the next language is mostly new spelling, and
`lux learn tour` reads the whole thing top to bottom. Every example is real lux
you can paste into a file and run. And when a program hits an error, the
diagnostic points you at the topic that explains it — a non-exhaustive `match`
ends with `help: run lux learn match` — so you learn the idea at the moment you
need it.

```
lux learn               # the menu
lux learn enums         # one topic, as a card
lux learn enums more    # the deeper level
lux learn basics        # the shapes every language shares
lux learn tour          # the whole language
```

## If you use Claude Code

Optional, and no part of learning lux. If you already work with Claude Code, this
writes a `CLAUDE.md` into whatever folder you keep your lux files in. Claude Code
reads it as that folder's project memory, and it asks Claude to help you learn
rather than write your programs for you: coach before solving, never hand over a
rewrite nobody asked for, and read lux's own reference instead of guessing from
the languages lux resembles.

```
curl -LsSf https://anderix.com/lux/tutor | sh
```

It says what it found and what it intends to do before changing anything, and it
leaves a `CLAUDE.md` of your own in place, adding its guidance in a marked block
you can delete. Nothing about lux changes either way.

## Building from source

lux is written in Rust with no dependencies.

```
cargo build --release
./target/release/lux run examples/hello.lux
```

## Testing

The goal is "same source, same behaviour, three targets": `lux run` is the
reference, the transpilers target it, and anywhere a translation behaves
differently is a bug to fix. The suite is how those divergences get caught.
`./check.sh` builds lux and runs everything against that fresh binary: the Rust
test suite, then the conformance and flex corpora, which transpile a body of
programs and diff each compiled translation against the interpreter — a divergence
shows up here, or in a real program, and becomes the next patch. `./check.sh fast`
runs just the build and the Rust tests for a tight loop. A leg whose compiler
(`go`, `rustc`, `swiftc`) isn't installed is skipped, so the suite runs on
whatever toolchains you have; CI runs the full sweep on all three.

## Status

lux is pre-1.0, deliberately, and has been under a feature freeze since August
2026 — bug fixes, cross-target divergence fixes, documentation and tests only.
Anything feature-shaped is queued rather than built, and a new feature restarts
the clock, because what a freeze produces is a track record and reopening one
costs the record rather than a few days.

1.0 is not a claim about quality. It is a promise that breaking you costs a major
version, and two things have to be true before that promise is worth making. The
freeze has to hold, which is the one kind of evidence that cannot be
manufactured. And lux has to be put in front of a beginner who has no reason to
be kind about it, because the claim on the first line of this file is about
teaching, and that is not something its author can grade from the inside.

The teaching surface itself is fully built. `lux run` covers the core —
`print`, `let`/`var`, the four basic types with conversions, arithmetic,
strings and taking one apart, `if`/`else`, `while`, `for ... in`, ranges,
arrays, functions with recursion, and scope — then your own types (structs,
enums with associated values, and exhaustive `match`), and no null:
`Option<T>` and `Result<T, E>` instead. The outside world is modeled as
those same two shapes — `readFile`, `writeFile`, `args`, `readLine`,
`input`, `print`/`eprint`, and `run(program, [args])` returning
`Result<Output, string>` — so fallible I/O is something you handle rather
than a crash. The transpiler backends are all live: `lux convert` turns any
of this into idiomatic Rust, Swift, or Go, each leaning on what that
language already has, and `lux build` compiles the Rust to a native binary.
Every feature is runnable and translatable to every one of them.

Around that core sits how you learn it. `lux learn` is the built-in reference — a
two-level card-and-`more` system, cross-referenced from error messages, that also
reads as guided lessons and a full tour. `lux magic` answers "how do I…?" with
small working spells, each carrying a trail back to the topic that explains it.
`lux crawl` drops a small text adventure whose whole world is one lux file you
play by running and change by editing — with a tutorial-free fast track
(`lux magic room`, `exit`, `thing`, `command`) for the tinkerer who would rather
skip straight to changing it. `lux trace` narrates a running program line by
line, for the bug that gives a wrong answer rather than an error. `lux editors
highlighting` sets up syntax highlighting for whichever of GNOME Text Editor,
Vim, Neovim, nano, or (on Windows) Notepad++ you already have — highlighting
only, nothing that completes or corrects. And `lux update` fetches the latest
release in place.

lux teaches from a point of view. It commits to no null, sum types with
exhaustive `match`, and immutability by default — not as the only way to write
programs, but as the habits the strongest current languages are converging on,
and ones easier to learn first than to bolt on later. That stand is the reason
lux exists: a language with no view on how programs should be written would
have nothing to teach.

If you are weighing lux up rather than learning it — a parent or teacher deciding
whether a language this small can carry a first year — [flex/](flex/) is written
for you: a corpus of the programs a first course reaches for, each run on all four
implementations, with a plain account of how far the language goes and exactly
where it stops. [flex/CONVERT.md](flex/CONVERT.md) sets lux source beside the Rust,
Swift, and Go it becomes — the graduation claim shown rather than asserted.

For the fuller history, see [CHANGELOG.md](CHANGELOG.md) and the scope notes at
the bottom of [learn-lux.md](learn-lux.md).

## License

MIT. Written by David M. Anderson with AI assistance.

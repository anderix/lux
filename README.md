# lux

lux is a small language built to be a great first language and then to be
outgrown. Every feature is the simplest honest version of something shared by
Rust, Swift, and Go, so what you learn here carries straight over when you move
on to one of those. The few hard ideas lux leaves out — ownership, classes,
goroutines — are the lessons those bigger languages exist to teach.

The full language fits on one page. Read [learn-lux.md](learn-lux.md): it is the
reference, the tutorial, and the test corpus all at once — the suite runs every
example in it. The same material is built into the binary, so once lux is
installed you can read it in your terminal with `lux learn`.

## Installing

On macOS or Linux, install a prebuilt `lux` with one command:

```
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/anderix/lux/releases/latest/download/lux-installer.sh | sh
```

To remove it again:

```
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/anderix/lux/main/uninstall.sh | sh
```

## Running a program

```
lux run examples/tour.lux
```

`lux run` interprets a program directly. `lux convert <rust|swift|go> <file.lux>`
prints your program as real source in that language, and `lux build <file.lux>`
runs the Rust translation through `rustc` to a native binary.

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

## Building from source

lux is written in Rust with no dependencies.

```
cargo build --release
./target/release/lux run examples/hello.lux
```

## Status

Early, and growing in milestones. `lux run` now covers the core (`print`,
`let`/`var`, the four basic types, arithmetic, strings, `if`/`else`, `while`),
functions with recursion, `for ... in`, ranges, and arrays, your own types
(structs, enums with associated values, and exhaustive `match`), and no null —
`Option<T>` and `Result<T, E>` instead. All three transpiler backends are live:
`lux convert` turns any of these into idiomatic Rust, Swift, or Go, each leaning
on what that language already has — Swift's enums and `Optional`, Go's interfaces
and `(value, error)` returns — and `lux build` compiles the Rust to a binary.
That completes the teaching surface: every feature is runnable and translatable
to all three. The milestones since have gone into `lux learn` — the built-in
reference, now a two-level card-and-`more` system cross-referenced from error
messages — and into the outside world: `readFile`, `writeFile`, `args`,
`readLine`, and `print`/`eprint` across stdout and stderr, fallible I/O modeled
as the same `Option` and `Result` lux already teaches. The latest milestone runs
other programs: `run(program, [args])` returns `Result<Output, string>`, where
`Output` — `status`, `stdout`, `stderr` — is the first built-in struct, and the
two-layer result keeps "did it launch" and "did it succeed" apart. See the scope
notes at the bottom of [learn-lux.md](learn-lux.md), or
[CHANGELOG.md](CHANGELOG.md) for the version history.

## License

MIT. Written by David M. Anderson with AI assistance.

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
topics and guided lessons; `lux learn <topic>` — say `lux learn match` — prints
one idea on a single screen with a runnable example; and `lux learn tour` reads
the whole language top to bottom. Every example is real lux you can paste into a
file and run. And when a program hits an error, the diagnostic points you at the
topic that explains it — a non-exhaustive `match` ends with `help: run lux learn
match` — so you learn the idea at the moment you need it.

```
lux learn          # the menu
lux learn enums    # one topic
lux learn start    # a guided lesson
lux learn tour     # the whole language
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
That completes the v0.1 teaching surface: every feature is runnable and
translatable to all three. See the scope notes at the bottom of
[learn-lux.md](learn-lux.md).

## License

MIT. Written by David M. Anderson with AI assistance.

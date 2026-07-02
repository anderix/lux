# lux

lux is a small language built to be a great first language and then to be
outgrown. Every feature is the simplest version of something shared by
Rust, Swift, and Go, so what you learn here carries straight over when you move
on to one of those. The few hard ideas lux leaves out — ownership, classes,
goroutines — are the lessons those bigger languages exist to teach.

The full language fits on one page. Read [learn-lux.md](learn-lux.md): it is the
reference, the tutorial, and the test corpus all at once — the suite runs every
example in it. The same material is built into the binary, so once lux is
installed you can read it in your terminal with `lux learn`.

## Installing

On macOS or Linux, install a prebuilt `lux` with one command — no Rust toolchain
needed:

```
curl -LsSf https://anderix.com/lux/install | sh
```

That URL is a stable front door to the latest release: running it again updates
in place, and once lux is installed `lux update` does the same from the binary
itself. To remove it again:

```
curl -LsSf https://anderix.com/lux/uninstall | sh
```

If you already have Rust, you can install from crates.io instead. The crate is
named `luxc`; the command it installs is `lux`:

```
cargo install luxc
```

Remove that build with `cargo uninstall luxc`.

## Running a program

```
lux run examples/tour.lux
```

`lux run` interprets a program directly. `lux convert <rust|swift|go> <file.lux>`
prints your program as real source in that language, and `lux build <file.lux>`
runs the Rust translation through `rustc` to a native binary.

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

## Building from source

lux is written in Rust with no dependencies.

```
cargo build --release
./target/release/lux run examples/hello.lux
```

## Status

Early, but the teaching surface is complete. `lux run` covers the core — `print`,
`let`/`var`, the four basic types with honest conversions, arithmetic, strings,
`if`/`else`, `while`, `for ... in`, ranges, arrays, functions with recursion, and
scope — then your own types (structs, enums with associated values, and
exhaustive `match`), and no null: `Option<T>` and `Result<T, E>` instead. The
outside world is modeled as those same two shapes — `readFile`, `writeFile`,
`args`, `readLine`, `input`, `print`/`eprint`, and `run(program, [args])`
returning `Result<Output, string>` — so fallible I/O is something you handle
rather than a crash. All three transpiler backends are live: `lux convert` turns
any of this into idiomatic Rust, Swift, or Go, each leaning on what that language
already has, and `lux build` compiles the Rust to a native binary. Every feature
is runnable and translatable to all three.

Around that core sits how you learn it. `lux learn` is the built-in reference — a
two-level card-and-`more` system, cross-referenced from error messages, that also
reads as guided lessons and a full tour. `lux magic` answers "how do I…?" with
small working spells, each carrying a trail back to the topic that explains it.
`lux crawl` drops a small text adventure whose whole world is one lux file you
play by running and change by editing — with a tutorial-free fast track
(`lux magic room`, `exit`, `thing`, `command`) for the tinkerer who would rather
skip straight to changing it. And `lux update` fetches the latest release in
place.

For the fuller history, see [CHANGELOG.md](CHANGELOG.md) and the scope notes at
the bottom of [learn-lux.md](learn-lux.md).

## License

MIT. Written by David M. Anderson with AI assistance.

# lux

lux is a small language built to be a great first language and then to be
outgrown. Every feature is the simplest honest version of something shared by
Rust, Swift, and Go, so what you learn here carries straight over when you move
on to one of those. The few hard ideas lux leaves out — ownership, classes,
goroutines — are the lessons those bigger languages exist to teach.

The full language fits on one page. Read [learn-lux.md](learn-lux.md): it is the
reference, the tutorial, and the test corpus all at once.

## Running a program

```
lux run examples/tour.lux
```

`lux build` (a native binary) and `lux convert rust|swift|go` (translate your
program to real source in one of the target languages) come in later milestones.
Today `lux run` interprets a program directly.

## Building from source

lux is written in Rust with no dependencies.

```
cargo build --release
./target/release/lux run examples/hello.lux
```

## Status

Early, and growing in milestones. `lux run` now covers the core (`print`,
`let`/`var`, the four basic types, arithmetic, strings, `if`/`else`, `while`),
functions with recursion, `for ... in`, ranges, and arrays, plus your own types:
structs, enums with associated values, and exhaustive `match`. `Option`/`Result`
and the transpiler backends follow, simplest first. See the v0.1 scope notes at
the bottom of [learn-lux.md](learn-lux.md).

## License

MIT. Written by David M. Anderson with AI assistance.

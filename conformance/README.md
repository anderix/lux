# Conformance

lux makes one central promise: the same source behaves the same way whether it's
interpreted or transpiled to Rust, Swift, or Go. This directory tests that
promise instead of assuming it.

`conformance.sh` takes each program below, runs it twice — once through
`lux run`, once through its compiled translation — and diffs the output. Same
input, same bytes out, or it's a failure.

```
./conformance.sh        # needs `lux`, plus any of go / rustc / swiftc
```

A failing leg is a real divergence to fix, never an expected tolerance. The
interpreter is the reference; when a target's output disagrees with it, the
target is wrong.

## The corpus

Each program is chosen to exercise a different part of the language rather than
to be interesting in itself:

| file | shape | exercises |
|---|---|---|
| `rpn.lux` | stack machine | `args()`, `parseInt`, struct-with-`Option` state, folding a fault forward instead of breaking |
| `life.lux` | simulation | flat-array index math, nested loops, a pure `step`, string building |
| `stats.lux` | Unix filter | `readLine` loop, `Option` handling, insertion sort |
| `doctor.lux` | orchestration | `run`, `Output` struct, the launch-vs-status failure split, `eprint` |
| `decide.lux` | decision table | nested `match` over two enums, exhaustiveness, `Result`-shaped verdicts |
| `tree.lux` | binary tree | a recursive enum, hand-built and folded three ways — indirection each target spells differently (Box, `indirect`, interface) |

These are the baseline. They all run correctly under `lux run` — that is the
fixed point. When a transpiled target disagrees, fix the target, not the
program: if a fix needs the corpus edited, the fix is wrong.

## Scope

The point is backend *correctness*, not language growth. These programs are
shaped the way they are because certain things deliberately don't exist in lux —
no maps, no `break`, no generics — and that shape is the teaching content
(insertion sort builds a new row; the RPN machine folds a fault forward). They
favour building a new value over mutating one even though a `var` field or element
can now be assigned, because that's the habit the language is teaching. Don't add
language features to make them nicer. A divergence here is a bug in a target, not
a missing feature.

All three legs — Go, Rust, and Swift — are wired. A leg whose compiler isn't on
PATH is skipped, so the suite runs on whatever toolchains are present.

## Declared seams

A difference is sometimes declared rather than treated as a failure — a place a
target's host language legitimately shows through, documented instead of papered
over. This is the same call lux makes elsewhere: fix a footgun, but document a
real seam.

**`swift` / `doctor` (skipped).** Swift launches subprocesses through
`/usr/bin/env` — the same PATH lookup Rust and Go do — so a *missing* program
returns env's exit 127, a status on the `ok` arm, rather than a launch failure on
the `err` arm. `doctor` deliberately probes a program that isn't there, so its
Swift output diverges; the suite skips the Swift comparison for `doctor`.

# Conformance

lux makes one central promise: the same source behaves the same way whether it's
interpreted or transpiled to Rust, Swift, or Go. This directory tests that
promise instead of assuming it.

`conformance.sh` takes each program below, runs it twice — once through
`lux run`, once through its compiled translation — and diffs the output. Same
input, same bytes out, or it's a failure.

```
./conformance.sh        # needs `lux` and `go` on PATH
```

A failing leg is a real divergence to fix, never an expected tolerance. The
interpreter is the reference; when a target's output disagrees with it, the
target is wrong.

## The corpus

Five programs, each chosen to exercise a different part of the language rather
than to be interesting in itself:

| file | shape | exercises |
|---|---|---|
| `rpn.lux` | stack machine | `args()`, `parseInt`, struct-with-`Option` state, folding a fault forward instead of breaking |
| `life.lux` | simulation | flat-array index math, nested loops, a pure `step`, string building |
| `stats.lux` | Unix filter | `readLine` loop, `Option` handling, insertion sort |
| `doctor.lux` | orchestration | `run`, `Output` struct, the launch-vs-status failure split, `eprint` |
| `decide.lux` | decision table | nested `match` over two enums, exhaustiveness, `Result`-shaped verdicts |

These are the baseline. They all run correctly under `lux run` — that is the
fixed point. When a transpiled target disagrees, fix the target, not the
program: if a fix needs the corpus edited, the fix is wrong.

## Scope

The point is backend *correctness*, not language growth. These programs are
shaped the way they are because certain things deliberately don't exist in lux —
no string splitting, no maps, no `break`, no mutation — and that shape is the
teaching content (insertion sort builds a new row; the RPN machine folds a fault
forward). Don't add language features to make them nicer. A divergence here is a
bug in a target, not a missing feature.

Only the Go leg is wired up today. Rust and Swift emit for every program but
aren't yet compiled and compared — adding those legs is the obvious next step,
and the shape of the check is identical.

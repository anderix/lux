# Flex

How far the language goes, and exactly where it stops.

`conformance/` asks whether lux keeps its promise that one source behaves the same
on three targets. This directory asks a different question: what can you actually
*write* in it? The programs here are chosen from the ones a first course reaches
for, each written the way lux wants it rather than the way the textbook prints it.
Some of them stop early, and where they stop is the most useful thing here.

Nothing in this directory is a tutorial. It is written for someone weighing lux up
— a parent, a teacher, anyone deciding whether a language this small can carry a
first year — who would rather read working programs than a feature list.

```
./flex.sh              # every program, every target
./flex.sh sieve bst    # just those
```

Each program runs through `lux run` and through its compiled Go, Rust, and Swift
translations, and the four outputs are diffed. The interpreter is the reference.
Every program here is deterministic, because lux has no way to produce a random
number and that is [deliberate](https://github.com/anderix/lux/issues/3) — so the
comparison needs no seeding and no tolerance. Same bytes, or a failure.

## The corpus

**Ground everyone recognizes.** Read these first if you have never seen lux; the
algorithms are ones you already know, so all that is new is the language.

| program | what it shows |
|---|---|
| `fizzbuzz` | an enum and an exhaustive `match` replacing a ladder of ifs — add a case and lux refuses to run until it's handled |
| `fib` | recursion beside iteration, and the cost of the difference made visible |
| `gcd` | the same algorithm written recursively and as a loop, plus `%` |
| `sieve` | crossing out multiples in place — assignment into the middle of a row |
| `collatz` | a `while` whose termination is an unsolved problem, said out loud |
| `roman` | table-driven conversion, and why a function reaches only its own locals |

**Sorting.** Five algorithms, one shape: a row goes in, a sorted row comes back.
A parameter can't be changed, so each copies into a local `var` first — which
means the caller's row is never touched, and that's visible in the output.

| program | what it shows |
|---|---|
| `bubble` | adjacent swaps, an early exit when a pass changes nothing |
| `selection` | fewest swaps of any of them, and why that isn't the same as fastest |
| `mergesort` | divide and conquer, allocating by nature rather than by restriction |
| `quicksort` | Lomuto's partition, and the exact point where "in place" stops at a function boundary |

**The type system.** This is lux's actual argument, and the section to read if you
are trying to decide whether it's a toy.

| program | what it shows |
|---|---|
| `binsearch` | `Option<int>` instead of returning -1 for "not found" |
| `list` | a linked list over a recursive enum, empty case named `nil` on purpose |
| `bst` | a search tree that sorts by its shape, and what an already-sorted input does to it |
| `expr` | an expression tree whose division can fail, and a `Result` travelling up two levels |
| `machine` | a vending machine where every state must answer every event |
| `safe` | chaining two things that might be missing, without a null anywhere |

**Programs that do work.** Small filters that run in a pipe, exercising `args`,
`readLine`, stdout and stderr as separate channels, and the two different ways a
line can come back empty.

| program | what it shows |
|---|---|
| `catn` | `cat -n` — the smallest useful filter, and the read-until-empty loop |
| `head` | `args()`, `args()[0]` being the program itself, and a bad count that doesn't crash |
| `wcl` | two of `wc`'s three columns, and a plain statement about the third |
| `uniqc` | `uniq -c`, writable precisely because the real one only collapses adjacent runs |
| `hist` | a bar chart, scaling, and junk input counted rather than fatal |

## Where lux stops

The walls are not all the same kind, and treating them as one list would
misrepresent the language in its own favour.

**Deliberate, and staying that way.** There is no randomness, because lux's one
load-bearing idea is that state is a value you can watch — `step(world, cmd) ->
World`, replayable by folding the same commands again — and a hidden die roll
breaks that on the first throw. A `Result` cannot be stored in a variable or handed
to `print`; it is handled where it is produced or returned for the caller to face,
which is what keeps one source crossing three targets. There are no classes, no user-defined
generics, and no ownership. Each of those is somebody's graduation lesson: Rust
takes over for ownership, Swift for classes, Go for goroutines.

**Not built yet, and wanted.** Strings cannot be split or indexed, and there is no
map type. Both are on the list ahead of anything else, pulled by programs that
needed them rather than by a wish list. Their absence shapes three programs here:
`wcl` reports lines and characters but not words, `uniqc` collapses only adjacent
runs, and `safe` searches two rows kept in step where a real lookup would use a
map. Each says so where it stops.

**Smaller edges.** A function sees only its parameters and its own locals — there
are no globals to reach up for, which is why `roman` keeps its tables inside the
function that walks them. Enums can refer to themselves but two enums cannot refer
to each other. There is no `break`, so a loop keeps its own answer to "am I done?",
which is a fair description of what `break` does anywhere.

## What this found

The point of running every program on every target is that a program which only
works interpreted proves nothing. Every program here now matches on all three
targets — but it took writing them to get there. Run `./flex.sh` for the live
state.

The corpus is what surfaced the divergences, and each was a bug in a target, not a
limit of the language: an empty array literal's type in Go, an enum `var` taking
its case's type, an error forwarded from a `match` arm returning one value where Go
wanted two, an array printed without commas, a reversed range crashing Swift, value
semantics leaking through a Go slice, an enum match dropping its wildcard case. None
was found by the smaller `conformance/` set; each was filed as an issue, fixed in
the target, and closed. The [CHANGELOG](../CHANGELOG.md) carries the history — which
is the argument for this directory existing.

## The rules

**A divergence means the target is wrong.** The interpreter defines the language.
When a translation disagrees, the translation is fixed; a program is never edited
to make a backend pass. If a fix would need the corpus changed, the fix is wrong.

**The corpus never asks for a feature.** If lux can't express something, the
program doesn't get written and the wall goes in the section above. Every wall
reads like a missing feature in the moment, which is exactly why the rule is
written down. `lux crawl` was built the same way, entirely on the language as it
stood.

**Findings leave as issues.** Work on the corpus and work on the language happen
separately, on purpose. Nothing in this directory has ever changed a line of lux.

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

**Grids.** An array of arrays, which is the shape most of a first course's
interesting problems arrive in. Nothing else in the repo uses one — `conformance`'s
Life deliberately flattens its board to `y * w + x` — so this section is also the
first real exercise of `[[T]]` across the four implementations.

| program | what it shows |
|---|---|
| `pascal` | rows of different lengths — a grid is a row of rows, not a rectangle |
| `matrix` | the triple loop worth memorising, and a copy that leaves the caller's matrix alone |
| `lcs` | a table standing in for repeated work, which is all dynamic programming is |
| `tictactoe` | eight lines to check, and the third ending beginners forget |
| `queens` | backtracking, where value semantics remove the undo step and its classic bug |
| `maze` | breadth-first search, a queue that never discards, and a room the eye reads wrong |

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

## What it becomes

[CONVERT.md](CONVERT.md) puts a handful of these beside the Rust, Swift, and Go that
`lux convert` makes of them — a recursive type, a value that might be missing, a
value that might fail, and a copy that stays a copy. It is the graduation argument
in the only form that actually carries it, which is four columns rather than a
paragraph. `./convert.sh` regenerates the full translations if you would rather read
whole files.

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
function that walks them. There is no `break`, so a loop keeps its own answer to
"am I done?", which is a fair description of what `break` does anywhere. `+=` on an
array adds one element rather than joining two, so `bst` carries a four-line
`joinRows` to stitch a walk back together.

## What this found

The point of running every program on every target is that a program which only
works interpreted proves nothing. Every program here runs correctly under `lux run`;
where a target disagrees or won't build, that is the finding. Run `./flex.sh` for
the live state, which is the only account of it that can't go stale.

The corpus is what surfaced the divergences, and each was a bug in a target, not a
limit of the language: an empty array literal's type in Go, an enum `var` taking
its case's type, an error forwarded from a `match` arm returning one value where Go
wanted two, an array printed without commas, a reversed range crashing Swift, value
semantics leaking through a Go slice, an enum match dropping its wildcard case. None
was found by the smaller `conformance/` set; each was filed as an issue, fixed in
the target, and closed. The [CHANGELOG](../CHANGELOG.md) carries the history — which
is the argument for this directory existing.

The grid section found three more the same way, on its first run, and they are worth
reading as a set because all three hid in the same blind spot. A loop that discards
its variable with `for _ in` — the natural way to say "do this n times" — lowered to
invalid Go ([#18](https://github.com/anderix/lux/issues/18)). Returning a string read
out of an array wouldn't compile on Rust, because an indexed `String` is the one
element type that isn't `Copy`
([#20](https://github.com/anderix/lux/issues/20)) — over `[[int]]` it compiled
anyway, which is what made it easy to miss, since it is the accessor every grid
program writes. And a variable named `none` bound correctly and then read as the
empty `Option` at every use site on all three targets
([#19](https://github.com/anderix/lux/issues/19)).

One more from the same section was about cost rather than correctness, and so was the
kind a diff will never catch. Go's value-semantics copy landed inside a loop's
condition when the bound was a function call, so `for i in 0..rows(m)` deep-copied
the whole grid on every iteration and an ordinary O(n²) walk ran cubic
([#21](https://github.com/anderix/lux/issues/21)). The output was identical, which
was exactly the problem: a learner who writes the obvious loop has no way to see what
it cost, and slow is much harder to notice than wrong. It was also the first finding
here the harness could not have produced, since the harness compares bytes and the
bytes agreed — it came from reading the emitted code while writing
[CONVERT.md](CONVERT.md), which is now a habit rather than an accident.

Two more came from probing the edges of what could be written rather than from a
program here: two enums that refer to each other compiled on Go but wanted a `Box` on
Rust and an `indirect` on Swift that neither got
([#17](https://github.com/anderix/lux/issues/17)), and recursion past the
interpreter's stack aborted the process instead of reporting a lux error
([#16](https://github.com/anderix/lux/issues/16)).

Two of what is open now sit where that second fix landed. The interpreter stops
runaway recursion at a fixed depth, but it cannot tell a runaway from a program that
simply goes deep, so a correct function that recurses past the ceiling is told it
"kept calling without stopping" — every clause of which is false for that program,
and all three compiled targets run it
([#26](https://github.com/anderix/lux/issues/26)). And the guard itself is
interpreter-only, so genuine infinite recursion behaves four different ways: a lux
error under `lux run`, a silent hang on Rust and Swift, which optimize the self-call
into a loop, and a gigabyte of stack followed by a runtime dump on Go
([#27](https://github.com/anderix/lux/issues/27)). That second one is the graduation
moment at its least forgiving — a program that told you what was wrong when you ran
it just stops saying anything once you build it.

The third is the sequel to the cubic walk, and it came the same way, from reading
emitted code rather than running it. Hoisting the loop bound moved the copy but did
not remove it, and a grid handed to an accessor inside an inner loop is still
deep-copied every time round. Swift is the one implementation that doesn't pay:
its arrays are copy-on-write, so a parameter nobody writes to is never copied, and on
identical source at n=800 it comes in two hundred times faster than Rust
([#28](https://github.com/anderix/lux/issues/28)). What makes that a defect rather
than a tradeoff is that lux refuses to compile a write through a parameter at all —
`a parameter never changes` — so the copy is guarding against a program nobody is
allowed to write.

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

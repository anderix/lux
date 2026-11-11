# Flex

What the language can carry, and what the toolchain does when you lean on it.

`conformance/` asks whether lux keeps its promise that one source behaves the same
on three targets, and it gates: red there means something regressed. This directory
asks a different question — what can you actually *write* in a language this small?
— and the programs below are the answer. They are the ones a first course reaches
for, each written the way lux wants it rather than the way the textbook prints it,
and some of them stop early. Where they stop is a real part of what is here.

The directory does two things. The corpus documents what lux can carry, and it is
read by someone weighing lux up: a parent, a teacher, anyone deciding whether a
language this small can carry a first year, who would rather read working programs
than a feature list. The probing finds bugs, and most of what it finds is not in the
language at all — it is in what the compiler does that the interpreter doesn't, and
in the gap between what lux is and what lux says about itself.

So read the tally at the bottom of a run carefully. A green corpus is a statement
about coverage, not about correctness. Every program in it is a program that
*works*, so no number of them can say how the toolchain treats a program that
doesn't.

Nothing in this directory is a tutorial.

```
./flex.sh              # every program, every target
./flex.sh sieve bst    # just those
```

Each program runs through `lux run` and through its compiled Go, Rust, and Swift
translations, and the four outputs are diffed. The interpreter is the reference.
Every leg is built the way a learner would build it — `lux build` for Rust, a plain
`go build` or `swiftc` for the other two, no optimization flags — because a corpus
that tests a configuration nobody runs is measuring the wrong thing.
Every program here is deterministic, because lux has no way to produce a random
number and that is [deliberate](https://github.com/anderix/lux/issues/3) — so the
comparison needs no seeding and no tolerance. Same bytes, or a failure.

The run also counts warnings in the generated source, because agreeing on the answer
is not enough. Translations must compile warning-clean, on the grounds that a learner
should be able to read the generated code without meeting a complaint about a line
they didn't write.

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
| `worklist` | a `for` walks the row as it was when the loop began, so a list that grows is walked in rounds |
| `roman` | table-driven conversion, and why a function reaches only its own locals |

**Sorting.** One shape throughout: a row goes in, a sorted row comes back.
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
| `regex` | Pike's matcher — a pattern parsed to a tree, compiled to instructions, and run as threads, where copied arrays defeat the textbook closure and then pay for themselves on group capture |
| `stocktake` | `==` comparing compound values by what they hold, all the way down, and `<` refusing to |

**Grids.** An array of arrays, which is the shape most of a first course's
interesting problems arrive in. Nothing else in the repo uses one — `conformance`'s
Life deliberately flattens its board to `y * w + x` — so this section is the
main exercise of `[[T]]` across the four implementations.

| program | what it shows |
|---|---|
| `pascal` | rows of different lengths — a grid is a row of rows, not a rectangle |
| `matrix` | the triple loop worth memorizing, and a copy that leaves the caller's matrix alone |
| `lcs` | a table standing in for repeated work, which is all dynamic programming is |
| `tictactoe` | eight lines to check, and the third ending beginners forget |
| `queens` | backtracking, where value semantics remove the undo step and its classic bug |
| `maze` | breadth-first search, a queue that never discards, and a room the eye reads wrong |

**The other shapes.** An array holds more of the same thing. These are the three ways
lux holds something else: a number that isn't whole, a record whose parts aren't
alike, and a pair of types that can't be described apart.

| program | what it shows |
|---|---|
| `stats` | `int` and `float` refusing to mix, and why `total / count` is the wrong average |
| `points` | structs — a row of them, and one holding a row — plus distances kept squared so they stay whole |
| `logic` | two enums that contain each other, which is what a syntax tree becomes past one node type |

**Programs that do work.** Small filters that run in a pipe, exercising `args`,
`readLine`, stdout and stderr as separate channels, and the two different ways a
line can come back empty.

| program | what it shows |
|---|---|
| `catn` | `cat -n` — the smallest useful filter, and the read-until-empty loop |
| `head` | `args()`, `args()[0]` being the program itself, and a bad count that doesn't crash |
| `wcl` | all three of `wc`'s columns, the third built from `split` and `replace` together |
| `uniqc` | `uniq -c`, writable precisely because the real one only collapses adjacent runs |
| `hist` | a bar chart, scaling, and junk input counted rather than fatal |
| `fields` | `split` keeping every empty field, and why a trustworthy count catches a ragged row |

**The bridge out.** Every program above runs top to bottom, which is what lux does and
what a first year needs. This one has a `main`, and it is the only one that does —
because `main` is the last thing lux teaches, not the first, and a corpus that
retrofitted it onto everything would misrepresent how the language is learned.

| program | what it shows |
|---|---|
| `bridge` | `func main` mapped straight onto Rust's and Go's, and what a translation looks like when almost nothing has to be added |

It is written to be converted rather than read. Everything in it is arithmetic,
strings, and calling a function, so the Rust and Go that come back are the same four
functions in the same order with `main` in the same place — plus exactly one thing the
program didn't ask for, a four-line division guard, because `celsius * 9 / 5` could
divide by zero and lux promised a sentence about that rather than a crash. That is a
more useful picture of graduation than an empty one: you leave with a short list of
what the language was doing for you, and now you can read all of it.

Swift is the cousin already across — its top level is the entry point, like
lux's — so `main` there becomes an ordinary function and a call to it. Which is why the
bridge is Rust's and Go's.

**The one that ships.** Everything above was written for this directory. `keep` was
not — it is the world `lux crawl` writes out, it lives in
[`examples/`](../examples/keep.lux), and it is the program most people who try lux run
first. It is tested here because being the highest-traffic program lux ships is a
reason to check it more often than the rest, not less.

| program | what it shows |
|---|---|
| `keep` | a whole game as `step(world, cmd) -> World`, replayable by folding the same commands again |

The harness walks it to the chamber and out, which covers the ending
and the file it writes on the way — and passes through the chamber twice, so one run
takes both branches of "is there a copy already?" That only works because every leg
runs in its own empty directory; a shared one would have the first leg's side effect
show up as the second leg's failure.

## What it becomes

[CONVERT.md](CONVERT.md) puts a handful of these beside the Rust, Swift, and Go that
`lux convert` makes of them — a recursive type, a value that might be missing, a
value that might fail, and a copy that stays a copy. It is the graduation argument
in the only form that actually carries it, which is four columns rather than a
paragraph. `./convert.sh` regenerates the full translations if you would rather read
whole files.

Its last section does the same for the keep, one seam at a time — the world that
replaces itself, the borrow lux chose for you, two ways to be empty, and a file that
might already be there. Those are the seams a transition guide wants, and they carry
stable headings so a guide can point at one rather than working out which lines matter.
The keep is the right program for that job for the same reason it's the wrong one for
the sections above: a reader already knows it, so none of their attention is spent on
what it does.

## Where lux stops

The walls are not all the same kind, and treating them as one list would
misrepresent the language in its own favor.

**Deliberate, and staying that way.** There is no randomness, because lux's central
idea is that state is a value you can watch — `step(world, cmd) -> World`,
replayable by folding the same commands again — and a hidden die roll breaks that on
the first throw. A `Result` cannot be stored in a variable or handed to `print`, and
cannot be a parameter either; it is handled where it is produced or returned for the
caller to face, which is what keeps one source crossing three targets. There are no
classes, no user-defined generics, and no ownership. Each of those is somebody's
graduation lesson: Rust takes over for ownership, Swift for classes, Go for
goroutines.

`main` is not on this list. Define one and lux calls it, as the compiled targets do.
It is taught last, as the bridge out rather than the ceremony you open with, and
every program in this directory is deliberately main-free because that is how lux
itself teaches.

**A seam rather than a wall.** One place the four implementations knowingly differ.
`lux run` refuses to order a NaN — `cannot compare with NaN` — because IEEE's answer,
that every comparison with a NaN is false, is a wrong answer wearing a right one's
clothes. The compiled targets give IEEE's answer. Everything else about a non-finite
float agrees: `inf`, `-inf` and `NaN` print the same on all four, and `int()` of one
saturates the same way. Only ordering differs, and only after you have made a NaN.

**Not built yet, and wanted.** Strings cannot be indexed, and there is no map type.
Both are on the list, pulled by programs that needed them rather than by a wish
list. The map's absence shapes two programs here: `uniqc` collapses only adjacent
runs, and `safe` searches two rows kept in step where a real lookup would use a
map. Each says so where it stops.

**Taking a string apart.** `wcl` counts words with `split` and `replace` together,
and the shape of that is instructive. `split` keeps empty fields, deliberately, so that
position still means something in a row of data; splitting `"two  words"` on a space
therefore gives three pieces. A word count wants the opposite and skips the empties
itself, with `replace` folding tabs into spaces first so one separator covers both.
Two built-ins do one job between them, and the corpus agrees with `wc` on all three
columns. Every wall recorded here is a claim about the language as it stands, not
a permanent feature of it.

**Smaller edges.** A function sees only its parameters and its own locals — there
are no globals to reach up for, which is why `roman` keeps its tables inside the
function that walks them. There is no `break`, so a loop keeps its own answer to
"am I done?", which is a fair description of what `break` does anywhere. `+=` on an
array adds one element rather than joining two, so `bst` carries a four-line
`joinRows` to stitch a walk back together. An array handed to a function is a copy,
which `queens` uses to delete the undo step from backtracking and `regex` meets from
the other side: the recursive walk that marks a shared visited-set cannot work,
because the marks are made in the copy, so it keeps an explicit stack in an array
that only ever grows with a height index standing in for removal. A float literal has
no exponent form —
`1.0e10` doesn't parse — which `stats` never needs but is worth knowing before you
reach for it.

## How it finds things

Every program here runs correctly under `lux run`; where a target disagrees or won't
build, that is the finding. Run `./flex.sh` for the live state, and see
[the releases](https://github.com/anderix/lux/releases) for what has been fixed.

The byte comparison catches a target rendering something differently from the other
three, and that is most of what it finds. It cannot see the rest, so the probing uses
other instruments as well.

**Programs that fail.** Dividing by zero, going off the end of an array, recursing
without end. The interpreter explains those mistakes in words a beginner can act on,
and the compiled targets carry the same runtime errors; `lux convert` and `lux build`
run the checks `lux run` does, so a program the interpreter refuses is refused before
anything is emitted. A claim about the tool belongs in a test rather than in prose,
and the suite holds that one.

**Reading the emitted code.** A deep copy landing inside a loop prints identical
output while turning an ordinary grid walk cubic; only reading the translation shows
it.

**Asking what a learner would write.** Adding to a list while looping over it, or
naming a function `main`, are what a newcomer types first and what a careful program
walks around.

**Where implementations traditionally disagree.** Text that isn't ASCII, the file and
process built-ins, floats that stop being finite, both output streams at once, and
values large or deep enough to stress `string()`. Writing more ordinary programs does
not reach these.

**Specifying before building.** A new built-in is run against the real compilers
before it is written. lux compares strings at the Unicode-scalar level on every
target, so a Swift spelling that matches on graphemes or canonical equivalence is the
wrong one even when it is the obvious one.

The harness compares bytes exactly, with no normalization: a rule that smooths over a
difference trades a false failure today for a hidden bug tomorrow. `./excerpts.sh`
pulls every code block out of `CONVERT.md` and checks each line against the corpus
sources and against what the emitters produce, because a document made of quotations
needs its quotations tested.

## The rules

**A divergence means the target is wrong — until someone argues otherwise, in
writing.** The interpreter defines the language. When a translation disagrees, the
translation is fixed; a program is never edited to make a backend pass, and if a fix
would need the corpus changed, the fix is wrong. That has decided every finding here
but one. `nan < 1.0` is refused by `lux run` and answered
`false` by all three targets, and carrying the guard across would wrap every float
comparison a learner writes in a helper call — `if x < y` becoming `if luxLt(x, y)` in
the code lux asks them to read. That was judged too much to pay, on the leg where it
matters least, for a case you only reach after producing a NaN. So the divergence
stands, deliberately, and is written down rather than quietly tolerated. The rule is
the default and the burden of proof, not a law.

**The corpus never asks for a feature.** If lux can't express something, the
program doesn't get written and the wall goes in the section above. Every wall
reads like a missing feature in the moment, which is why the rule is
written down. `lux crawl` follows the same rule, built entirely on the language as
it stands.

**Findings leave as issues.** Work on the corpus and work on the language happen
separately, on purpose. Nothing in this directory has ever changed a line of lux.

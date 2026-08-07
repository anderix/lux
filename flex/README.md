# Flex

What the language can carry, and what the toolchain does when you lean on it.

`conformance/` asks whether lux keeps its promise that one source behaves the same
on three targets, and it gates: red there means something regressed. This directory
started by asking a different question — what can you actually *write* in a language
this small? — and the programs below are the answer. They are the ones a first course
reaches for, each written the way lux wants it rather than the way the textbook
prints it, and some of them stop early. Where they stop is a real part of what is
here.

That was the whole job once and it isn't now. Writing the programs mapped the reach,
and then the reach stopped moving while the *findings* kept coming — from probing the
toolchain rather than from adding another program. So this directory does two things.
The corpus documents what lux can carry, and it is read by someone weighing lux up: a
parent, a teacher, anyone deciding whether a language this small can carry a first
year, who would rather read working programs than a feature list. The probing is the
part that finds bugs, and most of what it finds now is not in the language at all —
it is in what the compiler does that the interpreter doesn't, and in the gap between
what lux is and what lux says about itself.

Which means the tally at the bottom of a run should be read carefully, and it is the
one thing worth carrying away from here. A green corpus is a statement about
coverage, not about correctness. Every program in it is a program that *works*, so no
number of them can say how the toolchain treats a program that doesn't — and that
turned out to be where the sharpest finding was hiding, under a suite that had been
fully green for months.

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
is not the whole bar. lux holds its own examples to translations that compile
warning-clean, on the grounds that a learner should be able to read the generated code
without meeting a complaint about a line they didn't write — and until recently nothing
applied that bar to this corpus, which is four times the size. One program was failing
it, and the cause turned out not to be the program (#69).

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
| `regex` | Pike's matcher — a pattern parsed to a tree, compiled to instructions, and run as threads, where copied arrays defeat the textbook closure and then pay for themselves on group capture |
| `stocktake` | `==` comparing compound values by what they hold, all the way down, and `<` refusing to |

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

Swift is the cousin already across — its top level is the entry point, exactly like
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

The harness walks it fourteen commands to the chamber and out, which covers the ending
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
misrepresent the language in its own favour.

**Deliberate, and staying that way.** There is no randomness, because lux's one
load-bearing idea is that state is a value you can watch — `step(world, cmd) ->
World`, replayable by folding the same commands again — and a hidden die roll
breaks that on the first throw. A `Result` cannot be stored in a variable or handed
to `print`, and cannot be a parameter either; it is handled where it is produced or
returned for the caller to face, which is what keeps one source crossing three targets.
That last clause is newer than the rest — the interpreter used to accept a
`Result`-typed parameter and run it, while Go couldn't emit one at all, and the rule was
tightened rather than the backend taught a trick. There are no classes, no user-defined
generics, and no ownership. Each of those is somebody's graduation lesson: Rust
takes over for ownership, Swift for classes, Go for goroutines.

`main` is not on this list, and the story of how it got off is worth a paragraph.
For one release `func main` was refused outright — lux runs a program from its first
line, so an entry point bought nothing but a build-time collision. It is now the
program's entry point: define one and lux calls it, exactly as the compiled targets
do. What changed is not the capability but where it sits. It is taught last, as the
bridge out rather than the ceremony you open with, and every program in this
directory is deliberately main-free because that is how lux itself teaches.

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

**One of these walls fell, which is the point of writing them down.** Until 0.18.0
a string could not be taken apart at all, and `wcl` printed a line saying so where
`wc`'s word count belongs. `split` arrived, so it counts words — and the shape of
the fix is worth more than the fix. `split` keeps empty fields, deliberately, so
that position still means something in a row of data; splitting `"two  words"` on a
space therefore gives three pieces. A word count wants the opposite and skips the
empties itself, with `replace` folding tabs into spaces first so one separator
covers both. Two built-ins doing one job between them, and the corpus now agrees
with `wc` on all three columns. A wall recorded here is a claim with a date on it,
not a permanent feature of the language.

**Smaller edges.** A function sees only its parameters and its own locals — there
are no globals to reach up for, which is why `roman` keeps its tables inside the
function that walks them. There is no `break`, so a loop keeps its own answer to
"am I done?", which is a fair description of what `break` does anywhere. `+=` on an
array adds one element rather than joining two, so `bst` carries a four-line
`joinRows` to stitch a walk back together. An array handed to a function is a copy,
which `queens` uses to delete the undo step from backtracking and `regex` meets from
the other side: the recursive walk that marks a shared visited-set cannot work,
because the marks are made in the copy, so it keeps an explicit stack in an array
that only ever grows with a height index standing in for removal. A float literal has no exponent form —
`1.0e10` doesn't parse — which `stats` never needs but is worth knowing before you
reach for it.

## What this found

The point of running every program on every target is that a program which only
works interpreted proves nothing. Every program here runs correctly under `lux run`;
where a target disagrees or won't build, that is the finding. Run `./flex.sh` for the
live state, which is the only account of it that can't go stale, and the
[CHANGELOG](../CHANGELOG.md) for the history — which is the argument for this
directory existing.

Most of what it found was a target rendering something differently from the other
three: an empty array literal's type in Go, an enum `var` taking its case's type, an
array printed without commas, a reversed range crashing Swift, value semantics leaking
through a Go slice, a string read out of an array refusing to compile on Rust, a
struct spent by being named in an array literal, a whole float printing as `88` where
the other three said `88.0`. Each was a bug in a target rather than a limit of the
language, each was filed and fixed, and none was found by the smaller `conformance/`
set. Two enums that refer to each other, and a variable a learner happened to name
`none`, came the same way.

The most valuable thing it found wasn't a rendering bug at all. Running the corpus
past where programs produce answers and into where they *fail* — dividing by zero,
going off the end of an array, recursing without end — showed that lux's best argument
stopped working at exactly the wrong moment. The interpreter explains those mistakes
in words a beginner can act on; the compiled targets replaced them with a Rust panic,
a Go goroutine trace, or a Swift register dump, and `lux convert` ran no checks at
all, so a broken program was translated in silence and the learner met rustc — pointed
at a generated file in `/tmp`, sometimes advised to write `mut xs`, which is precisely
what lux forbids. That is the graduation moment at its least forgiving, and the loud
half of it is fixed: the targets now carry lux's runtime errors.

The other half is open, and this paragraph claimed otherwise until it was tested.
`lux convert` does run checks now — but only some of them, and the sentence here said
"convert and build check a program before emitting it" for several releases without
anyone asking which checks. Ten static errors out of twelve pass straight through
`lux convert` with exit 0, including one where `lux build` produces a working binary
for a program `lux run` refuses: `print("Score: " + 42)` is an error under the
interpreter, with a `lux learn strings` help line, and a compiled program that prints
`Score: 42`. The rule being discarded is one lux explicitly teaches. That is #60, and
it is the reason a claim about the tool belongs in a test rather than in prose — the
same lesson `wcl` taught by printing an obsolete wall, one directory over.

Two of the instruments here are not the harness. Reading the emitted code found the
two findings a byte-comparison structurally cannot see — a deep copy landing inside a
loop, turning an ordinary grid walk cubic while printing identical output, and then
the copy itself turning out to be unnecessary, since lux forbids writing through a
parameter and Swift's copy-on-write arrays had been proving it all along. And asking
what a *learner* would write, rather than what a well-behaved program does, found the
two that the careful programs had walked around: adding to a list while looping over
it, which is how anyone first writes a queue, and naming a function `main`, which is
the first thing anyone arriving from another language types.

One more lesson came from the harness itself. It carried a rule erasing the trailing
`.0` from every output before diffing, added defensively back when no program printed
a float — and when one finally did, that rule turned a real divergence into a pass. It
is gone, and the corpus now compares bytes exactly. A harness that smooths over a
difference trades a false failure today for a hidden bug tomorrow.

The corpus went looking for the far side of the map next — text that isn't ASCII, the
file and process built-ins, floats that stop being finite, and both output streams at
once — and that turned out to be where the four implementations had drifted furthest.
Swift measured a string in graphemes and compared it canonically, so a family emoji was
one character where the others said five and two spellings of an accented letter were
the same string where the others saw two. `parseInt` refused a number with a space in
front of it on one leg out of four. `readFile` and `run` built four different failure
strings, one of them in Objective-C vocabulary and wrong about which failure had
happened. Floats printed in exponent notation lux cannot parse back, and infinity and
NaN were spelled three ways. A warning written to stderr jumped to the top of the file
when the output was piped. All of it is fixed, and the argument for that section of the
map is that not one of those was found by writing more ordinary programs — each came
from asking where language implementations traditionally disagree, and then looking
there.

One more came from the least promising place on that map. Probing whether anything
broke at scale — twenty-thousand-element arrays, two-hundred-square grids,
three-hundred-deep recursive values — found nothing, which was the expected answer. But
a value that deep was the first one big enough that measuring `string()` of it seemed
interesting, and the length came back 7701 under `lux run` against 1694 on Go.
`string()` of a compound had never been given the treatment `print` was given: Rust and
Swift refused to build it, and Go quietly returned a different string, which would then
flow onward into whatever the program saved or compared. Fixed. The lesson kept is that
the probe finds what it finds rather than what you predicted, so a row worth little is
still worth working.

The most useful thing this directory did, it did before there was any code to test.
`contains`, `replace` and `split` were specified before they were written, and the
spec proposed the obvious Swift spellings — `range(of:)`, `replacingOccurrences`,
`components(separatedBy:)` — noting that the three targets disagreed only on an empty
search string. Run against the real compilers, they disagreed on a great deal more:
those are Foundation calls that match on graphemes and treat canonically equivalent
text as identical, so an accented name typed on a Mac and the same name typed on Linux
would have been one string on Swift and two everywhere else, and a word count of a
family emoji would have split three ways. lux had already bought that argument once —
`==` on strings emits `unicodeScalars.elementsEqual` rather than Swift's `==`, which
is what closed the grapheme divergence in the first place — so the three new built-ins
were written to match at the scalar level too, and they shipped agreeing on all four
legs. Nothing had to be found, filed and fixed, which is the cheapest a finding ever
gets.

The most recent round is the clearest illustration of all of this, so it is worth
following from start to finish. 0.18.0's naming rule was
discovered by accident — a new built-in brushed against a corpus program and exposed
three silent semantic bugs that had sat under a fully green suite for months. That
suggested the suites were confirming what somebody had thought to test rather than
mapping what lux does, so the next pass went looking specifically for long-standing
behaviour no program reaches. It found, in about an hour, that comparing two arrays or
two structs with `==` does not compile on Go and silently answers wrongly for an
`Option` of one (#58), that three duplicate declarations the interpreter accepts are
refused by every target (#59), that `lux convert` and `lux build` skip the type checks
`lux run` enforces, so a refused program can compile and run (#60), and that Go does
not copy a value into an array literal, leaving the original and the stored copy
entangled (#61).

Four more followed from the same question asked of the things around the language
rather than the language itself: `readFile`'s failure text still read three different
ways, and did so inside the `io` learn card's own example (#62); all four editor
highlighting files knew fourteen of the seventeen built-ins (#63); `lux --help`
advertised a subcommand lux answers by correcting (#64); and the *gating* suite was
still normalizing away a float difference that had been fixed, so it carried a standing
agreement to ignore the exact class of bug it exists to catch (#65).

None of that needed a new instrument. It needed the admission that a corpus of programs
which *work* cannot tell you how the tool handles programs that don't, and that a green
tally is a statement about coverage rather than about correctness.

0.18.1 fixed eight of the nine, added no language surface, and left the corpus at 102
of 102. What remains open is the largest one: `lux convert` and `lux build` still run
only some of the checks `lux run` does, so `print("Score: " + 42)` is refused by the
interpreter — with a `lux learn strings` trail, and a rule the language teaches — and
compiled by `lux build` into a program that prints `Score: 42` (#60). Until that closes,
the four legs agree about what programs *mean* considerably more than they agree about
which programs *exist*. Two smaller ones are open beside it: a bare `some(x)` bound
without an annotation builds on three legs and not Swift (#67), and the new sweep of
the learn examples covers the cards but not the `more` pages (#68).

Every divergence found before these is either fixed or, in exactly one case, examined
and deliberately kept — see the seam in *Where lux stops*, and the first of the rules
below.

One instrument is now a script rather than a habit. `./excerpts.sh` pulls every code
block out of `CONVERT.md` and checks each line against the corpus sources and against
what the emitters actually produce. Two findings came from doing that by hand, and
doing it by hand missed four lines that had gone stale — a parameter renamed in
`bubble`, and three array subscripts written before indexing became a bounds-checked
call on every target. A document made of quotations needs its quotations tested.

## The rules

**A divergence means the target is wrong — until someone argues otherwise, in
writing.** The interpreter defines the language. When a translation disagrees, the
translation is fixed; a program is never edited to make a backend pass, and if a fix
would need the corpus changed, the fix is wrong. That has decided every finding here
but one, and the exception is worth stating because a rule with no known exception is
usually a rule nobody has tested. `nan < 1.0` is refused by `lux run` and answered
`false` by all three targets, and carrying the guard across would wrap every float
comparison a learner writes in a helper call — `if x < y` becoming `if luxLt(x, y)` in
the code lux asks them to read. That was judged too much to pay, on the leg where it
matters least, for a case you only reach after producing a NaN. So the divergence
stands, deliberately, and is written down rather than quietly tolerated. The rule is
the default and the burden of proof, not a law.

**The corpus never asks for a feature.** If lux can't express something, the
program doesn't get written and the wall goes in the section above. Every wall
reads like a missing feature in the moment, which is exactly why the rule is
written down. `lux crawl` was built the same way, entirely on the language as it
stood.

**Findings leave as issues.** Work on the corpus and work on the language happen
separately, on purpose. Nothing in this directory has ever changed a line of lux.

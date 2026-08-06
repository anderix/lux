# Changelog

All notable changes to lux are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and lux follows
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [0.19.6] - 2026-08-06

Three parity fixes, no new language surface. Each is a place where `lux convert` exited
cleanly and handed back code the target compiler then rejected — the author writing
ordinary lux, the break surfacing only in generated code they never read. All three were
found writing real programs.

### Fixed

- **A struct or enum field named for a target's keyword compiles everywhere.** Field names
  skipped the reserved-word escaping that function, parameter, and local names already
  went through, so a struct field named `move` or `box` (Rust's keywords) or an enum
  payload labelled `where` (Swift's and Go's) converted fine and then would not compile.
  Field names and enum payload labels now route through the same escaping on every
  backend — `move` becomes `move_` in the generated source while lux still prints the name
  you wrote — so an ordinary noun is an ordinary field name again, and nobody is asked to
  memorise three languages' keyword lists to name a field. Type names stay as written, the
  one identifier lux does not escape (#77).

- **A struct built with its fields named out of declaration order compiles as Swift.** lux
  lets you name a struct's fields in any order; Swift's memberwise initializer takes them
  in declaration order and rejects any other, so a constructor written short-fields-first
  produced Swift that would not build. The Swift backend now sorts the arguments into
  declaration order — named arguments are order-independent, so the program means the same
  thing and now compiles (#78).

- **A parameter named `stride` no longer breaks Swift's `for` loops.** A `for` loop over a
  range lowers to Swift's `stride(from:to:by:)`, so a function taking a parameter named
  `stride` shadowed the function each of its loops was built on, and Swift alone failed to
  compile. The emitted call is now fully qualified as `Swift.stride`, which no binding the
  author makes can get in front of (#79).

## [0.19.5] - 2026-08-06

A parity fix, no new language surface. It closes the last place the four legs disagreed
about the same source — a function reaching for a file-level value — by refusing it
everywhere the interpreter already did.

### Fixed

- **A function that reads a file-level `let`/`var` is refused on every path.** A function
  body sees its parameters and the program's functions and types, but never the value
  names around it — lux has no closures — and the interpreter refuses a read that reaches
  outward. `lux convert` and `lux build` didn't: they emitted a function naming something
  its scope doesn't hold, so `rustc` and `go` failed with a name error in their own words,
  and Swift — whose top-level `let` is a real global the function can see — compiled and
  ran a six-line program the reference implementation rejects. The check now runs before
  every command, so all four legs refuse it with the one message (the boundary note added
  in 0.19.4), and nobody meets a target compiler for a name lux already caught. It fires
  only for the real case — a read that finds nothing in the function's own scope and names
  a top-level binding — so a parameter or local of the same name resolves as it always did.
  The language question underneath, whether a function should be able to see a file-level
  value, is settled as no: lux stays closure-free, and a value a function needs is passed
  in (#76).

## [0.19.4] - 2026-08-06

Three fixes from a session that wrote real programs in lux — an ASCII Mandelbrot, a
poker tutor — and hit the seams a tutorial hits. No new language surface: one closes the
last run-vs-emit split in the `Result` rule, one makes a native build survive a closed
pipe the way the other three legs already do, and one repairs a teaching trail that led
to the wrong idea.

### Fixed

- **A stored or printed `Result` is refused on every path, not just where the line
  runs.** The interpreter catches a `Result` kept in a `let` or handed to `print`, but
  only on a line it executes — so the misuse in a branch that never ran slipped past `lux
  run` while `lux convert` and `lux build` refused it, the run-vs-emit split 0.19.2 closed
  for built-in argument types. The check now runs before any command on every path, so a
  dead branch is caught up front too. It can't turn away a valid program — a `Result` is
  never storable — and it also settles a case where the two legs disagreed: `print(readFile(x))`
  now reads `a Result can't be printed` under both `lux run` and the emitters, where run
  said that and convert reported the argument type first.

- **A built native binary ends quietly when a pipe closes.** `program | head` stops a
  program early, and the interpreter and the Go and Swift builds all end on `SIGPIPE`
  without a word — but the Rust build panicked with a note about a file in the Rust
  standard library, since Rust's runtime ignores `SIGPIPE` and turns a closed pipe into an
  error `println!` unwraps. The emitted `main` now restores the default `SIGPIPE`
  disposition first, so the native binary is the fourth leg to end quietly rather than the
  one that blames the learner for a pipe working as intended. Dependency-free and
  `#[cfg(unix)]`, since the panic it prevents is a Unix pipe.

- **A file-level name read inside a function names the boundary that hid it.** A function
  body sees its parameters and the program's other functions and types, but never the
  `let`/`var` names around it — lux has no closures — so a tuning constant at the top of a
  file, reached for inside the function it tunes, failed with `is not defined`. The generic
  note then sent the learner to `lux learn scope`, whose card confirmed the wrong model. The
  error now says the name exists at the top of the file and a function only sees what it is
  handed, and the scope card gains the sentence it was missing. The note fires only for the
  real case — a top-level name, read from inside a function — so a plain typo and a
  top-level use-before-declaration keep the ordinary wording.

## [0.19.3] - 2026-08-06

A diagnostics fix, no change to what lux accepts or rejects. It corrects the error a
dropped `}` produced — the commonest structural slip, and the one that pointed nowhere
useful.

### Fixed

- **A missing `}` now points at the block it left open, not the end of the file.** A
  block parses statements until it meets `}` or runs out of tokens, so a dropped brace let
  the parser swallow everything after it and only notice at the end — the `expected '}' to
  close the block` error landed on the blank line past the last one, the right kind of
  error at the wrong place. It now names the block that was never closed and points the
  caret at the `{` that opened it, the region actually worth reading, with a note that the
  brace runs to the end of the file.

## [0.19.2] - 2026-08-06

Another hardening release, no new language surface. It closes the last concrete-type
checks the static pass didn't reach — the built-in functions' argument types — and the
last declaration-level divergence where `lux run` refused a program the three target
languages emitted anyway. It also corrects a compatibility claim the 0.19.0 and 0.19.1
notes stated too broadly.

### Changed

- **A built-in called with the wrong argument type is caught on every path.** The static
  check reached a wrong argument to a function you wrote, but not to a built-in one:
  `length(5)`, `contains(5, "a")`, `replace`/`split` on a non-string, `parseInt`/
  `parseFloat` on something that isn't text, `int`/`float` handed a string or anything
  that isn't a number, and the file and process seams `readFile`/`writeFile`/`run`. On a
  path that never ran, each slipped past `lux run` and reached the target compiler as an
  error in its words, not lux's. They are now refused ahead of running or emitting, in
  the interpreter's own wording, and still only where the argument's type is concretely
  known. One of these also emitted a wrong program: `int("x")` on Swift compiled to
  `Int("x")`, which is failable and prints `nil` — a value lux has no concept of, from a
  program lux refuses; the check now turns it away before any Swift is emitted (#70).

### Fixed

- **Two structs, enums, or functions of the same name are refused on every leg.** A
  second `struct S`, `enum E`, or `func f` was caught by `lux run` — `type `S` is already
  defined` — but `lux convert` and `lux build` emitted both definitions and left the
  target compiler to reject the duplicate in its own words, with no lux message on the
  path. Writing a second `func total(...)` further down a file rather than editing the
  first is the ordinary way this is met. It is now refused up front on every leg, with
  the message and span `lux run` already gave (#71).

- **A compatibility claim is corrected.** The 0.19.0 and 0.19.1 notes said the static
  check never rejects a program the interpreter would accept. It can: a `-> type`
  function whose only `return` sits inside a `while true` ran on the interpreter but not
  on Swift or Go, which reject the emitted code as a missing return, and the check now
  refuses it on every leg — the right answer, a split verdict replaced by one, but not
  what those words promised. The accurate claim, the one the corpus demonstrates, is that
  no program that behaves the same on all four legs is turned away; the wording in both
  entries above is fixed to say so (#72).

## [0.19.1] - 2026-08-06

A hardening release, no new language surface. It finishes the static type check that
0.19.0 introduced — extending it from the ten commonest rules to the whole of the
interpreter's concrete-type checking — and closes a gap in what the teaching material
is tested against.

### Changed

- **The static type check now covers every concrete-type rule the interpreter has.**
  0.19.0 caught the ten commonest on every path; the rest still fired only on a line
  that ran. Now a comparison between two types, a `!` on a number or `-` on a
  non-number, an out-of-type index or a range whose ends aren't ints, a struct or enum
  value built with wrong or missing fields, a match on something unmatchable or a value
  match with no `_`, and a `-> type` function that can run off its end are all refused
  ahead of running or emitting — in the interpreter's own words, on any path. It still
  declines every case it can't pin to a concrete type, so no program that behaves the
  same on all four legs is turned away (#60).

### Fixed

- **The teaching material's deeper pages are tested too.** `lux learn`'s agreement
  sweep — every example run on the interpreter and each backend, held to one output —
  covered each topic's leading card but not the code blocks on its earned *more* page.
  The learn parser now keeps each fence's language, so a lux block can be told from the
  Rust, Go, and Swift ones the `main` page shows on purpose, and every lux block on a
  more page is swept the same way. The blocks this reached already agreed (#68).

## [0.19.0] - 2026-08-05

lux is now statically type-checked. The interpreter always checked types as it ran, so
a type error tucked inside a branch that never executed slipped past `lux run` and only
surfaced when a target compiler rejected the build — the same source legal on one leg
and not another. A new pass applies those same type rules to every path, before any
command, so the four legs — the interpreter, Rust, Swift, and Go — agree on what counts
as a valid program. It enforces the rules the language already had, in the words the
interpreter already used; it adds no new type system, and no generics. Two backend
divergences ride along.

### Changed

- **Type errors are caught on every path, not only the ones that run.** `lux run`,
  `lux convert`, and `lux build` now share one static type check that enforces the
  interpreter's concrete-type rules — string-and-number arithmetic, mixed int and
  float, a non-bool condition, looping something that isn't an array, a wrong argument
  or return type, a var reassigned to another type, a non-exhaustive match, an unknown
  struct field — ahead of running or emitting. A mistake inside `if false { ... }` is
  refused up front, with the same message and `lux learn` trail it would have shown at
  run time, rather than compiling to a cryptic error from the target compiler. The pass
  never turns away a program that behaves the same on all four legs: wherever it cannot
  pin a concrete type — an empty array, a bare `none` — it stays silent and leaves the
  call to run time and the target compiler (#60).

### Fixed

- **A bare `some(x)` binding builds on Swift.** `let a = some(5)` — an optional bound
  without an annotation, the ordinary way a learner first reaches for one — ran on the
  interpreter and built on Rust and Go, but failed on Swift, which emitted `.some(5)`
  with no type to resolve it against. The Swift backend now carries the inferred element
  type onto the binding — `let a: Int? = .some(5)` — the way it already did for a
  written annotation. A bare `none`, whose element type really is open, keeps its own
  "say what it holds" rule (#67).

- **A valueless `var` no longer emits a dead initializer on Rust.** `var s: string`,
  assigned before it is used, emitted `let mut s: String = String::new();` — a zero the
  program never reads, which Rust warns about, in code the learner didn't write. The
  binding now defers its value where Rust can see it is assigned before it is read —
  straight-line, or both branches of an `if` — and keeps a reachable initializer only
  where it can't, such as an assignment reached only inside a loop. The output stays
  warning-clean and still compiles (#69).

## [0.18.1] - 2026-08-05

A hardening release: eight parity and correctness fixes, and no new language surface.
Each change makes the four legs — the interpreter, Rust, Swift, and Go — agree where
one had drifted, or refuses on every path what only one leg was already refusing. The
set of programs that work identically on all four is larger; the set of things the
language can do is unchanged.

### Fixed

- **Comparing compound values with `==` on Go.** `==` and `!=` on an array, a struct
  that holds one, or two cases of an enum did not build on the Go backend, and
  `some(a) == some(a)` compiled and returned `false` — a lux `Option` is a Go pointer,
  so the comparison read the address rather than the contents. A generated `luxEqual`
  now compares structurally, the way the interpreter, Rust, and Swift already did (#58).

- **Value semantics into an array literal on Go.** A place stored as an array-literal
  element — a struct holding an array, or a bare array — kept sharing its inner slice
  with the source, so mutating the source afterwards reached into the stored copy. The
  typed array path now deep-copies each element, like every other place a value flows
  into a new home (#61).

- **`readFile`'s failure reason reads the same on every leg.** The reason half of a file
  error differed three ways — the interpreter and Rust appended Rust's ` (os error 2)`,
  Swift dropped it, Go dropped it and lowercased the sentence. All four now build the
  reason from the same `strerror` form — `No such file or directory`. That program is
  the `io` learn card's own example, so the teaching material no longer contradicts the
  parity claim (#62).

- **`lux --help`** no longer advertises `lux editors install`, a name lux renamed to
  `highlighting` and now answers by correcting (#64).

- **Editor highlighting** for all four editors — GtkSourceView, nano, Notepad++, and
  Vim — now colours `contains`, `replace`, and `split`, the built-ins 0.18.0 added
  (#63).

- **The conformance suite** compares every byte, dropping a normalization that had
  outlived the whole-float rendering difference it once smoothed over — the exact class
  of difference the gating suite exists to catch (#65).

### Changed

- **An empty array literal now needs a type annotation.** `var xs = []` left its element
  type open: the interpreter guessed it from later use, Swift and Rust refused it, and
  Go inferred `[]any` and failed wherever the variable later met a typed position. lux
  now asks for the annotation up front — `var xs: [int] = []` — the same rule an empty
  `none` already meets, so all four legs agree it's illegal until named (#66).

- **Three more duplicate declarations are refused.** No enum may name a case twice, no
  struct a field twice, and no function may take a type's name — each was accepted by
  the interpreter and refused by the compiled targets, so each is settled now where the
  program is checked, on every path (#59).

## [0.18.0] - 2026-08-04

### Added

- **`contains`, `replace`, and `split`** — three string built-ins, siblings of
  `length`. `contains(s, needle)` asks whether one string appears inside another,
  `replace(s, from, to)` swaps every occurrence of one piece of text for another, and
  `split(s, sep)` breaks a string into an array of pieces at a separator. They match
  at the Unicode-scalar level — the level `length` counts at and `==` compares at — so
  the interpreter and all three targets agree even on text that is normalized
  differently or built from emoji; the Swift backend uses scalar helpers rather than
  Foundation's grapheme-aware search for exactly this reason. An empty search or
  separator is refused with a lux error instead of one of the three answers the
  targets disagree on. The built-in count is now seventeen. `contains` and `replace`
  join the `strings` lesson; `split` joins `arrays`, where an array made from real
  input gives `for` its first genuine job.

### Changed

- **One naming rule: built-in names are reserved, and a name can't shadow one already
  in scope.** A declared name — a variable, parameter, loop variable, match capture,
  function, or type — must be new where it is introduced: not a built-in (function,
  value, or type), not already the name of a function or type, and not still in scope
  from an enclosing block. This replaces several quiet, inconsistent behaviours a
  learner could stumble on: `func length` was silently dead code (the built-in won),
  `let none = 5` rebound the empty `Option`, and an inner block could shadow an outer
  name without a word. All of them now meet the same clear error lux already gave for a
  same-scope clash, and built-in names behave like the keywords they are. Reusing a
  name across blocks that never overlap — two separate loops, a parameter that repeats a
  top-level name — is still fine. (This reverses the #19 behaviour that let a variable
  shadow `none`.)

## [0.17.4] - 2026-08-04

### Added

- **`lux learn trace`** — a card for the `lux trace` tool, next to `errors` in the
  topic list and the tour. `lux trace` is aimed at a beginner whose program runs but
  gives the wrong answer — the situation with no error to read — and it had appeared
  only in `lux --help`, invisible to exactly the people who need it. The card shows a
  small program and its trace; the more page names where a wrong-answer bug usually
  hides (#56).

### Changed

- **The README opening is reworded and sharpened.** "The bigger languages exist to
  teach ownership, classes, goroutines" was a false claim about those languages'
  purpose, made to prop up a claim about lux; it is now "the ideas those bigger
  languages are built around, and they land better once you have written something
  that needs them." And "fits on one page" gains its countable version — "fourteen
  keywords and fourteen built-in functions" — which the lexer now pins: the keywords
  moved into a `const KEYWORDS: [(&str, Tok); 14]`, so adding one is a compile error
  against the count, and a test asserts both numbers against `BUILTINS`. A documented
  claim is now a checked one (#55).

## [0.17.3] - 2026-08-04

### Changed

- **`lux editors install` is now `lux editors highlighting`.** "editors install" read
  like it was about to install an editor; "highlighting" names what the command
  actually writes. Bare `lux editors` still reports status, and the old word now meets
  a lux-flavoured "knows `highlighting`, not `install`" rather than failing blankly.
- **gedit is no longer claimed as supported.** Modern gedit forked GtkSourceView into
  `libgedit-gtksourceview` and reads its own language-specs path, so the `.lang` lux
  writes to `~/.local/share/gtksourceview-5/` never reached it — it never actually
  worked. GNOME Text Editor (real GtkSourceView 5) is the covered editor; the report
  and the "looked for" list no longer mention gedit.

### Fixed

- **A program whose reader stops early no longer panics.** `lux run prog | head` — or
  `| less` and quitting, or `| grep -q` — now dies quietly on SIGPIPE (exit 141) like
  `seq | head` and the Go and Swift translations, instead of a rustc backtrace about a
  broken pipe (exit 101), the least lux-like output the tool produced. Rust's runtime
  sets SIGPIPE to `SIG_IGN`; the interpreter restores the default disposition at
  startup, dependency-free (#57). The Rust translation is deliberately left as-is —
  fixing it would clutter the generated code learners read, for a gap only a compiled
  binary piped into `head` exhibits.

## [0.17.2] - 2026-08-02

### Fixed

- **`string()` of a struct, enum, option, or array renders it lux's way.** 0.15.0
  routed `print` of a compound value through a generated `luxShow` on every backend;
  `string()` of the same value was never given the same treatment, so it diverged
  exactly the way `print` used to. Rust and Swift wouldn't build a `string(struct)`
  at all; Go quietly produced a different string — `{1 2}` for a struct, an array
  without commas — that then flowed on into whatever the program saved or compared,
  one thing under `lux run` and another after `lux build`. `string()` of a compound
  now calls `luxShow`, the same renderer `print` uses (#54).

- **`every_topic_runs` no longer hangs on an open stdin.** The test ran each `lux
  learn` example through the in-process interpreter against the ambient stdin, and
  the `input` topic's example reads stdin — so on a terminal or an open pipe the
  whole suite blocked forever in that one test, with no output. It now runs each
  example through the binary with an explicitly empty stdin (#53).

## [0.17.1] - 2026-08-02

A pass of the flex method — a corpus run through `lux run` and its compiled Rust,
Go, and Swift, every byte diffed — from the far side of the map: non-ASCII text,
the file and process built-ins, non-finite floats, and the two streams at once.
Thirteen findings, closing the gap between what the reference implementation does
and what the targets do. The flex corpus reaches 96/0. Plus the corrections the
0.17.0 `main` lesson earned once its Swift claims were checked.

### Fixed

- **Strings count and compare by Unicode scalar on Swift.** `length` of a family
  emoji was 1 where the interpreter, Rust, and Go all say 5, and two spellings of an
  accented letter compared equal where the others saw two strings — Swift measured
  graphemes and compared canonically. It now counts `unicodeScalars` and compares
  them, matching the code-point order the others get from their bytes. `lux learn
  strings` states the seam (#49). `length(a + b)` is parenthesised so `.count` binds
  to the whole thing (#50).
- **The Result rule holds where the value is produced, on every path.** A stored
  Result was refused by `lux run` but built and printed `Ok(...)`/`success(...)`
  anyway; a Result parameter ran interpreted, built on Rust and Swift, and emitted
  invalid Go. Both are refused now with the interpreter's own message before anything
  is emitted (#39, #42).
- **I/O errors name what was attempted, the same way everywhere.** `readFile`,
  `writeFile`, and `run` built four different failure strings; Swift's leaked
  Objective-C vocabulary and was factually wrong on a permission denial. Every target
  now builds `could not read/write/run <path>: <reason>`, always naming the path;
  Swift reads and writes through POSIX to get an accurate reason (#43). A missing
  program on Swift is now an `err`, not an `ok` with the wrapper's status 127 (#48).
- **Floats render positionally everywhere.** Small values printed in exponent form
  on the targets (`1e-5`), which lux can't parse back; a non-finite value printed
  three ways (Go `+Inf`, Swift `-nan`). All render through a `luxFloat` helper
  matching the interpreter — positional, decimal point kept, `inf`/`-inf`/`NaN`
  normalised (#47, #52). `int()` of a non-finite saturates instead of trapping
  (Swift) or going undefined (Go), the way the interpreter's `as i64` does (#52).
- **The `help:` trail survives to compiled binaries.** The runtime errors 0.16.x
  moved onto the targets kept their diagnosis but dropped the `help:` line — the part
  that names the rule and its `lux learn` topic. It's a constant string, so the
  bounds guard emits it again (#40).
- **Two Go codegen gaps.** A loop that names its variable but never reads it now
  drops to `for range xs` instead of emitting an unused-variable error (#44); a
  nested empty array literal keeps its element type instead of degrading to `[]any`
  (#45).
- **Two silent-wrong-answer sleepers on Swift.** `parseInt`/`parseFloat` now trim
  surrounding whitespace the other three accept, instead of returning `none` for a
  number with a stray space (#41); and `eprint` flushes stdout first, so a warning
  stays with the line it follows when the streams are merged into a pipe, rather than
  jumping to the top (#51).

### Changed

- **A Result parameter is refused on `lux run` too.** Where the interpreter used to
  accept a `Result`-typed parameter and run, it now refuses it — the same rule the
  `let` case enforces — so `lux run` agrees with the targets rather than accepting
  what Go can't emit (#42).
- **The `main` lesson tells the truth about Swift.** `main` is the shape Rust, Go,
  Java, and C require; Swift, like lux, lets the file be the program, so it has no
  `main` step. The graduation lesson is scoped to Rust and Go, and no longer implies
  every language requires a `main`. Also fixed a card-rendering bug that had been
  dropping content after a topic's first code fence (the `main` card's `func main`
  example, and `arrays`' grid example), with a test to guard it.

## [0.17.0] - 2026-08-02

lux gains the one piece of ceremony the C-family languages start with — and it
arrives last, as the bridge out. A program never needed a `main`, and still
doesn't: with none, the file is the program and runs top to bottom, the beginner's
whole world. But a top-level `func main` is now the program's entry point, and lux
runs it for you — the shape Rust, Go, Java, and C require (Swift, like lux, lets
the file be the program), met here for the first time with everything else already
understood. Where 0.16.0 refused
`func main` to spare a learner a build-time collision, it now accepts it and teaches
it: the same hello-world you started with, this time with `main` around it and every
character still yours.

### Added

- **A top-level `func main` is the program's entry point, and lux runs it.** Define
  one and lux calls it — no `main()` of your own — exactly as the compiled targets
  do. Each backend maps your `main` straight onto its own `fn main`/`func main` with
  no wrapper, so the generated code is the idiomatic thing a person would write and
  reads as the bridge it is; Swift, whose top level is already the entry point like
  lux's, gets a single `main()` call to start it. The interpreter and all three
  targets produce the same output.
- **A `main` learn topic — the capstone bridge.** `lux learn main` teaches the entry
  point as the last lesson before leaving: the hello-world you began with, dressed to
  travel, then the same program in Rust, Go, and Swift where every token now has a
  name. The starter world and every concept lesson stay main-free on purpose, so
  `main` lands as the single new idea it is. Its `more` page is the first to render
  fenced code blocks verbatim.

### Changed

- **`func main` is accepted, not refused.** The 0.16.0 refusal became a set of
  rules that make `main` an entry point rather than a mistake: it takes no values,
  returns nothing, shares the top level with nothing but definitions, and isn't
  called by hand — four errors that are one idea, each stating it and pointing at
  `lux learn main`. The rules run on `lux run`, `lux convert`, and `lux build`
  alike, so a program that breaks one meets a lux error in its own words on every
  path, never rustc's about a `main` it never wrote. A `func main` nested inside
  another function is untouched — an ordinary local.

## [0.16.1] - 2026-08-02

The fourth member of the runtime-error family — after runaway recursion, division
by zero, and overflow — reported the same way as division by zero.

### Fixed

- **An out-of-bounds index reports a lux error on the compiled targets.** Going
  past the end of an array is the most common runtime mistake a beginner makes, and
  the interpreter's message for it is the richest of the family: it names the index,
  the length, and the valid range, and its `lux learn arrays` trail states the
  off-by-one rule that caused it. After `lux build` that was lost — a Rust panic, a
  Go goroutine trace, a Swift register dump. Each backend now bounds-checks an index
  and reports in lux's words, on a read and on a write, and the output printed before
  the fault survives. A read borrows the element through a helper that evaluates its
  base once, so a nested `grid[i][j]` stays as cheap as before; a write checks each
  index as a statement ahead of the assignment. Ordinary indexing is unchanged, and
  the whole flex corpus still matches.

## [0.16.0] - 2026-08-02

Another pass of the same method — a corpus run through `lux run` and its compiled
Go, Rust, and Swift, every byte diffed — pushed past where a program prints the
same answer into where it *fails*: dividing by zero, overflowing an int, recursing
without end, calling a name that isn't there. These are the mistakes a beginner
actually makes, and they were exactly where the four implementations diverged most
and where `lux build` handed the learner a wall of rustc. Twelve findings, all
resolved. Some tighten what lux accepts — `func main` is refused, and `lux convert`
and `lux build` now check a program before emitting it — so this is a minor
release, not a patch.

### Changed

- **Integer overflow wraps on all four legs.** Swift trapped on overflow while the
  interpreter, Go, and release-Rust wrapped, and `lux build` — a debug rustc with
  overflow checks on — trapped too, so `lux run` and `lux build` disagreed over a
  flag nobody chose. The four now agree: integer arithmetic wraps. Trapping would be
  more honest about a silent wraparound, but it would wrap a guard-helper call around
  every `+`, `-`, and `*` in the generated code — the most basic arithmetic a learner
  reads — to catch a case a beginner essentially never reaches, unlike dividing by
  zero. Keeping the four in step and the generated code readable won. The interpreter
  wraps; Go already did; `lux build` compiles with overflow checks off so its Rust
  wraps and matches `lux run`; Swift takes its masking operators (`&+ &- &*`). Go and
  Rust source are exactly as readable as before.
- **`func main` is refused, with a reason.** `main` is the first function name a
  learner arriving from C, Java, Go, or Rust reaches for, and lux ran it fine, then
  generated its own `main` as the entry point, so the program wouldn't build on Rust
  or Go. It's now refused at the definition, on every command: lux runs a program
  from its first line and has no entry point to declare.
- **The recursion limit tells the truth, and is higher.** 0.15.1 stopped runaway
  recursion from aborting, but the message diagnosed a missing base case as fact —
  a lie to the learner whose program has a base case, reaches it, and simply nests
  deep. Depth alone can't tell the two apart, so the error now names the limit and
  offers both readings, pointing a genuinely deep program at `lux build` to run past
  it. The limit itself was 10,000, low enough to reach by accident on a real file;
  it's now 25,000, on a larger interpreter stack so the guard, not a stack overflow,
  is what stops a runaway even in a debug build. `lux build --help` notes the other
  direction: the compiled targets don't carry the guard, so a runaway there hangs or
  crashes, which is why you run a program with `lux run` while finding its bugs.

### Fixed

- **`lux convert` and `lux build` check a program before emitting it.** `lux run`
  catches a broken program and explains it in lux's words; convert and build did no
  checking, so the same program was translated and the learner met rustc — pointed at
  a generated file they never wrote, sometimes with advice (`mut xs`) that is exactly
  what lux forbids. Both now run the structural checks first, refusing with the
  interpreter's own message: a call to a function that isn't there or takes the wrong
  number of values, and a write through a parameter. The type-directed rules are left
  to the target compiler for now, since a static answer would risk refusing a valid
  program.
- **Division by zero reports a lux error on the compiled targets.** The interpreter
  said `division by zero` and exited; the built binary showed the host runtime doing
  it — a Rust panic trace, a Go goroutine dump, a Swift register dump. Integer `/` and
  `%` now guard the divisor and report in lux's words on every target. The three
  already detected the zero; only the message was wrong.
- **Rust: a struct named in an array literal is cloned, not moved.** Naming a value
  and then listing it — `let rect = [origin, …]` — moved it, so a later
  `print(origin)` wouldn't compile. The array element was the one move site the clone
  set missed.
- **Rust: changing an array while looping over it compiles.** Adding to a list while
  walking it — the first version of a queue or a flood fill — held the array borrowed,
  so the append wouldn't build, where the interpreter, Swift, and Go all accept it. It
  now iterates a snapshot, matching their semantics and releasing the borrow.
- **A read-only array parameter isn't copied at every call.** A function that returns
  a scalar can't leak a parameter's backing, and lux already forbids writing through a
  parameter, so the copy that guarded an accessor like `cols(m)` was defending against
  a write the language won't allow — and inside a loop it turned an O(n²) walk into
  O(n³). Go now passes such an argument as-is and Rust borrows it; Swift was already
  right, on copy-on-write.
- **Go: a whole float prints with its decimal point.** `fmt` rendered a `float`
  holding 88.0 as `88`, indistinguishable from an int — erasing the very distinction
  lux enforces at every arithmetic. A float now renders lux's way, as a scalar, inside
  an array, and through `string()`.
- **Go: `int()` of a float literal compiles.** `int(3.9)` emitted Go that Go rejects —
  it won't truncate a constant — so a float conversion now reaches the truncation as a
  runtime value.
- **Swift: an annotated `Option` binding keeps its type.** `let a: Option<int> =
  some(5)` dropped the annotation, leaving `.some(5)` with nothing to infer from. The
  annotation the program wrote is carried through.
- **A user type named `LuxShow` doesn't collide with the printer.** The trait (Rust)
  and protocol (Swift) injected for compound printing took a name a learner could also
  choose, so a `struct LuxShow` wouldn't build. The generated name now steps aside, so
  the learner keeps the name.
- **Rust: a large integer literal compiles.** A bare literal defaults to `i32` in
  Rust, so a number past that range — three billion, ordinary in a real file —
  overflowed the default type at compile time when it landed in an expression rather
  than an annotated binding. It now carries an `i64` suffix where it needs one; small
  literals stay bare.

## [0.15.1] - 2026-08-01

A follow-on to 0.15.0 by the same method that produced it: writing a corpus of
small programs and running each through `lux run` and its compiled Go, Rust, and
Swift, then diffing every byte. A new set of grid programs — an array of arrays,
the shape most of a first course's interesting problems arrive in — put `[[T]]`
through the four implementations for the first time and surfaced six more
divergences, each a bug in a target or the interpreter rather than a limit of the
language. All six are fixed, and nothing a working program did has changed.

### Fixed

- **Mutually recursive enums compile on Rust and Swift.** A self-referential enum
  already compiled everywhere, but two enums that refer to each other — an `Expr`
  that holds a `Fn` that holds an `Expr`, the shape any syntax tree takes once it
  grows past a single type — ran interpreted and on Go yet failed on Rust
  ("recursive types have infinite size") and Swift ("not marked indirect"). Go was
  fine because its enum lowers to an interface, already a pointer. The pass that
  places Rust's `Box` and Swift's `indirect` looked for an enum that names itself;
  it now follows the enum reference graph, so a field whose type cycles back gets
  the indirection wherever the cycle runs. Direct self-reference falls out of the
  same test, so the existing recursive types are unchanged.
- **A variable named `none` means the variable.** `none` names the empty `Option`,
  but a program that binds it as an ordinary name — `let none = 5` — meant the local
  under `lux run` and then read as the built-in at every use site on all three
  targets, compiling nowhere. Name resolution now takes the local first, the way it
  already did for the other eight built-in names, and a non-`Copy` value bound to
  `none` carries value semantics like any other place.
- **Go: a discarded loop variable compiles.** `for _ in 0..n` — the natural way to
  say "do this n times" — lowered `_` into all three slots of Go's C-style `for`,
  each of them invalid (`_ := 0`, `_ < n`, `_++`); the array form `for _ in xs`
  lowered to `for _, _ := range xs`, which has no new name on the left. A range now
  gets a throwaway counter and an array iterates with Go's bare `for range xs`.
  It's the spelling lux's own emitter writes for an unread loop variable, so the Go
  backend had to accept it back.
- **Rust: a string read out of an array and returned compiles.** `return row[c]`
  over a grid of strings couldn't move a `String` out of a `Vec` index; a returned
  value now copies a place the same way a binding or a call argument already did.
  Over `[[int]]` it compiled anyway, because an int element is `Copy` — which is what
  made it easy to miss, since it's the accessor every grid program writes.
- **Go: a computed loop bound is evaluated once, not every pass.** lux evaluates a
  range's bound once, but Go's C-`for` re-checks its condition every iteration, so a
  bound that's a call — `for i in 0..rows(m)` — ran every pass; and since 0.15.0
  deep-copies a grid handed to a function, an ordinary O(n²) walk went cubic. The
  bound is now hoisted to a variable before the loop, and only a literal, which
  can't change, stays in the condition. Same output, an order of magnitude less
  work.
- **Runaway recursion reports a lux error instead of aborting.** Recursion with no
  reachable base case — the classic beginner mistake — was the one place the
  interpreter fell through to its host language: a raw stack overflow, `SIGABRT`,
  exit 134, and not a word about the program that ran. The interpreter now counts
  how deep calls nest and stops at a limit with an ordinary lux error that names the
  function and points at the base case, exiting 1 like every other. To keep that
  limit in charge it runs on a larger stack, which also lets a correct program that
  simply recurses deep run where it used to abort — thousands of frames where the
  ceiling used to sit near two thousand.
- **The unknown-function note lists every built-in, and suggests a near miss.** A
  call to a name that doesn't exist names the built-ins it might have meant — the
  one place a stuck learner is told what exists — but the list had drifted three
  names behind the real set: `input`, `parseInt`, and `parseFloat` all work and are
  taught, and none appeared, so a learner reaching for a number-from-text parser saw
  a list with no parser on it. The note now renders a single source rather than a
  retyped list, so it can't drift again. And a near miss — a typo or a case slip
  like `parseint` or `readline`, the built-ins and the program's own functions alike
  — is redirected to the name that was meant, with the list kept for a name that's
  genuinely absent.

## [0.15.0] - 2026-08-01

The release that makes the three targets behave *identically* to `lux run` — not
just compile, but produce the same output for the same program, value semantics
and all. It was found by writing a corpus of small programs — sorts, a BST, an
expression evaluator, a state machine, Unix-style filters — then playtesting
realistic ones, running each through `lux run` and its compiled Go, Rust, and
Swift translations and diffing every byte. Dozens of programs now match on all
four. It also sharpens several diagnostics a learner meets early, turning raw
parser errors into trails that name the cause and point at the fix.

Most of this is bug fixes, but two things change behaviour a program could
notice, which is why it's a minor release: `print` of a compound value now reads
lux's way on every backend rather than each language's default, and `print` of a
`Result` is refused where the interpreter used to allow it.

### Changed

- **Printing a `Result` is refused.** The interpreter let `print(ok(5))` render
  `ok(5)`, but a Result is Go's `(value, error)` pair, not a single value to hand
  to `print`, and lux's own rule is to match a Result where it's produced. It's now
  refused with a trail that points at matching it and printing each side — the same
  rule that already keeps a Result out of a `let`.

### Fixed

- **Printing a struct, enum, or option reads the same on every backend.** A
  compound value deferred to each target's own formatting, so the same `print`
  read four ways: a struct as `P(x: 1, y: 2)` (lux), `P { x: 1, y: 2 }` (Rust), or
  `{1 2}` (Go); an enum case as `Shape.circle(radius: 5)`, `Circle(5)`, or `{5}`.
  Each backend now renders through a generated `luxShow` — a Go type switch, a
  Rust trait, a Swift protocol — so a struct, an enum case, an array of them, and a
  recursive tree all read lux's way and compose all the way down. A bare
  `some`/`none` in print position, which had no type to infer from, is pinned at
  the site: `print(some("north"))` compiled nowhere before and now prints
  `some(north)` everywhere.
- **Go: value semantics, through structs too.** A Go slice is a reference, so a
  value that holds one — an array bound from a place, or a struct with an array
  field — shared its backing, and mutating a copy reached back into the original: a
  sort mutated the row it was handed, and `var copy = grid; copy.cells[0] = 9`
  changed `grid`. A slice-bearing value is now deep-copied wherever it flows into a
  new place — a binding, a call argument, a struct field, an array element, an
  append — the same points Rust clones at, recursing so a board of grids two levels
  deep stays independent and a value handed to a function and back can't alias the
  caller.
- **Go: a `_` arm covers the rest of a match.** An enum match dropped its wildcard
  arm and `panic`ed on every case it didn't name (`match it { potion(let a) => …
  _ => … }` crashed on the other items); an `Option` or `Result` match with a
  wildcard compiled to a missing return. The `_` now lowers to the switch's
  `default` and fills whichever side wasn't named. Everyday code the exhaustive
  corpus missed.
- **Go: an empty array literal is typed at an argument or a return.** `total([])`
  and `empty => []` (returning `[int]`) emitted Go's untyped `[]any{}`, which
  won't assign to a typed slice. Both positions now take the element type from the
  parameter or the return, the way 0.14.0 already typed an annotated binding.
- **Go: a `var` of an enum case takes the enum's type, not the case's.** An enum
  lowers to a Go interface; `:=` inferred the concrete case struct, so
  reassigning a different case wouldn't compile — the ordinary way to accumulate a
  value, `var out = List.nil` then `out = push(out, x)` in a loop. The binding now
  pins the interface type. A bare `none` with a type annotation,
  `var result: Option<int> = none`, is pinned the same way — `:=` on an untyped
  `nil` couldn't be typed at all.
- **Go: a match used as a value infers its type from a concrete arm.** When the
  first arm reads a binding — `some(let v) => some(v)` — its type read as
  `Option<?>`, because the binding isn't in scope for the inference pass. Since
  every arm of a match yields the same type, inference now takes it from the first
  arm that's fully known, so accumulating an `Option` across a loop compiles.
- **Go: forwarding an error from a match arm returns the right shape.**
  `err(why) => err(why)` emitted one value where Go's `(value, error)` lowering
  wants two. A returning arm now takes the same return path a top-level
  `return err(why)` does — the arm the "handle a Result where it's produced" rule
  pushes every program toward.
- **Go: printing an array renders like lux, not `fmt`.** `print(xs)` on
  `[1, 2, 3]` came out `[1 2 3]`, space-separated, where every other target uses
  commas. Arrays now render with commas and recurse into nesting.
- **Swift: a reversed range iterates zero times instead of crashing.** A range
  whose end falls below its start is empty in the interpreter, Rust, and Go, but
  Swift's `..<` traps on out-of-order bounds — so a bubble sort's shrinking inner
  bound took the Swift build down on an empty row. Swift range loops now emit
  `stride(from:to:by:)`, which is empty rather than fatal.
- **Rust and Swift: a loop variable the body never reads is dropped.** It warned
  where the body counts without using the counter (`for i in 0..n`, drawing a bar
  of fixed width); it's now emitted as `_`, the same elision 0.14.2 gave unread
  match bindings. Go was already clean.
- **Assigning through a parameter or a loop variable now advises what actually
  works.** Both are immutable, and both correctly refuse a change — but the
  refusal reused the `let` note, "use `var` instead of `let`", pointing at syntax
  that doesn't exist: a parameter can't be a `var` (`func f(var xs: ...)` is a
  parse error), and neither can a loop's variable. A parameter is now named as one
  and pointed at the idiom that does work — copy it into a local `var` first,
  `var xs = input` — which is exactly what the new field/element assignment makes
  natural for an in-place sort. A loop variable is named as the loop's, and pointed
  at a `var` declared outside the loop. A real `let` keeps its original note. This
  was the first wall anyone hit writing a first in-place sort after 0.14.3.
- **An empty struct is refused where it's declared.** `struct Empty {}` could be
  declared but never built — `Empty()` reads as a call, so the interpreter said
  "unknown function" and the backends diverged. It's now refused at the
  declaration ("a struct needs at least one field") and pointed at the enum, which
  is how you name a value that carries no data.
- **A construction field that forgot its label names the fix.**
  `Color.named("teal")` gave "expected a field name", pointing at the value; it
  now says "this value needs a label" and shows the `name: value` form. And a
  `return` inside a match arm, which gave "expected a value", now explains that a
  match arm is a value, not a statement, and shows returning the whole match.

## [0.14.3] - 2026-08-01

### Added

- **Assigning to a struct field or an array element.** `w.doorOpen = true` and
  `items[0] = "lantern"` now work when the root is a `var` — `count += 1` through a
  field too. lux was the only one of its three targets that refused this, so it was
  stricter than every language it's a stepping stone toward; it now matches the
  centroid, gated the way Swift's `var` and Rust's `mut` gate it. A `let` still
  refuses, now with a real diagnostic that names the rule ("cannot change
  `w.doorOpen` — `w` was declared with let") instead of a raw parse error, and the
  left of an assignment must be a place, not a value. Value semantics are
  preserved: `var a = w; a.doorOpen = true` leaves `w` untouched, the same as Go
  and Swift. The crawl still builds each turn's World fresh — now a deliberate
  style rather than the only option.

### Fixed

The crawl starter world — the program `lux crawl` hands every learner — now
converts, compiles, and plays identically on all three backends. It built only on
Swift before; two seams stood in the way, and a handed program that won't compile
is a different bar from a learner meeting an ownership rule in code they wrote.

- **Go: `Option` of an enum compiles.** An enum lowers to a Go interface, which is
  already nil-able, so `Option<Room>` was emitting `*Room` — a pointer to an
  interface, which almost nothing satisfies. It now emits the bare interface with
  `nil` for `none`: the type, `some`/`none` construction, the match binding (no
  pointer deref), and `Result` over an enum's error slot all follow. `Option` of
  an `int`/`string`/struct still uses the pointer. This is the natural shape of any
  lookup that can fail — `exit(room, dir) -> Option<Room>`.
- **Rust: a value moved into a container and read again compiles.** A non-Copy
  value pushed into an array, put in a struct field, or handed to an enum /
  `some` / `ok` / `err` constructor was moved, so a later read was a use-after-move
  — picking something up and then naming what you picked up, the natural order.
  Such a value is now cloned at the move site, matching the clone-on-pass the
  backend already did for call arguments, preserving lux's value semantics. This
  also closed the `rust`/`rpn` conformance seam, which is now actively checked
  (32 matched, 0 differed).
- **Every subcommand answers `--help`.** Only the top-level `lux --help` did;
  `lux <cmd> --help` fell through to argument handling — `lux crawl --help`
  scaffolded a folder literally named `--help`, and the rest reported a missing
  file or topic. `--help`/`-h` on any of `run`, `trace`, `crawl`, `build`,
  `convert`, `learn`, `magic`, `editors`, and `update` now prints that command's
  usage and does nothing else.

## [0.14.2] - 2026-08-01

### Added

- **Recursive enums cross all three targets.** An enum whose field stores its own
  type — a tree's `node(left: Tree, …, right: Tree)`, a linked list, an expression
  evaluator — ran interpreted but wouldn't compile as Rust or Swift, which needed
  the indirection spelled out. The Rust backend now boxes a recursive field and
  derefs it on read; the Swift backend marks the enum `indirect`; Go already
  carried the indirection in its interface encoding. A learner's first tree now
  behaves the same everywhere. Covered by a new `tree.lux` in the conformance
  suite. (Direct self-reference; mutual recursion between two enums isn't handled.)

### Fixed

- **A match binding can share a name with the emitter's Go scratch variable.** The
  Go type switch opens with a subject `v` and the `Result` lowering names its
  error `err`; an arm that bound a variable with that same name — `full(let v, …)`
  or `err(let err)` — produced Go that redeclared the scratch and wouldn't compile.
  A nested match compounded it: an inner switch reused `v` while an outer arm still
  held it, returning the wrong value. The scratch now steps aside — to `v_`, `v__`,
  … — past every name in reach: the arm's own captures, anything an enclosing match
  still holds, and outer scratch names. The common single match still reads `v` and
  `err`.
- **Rust and Swift drop a match binding the arm never reads.** A `node(let l, let v,
  let r) => v` that ignores the subtrees emitted `l` and `r` as live captures, which
  Rust and Swift warn on. An unread binding is now `_`, so the output is warning-
  clean — the same elision the Go backend already did out of necessity, since an
  unused local is a hard error there.
- **A Swift enum case named after a keyword compiles.** Swift emits the bare
  lowercase case name, so an enum case called `nil` — the textbook empty-list case
  — produced `case nil`, which Swift rejects. Such a case is now backtick-quoted
  (`` case `nil` ``) at the declaration, in match patterns, and at construction.
  Go and Rust were never affected: they qualify a case (`TreeNil`, `Tree::Nil`),
  which can't collide with a lowercase keyword.

## [0.14.1] - 2026-07-31

### Fixed

- **Storing a `Result` in a struct field is caught too.** The rule that a
  `Result` is handled where it's produced, not stored, was enforced on `let`/`var`
  bindings but slipped through a struct field — `Box(r: half(4))` ran interpreted
  yet emitted invalid Go. Building a struct with a `Result` field now gives the
  same error, pointed at the field.

## [0.14.0] - 2026-07-31

### Added

- **A conformance suite that tests "same source, three targets."** A new
  `conformance/` directory holds five non-trivial programs and a harness that runs
  each through the interpreter and through its compiled Go, Rust, and Swift
  translations, then diffs the output — so the promise that a program behaves the
  same everywhere is checked, not assumed. It surfaced the fixes below, and it
  declares the few honest seams (Go's whole-float rendering, a Rust ownership
  case, a Swift subprocess case) rather than papering over them.

### Changed

- **A `Result` is handled where it's produced, not stored.** A value that might
  fail can be matched right where it's made, or returned for the caller to face,
  but it can no longer be stashed in a `let` or `var`. That mirrors how Go handles
  errors — at the call site, not three lines later — and is what keeps the same
  source crossing all three targets, since Go models a `Result` as its
  `(value, error)` return rather than a value you hold onto. An `Option` is still
  storable; a missing value is a real value everywhere. Keeping a `Result` around
  is something you graduate into when you move up to Rust or Swift.
- **Calls that return an `Option` bind without an annotation.** `let n =
  parseInt(x)` and the like — including your own functions that return
  `Option<T>` — no longer need a type annotation on the binding, even on the
  `none` path. A bare `none` literal still does, since its type really is open.

### Fixed

- **The Go translation compiles for match arms that ignore their binding.** A
  `some(let x) => …` that never reads `x` used to emit an unused local, which Go
  rejects where Rust and Swift only warn. A binding a branch doesn't read is now
  dropped — and the `switch` guard along with it when no arm needs one.
- **The Go translation types an empty array from its annotation.** `var xs: [int]
  = []` now emits `[]int{}`, not Go's untyped `[]any{}`, which wouldn't assign to
  a typed slice.
- **The Rust translation handles a keyword-named loop variable.** A loop over a
  variable named `gen` (a Rust keyword) is now spelled the same way where it's
  declared and where it's read.
- **The Rust translation parenthesizes a cast before `<`.** `length(x) < n` no
  longer emits code Rust reads as the start of generic arguments.

## [0.13.0] - 2026-07-31

### Added

- **lux runs on Windows.** Every release now includes a native Windows build
  (`x86_64-pc-windows-msvc`) alongside the macOS and Linux ones, installed with a
  single PowerShell line — `irm https://anderix.com/lux/install.ps1 | iex` — the
  twin of the shell installer. lux was already portable Rust; this makes Windows a
  first-class target rather than a build-it-yourself one.
- **Syntax highlighting for Notepad++.** On Windows, `lux editors install` writes a
  User Defined Language into Notepad++'s config, so `.lux` files colour the moment
  you open them. It carries the same palette as lux's other editors — violet
  keywords, teal types, rose built-ins, ocher literals, muted slate comments,
  colour-blind safe — and uses foreground-only styling so the one file reads
  correctly on both the light and dark Notepad++ themes.

### Changed

- **`lux update` knows what to do on Windows.** A running program can't overwrite
  its own file on Windows, so rather than fail mid-swap, `lux update` there prints
  the one-line PowerShell installer to run in a fresh terminal. On macOS and Linux
  it still updates in place.

## [0.12.2] - 2026-07-31

### Fixed

- **nano syntax highlighting is now legible on any terminal.** The nano colours
  were built from the basic eight names (`brightblack`, `brightblue`, and the
  rest), which every terminal theme is free to render its own way — so comments
  came out as a near-invisible grey on a dark background and keywords sat too
  close to it in a low-contrast blue. The palette now uses nano's fixed mid-tone
  colour names, which map to set positions in the 256-colour palette and render
  the same regardless of the terminal's theme, and every colour is chosen to stay
  readable on both a black and a white background. It remains colour-blind safe:
  keywords in violet, types in teal, built-ins in rose, literals in ocher, and
  comments in a muted slate, told apart by hue and lightness with no red or green.

## [0.12.1] - 2026-07-30

### Fixed

- **`lux trace` announces a printing line before its output.** A line that
  prints used to show its trace line *after* the text it put on screen, so the
  narration read a step behind. A bare expression is now narrated before it
  runs, while value bindings still report after with the value they landed on,
  and stdout is flushed before each trace line — so the order holds even when the
  two streams are merged with `2>&1`.

## [0.12.0] - 2026-07-30

### Added

- **`lux trace` narrates a program as it runs.** It runs a program exactly like
  `lux run` — same input, same output — but prints each line as it executes with
  the state it changes beside it: a new value, a loop variable climbing, the
  branch an `if` took, the answer a `readLine` handed back. The narration goes to
  stderr while the program's own output stays on stdout, so the two can be
  watched together or split with a redirect — play a crawl clean on screen and
  capture the trace with `2> trace.log` to read afterward, which also makes that
  log a replayable record of exactly what happened. It steps into your functions
  rather than over them, so the whole computation is visible, and shows values
  with strings quoted, so `some("north")` reads unmistakably as text. It is the
  simplest form of a debugger: the habit of watching execution, met before the
  bigger tools that formalize it.

## [0.11.0] - 2026-07-30

### Added

- **`lux editors` sets up editor syntax highlighting.** lux ships small
  highlighting files for GtkSourceView (gedit and GNOME Text Editor), Vim and
  Neovim, and nano, embedded in the binary the same way `lux learn` carries its
  reference. `lux editors` on its own reports which of those editors are on the
  machine and whether highlighting is installed; `lux editors install` writes it
  for each one found, creating the config directories it needs and adding nano's
  one `include` line to `~/.nanorc` if it isn't already there. It's idempotent —
  a second run reports what's already current and rewrites nothing, so hand-tuned
  nano colours survive. Everything lands under the user's own home, so it needs
  no sudo. The highlighting is exactly that: colour on the words, with no
  completion or correction. `lux update` now ends with a one-line pointer to
  `lux editors install` when there's an editor to mention, but never writes those
  files itself — updating the binary can't surprise anyone by rewriting an editor
  config.

## [0.10.1] - 2026-07-09

### Fixed

- **An unterminated string now points at the line it opened on.** A string
  missing its closing `"` used to swallow the newline and every line after it
  until it found the next `"` somewhere below, fold everything between into one
  giant string, and then blame a far-off line for the leftover — so the reported
  error sat nowhere near the real mistake. The lexer now stops at the end of the
  line and reports the missing `"` at the opening quote, where the fix belongs,
  with a note that a string closes on the line it opens. An apostrophe inside a
  string was never the cause and never needs escaping.

## [0.10.0] - 2026-07-02

### Added

- **`lux update` updates lux in place.** It re-runs the same stable installer the
  docs print, fetching the latest release into the user-owned `~/.cargo/bin` — so
  no sudo — and shows a graceful fallback with the manual command if `curl` isn't
  found. One discoverable command (it's in `--help`) to stay current, and it
  works whether lux was installed by the shell installer or `cargo install`,
  since both land in the same directory.

### Changed

- **Short, stable install / update / uninstall URLs.** The install and uninstall
  commands now go through `https://anderix.com/lux/install` and `.../uninstall`,
  which redirect to the repo's `install.sh` / `uninstall.sh`. The beginner-facing
  command is far less gnarly, the URL never changes across releases, and
  re-running install (or `lux update`) always lands the latest.
- **Releases publish to crates.io.** A token-gated publish workflow keeps the
  `luxc` crate in lockstep with each GitHub release, so `cargo install luxc` no
  longer lags behind the prebuilt installer.

## [0.9.0] - 2026-07-02

### Added

- **`input(...)` — a plain-string front door to reading input.** It shows an
  optional prompt on the same line and hands back the line someone types as an
  ordinary `string`, with no `Option` and no `match` to open. That lets an
  interactive program — the single strongest hook for a beginner — happen right
  after strings, instead of being gated behind `Option`, `match`, and functions
  the way `readLine` is. The honesty is not lost, only relocated: `input` is a
  convenience over the primitive `readLine`, which still returns
  `Option<string>` for the cases where "they typed nothing" and "the input ran
  out" must be told apart (reading a piped file line by line). End of input
  folds into an empty string, a graceful degrade rather than a hidden absence.
  Reading a *number* deliberately keeps its `Option` via `parseInt` — a
  stand-in `0` would be exactly the lie lux's no-null design exists to prevent.
  Runs in the interpreter and lowers to a helper in all three backends (Rust,
  Swift, Go); `lux learn input` is the new card, and the guided `start` lesson
  now ends on an interactive program.
- **A tutorial-free fast track for tinkering with `lux crawl`.** The keep's
  `read-me-first.txt` now has a "want to change it?" half that sends a motivated
  player straight into `world.lux` with a recipe for each edit, and the win
  screen points at the same recipes. Those recipes are four new spells —
  `lux magic room`, `exit`, `thing`, and `command` — each a tiny working world
  that mirrors the shape you change in `world.lux`. The `exit` recipe returns a
  plain `Room` (a direction with no arm leaves you put) rather than the
  `Option<Room>` `world.lux` uses, so it stays transpilable, and its comment
  bridges to the Option version. The command loop's comment now explains why the
  keep reads with `readLine` rather than `input`.

## [0.8.3] - 2026-06-30

### Changed

- **`lux magic input` and `lux magic number` now hand back a value you can
  keep.** Both spells used to read inside a `match` and use the answer only
  within the arm, so the bound name died at the closing brace — copy the spell,
  try to use the answer one line later, and lux says it isn't defined. That is
  the first wall a beginner hits after reading input. Each spell now wraps the
  read in a small helper — `ask` returns a plain `string`, `askNumber` a plain
  `int`, each with a sensible default when the input ends or doesn't parse — so
  `let name = ask("...")` puts the answer in a variable you use anywhere. The
  `match` and both arms stay in plain sight, and the empty-or-zero default
  mirrors Swift's `readLine() ?? ""`. Trails grew to suit: `input` is now
  `option · match · functions`, and `number` adds `conversions`.

### Fixed

- **Tutorial prose in `lux learn`.** Repaired a duplicated sentence and a
  missing blank line in the `strings` card, three typos (`exaclty`, `containt`,
  and `progress` where `process` was meant), and a comma splice in the
  `functions` card, and tightened a few `variable`/`value` slips. Also pared an
  over-used "honest" back to the two places it earns — the opening thesis and
  the closing `beyond` note — so it reads as a motif, not a tic.

## [0.8.2] - 2026-06-20

### Added

- **`lux magic run`** — the capstone spell: run another program and read back
  what it said. It runs further ahead of the learn ladder than the other spells —
  its trail is four topics (`result`, `match`, `structs`, `shell`) — and that's
  the point: the moment a player realizes their own lux program can drive a real
  command is a payoff worth reaching for before the ideas underneath are all in
  place. Like every spell it already works and carries its trail home.

## [0.8.1] - 2026-06-20

### Added

- **Three more spells for `lux magic`.** Where the first three answer the
  listening question, these answer the next ones a player hits while building a
  world. `lux magic list` carries more than one thing — an array grown with `+=`,
  walked with a `for` loop. `lux magic save` keeps something so it's there next
  time, writing and reading a file as the same `Result` you match. `lux magic
  args` reads what's typed after the file name, the second way a program is told
  things alongside `readLine`. Each carries its trail back into `lux learn`, and
  every one is real lux the suite runs and translates.

### Fixed

- **Invalid Go from a `_`-bound `Result` or `Option` arm.** An `err(let _)` or
  `some(let _)` match arm emitted `_ := err.Error()` / `_ := *ptr`, which Go
  rejects (`no new variables on left side of :=`). The Go backend now skips the
  binding when it is `_` — the error or pointer is already consumed by the
  `== nil` test — so a match that ignores its payload translates cleanly. This
  also clears the same two errors from `examples/keep.lux`'s Go output.

## [0.8.0] - 2026-06-20

### Added

- **`parseInt` and `parseFloat`** — read a number out of text. Because the text
  might not be a number, each hands back an `Option` — `some(n)` when it parsed,
  `none` when it did not — the same shape you already `match` on, so bad input is
  a value you handle rather than a crash.
- **`lux learn conversions`** — a new topic (in the `safety` lesson, next to
  `option`) on the line between converting and parsing: a conversion is total, a
  parse can fail, and folding the two together is where the crash hides.
- **`lux magic`** — spells for things you want to do now. Where `lux learn` is a
  concept ladder, magic is task-indexed ("how do I read input?"): a small program
  that already works, ending with a trail to the `lux learn` topics that explain
  it. A spell is allowed to run ahead of where you've climbed the ladder — that's
  the point — and the same spell reads as plain lux once its trail is walked. The
  first three answer the question a player hits the moment they want their world
  to listen: reading a line, reading a number (on the new `parseInt`), and a
  read-a-command loop. Every spell is real lux the suite runs and translates.

### Changed

- **`int` and `float` are now total conversions.** They convert between numbers
  and pass their own type through; they no longer parse strings. `int("5")` was
  the one operation in lux that could fail by aborting the program — a quiet
  contradiction of the no-hidden-failures rule the language teaches everywhere
  else. That string case now errors with a trail pointing at `parseInt` /
  `parseFloat`. (This also removes a latent bug in the Go backend, where
  `int(aString)` had emitted invalid Go.)

## [0.7.2] - 2026-06-20

### Changed

- First playtest fixes for The Little Keep. The help now heads its two columns
  ("command" and "what the command does") so the right-hand descriptions can't be
  read as commands. The cellar's west passage and downward steps are split into
  two clear sentences instead of one blurred breath, so they read as two separate
  exits. And the vault now names the steps that lead back up, so a player below
  knows `up` returns.

## [0.7.1] - 2026-06-20

### Added

- **The secret is now earned, not handed over.** Reaching the chamber behind the
  locked door — the reward for solving the keep — is where the world reveals that
  it is a program you can read and change. The scaffolded scroll shrinks to just
  how to play.
- **A keepsake on disk.** Reaching the chamber writes the secret to
  `the-secret.txt`, so it's there after you quit. To decide whether to write it,
  the keep reads the file first, so it writes only once and never clobbers a copy
  you've started editing — read-then-write, the honest shape of file I/O, so the
  chamber teaches `readFile` and `writeFile` together.
- **A hidden `take gold`** in the lit vault rewards the obvious impulse and nudges
  that you can add a command of your own the same way — it was never in the help,
  just a line in the file.

### Changed

- `lux crawl`'s summary now leads with `cd` into the new folder, so its paths and
  the scroll agree on where you're standing; the help columns line up; and the
  world's header points at `world.lux`, the name the scaffolded file actually has.

## [0.7.0] - 2026-06-19

### Added

- **`lux crawl`** — scaffolds a small, playable text adventure into the current
  directory and tells you how to play and edit it. The whole world is one lux
  file (`world.lux`): the rooms are an enum, where you stand and what you carry
  is a struct, and a turn is a function that takes the world and your command and
  returns the next one. You play it by running it and change it by editing it —
  the first step toward building your own. Running `lux crawl` over a crawl you
  already started reports where it is instead of overwriting it.
- **`lux learn crawl`** — a new topic (and a `build` lesson) on how a world is
  put together, with the `step(world, command) -> world` idea on its `more` page.
- **`examples/keep.lux`** — "The Little Keep," the world `lux crawl` scaffolds: a
  brass key behind a locked door, a torch that lights a dark vault, an inventory
  that grows. Built on today's language on purpose — exact-match commands and all.

### Changed

- **Reserved-word collisions in the transpilers are now handled.** A lux name
  that is a keyword in a target language (`go`, `where`, `map`, …) gets a trailing
  `_` in that backend only, at value positions — function names, parameters, and
  locals — so a program that uses such a name still compiles. Type names, struct
  fields, and enum cases are left as written (a documented edge).
- A payload-less enum case used in a comparison now parenthesises in Go
  (`(RoomHall{})`), so `room == Room.hall` no longer trips Go's block-vs-literal
  parse inside an `if`.
- Documented the remaining transpiler edges (Go's `Option<enum>` and empty-array
  literal, Rust's value-after-move) in learn-lux.md's scope notes, as honest
  boundaries rather than worked around.

## [0.6.0] - 2026-06-19

### Added

- **Errors that open trails.** A diagnostic can now end with a `help:` line that
  points at the `lux learn` topic behind the mistake, each carrying a one-line
  lure — a hint at why the idea is worth following — so an error becomes a
  doorway into the reference instead of a dead end. The error sites that sit on a
  concept now carry one; self-evident fixes deliberately do not, so the trails
  stay signal rather than noise.
- **`lux learn errors`** — a new topic, the first card in the `start` lesson, on
  reading what lux says back: the message, the caret, the `note:`, and the
  `help:` trail. It frames hitting an error as a normal part of writing a
  program, something you read and answer, not a failure.
- **`lux learn beyond`** — a closing page, and the last note of `lux learn tour`,
  on what carries past lux once it is outgrown: the handful of thinking moves
  underneath the syntax, and that you can build your own tools instead of only
  using the ones handed to you. The human companion to the `basics` skeleton and
  the graduation ladder.

### Changed

- The `help:` line now reads `` `lux learn <topic>` — <why> `` rather than the
  older "run ... to read about this": an invitation to follow a trail instead of
  an instruction to go read.
- Reworded the "not a parameterized type" note to drop the jargon "type
  parameters" for "a type in angle brackets, like `Option<int>`".

## [0.5.0] - 2026-06-18

### Added

- **Running other programs.** `run(program, [args])` launches a command and
  captures what it produced, returning `Result<Output, string>`. `Output` is the
  first built-in struct lux hands to a program — `status`, `stdout`, and `stderr`,
  read by name. Failure comes in two layers, both in plain sight: the `Result`
  says whether the command *launched* (a missing program is the `err` arm), and
  the `status` inside says whether it *succeeded* (a command can launch fine and
  still report failure with a non-zero code, the way `false` does). The arguments
  are a list, never a shell string, so there is no shell in the middle to misread
  a space or a quote, and nothing to inject. The child's input is empty; feeding
  a program its input is left out on purpose, a lesson for a bigger language.
- **`lux learn shell`** — a topic card and `more` page on running other programs,
  added as the capstone of the `safety` guided lesson, with a new row in the
  graduation ladder. The `more` page names the one honest limit: `run` is batch
  capture, not a live pipe.

### Changed

- `run` translates through every backend: Rust's `std::process::Command`, Go's
  `os/exec` with a `bytes.Buffer` per stream, and Swift's `Foundation` `Process`
  reached through `/usr/bin/env` so a bare program name gets the same `PATH`
  lookup it does everywhere else.

## [0.4.0] - 2026-06-18

### Added

- **The outside world: basic I/O.** Five builtins for reading and writing the
  world beyond the program, each one fallible the same way lux already teaches.
  `readFile(path)` returns `Result<string, string>` and `writeFile(path,
  contents)` returns `Result<Unit, string>`, so a missing file or a failed write
  comes back as a value you `match`, never a surprise. `args()` returns the
  command line as `[string]`, the program itself at index 0. `readLine()` returns
  `Option<string>` — a line, or `none` at the end of input — so a loop over it
  reads the same whether a person is typing or a file is piped in. `eprint(...)`
  writes to stderr, beside the existing `print` on stdout, so a program's output
  stays clean for the next program to read.
- **`Unit` is now a spellable, matchable type.** `writeFile`'s success carries
  nothing, so its type is `Result<Unit, string>`; `Unit` validates as a type name
  and a value still prints as `nothing`.
- **`lux learn io`** — a new topic card and `more` page covering the outside
  world, added as the capstone of the `safety` guided lesson, with new rows in
  the graduation ladder.

### Changed

- All five I/O builtins translate through every backend: Rust's `std::fs` /
  `std::env` / `eprintln!`, Go's `os` package handing back its `(value, error)`
  pairs, and Swift's `Foundation` with its throwing file calls and native
  `readLine()`. Generated Go now lowers a `Result` match to an `if`-init
  (`if text, err := readFile(p); err == nil`), so two reads in one block no
  longer collide on their names — also the more idiomatic Go.

## [0.3.2] - 2026-06-18

### Changed

- Made the docs current and dropped `v0.1` version-pinning, which had been used
  across the docs, source comments, two user-facing error notes, and the example
  headers as shorthand for "lux as it currently stands." Each became present-tense
  framing that won't go stale. README's Status now mentions the `lux learn` work,
  and the scope notes split the `lux learn` second level out as its own milestone.

## [0.3.1] - 2026-06-18

### Changed

- The deeper level of a topic is now a plain trailing word — `lux learn match
  more` — rather than a `--more` flag.

## [0.3.0] - 2026-06-18

### Added

- **Two-level `lux learn`.** Every topic is a one-screen card by default, with an
  earned `more` page carrying the deeper why, the universal name for the concept,
  and reason-annotated cross-references to related topics. Added the `scope`
  topic, the `lux learn basics` skeleton of the shapes every procedural language
  shares, and terminal table rendering.

## [0.2.0] - 2026-06-17

### Added

- **`lux learn`** — the language reference, built into the binary so it always
  matches the binary's behavior and needs no network or stray file. Error
  messages now point at the topic that explains them, so the idea is one command
  away at the moment you hit it.

## [0.1.0] - 2026-06-17

### Added

- First released build. `lux run` interprets the core language — `print`,
  `let`/`var`, the four basic types, arithmetic, strings, `if`/`else`, `while`,
  functions with recursion, `for ... in`, ranges, arrays, structs, enums with
  associated values, exhaustive `match`, and no null (`Option` and `Result`).
- `lux convert` translates any program into idiomatic Rust, Swift, or Go, and
  `lux build` compiles the Rust translation to a native binary.
- A `curl` installer and uninstaller.

[0.15.0]: https://github.com/anderix/lux/releases/tag/v0.15.0
[0.14.3]: https://github.com/anderix/lux/releases/tag/v0.14.3
[0.14.2]: https://github.com/anderix/lux/releases/tag/v0.14.2
[0.14.1]: https://github.com/anderix/lux/releases/tag/v0.14.1
[0.14.0]: https://github.com/anderix/lux/releases/tag/v0.14.0
[0.13.0]: https://github.com/anderix/lux/releases/tag/v0.13.0
[0.12.2]: https://github.com/anderix/lux/releases/tag/v0.12.2
[0.12.1]: https://github.com/anderix/lux/releases/tag/v0.12.1
[0.12.0]: https://github.com/anderix/lux/releases/tag/v0.12.0
[0.11.0]: https://github.com/anderix/lux/releases/tag/v0.11.0
[0.10.1]: https://github.com/anderix/lux/releases/tag/v0.10.1
[0.10.0]: https://github.com/anderix/lux/releases/tag/v0.10.0
[0.9.0]: https://github.com/anderix/lux/releases/tag/v0.9.0
[0.8.3]: https://github.com/anderix/lux/releases/tag/v0.8.3
[0.8.2]: https://github.com/anderix/lux/releases/tag/v0.8.2
[0.8.1]: https://github.com/anderix/lux/releases/tag/v0.8.1
[0.8.0]: https://github.com/anderix/lux/releases/tag/v0.8.0
[0.7.2]: https://github.com/anderix/lux/releases/tag/v0.7.2
[0.7.1]: https://github.com/anderix/lux/releases/tag/v0.7.1
[0.7.0]: https://github.com/anderix/lux/releases/tag/v0.7.0
[0.6.0]: https://github.com/anderix/lux/releases/tag/v0.6.0
[0.5.0]: https://github.com/anderix/lux/releases/tag/v0.5.0
[0.4.0]: https://github.com/anderix/lux/releases/tag/v0.4.0
[0.3.2]: https://github.com/anderix/lux/releases/tag/v0.3.2
[0.3.1]: https://github.com/anderix/lux/releases/tag/v0.3.1
[0.3.0]: https://github.com/anderix/lux/releases/tag/v0.3.0
[0.2.0]: https://github.com/anderix/lux/releases/tag/v0.2.0
[0.1.0]: https://github.com/anderix/lux/releases/tag/v0.1.0

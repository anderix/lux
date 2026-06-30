# Changelog

All notable changes to lux are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and lux follows
[semantic versioning](https://semver.org/spec/v2.0.0.html).

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

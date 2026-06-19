# Changelog

All notable changes to lux are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and lux follows
[semantic versioning](https://semver.org/spec/v2.0.0.html).

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

[0.5.0]: https://github.com/anderix/lux/releases/tag/v0.5.0
[0.4.0]: https://github.com/anderix/lux/releases/tag/v0.4.0
[0.3.2]: https://github.com/anderix/lux/releases/tag/v0.3.2
[0.3.1]: https://github.com/anderix/lux/releases/tag/v0.3.1
[0.3.0]: https://github.com/anderix/lux/releases/tag/v0.3.0
[0.2.0]: https://github.com/anderix/lux/releases/tag/v0.2.0
[0.1.0]: https://github.com/anderix/lux/releases/tag/v0.1.0

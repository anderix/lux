# Learn lux

lux is a small language built to be a great *first* language and then to be
outgrown. Every feature is the simplest honest version of something shared by
Rust, Swift, and Go, so what you learn here carries straight over when you move
on to one of those.

This file is the whole language, one short topic at a time. Every example is
real lux that runs — the test suite runs them all. Each topic is a short card
you can read in under a minute; add `more` to any of them — `lux learn match
more` — for the deeper why and where the idea goes next. Read it in your
terminal: `lux learn` for the menu, `lux learn <topic>` for one card, `lux learn
basics` for the handful of shapes every language shares, `lux learn tour` for
the whole thing.

<!-- topic: hello -->
## hello — your first program

lux runs your statements from top to bottom, and `print` shows text on the
screen. There is no setup to write first: the file is the program.

```lux
// Two slashes start a comment. There is no block comment — one way to do it.
print("Hello, world!")
print("two", "words")   // print separates its arguments with spaces
```

<!-- more -->
Most languages make you write a `main` function and a little setup before
anything runs — that is the boilerplate lux leaves out, so your first program is
just the line you care about. When you move on to Rust, Go, Java, or C, `main`
comes back, and now you will know what it was always for: the one place the
program is told where to begin.

<!-- topic: variables -->
## variables — let and var

`let` names a value that never changes and `var` names one you can reassign;
lux has four basic types — int, float, string, and bool.

```lux
let pi = 3.14159       // float — a let never changes
let name = "Ada"       // string
var score = 0          // int — a var can be reassigned
score = score + 10
print(name, "has", score, "points; pi is", pi)
```

> try: add the line `pi = 3.0` and run it — lux stops you, because a `let` never changes.

<!-- more -->
Giving a value a name is *assignment*, the oldest idea in programming. The line
lux draws — `let` for a name that holds still, `var` for one that moves — shows
up nearly everywhere: Rust's `let` and `let mut`, Swift's `let` and `var`,
`const` in many others. Reaching for `let` first and `var` only when something
truly must change is a habit all of those languages reward, because a name that
cannot change is one less thing that can surprise you.

> see: scope — a name you make has to live somewhere, and that somewhere is its scope

<!-- topic: numbers -->
## numbers — arithmetic

The usual arithmetic, but int division throws away the remainder, and lux never
mixes int and float for you.

```lux
print("2 + 3   =", 2 + 3)
print("7 / 2   =", 7 / 2)         // 3 — int division drops the remainder
print("7 % 2   =", 7 % 2)         // 1 — the remainder
print("7.0/2.0 =", 7.0 / 2.0)     // 3.5 — float division keeps the fraction
print("mix on purpose:", float(7) / 2.0)
```

> try: change `float(7) / 2.0` to `7 / 2.0` and run — lux won't mix an int and a float unless you say so.

<!-- more -->
int and float are *scalar* types — single values, the atoms everything larger is
built from, alongside bool. The rule that lux will not mix them without
`float(...)` looks strict, but it is the same line Rust and Go draw: a silent
jump from whole numbers to fractions is a classic place for bugs to hide, so the
languages that care make you say when you mean it. The `%` remainder and
integer division that drops the fraction are everywhere too — they are how you
ask "is this even?" or "what is left over?" in almost any language.

> see: strings — the scalar that holds text, and why turning a number into one is on you

<!-- topic: strings -->
## strings — text

Join strings with `+` (both sides must already be strings), turn a number into
text with `string(...)`, and count characters with `length`.

```lux
let name = "Ada"
print("Hello, " + name + "!")
print("Score: " + string(42))         // convert the number first
print("letters in café:", length("café"))   // 4 — characters, not bytes
```

> try: drop the `string(...)` so it reads `"Score: " + 42` and run — lux won't glue a string to an int.

<!-- more -->
A string is really a *sequence* of characters — a compound value wearing a
friendly face, which is why `length` measures it the way it measures an array.
Most languages give strings their own type with a pile of built-in operations,
but the constant across all of them is that text is data you measure, join, and
take apart — never something the language quietly turns into a number for you,
which is why lux makes you ask for `string(...)`.

> see: arrays — a string is a sequence of characters, measured the way an array is

<!-- topic: booleans -->
## booleans — true and false

`&&`, `||`, and `!` combine true and false, and the comparisons `> < >= <= == !=`
produce them.

```lux
let sunny = true
let warm = false
print("go outside?", sunny && warm)    // and — both must be true
print("either one?", sunny || warm)    // or  — one is enough
print("not sunny?", !sunny)
print("3 > 2 is", 3 > 2)
```

<!-- more -->
bool is the third scalar type and the smallest — just `true` or `false`. Every
decision a program makes comes down to one: a comparison like `3 > 2` produces a
bool, and `if`, `while`, and the logic operators all run on them. Once you can
see the bool flowing out of a condition and into the control flow, the branching
in any language stops being mysterious — it is always a bool steering the road.

> see: if — a bool is exactly what an `if` tests · while — and what a loop tests to keep going

<!-- topic: if -->
## if — making decisions

`if` runs a block when its condition is true; there are no parentheses around
the condition, and the braces are always required.

```lux
let score = 75
if score >= 90 {
    print("A")
} else if score >= 60 {
    print("passing")
} else {
    print("try again")
}
```

<!-- more -->
`if` is *selection*, one of the two control structures every procedural language
is built from. The shape is universal — test a bool, run a block, optionally
run another instead — and what changes between languages is only paint, like
whether the condition needs parentheses around it. Read one if/else ladder and
you can read the branching in all of them on sight.

> see: while — the other control structure: repeating instead of choosing · booleans — the bool an `if` runs on

<!-- topic: while -->
## while — repeating

`while` runs its block over and over as long as the condition stays true.

```lux
var n = 0
while n < 5 {
    print("n is", n)
    n += 1
}
```

<!-- more -->
`while` is *iteration*, the other control structure, and it is the honest one:
keep going as long as a bool stays true. The `for` loop you will meet next is
usually just a tidier `while` for a known range or a collection. Go drops the
word `while` entirely and writes every loop with `for` — seeing that the two are
the same idea underneath is the whole lesson.

> see: for — the tidier loop for a range or a collection · if — the control structure that chooses instead of repeats

<!-- topic: arrays -->
## arrays — many values of one type

An array holds many values of the same type, written `[int]`, and you read an
element by position, counting from 0.

```lux
let primes: [int] = [2, 3, 5, 7, 11]
print("first:", primes[0], "count:", length(primes))
var queue = [1, 2, 3]
queue += 4                      // add to a var array
print("queue:", queue)
```

> try: read `primes[10]` and run — lux stops you at an index past the end of the array.

<!-- more -->
An array is a *data structure* — the simplest compound type, many values of one
type laid in a row and reached by an index that counts from 0. That zero-based
indexing and the square brackets are nearly universal; what later languages add
is variety — lists, slices, vectors, arrays that grow — but the mental model you
have here, "a numbered row of values," carries straight into all of them.

> see: for — the loop built for walking an array · structs — the other compound type, a few different things instead of many of the same

<!-- topic: for -->
## for — over things

`for x in xs` walks every element of an array; `for i in 0..5` counts over a
range whose end is not included.

```lux
let primes = [2, 3, 5, 7, 11]
var sum = 0
for p in primes {
    sum += p
}
print("sum of primes:", sum)
for i in 0..3 {
    print("tick", i)
}
```

<!-- more -->
`for x in xs` is *iteration over a collection* — the loop you will write most,
visiting each item once, in order. The range form `0..n` is the same loop
counting instead of walking. Languages spell it differently — Go writes `for i,
x := range xs` and Rust writes `for x in xs` just like lux — but the job never
changes, and underneath it is still the `while` loop you already know.

> see: while — what a `for` loop is underneath · arrays — the collection it walks

<!-- topic: functions -->
## functions — name a piece of work

Write `func name(p: type) -> type { ... }`, call it positionally, and a function
may call itself.

```lux
func factorial(n: int) -> int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}
print("5! =", factorial(5))
```

<!-- more -->
A function is *packaged work* with a name, *parameters* going in and a result
coming back — the unit every language leans on to keep a program from becoming
one endless script. Recursion, a function calling itself, and the fact that each
call gets its own fresh set of names are not lux quirks; they are how functions
behave everywhere, and that fresh set of names is exactly what `lux learn scope`
is about.

> see: scope — each call gets its own, which is why a name made inside stays inside

<!-- topic: scope -->
## scope — where a name lives

A name lives only inside the block where you declare it — the `{ }` of a
function, an `if`, or a loop. Step outside that block and the name is gone.

```lux
let planet = "Earth"            // planet lives in the whole program
func loud(word: string) -> string {
    let banged = word + "!"     // banged lives only inside loud
    return banged
}
print(loud(planet))
print(planet, "is still here")  // planet is still in scope out here
```

> try: add `print(banged)` as the last line and run — lux says `banged` isn't defined out here, because it only ever existed inside `loud`.

<!-- more -->
Every language has *scope* — the region of a program where a name means
something. lux keeps it simple: a fresh scope opens at every `{` and closes at
its matching `}`, and an inner scope can see the names around it but never the
other way around. The bigger languages add more kinds — file and module scope,
namespaces, and *closures* that let a function carry its scope around with it —
but the rule you just learned is the spine that all of them are built on.

> see: functions — the most common fresh scope, one per call · variables — the names that live in a scope

<!-- topic: structs -->
## structs — your own types

A struct groups related values into one type; build it by naming its fields,
and read a field with a dot.

```lux
struct Point {
    x: int
    y: int
}
let here = Point(x: 3, y: 4)
print("x is", here.x, "y is", here.y)
print("same point?", here == Point(x: 3, y: 4))
```

<!-- more -->
A struct is a *record* — the other compound type, gathering values that belong
together under one name. Where an array is many of the *same* thing, a struct is
a few *different* things treated as a unit: a point is its `x` and its `y`.
Almost every language has this shape, under names like record, struct, or data
class, and it is the first step toward objects — which the larger languages
build by attaching behaviour to the data a struct holds.

> see: enums — its partner: a struct is "this and that," an enum is "this or that"

<!-- topic: enums -->
## enums — one of several shapes

An enum says a value is exactly one of a fixed set of cases, and each case can
carry its own values — the idea that makes illegal states impossible.

```lux
enum Shape {
    circle(radius: float)
    rectangle(width: float, height: float)
    dot
}
let c = Shape.circle(radius: 2.0)
print(c)
```

<!-- more -->
An enum whose cases carry values is a *sum type*, also called a *tagged union*:
a value that is exactly one of several shapes, each with its own data. It is the
partner to the struct — a struct is "this *and* that," an enum is "this *or*
that." Rust and Swift have it and lean on it hard; Go does not, which is why
`lux convert go` has to fake it with structs and a tag field — a clear look at
what the feature actually buys you.

> see: match — how you take an enum apart, one case at a time · structs — its partner shape, "and" to the enum's "or"

<!-- topic: match -->
## match — take a value apart

`match` picks the arm for the case in hand and binds the values inside it, and
you must cover every case or lux refuses to run.

```lux
enum Shape {
    circle(radius: float)
    square(side: float)
}
func area(s: Shape) -> float {
    return match s {
        circle(let r) => 3.14159 * r * r
        square(let a) => a * a
    }
}
print("circle:", area(Shape.circle(radius: 2.0)))
print("square:", area(Shape.square(side: 3.0)))
```

> try: delete the `square` arm and run — lux refuses a match that leaves a case unhandled.

<!-- more -->
`match` is *pattern matching*: it picks the arm for the shape in hand and unpacks
the values inside in a single move. The rule that you must cover every case is
where its power comes from — the language itself, not your memory, guarantees
nothing slips through. Rust and Swift's `switch` work this way; the older
`switch` of C and Java does not, so forgetting a case there is a quiet bug
instead of a refusal to run.

> see: enums — the shapes a `match` takes apart · option — the missing-or-present value you match on

<!-- topic: option -->
## option — a value that might be missing

There is no null. A value that might be missing has type `Option<int>`: either
`some(x)` or `none`, and `match` makes you handle the empty case.

```lux
func firstEven(xs: [int]) -> Option<int> {
    for x in xs {
        if x % 2 == 0 {
            return some(x)
        }
    }
    return none
}
match firstEven([1, 3, 4, 7]) {
    some(let x) => print("first even:", x)
    none        => print("no even number")
}
```

> try: search `[1, 3, 5]` instead — the `none` arm is how lux makes sure you handled "nothing there".

<!-- more -->
`Option` is how lux says "this might be missing" without a null. Null — a value
that pretends to be there and is not — is famous enough to have a nickname, the
billion-dollar mistake, for all the crashes it has caused. Languages that
learned from it make "missing" a shape you have to open with `match` before you
can reach what is inside: Rust's `Option`, Swift's `Optional`. That forced
check, paid once up front, is the whole point.

> see: result — its sibling, for failing with a reason instead of just being absent · match — how you open one safely

<!-- topic: result -->
## result — a value that might fail

When something can fail with a reason, use `Result<int, string>`: either `ok(x)`
or `err(reason)`. Errors are just values you match on, not a hidden mechanism.

```lux
func half(n: int) -> Result<int, string> {
    if n % 2 == 0 {
        return ok(n / 2)
    }
    return err("not even")
}
for n in [8, 7] {
    match half(n) {
        ok(let h)  => print(n, "halves to", h)
        err(let e) => print(n, "can't:", e)
    }
}
```

<!-- more -->
`Result` is the same trick as `Option`, but for failure: a value that is either
`ok` with the answer or `err` with a reason, the reason carried as ordinary data
rather than thrown across the program. Go's `(value, error)` return is this idea
in different clothes, and Rust's `Result` is `Option`'s sibling. Treating a
failure as a value you `match` on, instead of a hidden mechanism that jumps out
of your code, is what keeps the failure path in plain sight.

> see: option — its sibling, for "missing" rather than "failed" · match — how you handle both arms

## The shape every language shares

lux is a launch pad, and so is this page. Almost every language you will meet is
built from the same short list of parts. Learn them once here and most of the
next language is just new spelling for things you already understand.

| every language has | what it is | learn it here |
|---|---|---|
| scalar values | int, float, bool, or text | `numbers`, `booleans`, `strings` |
| compound values | many values gathered up | `arrays`, `structs` |
| assignment | giving a value a name | `variables` |
| selection | choosing what runs next | `if` |
| iteration | repeating work | `while`, `for` |
| packaged work | functions with parameters | `functions` |
| scope | where each name means something | `scope` |

Spot those few shapes and most of any procedural language is readable; the rest
is punctuation and keywords, the goofy syntax that makes the same ideas look
different. That is what lets you open a "learn X in Y minutes" page for a
language you have never seen and follow along.

## Where each feature takes you

Here is the map of what graduates where, so the next language is never a cold
start.

| lux | Rust | Swift | Go |
|---|---|---|---|
| `let` / `var` | `let` / `let mut` | `let` / `var` | `const` / `:=` |
| `x: int` | `x: i32` | `x: Int` | `x int` |
| `func f(x: int) -> int` | `fn f(x: i32) -> i32` | `func f(x: Int) -> Int` | `func f(x int) int` |
| `for x in xs` | `for x in xs` | `for x in xs` | `for _, x := range xs` |
| `while c` | `while c` | `while c` | `for c` |
| `match` | `match` | `switch` | `switch` (leaner) |
| `enum` with values | `enum` with values | `enum` with values | *fake with structs* |
| `Option` / `Result` | `Option` / `Result` | `Optional` | `(value, error)` |
| `[int]` | `Vec<i32>` | `[Int]` | `[]int` |

Going **up** the ladder (Swift, Rust) you gain hard new ideas lux left out on
purpose: classes and protocols are the Swift lesson; ownership and lifetimes are
the Rust lesson. Going **down** (Go) you keep the same shapes but rebuild a few
conveniences by hand — that is the lesson in how they actually work.

<!-- learn:end -->

---

## For the maintainer

The sections below are notes on lux itself, not part of `lux learn`.

### Scope

The teaching surface above is the whole of lux v0.1. The interpreter grew toward
it in milestones, simplest first:

1. **`lux run` core** — `print`, `let`/`var`, the four basic types, arithmetic,
   strings, `if`/`else`, `while`.
2. **Functions** — definitions, parameters, return, recursion, `for ... in`,
   ranges, arrays.
3. **Types** — structs, then enums and `match`.
4. **No null** — `Option`/`Result` and the generics machinery they need.
5. **`lux convert rust`** — the first transpiler backend (also powers
   `lux build`).
6. **`lux convert swift` / `lux convert go`** — the remaining backends.
7. **`lux learn`** — the reference and tutorial, built into the binary and
   cross-referenced from error messages.

Milestone 7 grew a second level: every topic is a short *card* by default, with
an optional `more` page carrying the deeper why, the universal name for the
concept, and where it goes in other languages. `lux learn basics` is the
procedural-language skeleton; the cross-references that bind related topics live
on the `more` pages, each with a reason. The `scope` topic was added here,
once the interpreter's block scoping was confirmed to enforce it.

### Settled syntax decisions

- **Function calls are positional** — `factorial(3)`, not `factorial(n: 3)`.
  Matches Rust and Go; Swift's argument labels become a graduation lesson.
- **Struct and enum *construction* names its fields** — `Point(x: 0, y: 0)`,
  `Shape.circle(radius: 2.0)`, mirroring Rust.
- **`match` arms use `=>`**, kept distinct from the function return `->`: `->`
  names a return *type*, `=>` maps a pattern to a *value*.
- **Ranges use `0..10`** (end-exclusive), matching Rust.
- **A trailing comma is allowed** in any comma-separated list.
- **No string interpolation in v0.1** — `+` with explicit `string(...)`, plus
  multi-argument `print`, keeps the no-coercion lesson front and center.

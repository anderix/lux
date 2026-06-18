# Learn lux

lux is a small language built to be a great *first* language and then to be
outgrown. Every feature is the simplest honest version of something shared by
Rust, Swift, and Go, so what you learn here carries straight over when you move
on to one of those.

This file is the whole language, one short topic at a time. Every example is
real lux that runs — the test suite runs them all. You can read the same
material in your terminal: `lux learn` for the menu, `lux learn <topic>` for one
idea, `lux learn tour` for the whole thing.

<!-- topic: hello -->
## hello — your first program

There is no `main` and no boilerplate: statements run top to bottom, and `print`
is built in.

```lux
// Two slashes start a comment. There is no block comment — one way to do it.
print("Hello, world!")
print("two", "words")   // print separates its arguments with spaces
```

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

## Where each feature takes you

lux is a launch pad. Here is the map of what graduates where, so the next
language is never a cold start.

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

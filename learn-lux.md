# Learn lux in Y Minutes

lux is a small language built to be a great *first* language and then to be
outgrown. Every feature is the simplest honest version of something shared by
Rust, Swift, and Go. What you learn here transfers directly when you graduate to
one of those — and the few things lux deliberately leaves out are the lessons
those bigger languages exist to teach.

This file is the whole language. Read it top to bottom; every block is real lux.

```lux
// ----------------------------------------------------------------------------
// 1. Comments
// ----------------------------------------------------------------------------

// Two slashes start a comment that runs to the end of the line.
// There is no block-comment form. One way to do it.


// ----------------------------------------------------------------------------
// 2. Your first program
// ----------------------------------------------------------------------------

// There is no `main`, no boilerplate, no imports. Statements at the top of the
// file run top to bottom. `print` is built in.

print("Hello, world!")

// `print` takes any number of values and prints them separated by spaces.
print("two", "words")          // two words


// ----------------------------------------------------------------------------
// 3. Values and their types
// ----------------------------------------------------------------------------

// lux has four basic types. The names are lowercase.

42        // int     — whole numbers
3.14      // float   — numbers with a decimal point (always at least one digit
          //           after the dot: 3.0, not 3.)
"hello"   // string  — text in double quotes
true      // bool    — true or false


// ----------------------------------------------------------------------------
// 4. Variables: let and var
// ----------------------------------------------------------------------------

// `let` makes a name that never changes. Reach for it first; most names never
// need to change.
let pi = 3.14159

// `var` makes a name you can reassign later.
var score = 0
score = 10
score = score + 5

// pi = 3.0                     // ERROR: pi was declared with let

// lux figures out the type from the value, so you usually don't write it. When
// you want to be explicit (or there's no starting value), annotate with a colon:
let name: string = "Ada"
var count: int = 0

// lux never converts types behind your back. This is on purpose: it's the bug
// that bites you in languages that guess.
// let bad = "5" + 3           // ERROR: can't add a string and an int


// ----------------------------------------------------------------------------
// 5. Numbers
// ----------------------------------------------------------------------------

let sum   = 2 + 3              // 5
let diff  = 10 - 4            // 6
let prod  = 6 * 7            // 42
let quot  = 7 / 2          // 3   — int divided by int throws away the remainder
let rem   = 7 % 2        // 1   — % is the remainder
let exact = 7.0 / 2.0  // 3.5 — float division keeps the fraction

// Mixing int and float is an error — convert on purpose:
// let mixed = 7 / 2.0        // ERROR
let mixed = float(7) / 2.0    // 3.5


// ----------------------------------------------------------------------------
// 6. Strings
// ----------------------------------------------------------------------------

// Join strings with +. Both sides must already be strings.
let greeting = "Hello, " + name + "!"

// To put a number in a string, convert it first:
let label = "Score: " + string(score)

// But for printing, you don't need to — print stringifies each argument:
print("Score:", score)        // Score: 15

// Escapes inside strings:
let lines = "first\nsecond"   // \n newline
let tabbed = "a\tb"           // \t tab
let quote = "she said \"hi\"" // \" a quote
let path = "C:\\Users"        // \\ a backslash

// Build up a var string with +=:
var message = "Hello"
message += ", world"          // message is now "Hello, world"

// length counts the characters in a string (not the bytes):
let letters = length("café")  // 4


// ----------------------------------------------------------------------------
// 7. Booleans and logic
// ----------------------------------------------------------------------------

let yes = true
let no  = false

let both   = yes && no        // and  — true only if both are true
let either = yes || no        // or   — true if either is true
let flip   = !yes             // not  — false

// Comparisons produce bools:
let isBig = score > 100       // >  <  >=  <=  ==  !=


// ----------------------------------------------------------------------------
// 8. Arrays
// ----------------------------------------------------------------------------

// An array holds many values of the same type. The type is written [int].
let primes: [int] = [2, 3, 5, 7, 11]

// Inference works here too:
let names = ["Ada", "Alan", "Grace"]

// Read an element by position, counting from 0:
let first = primes[0]         // 2

// Length:
let howMany = length(primes)  // 5

// Add to a var array with +=:
var queue = [1, 2, 3]
queue += 4                    // queue is now [1, 2, 3, 4]


// ----------------------------------------------------------------------------
// 9. Making decisions: if / else
// ----------------------------------------------------------------------------

// No parentheses around the condition. Braces are always required.
if score > 100 {
    print("high score!")
} else if score > 0 {
    print("not bad")
} else {
    print("try again")
}


// ----------------------------------------------------------------------------
// 10. Repeating: while
// ----------------------------------------------------------------------------

// `while` runs its body as long as the condition stays true.
var n = 0
while n < 5 {
    print(n)                  // 0 1 2 3 4
    n += 1
}


// ----------------------------------------------------------------------------
// 11. Repeating over things: for ... in
// ----------------------------------------------------------------------------

// Walk every element of an array:
for prime in primes {
    print(prime)
}

// Count over a range. 0..5 means 0, 1, 2, 3, 4 — the end is not included.
for i in 0..5 {
    print(i)
}


// ----------------------------------------------------------------------------
// 12. Functions
// ----------------------------------------------------------------------------

// `func name(parameters) -> returnType { ... }`. Each parameter is `name: type`.
func square(x: int) -> int {
    return x * x
}

let nine = square(3)          // call with a value in the parentheses

// A function that returns nothing just leaves off the -> arrow:
func greet(who: string) {
    print("Hello,", who)
}

greet("world")

// Functions can call themselves (recursion):
func factorial(n: int) -> int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

// You can call a function before it appears in the file — order doesn't matter.


// ----------------------------------------------------------------------------
// 13. Structs: your own types
// ----------------------------------------------------------------------------

// A struct groups related values under one name.
struct Point {
    x: int
    y: int
}

// Build one by naming its fields:
let origin = Point(x: 0, y: 0)

// Read a field with a dot:
print(origin.x)               // 0

// Structs work as parameters and return values like any other type:
func distanceSquared(p: Point) -> int {
    return p.x * p.x + p.y * p.y
}


// ----------------------------------------------------------------------------
// 14. Enums and match: one of several shapes
// ----------------------------------------------------------------------------

// An enum is a type that is exactly one of a fixed set of cases. Cases can carry
// their own values. This is the idea that makes illegal states impossible.
enum Shape {
    circle(radius: float)
    rectangle(width: float, height: float)
    dot
}

let c = Shape.circle(radius: 2.0)

// `match` looks at which case a value is and pulls out the values inside it.
// You must handle every case, or lux stops you. A `match` is an expression —
// it produces the value of the one arm that fits — so you `return` it.
func area(s: Shape) -> float {
    return match s {
        circle(let r)           => 3.14159 * r * r
        rectangle(let w, let h) => w * h
        dot                     => 0.0
    }
}

// match works on plain values too, with _ as the catch-all. Because the set of
// ints is open-ended, a value match always needs a `_`:
func name_of(n: int) -> string {
    return match n {
        0 => "zero"
        1 => "one"
        _ => "many"
    }
}


// ----------------------------------------------------------------------------
// 15. No null: Option and Result
// ----------------------------------------------------------------------------

// lux has no `null`/`nil`. A value that might be missing has the type
// Option<T>: it is either `some(value)` or `none`. The type system forces you
// to deal with the missing case, so it can never surprise you.

func indexOf(items: [int], target: int) -> Option<int> {
    var i = 0
    for item in items {
        if item == target {
            return some(i)
        }
        i += 1
    }
    return none
}

match indexOf(primes, 5) {
    some(let idx) => print("found at", idx)
    none          => print("not in the list")
}

// `none` on its own doesn't say what it's an Option *of*, so when there's
// nothing else to go on, name the type:
let missing: Option<int> = none

// When an operation can *fail with a reason*, use Result<T, E>: either
// `ok(value)` or `err(reason)`. This is how lux does errors — they're just
// values you match on, not a separate hidden mechanism.

func half(n: int) -> Result<int, string> {
    if n % 2 == 0 {
        return ok(n / 2)
    }
    return err("not even")
}

match half(7) {
    ok(let h)  => print("half is", h)
    err(let e) => print("error:", e)
}
```

---

## Where each feature takes you

lux is a launch pad. Here's the map of what graduates where, so the next
language is never a cold start.

| lux | Rust | Swift | Go | C |
|---|---|---|---|---|
| `let` / `var` | `let` / `let mut` | `let` / `var` | `const` / `:=` | (no default-immutable) |
| `x: int` | `x: i32` | `x: Int` | `x int` | `int x` |
| `func f(x: int) -> int` | `fn f(x: i32) -> i32` | `func f(x: Int) -> Int` | `func f(x int) int` | `int f(int x)` |
| `for x in xs` | `for x in xs` | `for x in xs` | `for _, x := range xs` | `for(;;)` |
| `while c` | `while c` | `while c` | `for c` | `while c` |
| `match` | `match` | `switch` | `switch` (leaner) | `switch` (leaner) |
| `enum` with values | `enum` with values | `enum` with values | *fake with structs* | *fake with int + union* |
| `Option` / `Result` | `Option` / `Result` | `Optional` | `(value, error)` | `NULL` / sentinel |
| `[int]` | `Vec<i32>` | `[Int]` | `[]int` | array + length |

Going **up** the ladder (Swift, Rust) you gain hard new ideas lux left out on
purpose: classes and protocols are the Swift lesson; ownership and lifetimes are
the Rust lesson. Going **down** (Go, C) you keep the same shapes but rebuild a
few conveniences by hand — that's the lesson in how they actually work.

---

## v0.1 scope

The spec above is the whole teaching surface. The interpreter grows toward it in
milestones, simplest first, so there's something runnable from week one:

1. **`lux run` core** — `print`, `let`/`var`, the four basic types, arithmetic,
   strings, `if`/`else`, `while`. (Enough to teach a first hour.)
2. **Functions** — definitions, parameters, return, recursion, `for ... in`,
   ranges, arrays.
3. **Types** — structs, then enums and `match`.
4. **No null** — `Option`/`Result` and the generics machinery they need.
5. **`lux convert rust`** — the first transpiler backend (also powers
   `lux build`, which converts to Rust and invokes `rustc`).
6. **`lux convert swift` / `lux convert go`** — the remaining backends.

## Open syntax decisions (settled here, flag to revisit)

- **Function calls are positional** — `factorial(3)`, not Swift's labeled
  `factorial(n: 3)`. Matches Rust, Go, and C (three of four); Swift's argument
  labels become a graduation lesson.
- **Struct and enum *construction* names its fields** — `Point(x: 0, y: 0)`,
  `Shape.circle(radius: 2.0)`. This mirrors Rust exactly (positional calls,
  labeled struct literals) and reads clearly for a learner.
- **`match` arms use `=>`** (matching Rust exactly), kept distinct from the
  function return `->`. The two arrows mean two different things — `->` names a
  return *type*, `=>` maps a pattern to a *value* — and lux teaches that
  distinction the same way Rust draws it, so both transfer cleanly.
- **Ranges use `0..10`** (end-exclusive), matching Rust.
- **A trailing comma is allowed** in any comma-separated list (arrays, call
  arguments, struct/enum fields, parameters, match captures), the way Rust and
  Swift allow it — handy for multi-line literals. lux never requires one, unlike
  Go.
- **No string interpolation in v0.1** — `+` with explicit `string(...)`
  conversion, plus multi-argument `print`. Keeps the no-coercion lesson front
  and center; interpolation can come later if the kids want it.

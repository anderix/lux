# Three translations

lux calls itself a stepping stone, which is easy to say and hard to show. This is
the showing: the same source beside what `lux convert` makes of it in Rust, Swift,
and Go. Nothing here is hand-written — every block below was lifted from the output
of `lux convert <target> <program>` on a program in this directory, and `flex.sh`
compiles and diffs those same translations on every run.

Read it as the answer to "what am I not learning?" A learner who writes the lux and
then meets the Rust has not been protected from `Box`, `clone`, and lifetimes; they
have been given a program they already understand to meet them in.

```
./convert.sh list          # dump all three translations of one program
./convert.sh keep          # the world `lux crawl` writes out
./convert.sh               # every program, into a directory you can browse
```

The last section takes the keep — the program `lux crawl` hands a learner — one seam
at a time. Those seams have stable headings on purpose, so a guide written elsewhere
can point at one instead of working out which lines matter.

## A type that contains itself

A linked list is the first structure that has to refer to its own type, and it is
where the three targets first stop agreeing. From [`list.lux`](list.lux):

```lux
enum List {
    nil
    cons(head: int, tail: List)
}

func size(l: List) -> int {
    return match l {
        nil => 0
        cons(let h, let rest) => 1 + size(rest)
    }
}
```

Rust cannot size a type that contains itself, so the tail goes behind a pointer, and
reading it back out has to be explicit about who owns what:

```rust
enum List {
    Nil,
    Cons(i64, Box<List>),
}

fn size(l: List) -> i64 {
    return match l {
        List::Nil => 0,
        List::Cons(_, rest) => 1 + size(*rest.clone()),
    };
}
```

Swift has the same problem and a keyword for it. Note the backticks: `nil` is
Swift's own word, and the case keeps the name the textbook gives it anyway:

```swift
indirect enum List: Equatable {
    case `nil`
    case cons(head: Int, tail: List)
}

func size(_ l: List) -> Int {
    switch l {
    case .`nil`:
        return 0
    case .cons(_, let rest):
        return 1 + size(rest)
    }
}
```

Go has no enum at all. What it has is an interface, and the trick is a method nobody
calls, whose only job is to be unimplementable outside this file — so `List` is
closed the way an enum is closed:

```go
type List interface{ isList() }

type ListNil struct{}

func (ListNil) isList() {}

type ListCons struct {
	head int
	tail List
}

func (ListCons) isList() {}

func size(l List) int {
	switch v := l.(type) {
	case ListNil:
		return 0
	case ListCons:
		rest := v.tail
		return 1 + size(rest)
	}
	panic("unreachable")
}
```

The `panic("unreachable")` is the seam worth staring at. lux checked at compile time
that the match covered every case; Go's type switch cannot express that, so the
guarantee survives as a line of code that will never run. Rust and Swift both keep
the check — delete an arm from either and the compiler refuses.

Three spellings of one idea, and each is somebody's lesson: ownership is Rust's,
value types are Swift's, and structural interfaces are Go's.

## A value that might be missing

From [`binsearch.lux`](binsearch.lux), a search that returns where it found something
or nothing at all — never `-1`:

```lux
func find(xs: [int], target: int) -> Option<int> {
    var lo = 0
    var hi = length(xs) - 1
    while lo <= hi {
        let mid = lo + (hi - lo) / 2
        if xs[mid] == target {
            return some(mid)
        } else if xs[mid] < target {
            lo = mid + 1
        } else {
            hi = mid - 1
        }
    }
    return none
}
```

Rust has the type by that name:

```rust
fn find(xs: Vec<i64>, target: i64) -> Option<i64> {
    ...
        if *lux_index(&xs, mid) == target {
            return Some(mid);
    ...
    return None;
}
```

Swift has it as a suffix, which is the same idea wearing less punctuation — `Int?`
is `Optional<Int>`:

```swift
func find(_ xs: [Int], _ target: Int) -> Int? {
    ...
        if luxIndex(xs, mid) == target {
            return .some(mid)
    ...
    return nil
}
```

Go has nothing, so a pointer stands in, and a one-line generic helper exists to make
one out of a value:

```go
func ptr[T any](v T) *T {
	return &v
}

func find(xs []int, target int) *int {
    ...
		if luxIndex(xs, mid) == target {
			return ptr(mid)
    ...
	return nil
}
```

`luxIndex` is in all three, and it is not decoration. lux reports a friendly error
for an index past the end of a row; the translations have to carry that error rather
than fall back to a Rust panic, a Swift trap, or Go reading whatever is there. So the
subscript a learner wrote as `xs[mid]` comes out as a checked call on every target —
one of the places the generated code is doing work the source doesn't show.

Here the three are not equal and it is worth being honest about it. Rust and Swift
will not let the caller read the value without handling the empty case. Go will: a
`*int` can be dereferenced without checking, and that is the nil-pointer panic lux
exists to make unthinkable. The lux program cannot express the mistake, so the Go it
produces doesn't contain it — but the language it produced it in still would.

## A value that might fail

From [`expr.lux`](expr.lux), an evaluator whose division can fail. The nesting in the
lux is not stylistic: a `Result` can't be stored in a variable, so each one is taken
apart where it is produced.

```lux
func eval(e: Expr) -> Result<int, string> {
    return match e {
        num(let value) => ok(value)
        binary(let op, let left, let right) => match eval(left) {
            err(let reason) => err(reason)
            ok(let a) => match eval(right) {
                err(let reason) => err(reason)
                ok(let b) => apply(op, a, b)
            }
        }
    }
}
```

Rust and Swift both have `Result` and the shape survives almost intact:

```rust
fn eval(e: Expr) -> Result<i64, String> {
    return match e {
        Expr::Num(value) => Ok(value),
        Expr::Binary(op, left, right) => match eval(*left.clone()) {
            Err(reason) => Err(reason.clone()),
            Ok(a) => match eval(*right.clone()) {
                Err(reason) => Err(reason.clone()),
                Ok(b) => apply(op.clone(), a, b),
```

```swift
func eval(_ e: Expr) -> Result<Int, String> {
    switch e {
    case .num(let value):
        return .success(value)
    case .binary(let op, let left, let right):
        switch eval(left) {
        case .failure(let reason):
            return .failure(reason)
        case .success(let a):
            switch eval(right) {
```

Go's answer is the `(value, error)` pair every Go programmer types a hundred times a
day, and the nested match becomes the nested `if err == nil` that pair produces:

```go
func eval(e Expr) (int, error) {
	switch v := e.(type) {
	case ExprNum:
		value := v.value
		return value, nil
	case ExprBinary:
		op := v.op
		left := v.left
		right := v.right
		if a, err := eval(left); err == nil {
			if b, err_ := eval(right); err_ == nil {
				return apply(op, a, b)
			} else {
				reason := err_.Error()
				return 0, errors.New(reason)
			}
		} else {
```

This is the clearest case of lux's rule paying for itself. "Handle a failure where it
happens" sounds like a restriction while you're writing it, and then it turns out to
be the only rule under which one source can become all three of these.

## A copy that stays a copy

Every sort in this directory starts the same way, from [`bubble.lux`](bubble.lux):

```lux
func bubble(values: [int]) -> [int] {
    var xs = values
    ...
}
```

A parameter is immutable, so the row is copied into a local first. What that costs
depends entirely on where you're standing.

```rust
let mut xs: Vec<i64> = values.clone();
```

```swift
var xs = values
```

```go
xs := append([]int{}, values...)
```

Swift pays nothing to write, because its arrays are already value types. Rust names
the cost out loud, which is the whole of Rust's argument. Go's slices are references,
so the copy has to be constructed by hand — and for a nested value like a grid it
takes a generic helper and a closure per level, as when `matrix` hands two grids to
`multiply`:

```go
copySlice(a, func(__e []int) []int { return append([]int{}, __e...) })
```

That line is the single best answer to "why would I use a small language first." The
learner wrote `var xs = values`. Somebody has to write the rest, and it may as well
not be a thirteen-year-old on their second week.

The copy only appears where it could actually be observed. A function that takes a
grid and hands back a number can't leak what it was given, so `rows(m)` and `cols(m)`
get no copy at all — Go passes the grid straight in and Rust borrows it:

```rust
let mut out: Vec<Vec<i64>> = filled(cols(&m), rows(&m), 0);
```

That `&` is the whole of the ownership lesson in one character, and it is worth
knowing that lux decided it for you. The learner wrote `cols(m)` both times.

## The keep, three ways

Everything above uses a small program chosen to isolate one idea. This section uses
the opposite: [`keep.lux`](../examples/keep.lux), the world `lux crawl` writes out. It
is the program most people who try lux run first, and the only one a reader is likely
to already know by heart — which makes it the right place to meet a translation,
because nothing about the *program* is new and all of the attention is free for the
target language.

The four seams below are the ones worth reading. Each has a stable heading, so a guide
written elsewhere can point at one rather than re-deriving which lines matter.

### The keep: the world that replaces itself

The keep never changes the world. It builds the next one — `step(w, cmd) -> World` —
and the loop assigns the result back. That one decision is what makes the game
replayable by folding the same commands again, and it survives translation intact:

```rust
struct World {
    room: Room,
    items: Vec<String>,
    door_open: bool,
    playing: bool,
}
```

```swift
struct World: Equatable {
    var room: Room
    var items: [String]
    var doorOpen: Bool
    var playing: Bool
}
```

```go
type World struct {
	room     Room
	items    []string
	doorOpen bool
	playing  bool
}
```

Three declarations of the same four fields, and the only real difference is what each
language needed to be told: Rust renames to snake_case, Swift adds `Equatable` so two
worlds can be compared, and Go's are lowercase because nothing outside the file reads
them.

### The keep: the borrow lux chose for you

`has` asks whether something is in your pack. It reads the pack and never writes to
it — and lux, which forbids writing through a parameter at all, knows that:

```lux
func has(items: [string], thing: string) -> bool {
```

```rust
fn has(items: &Vec<String>, thing: String) -> bool {
...
        if has(&w.items, "key".to_string()) {
```

```go
func has(items []string, thing string) bool {
```

```swift
func has(_ items: [String], _ thing: String) -> Bool {
```

That `&` is the whole ownership lesson in one character, and it is the single most
useful line in this document for anyone heading to Rust. The learner wrote `has(w.items,
"key")` and did not think about ownership once. Rust cannot avoid thinking about it,
so lux thought about it for them and passed a borrow — no copy, no move, and `w.items`
still usable on the next line.

It is worth knowing that this line is *young*. Until the read-only parameter fix it
read `has(w.items.clone(), …)`, copying the whole pack on every lookup. Same program,
same output, an entirely different lesson — which is the argument for a guide deriving
from a translation that gets rebuilt rather than one transcribed by hand.

### The keep: two ways to be empty

The play loop has to tell a blank line you pressed Enter on from the input running out,
which is why it uses `readLine()` and not `input()`:

```lux
world = match readLine() {
    some(let line) => step(world, line)
    none           => leave(world)
}
```

Rust has the match, so the shape survives:

```rust
world = match read_line() {
    Some(line) => step(world.clone(), line.clone()),
    None => leave(world.clone()),
};
```

Swift and Go both have to make it a *statement*, and a statement can't be assigned —
so each wraps it in a function defined and called on the spot, which is a shape worth
recognising because you will write it yourself one day:

```swift
world = { () -> World in
    switch readLine() {
    case .some(let line):
        return step(world, line)
    case .none:
        return leave(world)
    }
}()
```

```go
world = func() World {
	if lineOpt := readLine(); lineOpt != nil {
		line := *lineOpt
		return step(copyWorld(world), line)
	} else {
		return leave(copyWorld(world))
	}
}()
```

Go's is the furthest from the original: no match, no Option, so the two cases become a
nil check on a pointer, and the whole thing still has to be an expression to be
assigned. Read it next to the four lines of lux above and the distance is the point.

### The keep: a file that might already be there

Reaching the chamber writes `the-secret.txt`, unless it is already there. Two things
that can fail, one nested inside the other, and no variable holding either:

```rust
match { let p = "the-secret.txt".to_string(); std::fs::read_to_string(&p).map_err(|e| format!("could not read {}: {}", p, e)) } {
    Ok(_) => println!("{}", "(Your copy is already saved in the-secret.txt.)"),
    Err(_) => match { let p = "the-secret.txt".to_string(); std::fs::write(&p, note.clone()).map_err(|e| format!("could not write {}: {}", p, e)) } {
        Ok(_) => println!("{}", "(There's a copy in the-secret.txt now, so it's not lost when you leave.)"),
        Err(_) => println!("{}", "(I couldn't leave a copy on disk, but it's all here on the screen.)"),
    },
};
```

The block around each call is doing something worth noticing. `std::fs` would hand
back its own message — `No such file or directory (os error 2)` — which doesn't say
*which* file. lux binds the path first so it can build `could not read <path>: <reason>`,
the same sentence on all three targets. A program that reads several files and reports
only "no such file" has told the reader almost nothing, so the wrapper exists to keep
the path in the message.

This is the rule from *A value that might fail* doing real work rather than
demonstrating itself. The keep never asks "did that succeed?" and stores the answer —
it answers each failure where it happens, which is why all three branches are visible
here and none of them can be forgotten.

## The last difference

Everything above is a seam where lux and a target disagree about how to say something.
[`bridge.lux`](bridge.lux) is the one place they disagree about how to *start*, and it
is the last difference left.

```lux
func main() {
    print("celsius   fahrenheit")
    for step in 0..8 {
        print(line(-40 + step * 20))
    }
```

Rust and Go both require a named entry point, so lux's `main` becomes theirs, directly:

```rust
fn main() {
    println!("{}", "celsius   fahrenheit");
    for step in 0..8 {
        println!("{}", line(-40 + step * 20));
    }
```

```go
func main() {
	fmt.Println("celsius   fahrenheit")
	for step := 0; step < 8; step++ {
		fmt.Println(line(-40 + step*20))
	}
```

Swift's top level already *is* the entry point — the same model lux has had since line
one — so there is no entry point to map onto. `main` becomes an ordinary function with
a call at the bottom of the file:

```swift
func main() {
    print("celsius   fahrenheit")
    for step in stride(from: 0, to: 8, by: 1) {
        print(line(-40 &+ step &* 20))
    }
...
main()
```

Which is the honest translation, and the reason the bridge is Rust's and Go's: Swift
was never on the other side of this one.

Two smaller things in that Swift are worth catching. `stride(from:to:by:)` is what a
half-open range becomes when the language has no `0..8`. And `&+` and `&*` are Swift's
*masking* operators — the ones that wrap on overflow instead of trapping. Swift traps
by default and the other three wrap, so lux asks for wrapping explicitly to keep the
four in step. Neither is something the learner wrote, and both are the kind of thing
that would take an afternoon to discover alone.

### What comes along

`bridge` is arithmetic, strings, and function calls, so its translation is very nearly
the file you wrote. Very nearly — one function arrives uninvited:

```rust
fn lux_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        eprintln!("error: division by zero");
        std::process::exit(1);
    }
    a / b
}

fn fahrenheit(celsius: i64) -> i64 {
    return lux_div(celsius * 9, 5) + 32;
}
```

`celsius * 9 / 5` could divide by zero in principle, and lux promised a sentence about
that rather than a crash, so the promise travels with the program as four readable
lines. That is a better picture of graduation than an empty diff would be. You do not
leave with nothing added — you leave with a short list of what was being done for you,
in a language where you can now read every line of it.

## What surrounds all of this

Every translation opens with a prelude the program didn't ask for. Part of it is a
`LuxShow` trait in Rust, a protocol in Swift, a type switch in Go, so that printing a
struct, an enum, or a tree reads the same on all four implementations rather than
deferring to each language's own formatting — which is the reason `flex.sh` can diff
bytes instead of eyeballing shapes.

The rest of it is the safety net. Every `xs[i]` in your program becomes a call through
a checked accessor, so that going past the end of a row says what lux would say rather
than what the host runtime would:

```go
row := append([]int{}, luxIndex(m, i)...)
```

Reading that is worth a minute, because it is the clearest picture of what a language
does *for* you. You wrote `m[i]`. What runs is a bounds check that knows the length,
knows the index, and knows how to explain the difference — and it is there on every
one of the three, in three spellings, without being asked for.

The rest of the net is the same idea applied to arithmetic and to printing: `luxDiv`
and `luxMod` refuse a zero divisor in lux's words rather than the host runtime's, and
`luxFloat` renders every float the way the interpreter does — positional, decimal point
kept, `inf` and `NaN` spelled the same on all three. Each of those exists because the
three targets disagreed about it until a program in this corpus caught them at it.

The translations are not a museum piece. They are compiled and run on every pass of
the harness, so every excerpt above is what the current compiler emits rather than
what it emitted once. Reading them is also how two of the findings in
[the README](README.md) were caught — a translation can be correct in every byte it
prints and still be doing far more work than it needs to, which is the one thing
`flex.sh` cannot see.

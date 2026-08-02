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
./convert.sh               # every program, into a directory you can browse
```

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
        if xs[(mid) as usize] == target {
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
        if xs[mid] == target {
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
		if xs[mid] == target {
			return ptr(mid)
    ...
	return nil
}
```

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
func bubble(input: [int]) -> [int] {
    var xs = input
    ...
}
```

A parameter is immutable, so the row is copied into a local first. What that costs
depends entirely on where you're standing.

```rust
let mut xs: Vec<i64> = input.clone();
```

```swift
var xs = input
```

```go
xs := append([]int{}, input...)
```

Swift pays nothing to write, because its arrays are already value types. Rust names
the cost out loud, which is the whole of Rust's argument. Go's slices are references,
so the copy has to be constructed by hand — and for a nested value like a grid it
takes a generic helper and a closure per level:

```go
copySlice(m, func(__e []int) []int { return append([]int{}, __e...) })
```

That line is the single best answer to "why would I use a small language first." The
learner wrote `var xs = input`. Somebody has to write the rest, and it may as well
not be a thirteen-year-old on their second week.

## What surrounds all of this

Every translation opens with a prelude the program didn't ask for — a `LuxShow`
trait in Rust, a protocol in Swift, a type switch in Go — so that printing a struct,
an enum, or a tree reads the same on all four implementations rather than deferring
to each language's own formatting. It is generated, it is skippable, and it is the
reason `flex.sh` can diff bytes instead of eyeballing shapes.

The translations are not a museum piece. They are compiled and run on every pass of
the harness, so every excerpt above is what the current compiler emits rather than
what it emitted once. Reading them is also how two of the findings in
[the README](README.md) were caught — a translation can be correct in every byte it
prints and still be doing far more work than it needs to, which is the one thing
`flex.sh` cannot see.

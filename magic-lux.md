<!-- The `lux magic` content, baked into the binary the same way learn-lux.md
is. A spell is a small program that already works, with a trail to the lux
learn topics that explain the ideas it leans on. Spells are allowed to run a
little ahead of where the learn ladder has reached — that's the whole point —
but every one is real lux the test suite runs and translates, and every one
ends under empty input so the suite never hangs. -->

A spell is a small program that already works, carried with a trail to where
each idea it uses is explained. You don't have to understand all of it yet. Copy
one, cast it, change it — and when you reach its trail in `lux learn`, the magic
turns into something you simply understand. It was never really magic; it was lux.

<!-- spell: input -->
## input — read a line someone types

How do I ask a question and use the answer?

```lux
// readLine gives back the line someone typed — or "nothing" if the input has
// ended. You match the two cases so the empty one can never surprise you.
print("What is your name?")
match readLine() {
    some(let name) => print("Hello, " + name + "!")
    none           => print("(no answer)")
}
```

> trail: option · match

<!-- spell: number -->
## number — read a number someone types

How do I read a number, not just text?

```lux
// readLine always hands you text, and text is not a number until you parse it.
// parseInt answers some(n) when it worked and none when the text wasn't a
// number — so a typo is a case you handle, not a crash.
print("Pick a number:")
match readLine() {
    some(let line) => match parseInt(line) {
        some(let n) => print("twice that is", n + n)
        none        => print("that wasn't a number")
    }
    none => print("(no answer)")
}
```

> trail: option · match · conversions

<!-- spell: loop -->
## loop — keep going until they quit

How do I read a command over and over until someone is done?

```lux
// The heart of a game: read a command, do something, repeat — until they quit
// or the input ends. The loop keeps going while `playing` is true.
func handle(command: string) -> bool {
    if command == "quit" {
        print("Bye!")
        return false
    }
    print("You said: " + command)
    return true
}

var playing = true
while playing {
    print("Type a command (or quit):")
    playing = match readLine() {
        some(let command) => handle(command)
        none              => false
    }
}
```

> trail: while · functions · option · match

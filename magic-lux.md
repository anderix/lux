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

<!-- spell: list -->
## list — carry more than one thing

How do I keep a list of things and add to it?

```lux
// A pack is an array — a list that grows. += puts a new thing on the end, a for
// loop walks what's there, and length tells you how much you're carrying.
var pack = ["torch"]
pack += "key"
print("You are carrying:")
for thing in pack {
    print("  " + thing)
}
print("That's", length(pack), "things.")
```

> trail: arrays · for

<!-- spell: save -->
## save — keep something so it's there next time

How do I save so it's not lost when the program ends?

```lux
// Save by writing to a file, load by reading it back. The disk is the outside
// world, so each call can fail and hands you a Result you match — never a crash.
match writeFile("save.txt", "room: hall\nkey: yes\n") {
    ok(let _)  => print("Saved.")
    err(let why) => print("Couldn't save:", why)
}
match readFile("save.txt") {
    ok(let text) => print("Loaded:\n" + text)
    err(let _)   => print("No save yet — start a new game.")
}
```

> trail: io · result · match

<!-- spell: args -->
## args — read what's typed after the file name

How do I let someone tell my program something when they start it?

```lux
// Two ways a program gets told things: asked while it runs (readLine), or handed
// to it at launch. args() is the second — the words after the file name. args()[0]
// is the program itself, so what the user passed starts at index 1.
let words = args()
if length(words) > 1 {
    print("You chose:", words[1])
} else {
    print("Tip: run it as  lux run game.lux hard  to pick a mode.")
}
```

> trail: io · arrays

<!-- spell: run -->
## run — use another program from yours

How do I run a real command and use what it says?

```lux
// Your program can run another program and read back what it said. run hands you
// a Result: err if it couldn't even launch, ok if it did. Inside the output,
// status is whether it worked and stdout is the text it printed.
match run("echo", ["hello from lux"]) {
    ok(let out)  => print("echo said:", out.stdout)
    err(let why) => print("couldn't run it:", why)
}
```

> trail: result · match · structs · shell

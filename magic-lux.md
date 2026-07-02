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
// input shows your question and hands back the line someone types, as a plain
// string you can keep and use anywhere. Leave the parentheses empty to just
// read a line without asking anything first.
let name = input("What is your name? ")
print("Hello, " + name + "!")
```

> trail: input · strings

<!-- spell: number -->
## number — read a number someone types

How do I read a number, not just text?

```lux
// input reads the line as text; text is not a number until you parse it. This
// helper hands what was typed to parseInt and unwraps the answer to a plain int
// — 0 if it wasn't a number — so you get a number you can keep and calculate with.
func askNumber(question: string) -> int {
    return match parseInt(input(question)) {
        some(let n) => n
        none        => 0
    }
}

let age = askNumber("How old are you? ")
print("Next year you will be", age + 1)
```

> trail: input · conversions · match · functions

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

<!-- spell: room -->
## room — add a new place to your keep

How do I add a new room to the world?

```lux
// Every place in the keep is a case of the Room enum, and describe() has one arm
// that says what you see there. To add a room, add both: the case, and its line.
// match won't run until every room has a description, so it catches one you
// started and forgot to finish — a safety net, not a scolding.
enum Room {
    hall
    tower       // the new room
}

func describe(room: Room) -> string {
    return match room {
        hall  => "A wide hall, its banners long rotted."
        tower => "A high tower, the whole valley laid out below."   // its new line
    }
}

print(describe(Room.tower))
```

> trail: enums · match · crawl

<!-- spell: exit -->
## exit — connect two rooms so you can walk between them

How do I open a path from one room to another?

```lux
enum Room {
    hall
    tower
}

func describe(room: Room) -> string {
    return match room {
        hall  => "A wide hall."
        tower => "A high tower, the valley laid out below."
    }
}

// go() is the keep's map: given a room and a direction, it says which room you
// end up in. To open a path, add a direction to a room's arm. Put it in both
// rooms and the way runs both directions, like a real door. A direction with no
// arm falls to the _ line, where you simply stay where you are.
func go(here: Room, dir: string) -> Room {
    return match here {
        hall  => match dir {
            "up" => Room.tower     // the hall now has a way up
            _    => here
        }
        tower => match dir {
            "down" => Room.hall    // and the tower a way back down
            _      => here
        }
    }
}

// Walk "up" out of the hall and describe where you land.
let there = go(Room.hall, "up")
print(describe(there))
// world.lux goes one step further: its map hands back an Option<Room>, so a
// direction that leads nowhere is `none` and the keep can say "you can't go that
// way" instead of quietly keeping you put. Same edit, one idea fancier — see
// `lux learn option`.
```

> trail: enums · match · option · crawl

<!-- spell: thing -->
## thing — hide something to pick up and carry

How do I add a thing the player can take?

```lux
// What you carry is an array — a pack that grows. To give the player a new thing,
// put it on the pack with +=. has() checks whether it's already there by walking
// the pack one item at a time, so you never pick the same thing up twice.
func has(items: [string], thing: string) -> bool {
    for it in items {
        if it == thing {
            return true
        }
    }
    return false
}

var pack = ["torch"]
if has(pack, "lantern") {
    print("You already have the lantern.")
} else {
    pack += "lantern"
    print("You take the lantern.")
}
print("Carrying:", pack)
```

> trail: arrays · for · crawl

<!-- spell: command -->
## command — teach the keep a new word to type

How do I add a new command the player can type?

```lux
// step() is the heart of the keep: it matches on what the player typed and runs
// the matching line. To teach a new word, add an arm for it. The _ arm at the
// bottom catches everything you didn't name, so an odd word gets a gentle reply.
func step(command: string) -> string {
    return match command {
        "look" => "You see an old stone hall."
        "sing" => "Your voice rings off the walls — something shifts in the dark."
        "quit" => "Farewell."
        _      => "You can't do that here."
    }
}

print(step("sing"))
```

> trail: match · functions · crawl

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

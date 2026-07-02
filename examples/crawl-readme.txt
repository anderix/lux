READ ME FIRST
=============

You have a little keep to explore. Play it first:

    lux run world.lux

Type what you want to do — look, north, take key — and press enter. If you
get stuck, type help. Type quit when you want to stop.

Go on — see how far you can get.


WANT TO CHANGE IT?
==================

The whole keep is one file: world.lux. Open it in any editor and you will
find every room, every exit, every thing to pick up, written out in plain
lux. Change a line, save, and run it again — the keep changes because you
changed it.

You do not need the tutorial to start. Each kind of change has a spell: a
tiny working example you copy and adapt. Cast one to see the shape, then make
the same edit in world.lux.

    lux magic room      add a new room
    lux magic exit      connect two rooms so you can walk between them
    lux magic thing     hide something to pick up and carry
    lux magic command   teach the keep a new word to type
    lux magic input     make the keep ask a question and use the answer

See them all with  lux magic.  Each spell ends with "how it works" — a trail
into  lux learn  for when you want the why, not just the recipe.

The easiest first win is to change what a room says: find its line in
describe(), rewrite the words, and run it again. Break things. It's your keep
now.

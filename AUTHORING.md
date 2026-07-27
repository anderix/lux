# Authoring the lux learning material

lux teaches through several independent paths. A learner takes one; they do not
read the others. The web tutorial (`anderix.com/learn/programming`) is read
front to back by someone who wants the whole story before writing code. The
terminal cards (`learn-lux.md`, shown by `lux learn`) are dipped into by someone
who reads a few and starts building. The magic spells (`magic-lux.md`, shown by
`lux magic`) are a task-indexed cookbook reached when a learner wants to *do* a
specific thing. The `crawl` game and its modding spells are a fourth path for
the tinkerer who never opens a tutorial at all.

These notes are the conventions that keep those paths good. They apply to every
surface, in this repo and in the web tutorial.

## Each path is complete on its own

Treat every path like a choose-your-own-adventure book: the page in the reader's
hands has to stand up by itself. Cross-references between paths are good and
encouraged — a card's `see:` line, a tutorial link — but a cross-reference is
always "you may also turn to page 40," never "you must turn to page 40 or this
sentence is gibberish." A reader who never follows the pointer must still
understand the page.

The sharp edge of this is **naming the word on the page**. When a term does real
work in a sentence — `parse`, `Option`, `int`, `float` — name what it means
right there, the first time it appears on that page, even if it is defined more
fully somewhere else. A behavioral description is not enough on its own: the
tutorial once explained what `parseInt` *did* ("reads a number out of text, and
hands back an `Option`") without ever saying that "parse" means to read
structured meaning out of raw text — and a careful reader who had gone through
the whole thing still stopped and asked "what is parseInt?" The behavior was
clear; the word was a magic incantation. Name the word.

A term may be named briefly inline and expanded elsewhere — a one-clause gloss in
the card's main body, the full treatment in its `more` reveal, the language
ladder in the tutorial. That is the right shape. What is not allowed is a
load-bearing term whose only definition lives on a page the reader may never
turn to: another card, a `more` reveal they did not expand, a different book.

When you introduce a concept, check it against every path it appears in, not
just the one you are editing. The same idea usually lives in the tutorial, a
card, and maybe a spell; fixing the word in one book does not fix it in the
others.

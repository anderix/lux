#!/usr/bin/env bash
# excerpts.sh — is every code block in CONVERT.md still true?
#
# CONVERT.md's argument is made out of quotations: this is the lux you wrote, here is
# what each target does with it. A quotation that has drifted from the source is worse
# than no quotation, because it reads as evidence. And drift is the normal case —
# renaming a parameter in a corpus program, or changing what the emitter produces,
# breaks a document nothing else touches and nothing else tests.
#
# So this pulls every fenced block out of CONVERT.md and checks each line against the
# real thing: `lux` blocks against the corpus sources, and rust/swift/go blocks against
# what the emitters produce for every program right now. Elisions (`...`), comments and
# bare punctuation are skipped; everything else has to appear verbatim somewhere.
#
# It reports rather than gates, like flex.sh — a stale excerpt is a finding to fix, not
# a reason to block a push.
#
# Usage:  ./excerpts.sh        (needs `lux`)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Every translation of every program, concatenated — the haystack for target blocks.
# The keep lives in examples/, the same place flex.sh reads it from.
for f in *.lux ../examples/keep.lux; do
    for t in rust swift go; do
        lux convert "$t" "$f" >> "$WORK/emitted.txt" 2>/dev/null
    done
done
cat ./*.lux ../examples/keep.lux > "$WORK/sources.txt"

python3 - "$WORK/sources.txt" "$WORK/emitted.txt" <<'PY'
import sys

sources = open(sys.argv[1]).read()
emitted = open(sys.argv[2]).read()
doc = open('CONVERT.md').read().split('\n')

blocks, inblock, lang, buf, start = [], False, None, [], 0
for i, line in enumerate(doc):
    if line.startswith('```'):
        if not inblock:
            inblock, lang, buf, start = True, line[3:].strip(), [], i + 1
        else:
            blocks.append((lang, start, buf))
            inblock = False
    elif inblock:
        buf.append(line)

# An elision or a lone brace matches everything, so checking it proves nothing.
SKIP = {'...', '}', '{', '})', ');', 'end', '*/'}
stale = 0
for lang, start, buf in blocks:
    if lang not in ('lux', 'rust', 'swift', 'go'):
        continue
    hay = sources if lang == 'lux' else emitted
    for j, line in enumerate(buf):
        s = line.strip()
        if not s or s in SKIP or s.startswith('//') or s.startswith('#'):
            continue
        if s not in hay:
            stale += 1
            print(f"  STALE  CONVERT.md:{start + j + 1} [{lang}] {s[:88]}")

print(f"\n{len(blocks)} blocks checked, {stale} stale lines")
PY

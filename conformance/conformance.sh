#!/usr/bin/env bash
# conformance.sh — does the transpiled program behave like the interpreted one?
#
# This is the "same source, same semantics, three targets" claim, tested rather
# than assumed. For each case it runs the program twice — once through `lux run`
# and once through the compiled Go translation — and diffs the output.
#
# Usage:  ./conformance.sh            (needs `lux` and `go` on PATH)
#
# Extend it to Rust and Swift by adding the equivalent build lines; the shape
# of the check is identical.

set -u
DEMOS="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
PASS=0
FAIL=0

build_go () {
    local name="$1"
    lux convert go "$DEMOS/$name.lux" > "$WORK/$name.go" 2>/dev/null || return 1
    (cd "$WORK" && go build -o "$name.bin" "$name.go" 2>"$WORK/$name.err")
}

# The one declared seam between the interpreter and the Go build: Go's fmt prints
# a whole float without the trailing `.0` the interpreter, Rust, and Swift all keep
# (`5` vs `5.0`). The value is identical — only Go's rendering differs — so it is
# documented here as a tolerance rather than fixed in the backend. Normalize that
# single difference, and nothing else, so every other byte still has to match.
norm () { sed -E 's/([0-9])\.0($|[^0-9])/\1\2/g'; }

# check <name> <args-or-stdin-pipeline>
check () {
    local name="$1" label="$2" cmd="$3"
    local a b
    if [ ! -x "$WORK/$name.bin" ]; then
        printf '  SKIP    %s (no Go build)\n' "$label"
        return
    fi
    a="$(eval "${cmd//BIN/$WORK/$name.bin}" 2>&1 | norm)"
    b="$(eval "${cmd//BIN/lux run $DEMOS/$name.lux}" 2>&1 | norm)"
    if [ "$a" = "$b" ]; then
        printf '  MATCH   %s\n' "$label"
        PASS=$((PASS + 1))
    else
        printf '  DIFFER  %s\n' "$label"
        diff <(printf '%s\n' "$b") <(printf '%s\n' "$a") | sed 's/^/          /' | head -6
        FAIL=$((FAIL + 1))
    fi
}

echo "building Go translations"
for prog in rpn life stats doctor decide; do
    if build_go "$prog"; then
        printf '  ok      %s\n' "$prog"
    else
        printf '  BUILD FAILED  %s\n' "$prog"
        head -5 "$WORK/$prog.err" | sed 's/^/          /'
        FAIL=$((FAIL + 1))
    fi
done

echo
echo "comparing behaviour  (whole-float .0 rendering is a declared seam, normalized)"
check rpn    "rpn — arithmetic"        'BIN 3 4 + 2 x'
check rpn    "rpn — divide by zero"    'BIN 5 0 /'
check rpn    "rpn — stack underflow"   'BIN 1 +'
check rpn    "rpn — unknown operator"  'BIN 2 3 ^'
check life   "life — 5 generations"    'BIN'
check stats  "stats — seq 1..100"      'seq 1 100 | BIN'
check stats  "stats — junk lines"      "printf '5\n3\nnope\n9\n' | BIN"
check stats  "stats — empty input"     "printf '' | BIN"
check doctor "doctor — subprocesses"   'BIN'
check decide "decide — policy table"   'BIN'

echo
echo "$PASS matched, $FAIL differed"
rm -rf "$WORK"
[ "$FAIL" -eq 0 ]

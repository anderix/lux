#!/usr/bin/env bash
# flex.sh — does every program in the corpus behave the same on all four legs?
#
# The corpus exists to map the language's reach, and a program that only works
# interpreted maps nothing. So each one runs through `lux run` and through its
# compiled Go, Rust, and Swift translations, and the outputs are diffed. The
# interpreter is the reference; a target that disagrees is wrong.
#
# Each leg is built the way a learner would build it — `lux build` for Rust, and a
# plain `go build` or `swiftc` for the other two. No optimization flags: a corpus
# that tests a configuration nobody runs is measuring the wrong thing.
#
# Every program here is deterministic — lux has no way to produce a random
# number, deliberately (anderix/lux#3) — so this needs no seeding and no
# tolerance. Same bytes or a failure.
#
# Usage:  ./flex.sh            (needs `lux`, plus any of go / rustc / swiftc)
#         ./flex.sh sieve bst  (just those programs)

set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PASS=0
FAIL=0

have () { command -v "$1" >/dev/null 2>&1; }
TARGETS=()
have go && TARGETS+=(go)
have rustc && TARGETS+=(rust)
have swiftc && TARGETS+=(swift)
if [ ${#TARGETS[@]} -eq 0 ]; then
    echo "no target compiler found — install one of go, rustc, swiftc"
    exit 1
fi

# How each program is driven. BIN is replaced by the thing under test, so the
# same line runs the interpreter and every compiled translation.
invocation () {
    case "$1" in
        # Run twice: a good count, then a typo'd one. The second is what a learner
        # actually types, and until it was added `head`'s eprint — one of only two in
        # the corpus — was unreachable in every run this harness had ever done. Note
        # it does not cover #51: `head` warns before it prints anything, so Swift's
        # habit of flushing all of stderr ahead of stdout happens to give the same
        # order. Catching that needs a warning in the middle, which nothing here does.
        head)  echo "seq 1 100 | BIN 3; seq 1 100 | BIN xyz" ;;
        catn)  echo "printf 'alpha\nbeta\ngamma\n' | BIN" ;;
        # Not all ASCII, on purpose. `wcl` counts characters, and what counts as one
        # is the thing the four implementations disagree about hardest — Swift counts
        # what you can see, the others count Unicode scalars. Every other string in
        # this corpus is ASCII, so the first accented name or emoji a learner types
        # would land outside everything ever tested.
        #
        # The whitespace is equally deliberate, now that the third column is a real
        # word count rather than an apology. A run of spaces, a leading and trailing
        # space, a tab, and an empty line are the four ways splitting on a space
        # produces a field that isn't a word, and each is a line here — otherwise the
        # skip-the-empties branch never runs and the count is right only by luck.
        wcl)   echo "printf 'one\nwe won 🇨🇦\ncafé  costs   3\n\tone\ttab  and spaces \n\nlast\n' | BIN" ;;
        uniqc) echo "printf 'a\na\nb\na\na\na\n' | BIN" ;;
        # Every shape a row can be wrong in, because the program's whole claim is
        # that the field count is trustworthy: an empty field in the middle, empties
        # at both ends, a row with too many fields, one with too few, and a row that
        # is nothing but separators. The space in "new york" reaches the quoting
        # note. Run twice, the second with no input at all — the no-header branch is
        # otherwise unreachable, which is how three of this corpus's coverage holes
        # started.
        fields) echo "printf 'name,city,role\nada,london,maths\nalan,,logic\n,manchester,\ngrace,new york,navy,extra\nshort\n,,\n' | BIN; printf '' | BIN" ;;
        # Stray spaces on purpose: real input has them, whether it came from a
        # column-aligned file, a paste, or a person typing. `parseInt` trims, so the
        # program's answer shouldn't change — and where it does, that's a finding.
        # Run twice, the second with nothing numeric in it. `hist`'s "no numbers"
        # warning and its "nothing to chart" line were unreachable in every run this
        # harness had done — an audit of which output lines the corpus inputs actually
        # produce turned them up as the only two left unreached anywhere.
        hist)  echo "printf '3\n 9\nnope\n14 \n' | BIN; printf 'nope\nalso nope\n' | BIN" ;;
        # A full tour, not a stroll. The first version of this walk read plausibly
        # and never reached the cellar or the vault at all — four of its commands
        # were no-ops answered with "You can't go that way", so the torch, the gold
        # and two of the five rooms went untested while the run looked healthy.
        #
        # This one visits every room and takes every branch: help, an unknown
        # command, a wall, the locked door before the key and after, taking a thing
        # twice, opening an open door, the vault in the dark and then with the torch,
        # and the gold twice. It also passes through the chamber twice, so one run
        # covers both sides of "is there a copy already?" — writing the-secret.txt
        # and then finding it. That last part only holds because each leg runs in a
        # clean directory; see `rundir` below.
        keep)  echo "printf 'help\ndance\nsouth\nnorth\nopen door\nnorth\ntake key\ntake key\nopen door\nopen door\neast\ndown\ntake gold\nup\ntake torch\ndown\ntake gold\ntake gold\nup\nwest\nnorth\nlook\nsouth\nquit\n' | BIN" ;;
        *)     echo "BIN" ;;
    esac
}

# Most programs live here. The keep doesn't: it ships to learners as the thing
# `lux crawl` writes out, so it's tested where it actually lives rather than copied
# in, and a copy would be one more thing to keep in step.
source_for () {
    case "$1" in
        keep) echo "$HERE/../examples/keep.lux" ;;
        *)    echo "$HERE/$1.lux" ;;
    esac
}

# Every leg runs in its own empty directory, and never in the repo. A program that
# writes a file would otherwise leave it behind — `keep` writes the-secret.txt when
# you reach the chamber — and worse, the leg that ran first would change what the
# next one sees, which shows up as a one-line diff that looks like a backend bug and
# isn't. Fresh directory per leg, so every leg meets the same world.
#
# Named after the leg rather than counted, because this is called from inside a
# command substitution and a counter would increment in the subshell and be lost —
# every leg would land in run1 and share a world, which is the exact failure this
# exists to prevent.
rundir () { # <label>
    local d="$WORK/run.$1"
    rm -rf "$d"
    mkdir -p "$d"
    echo "$d"
}

# Outputs are compared byte for byte, with nothing normalized away. There used to
# be a rule here erasing the trailing `.0` Go drops from a whole float, written
# defensively back when no program printed a float at all. `stats` prints several,
# and the rule turned that divergence into a pass — so it's gone and the divergence
# is filed instead. A harness that smooths over a difference trades a false failure
# today for a hidden bug tomorrow, and hiding a finding is the one thing this
# directory must not do.

build () { # <target> <program>
    local target="$1" prog="$2"
    local src="$WORK/$prog.$target"
    local bin="$WORK/$prog.$target.bin"
    case "$target" in
        go)
            mkdir -p "$src" || return 1
            lux convert go "$(source_for "$prog")" > "$src/main.go" 2>"$WORK/$prog.$target.err" || return 1
            ( cd "$src" && go mod init flex >/dev/null 2>&1 && go build -o "$bin" . ) \
                2>>"$WORK/$prog.$target.err" || return 1
            ;;
        rust)
            # `lux build` is the whole Rust leg, because it is the single command a
            # learner is given for turning a program into a binary. Driving rustc by
            # hand here would test a configuration nobody produces — which it did,
            # with -O, until an overflow turned out to behave differently under the
            # two (anderix/lux#35).
            mkdir -p "$src" || return 1
            ( cd "$src" && lux build "$(source_for "$prog")" ) >"$WORK/$prog.$target.err" 2>&1 || return 1
            mv "$src/$prog" "$bin" 2>/dev/null || return 1
            ;;
        swift)
            lux convert swift "$(source_for "$prog")" > "$src.swift" 2>"$WORK/$prog.$target.err" || return 1
            swiftc -o "$bin" "$src.swift" 2>>"$WORK/$prog.$target.err" || return 1
            ;;
    esac
    return 0
}

# How many warnings a build produced. The bar is lux's own: `tests/transpile.rs`
# holds every example to warning-clean Rust and Swift, because the backends' job is
# source a learner can read without a warning about code they didn't write. Nothing
# applied that bar to this corpus, which is four times the size — and one program
# was failing it (a dead initializer in `lcs`, which also turned up anderix/lux#69).
#
# Counted from the stderr the build already captured, so it costs nothing extra, and
# reported rather than failed: a warning is a finding, the same as a divergence.
# `go build` has no warning tier — an unused local is a hard error there — so Go
# contributes nothing here and that is not an omission.
#
# rustc's closing "N warnings emitted" is itself a line matching `warning`, so it is
# excluded — counting it turns one warning into two.
warnings_in () { # <program> <target>
    grep -E '^warning|warning:' "$WORK/$1.$2.err" 2>/dev/null \
        | grep -cvE 'warnings? emitted' || echo 0
}

PROGRAMS=(fizzbuzz fib gcd sieve collatz roman
          bubble selection mergesort quicksort
          binsearch list bst expr machine safe regex
          pascal matrix lcs tictactoe queens maze
          stats points logic worklist stocktake
          catn head wcl uniqc hist fields
          bridge keep)

if [ $# -gt 0 ]; then
    PROGRAMS=("$@")
fi

echo "targets: ${TARGETS[*]}"
echo "programs: ${#PROGRAMS[@]}"
echo
echo "building translations"
# A target that fails to build is reported and skipped for that program only —
# the other two still get compared, so one broken backend doesn't hide the state
# of the others.
BUILDS=0
WARNED=0
for prog in "${PROGRAMS[@]}"; do
    for target in "${TARGETS[@]}"; do
        if build "$target" "$prog"; then
            BUILDS=$((BUILDS + 1))
            n="$(warnings_in "$prog" "$target")"
            if [ "$n" -gt 0 ] 2>/dev/null; then
                printf '  WARN    %-5s %-11s %s in generated source\n' "$target" "$prog" "$n"
                grep -E '^warning|warning:' "$WORK/$prog.$target.err" 2>/dev/null \
                    | grep -vE 'warnings? emitted' | head -2 | sed 's/^/            /'
                WARNED=$((WARNED + 1))
            fi
        else
            printf '  BUILD   %-5s %s\n' "$target" "$prog"
            head -3 "$WORK/$prog.$target.err" 2>/dev/null | sed 's/^/          /'
            FAIL=$((FAIL + 1))
        fi
    done
done
printf '  %d of %d translations built, %d with warnings\n' "$BUILDS" "$(( ${#PROGRAMS[@]} * ${#TARGETS[@]} ))" "$WARNED"

echo
echo "comparing behaviour"
for prog in "${PROGRAMS[@]}"; do
    cmd="$(invocation "$prog")"
    ref="$( cd "$(rundir "$prog.ref")" && eval "${cmd//BIN/lux run \"$(source_for "$prog")\"}" 2>&1 )"
    for target in "${TARGETS[@]}"; do
        bin="$WORK/$prog.$target.bin"
        [ -x "$bin" ] || continue
        out="$( cd "$(rundir "$prog.$target")" && eval "${cmd//BIN/$bin}" 2>&1 )"
        if [ "$out" = "$ref" ]; then
            printf '  MATCH   %-5s %s\n' "$target" "$prog"
            PASS=$((PASS + 1))
        else
            printf '  DIFFER  %-5s %s\n' "$target" "$prog"
            diff <(printf '%s\n' "$ref") <(printf '%s\n' "$out") | sed 's/^/          /' | head -8
            FAIL=$((FAIL + 1))
        fi
    done
done

echo
echo "$PASS matched, $FAIL differed"
[ "$FAIL" -eq 0 ]

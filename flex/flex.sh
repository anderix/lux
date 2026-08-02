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
        head)  echo "seq 1 100 | BIN 3" ;;
        catn)  echo "printf 'alpha\nbeta\ngamma\n' | BIN" ;;
        wcl)   echo "printf 'one\ntwo\nthree and more\n' | BIN" ;;
        uniqc) echo "printf 'a\na\nb\na\na\na\n' | BIN" ;;
        # Stray spaces on purpose: real input has them, whether it came from a
        # column-aligned file, a paste, or a person typing. `parseInt` trims, so the
        # program's answer shouldn't change — and where it does, that's a finding.
        hist)  echo "printf '3\n 9\nnope\n14 \n' | BIN" ;;
        *)     echo "BIN" ;;
    esac
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
            lux convert go "$HERE/$prog.lux" > "$src/main.go" 2>"$WORK/$prog.$target.err" || return 1
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
            ( cd "$src" && lux build "$HERE/$prog.lux" ) >"$WORK/$prog.$target.err" 2>&1 || return 1
            mv "$src/$prog" "$bin" 2>/dev/null || return 1
            ;;
        swift)
            lux convert swift "$HERE/$prog.lux" > "$src.swift" 2>"$WORK/$prog.$target.err" || return 1
            swiftc -o "$bin" "$src.swift" 2>>"$WORK/$prog.$target.err" || return 1
            ;;
    esac
    return 0
}

PROGRAMS=(fizzbuzz fib gcd sieve collatz roman
          bubble selection mergesort quicksort
          binsearch list bst expr machine safe
          pascal matrix lcs tictactoe queens maze
          stats points logic
          catn head wcl uniqc hist)

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
for prog in "${PROGRAMS[@]}"; do
    for target in "${TARGETS[@]}"; do
        if build "$target" "$prog"; then
            BUILDS=$((BUILDS + 1))
        else
            printf '  BUILD   %-5s %s\n' "$target" "$prog"
            head -3 "$WORK/$prog.$target.err" 2>/dev/null | sed 's/^/          /'
            FAIL=$((FAIL + 1))
        fi
    done
done
printf '  %d of %d translations built\n' "$BUILDS" "$(( ${#PROGRAMS[@]} * ${#TARGETS[@]} ))"

echo
echo "comparing behaviour"
for prog in "${PROGRAMS[@]}"; do
    cmd="$(invocation "$prog")"
    ref="$(eval "${cmd//BIN/lux run \"$HERE/$prog.lux\"}" 2>&1)"
    for target in "${TARGETS[@]}"; do
        bin="$WORK/$prog.$target.bin"
        [ -x "$bin" ] || continue
        out="$(eval "${cmd//BIN/$bin}" 2>&1)"
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

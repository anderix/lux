#!/usr/bin/env bash
# conformance.sh — does each transpiled program behave like the interpreted one?
#
# "Same source, same behaviour, three targets" — tested, not assumed. For each
# program the interpreter's output is the reference, and every compiled
# translation (Go, Rust, Swift) is diffed against it. A leg whose compiler isn't
# on PATH is skipped, so the suite runs on whatever toolchains you have.
#
# Usage:  ./conformance.sh        (needs `lux`, plus any of go / rustc / swiftc)

set -u
DEMOS="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
PASS=0
FAIL=0

# Which targets can we build here? Skip a leg whose compiler is absent.
have () { command -v "$1" >/dev/null 2>&1; }
TARGETS=()
have go && TARGETS+=(go)
have rustc && TARGETS+=(rust)
have swiftc && TARGETS+=(swift)
if [ ${#TARGETS[@]} -eq 0 ]; then
    echo "no target compiler found — install one of go, rustc, swiftc"
    exit 1
fi

# One difference is a declared seam, not a conformance failure, so the comparison
# is skipped for that one target+program (see README.md):
#   swift/doctor — Swift launches programs via /usr/bin/env, so a *missing* one
#                  comes back as env's exit 127 (a status, i.e. the ok arm) rather
#                  than a launch failure; doctor deliberately probes one that
#                  isn't there.
declared_seam () { # <target> <program>
    case "$1/$2" in
        swift/doctor) return 0 ;;
        *) return 1 ;;
    esac
}

# Build <target> <program> -> $WORK/<program>.<target>.bin, non-zero on failure.
build () {
    local target="$1" name="$2" src bin err
    err="$WORK/$name.$target.err"
    bin="$WORK/$name.$target.bin"
    case "$target" in
        go)
            src="$WORK/$name.go"
            lux convert go "$DEMOS/$name.lux" >"$src" 2>/dev/null || return 1
            (cd "$WORK" && go build -o "$bin" "$src" 2>"$err") ;;
        rust)
            src="$WORK/$name.rs"
            lux convert rust "$DEMOS/$name.lux" >"$src" 2>/dev/null || return 1
            rustc "$src" -o "$bin" 2>"$err" ;;
        swift)
            src="$WORK/$name.swift"
            lux convert swift "$DEMOS/$name.lux" >"$src" 2>/dev/null || return 1
            swiftc "$src" -o "$bin" 2>"$err" ;;
    esac
}

# The one global tolerance: Go's fmt prints a whole float without the trailing
# `.0` the interpreter, Rust, and Swift keep (`5` vs `5.0`). Same value, only the
# rendering differs — declared here, not fixed in the backend. Normalize that one
# thing, nothing else, so every other byte still has to match.
norm () { sed -E 's/([0-9])\.0($|[^0-9])/\1\2/g'; }

# check <program> <label> <command with BIN placeholder>
check () {
    local name="$1" label="$2" cmd="$3" ref out target bin
    ref="$(eval "${cmd//BIN/lux run $DEMOS/$name.lux}" 2>&1 | norm)"
    for target in "${TARGETS[@]}"; do
        if declared_seam "$target" "$name"; then
            printf '  SEAM    %-5s %s (declared, see README)\n' "$target" "$label"
            continue
        fi
        bin="$WORK/$name.$target.bin"
        if [ ! -x "$bin" ]; then
            printf '  SKIP    %-5s %s (no build)\n' "$target" "$label"
            continue
        fi
        out="$(eval "${cmd//BIN/$bin}" 2>&1 | norm)"
        if [ "$out" = "$ref" ]; then
            printf '  MATCH   %-5s %s\n' "$target" "$label"
            PASS=$((PASS + 1))
        else
            printf '  DIFFER  %-5s %s\n' "$target" "$label"
            diff <(printf '%s\n' "$ref") <(printf '%s\n' "$out") | sed 's/^/          /' | head -6
            FAIL=$((FAIL + 1))
        fi
    done
}

echo "targets: ${TARGETS[*]}"
echo
echo "building translations"
for prog in rpn life stats doctor decide tree; do
    for target in "${TARGETS[@]}"; do
        declared_seam "$target" "$prog" && continue
        if build "$target" "$prog"; then
            printf '  ok      %-5s %s\n' "$target" "$prog"
        else
            printf '  BUILD   %-5s %s\n' "$target" "$prog"
            head -4 "$WORK/$prog.$target.err" 2>/dev/null | sed 's/^/          /'
            FAIL=$((FAIL + 1))
        fi
    done
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
check tree   "tree — recursive enum"   'BIN'

echo
echo "$PASS matched, $FAIL differed"
rm -rf "$WORK"
[ "$FAIL" -eq 0 ]

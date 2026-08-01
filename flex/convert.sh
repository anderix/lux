#!/usr/bin/env bash
# convert.sh — dump each program beside its three translations, for reading.
#
# CONVERT.md walks the interesting seams in excerpt form. This is the same thing
# without the commentary: whole files, so you can see how much of a translation is
# your program and how much is the prelude that makes printing agree.
#
# Nothing here is committed. The translations are generated from the sources on
# demand, so they can't drift from what the compiler is actually fed — `flex.sh`
# builds and diffs these same outputs.
#
# Usage:  ./convert.sh          (every program, into ./translations)
#         ./convert.sh list bst (just those, to stdout)

set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
TARGETS=(rust swift go)
EXT_rust=rs
EXT_swift=swift
EXT_go=go

if ! command -v lux >/dev/null 2>&1; then
    echo "lux is not on PATH"
    exit 1
fi

# Named programs go to stdout, so a single one can be read or piped without
# leaving anything behind.
if [ $# -gt 0 ]; then
    for prog in "$@"; do
        src="$HERE/$prog.lux"
        if [ ! -f "$src" ]; then
            echo "no such program: $prog"
            exit 1
        fi
        for target in "${TARGETS[@]}"; do
            printf '===== %s — %s =====\n' "$prog" "$target"
            lux convert "$target" "$src" || echo "(conversion failed)"
            echo
        done
    done
    exit 0
fi

OUT="$HERE/translations"
rm -rf "$OUT"
mkdir -p "$OUT"

written=0
failed=0
for src in "$HERE"/*.lux; do
    prog="$(basename "$src" .lux)"
    for target in "${TARGETS[@]}"; do
        ext="EXT_$target"
        dest="$OUT/$prog.${!ext}"
        if lux convert "$target" "$src" > "$dest" 2>/dev/null; then
            written=$(( written + 1 ))
        else
            rm -f "$dest"
            printf '  FAILED  %-6s %s\n' "$target" "$prog"
            failed=$(( failed + 1 ))
        fi
    done
done

printf '%d translations in %s\n' "$written" "${OUT#$HERE/}"
if [ "$failed" -gt 0 ]; then
    printf '%d could not be converted — see the README\n' "$failed"
fi

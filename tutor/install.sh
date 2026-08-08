#!/bin/sh
# Install the lux tutor guidance into the current directory.
#
# Optional, and no part of learning lux. It adds guidance to a CLAUDE.md telling
# Claude Code how to help someone learning lux without doing the work for them.
# Run it in the folder where you keep your lux files:
#
#   curl -LsSf https://anderix.com/lux/tutor | sh
#
# It only ever touches CLAUDE.md in the current directory, and it writes its
# guidance inside a marked block:
#
#   no CLAUDE.md yet ....... writes one
#   CLAUDE.md, no block .... appends the block, leaving your own notes alone
#   block already there .... replaces just that block, so re-running updates it
#
# To remove it, delete the block between the two lux tutor markers, or delete
# the file if it holds nothing else.
set -eu

src="https://raw.githubusercontent.com/anderix/lux/main/tutor/CLAUDE.md"
target="CLAUDE.md"
begin="<!-- lux tutor: begin — managed block, edits here are overwritten -->"
end="<!-- lux tutor: end -->"

tmp="$(mktemp)"
new="$(mktemp)"
trap 'rm -f "$tmp" "$new"' EXIT INT HUP TERM

curl --proto '=https' --tlsv1.2 -LsSf "$src" -o "$tmp"

if [ ! -e "$target" ]; then
    {
        echo "$begin"
        cat "$tmp"
        echo "$end"
    } > "$target"
    echo "Wrote $target."

elif grep -qF "$begin" "$target"; then
    if ! grep -qF "$end" "$target"; then
        echo "$target has a lux tutor start marker but no end marker." >&2
        echo "Fix or remove the block by hand, then run this again." >&2
        exit 1
    fi
    awk -v b="$begin" -v e="$end" -v f="$tmp" '
        $0 == b { print; while ((getline line < f) > 0) print line; skip = 1; next }
        skip && $0 == e { print; skip = 0; next }
        skip { next }
        { print }
    ' "$target" > "$new"
    cat "$new" > "$target"
    echo "Updated the lux tutor block in $target. Your own notes are untouched."

else
    {
        echo ""
        echo "$begin"
        cat "$tmp"
        echo "$end"
    } >> "$target"
    echo "Added the lux tutor block to your existing $target."
fi

echo "Claude Code reads it whenever you work in this folder."

#!/bin/sh
# Install the lux tutor guidance into the current directory.
#
# Optional, and no part of learning lux. It writes a CLAUDE.md that tells Claude
# Code how to help someone learning lux without doing the work for them. Run it
# in the folder where you keep your lux files:
#
#   curl -LsSf https://anderix.com/lux/tutor | sh
#
# It never writes outside the current directory and never overwrites a CLAUDE.md
# that is already there. To remove it again, delete the file.
set -eu

src="https://raw.githubusercontent.com/anderix/lux/main/tutor/CLAUDE.md"
target="CLAUDE.md"

if [ -e "$target" ]; then
    echo "There is already a $target in this folder, so nothing was written." >&2
    echo "Read $src and add the parts you want by hand." >&2
    exit 1
fi

curl --proto '=https' --tlsv1.2 -LsSf "$src" -o "$target"

echo "Wrote $target."
echo "Claude Code reads it whenever you work in this folder."

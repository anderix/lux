#!/bin/sh
# Uninstall lux, reversing the cargo-dist shell installer.
#
# The installer leaves a receipt at ~/.config/lux/lux-receipt.json recording the
# binary it placed and where, so this reads that to remove exactly what was
# installed, then deletes the receipt. If the receipt is gone it falls back to
# the usual install locations. Safe to run more than once.
#
#   curl -LsSf https://anderix.com/lux/uninstall | sh
set -eu

app="lux"
receipt_home="${XDG_CONFIG_HOME:-$HOME/.config}/$app"
receipt="$receipt_home/$app-receipt.json"
removed=0

remove_file() {
    if [ -e "$1" ]; then
        rm -f "$1"
        echo "removed $1"
        removed=1
    fi
}

# Remove a binary given a base that may be either the bin dir itself or the
# install prefix above it — cargo-dist's "cargo-home" layout puts binaries in
# <prefix>/bin, while a flat layout puts them directly in <prefix>.
remove_binary() {
    remove_file "$1/$2"
    remove_file "$1/bin/$2"
}

if [ -f "$receipt" ]; then
    prefix=$(grep -o '"install_prefix":"[^"]*"' "$receipt" | sed 's/.*:"//; s/"$//')
    bins=$(grep -o '"binaries":\[[^]]*\]' "$receipt" | sed 's/"binaries":\[//; s/]//; s/"//g; s/,/ /g')
    if [ -n "$prefix" ] && [ -n "$bins" ]; then
        for b in $bins; do
            remove_binary "$prefix" "$b"
        done
    fi
    rm -rf "$receipt_home"
    echo "removed $receipt_home"
    removed=1
else
    # No receipt — try the locations the installer would have used.
    for base in "${LUX_INSTALL_DIR:-}" "${CARGO_HOME:-$HOME/.cargo}" "$HOME/.local"; do
        [ -n "$base" ] || continue
        remove_binary "$base" "$app"
    done
    if [ -d "$receipt_home" ]; then
        rm -rf "$receipt_home"
        removed=1
    fi
fi

if [ "$removed" -eq 1 ]; then
    echo "lux uninstalled."
else
    echo "lux was not found; nothing to remove."
fi

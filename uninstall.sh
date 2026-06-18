#!/bin/sh
# Uninstall lux, reversing the cargo-dist shell installer.
#
# The installer leaves a receipt at ~/.config/lux/lux-receipt.json recording the
# binary it placed and where, so this reads that to remove exactly what was
# installed, then deletes the receipt. If the receipt is gone it falls back to
# the usual install locations. Safe to run more than once.
#
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://raw.githubusercontent.com/anderix/lux/main/uninstall.sh | sh
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

if [ -f "$receipt" ]; then
    prefix=$(grep -o '"install_prefix":"[^"]*"' "$receipt" | sed 's/.*:"//; s/"$//')
    bins=$(grep -o '"binaries":\[[^]]*\]' "$receipt" | sed 's/"binaries":\[//; s/]//; s/"//g; s/,/ /g')
    if [ -n "$prefix" ] && [ -n "$bins" ]; then
        for b in $bins; do
            remove_file "$prefix/$b"
        done
    fi
    rm -rf "$receipt_home"
    echo "removed $receipt_home"
    removed=1
else
    # No receipt — try the locations the installer would have used.
    for dir in "${LUX_INSTALL_DIR:-}" "${CARGO_HOME:-$HOME/.cargo}/bin" "$HOME/.local/bin"; do
        [ -n "$dir" ] || continue
        remove_file "$dir/$app"
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

#!/bin/sh
# Install lux by fetching and running the cargo-dist shell installer from the
# latest GitHub release.
#
# This is a stable front door. The release asset is named for the crate
# (luxc-installer.sh), and that name can change; this wrapper keeps the install
# command constant, mirroring uninstall.sh.
#
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://raw.githubusercontent.com/anderix/lux/main/install.sh | sh
set -eu

installer="https://github.com/anderix/lux/releases/latest/download/luxc-installer.sh"

curl --proto '=https' --tlsv1.2 -LsSf "$installer" | sh

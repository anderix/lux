# Install lux by fetching and running the cargo-dist PowerShell installer from
# the latest GitHub release.
#
# This is a stable front door. The release asset is named for the crate
# (luxc-installer.ps1), and that name can change; this wrapper keeps a constant
# target, and the public command is shorter still via a redirect on anderix.com:
#
#   irm https://anderix.com/lux/install.ps1 | iex

$installer = "https://github.com/anderix/lux/releases/latest/download/luxc-installer.ps1"

Invoke-RestMethod -Uri $installer | Invoke-Expression

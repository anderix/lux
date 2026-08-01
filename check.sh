#!/usr/bin/env bash
# check.sh — the one command that answers "did I break anything?"
#
# Builds lux, then runs every check against *that* freshly-built binary: the Rust
# test suite, then the conformance and flex corpora on whatever target compilers
# are installed. The corpora shell out to `lux`, so this puts the build directory
# first on PATH — otherwise they'd silently test whichever lux is installed, not
# the one you just changed.
#
# Usage:
#   ./check.sh          fmt, clippy, build, cargo test, conformance, flex  (full)
#   ./check.sh fast     fmt, clippy, build, cargo test only    (the tight loop)
#
# Exits non-zero on the first failure, so it drops cleanly into a pre-push hook
# or CI. A missing target compiler isn't a failure — that leg is skipped, the
# same as the suites do on their own.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

mode="${1:-full}"

# The same gate CI's fast job runs, mirrored locally so a formatting or lint slip
# is caught before the push rather than by a red CI run.
echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy"
cargo clippy --all-targets --quiet -- -D warnings

echo "==> cargo build"
cargo build --quiet

echo "==> cargo test"
cargo test --quiet

if [ "$mode" = "fast" ]; then
    echo
    echo "Fast checks passed. (Run without 'fast' to include the conformance and flex corpora.)"
    exit 0
fi

# Test the binary just built, never a stale installed one.
export PATH="$ROOT/target/debug:$PATH"

echo "==> conformance corpus"
bash conformance/conformance.sh

echo "==> flex corpus"
bash flex/flex.sh

echo
echo "All checks passed."

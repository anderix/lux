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
# The gated checks exit non-zero on the first failure, so this drops cleanly into
# a pre-push hook or CI. The flex corpus is the exception: it reports its tally but
# never fails the run, because finding a divergence is its job, not a regression
# (#22). A missing target compiler isn't a failure either — that leg is skipped,
# the same as the suites do on their own.
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

# conformance/ is the baseline: its programs cover the language, and red there
# means something regressed — so it gates.
echo "==> conformance corpus"
bash conformance/conformance.sh

# flex/ is the frontier: its job is to find divergences by running programs nobody
# has written before, so going red is a finding to file, not a push blocker (#22).
# It runs here for visibility — the tally prints on every check — but never fails
# the gate, so a discovery in flex can't block work that has nothing to do with it.
echo "==> flex corpus (reports findings, does not gate)"
if bash flex/flex.sh; then
    flex_note=""
else
    flex_note=" Flex reported divergences above — findings to file, not push blockers."
fi

echo
echo "All gated checks passed.${flex_note}"

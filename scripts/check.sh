#!/usr/bin/env bash
# The gate for every change: format, lints, the float guard and the tests
# (debug and release). Nothing lands without all of this passing.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
# --all (not default-members): formatting lm-host needs only parsing, and its
# sources must stay formatted too.
cargo fmt --all -- --check

# No --workspace here: that would include lm-host, which cannot build in WSL
# (no Bevy system libraries). Bare cargo commands use the workspace's
# default-members; lm-host is gated by a Windows build via win-build.sh.
echo "==> cargo clippy -D warnings"
cargo clippy --all-targets -- -D warnings

echo "==> no-float guard"
bash scripts/check-no-float.sh

echo "==> cargo test (debug)"
cargo test

echo "==> cargo test (release)"
cargo test --release

echo "==> lm golden"
cargo run --release --bin lm -- golden

echo "==> acceptance session"
bash scripts/acceptance.sh

echo "==> all checks passed"

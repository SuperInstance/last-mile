#!/usr/bin/env bash
# The gate for every change: format, lints, the float guard and the tests
# (debug and release). Nothing lands without all of this passing.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
# --all (not default-members): formatting lm-host needs only parsing, and its
# sources must stay formatted too.
cargo fmt --all -- --check

# --workspace --exclude lm-host: every crate is checked (new ones too, automatically)
# except lm-host, which cannot build in WSL
# (no Bevy system libraries). Bare cargo commands use the workspace's
# default-members; lm-host is gated by a Windows build via win-build.sh.
echo "==> cargo clippy -D warnings"
cargo clippy --workspace --exclude lm-host --all-targets -- -D warnings

echo "==> no-float guard"
bash scripts/check-no-float.sh

echo "==> cargo test (debug)"
cargo test --workspace --exclude lm-host

echo "==> cargo test (release)"
cargo test --workspace --exclude lm-host --release

echo "==> cargo test (release, ignored: the fairness guard)"
# The fairness guard plays 200 full rounds, which is a minute of wall-clock
# in release and far too slow for a debug run, so it is marked #[ignore]
# and runs here.
cargo test --release -p lm-core --test fairness -- --ignored

echo "==> lm golden"
cargo run --release --bin lm -- golden

echo "==> acceptance session"
bash scripts/acceptance.sh

echo "==> all checks passed"

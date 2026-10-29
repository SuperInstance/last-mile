#!/usr/bin/env bash
# Build the archive on the Orin and run the golden check there.
#
# The Orin is aarch64 and the ProArt is x86, so the binary is built
# **natively on the Orin** — not cross-compiled on the ProArt. The plan's
# decision says why: a cross-built binary carries the ProArt's glibc, and
# the Orin may not have it; a native build has neither problem, and it needs
# no new toolchain on the ProArt.
#
# The golden check that runs there is the phase's headline proof: the same
# three pinned seeds, played afresh on the ARM machine, have to reach the
# same three hashes the x86 machine reached in Phase 1. `OK 3/3` is the
# answer that closes the loop, and a **non-zero exit** is the answer that
# says the two machines do not play the same game.
#
# The script is idempotent: it clones the branch when the tree is not there
# and pulls it when it is, and running it twice does the same work and
# reports the same result.
#
# Environment:
#   ORIN_HOST    the Orin's host name or address   (required, unless --local)
#   ORIN_USER    the user to log in as             (default: the current user)
#   ORIN_DIR     where the tree lives on the Orin   (default: ~/src/last-mile)
#   ORIN_BRANCH  the branch to build                (default: phase6-3d)
#   ORIN_REPO    the repository to clone            (default: this project's origin)
#   ORIN_STORE   the archive's store directory      (default: ~/.local/share/last-mile/archive)
#
# Usage:
#   scripts/orin-build.sh             drive the Orin over SSH
#   scripts/orin-build.sh --local     run the same steps on this machine
#   scripts/orin-build.sh --dry-run   print the steps instead of running them
set -euo pipefail
cd "$(dirname "$0")/.."

# The branch the phase lands on, and the repository it comes from.
BRANCH="${ORIN_BRANCH:-phase6-3d}"
REPO="${ORIN_REPO:-https://github.com/SuperInstance/last-mile.git}"
DIR="${ORIN_DIR:-$HOME/src/last-mile}"
STORE="${ORIN_STORE:-$HOME/.local/share/last-mile/archive}"
HOST="${ORIN_HOST:-}"
LOGIN="${ORIN_USER:-$(id -un)}"

usage() {
    sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//; $d'
}

MODE="ssh"
for arg in "$@"; do
    case "$arg" in
        --local) MODE="local" ;;
        --dry-run | --print) MODE="print" ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "orin-build: unknown argument $arg" >&2
            exit 2
            ;;
    esac
done

# The steps, as a script the Orin runs. It is written once and used three
# ways: printed, piped over SSH, or run here with --local.
remote_script() {
    cat <<REMOTE
set -euo pipefail

say() { printf '==> %s\n' "\$*"; }

say "the toolchain"
command -v git >/dev/null || { echo "git is not on the box" >&2; exit 1; }
if ! command -v cargo >/dev/null; then
    echo "cargo is not on the box: install Rust 1.99 first (see docs/handoffs/orin-archive-and-golden.md)" >&2
    exit 1
fi
rustc --version
cargo --version
command -v nice  >/dev/null || { echo "nice is not on the box: the archive cannot yield without it" >&2; exit 1; }
command -v ionice >/dev/null || { echo "ionice is not on the box: the archive cannot yield without it" >&2; exit 1; }

say "the tree ($BRANCH)"
if [ ! -d "$DIR/.git" ]; then
    mkdir -p "$DIR"
    git clone --branch "$BRANCH" "$REPO" "$DIR"
else
    git -C "$DIR" fetch origin "$BRANCH"
    git -C "$DIR" checkout "$BRANCH"
    git -C "$DIR" pull --ff-only origin "$BRANCH"
fi

say "the build"
cd "$DIR"
cargo build --release -p lm-archive -p lm-tools

say "the golden check (the x86 <-> ARM proof)"
cargo run --release --bin lm -- golden

say "the archive's own golden check (recorded in the store)"
mkdir -p "$STORE"
"$DIR/target/release/lm-archive" golden --store "$STORE"

say "the machine"
uname -m
cat "$STORE/reports/golden.json"

say "done"
REMOTE
}

case "$MODE" in
    print)
        remote_script
        ;;
    local)
        script="$(mktemp)"
        trap 'rm -f "$script"' EXIT
        remote_script >"$script"
        bash "$script"
        ;;
    ssh)
        if [ -z "$HOST" ]; then
            echo "orin-build: ORIN_HOST is not set (the Orin's host name or address)" >&2
            echo "orin-build: use --dry-run to see the steps, or --local to run them here" >&2
            exit 2
        fi
        script="$(mktemp)"
        trap 'rm -f "$script"' EXIT
        remote_script >"$script"
        ssh "$LOGIN@$HOST" 'bash -s' <"$script"
        ;;
esac

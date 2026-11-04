#!/usr/bin/env bash
# Build (and run) lm-host with Windows cargo, from this WSL worktree.
#
# lm-host is Windows-only: this WSL toolchain has none of Bevy's Linux system
# libraries (alsa, udev, x11 — no pkg-config even), and building across
# \\wsl.localhost is forbidden by the plan. So instead of a Windows git
# clone (a private repo; Windows git may lack credentials), this script
# mirrors the worktree to a Windows-side directory with rsync — excluding
# target/ (the mirror keeps its own, so Windows rebuilds are incremental),
# .git/ and runs/ — and runs cargo there.
#
# usage: bash scripts/win-build.sh [branch] <cargo args...>
# example:
#   bash scripts/win-build.sh run -p lm-host --release -- \
#       --screenshot 'C:\Users\casey\src\shot1.png' --frames 30
set -euo pipefail

WIN_DIR='C:\Users\casey\src\last-mile-build'
WSL_DIR=/mnt/c/Users/casey/src/last-mile-build
CARGO_BIN='C:\Users\casey\.cargo\bin'

if [ "$#" -lt 1 ]; then
    echo "usage: bash scripts/win-build.sh [branch] <cargo args...>" >&2
    exit 2
fi

# The plan's interface is `win-build.sh <branch> [cargo args…]`. The branch is
# accepted and ignored: this script mirrors the worktree it is run from, and
# that worktree is already on the branch. A leading token that is not a cargo
# subcommand is therefore dropped, so both spellings work.
case " build check clippy test doc fmt run bench clean tree " in
    *" $1 "*) ;;
    *)
        shift
        ;;
esac

if [ "$#" -lt 1 ]; then
    echo "usage: bash scripts/win-build.sh [branch] <cargo args...>" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

mkdir -p "$WSL_DIR"
rsync -a --delete --exclude=target --exclude=.git --exclude=runs ./ "$WSL_DIR"/

# Run from the Windows side of the mount so cmd.exe starts on a Windows path,
# not a \\wsl.localhost UNC path it would only warn about.
cd "$WSL_DIR"
cmd.exe /c "cd /d $WIN_DIR && set \"PATH=$CARGO_BIN;%PATH%\" && cargo $*"

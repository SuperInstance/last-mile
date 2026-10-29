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
# usage: bash scripts/win-build.sh <cargo args...>
# example:
#   bash scripts/win-build.sh run -p lm-host --release -- \
#       --screenshot 'C:\Users\casey\src\shot1.png' --frames 30
set -euo pipefail

WIN_DIR='C:\Users\casey\src\last-mile-build'
WSL_DIR=/mnt/c/Users/casey/src/last-mile-build
CARGO_BIN='C:\Users\casey\.cargo\bin'

if [ "$#" -lt 1 ]; then
    echo "usage: bash scripts/win-build.sh <cargo args...>" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

mkdir -p "$WSL_DIR"
rsync -a --delete --exclude=target --exclude=.git --exclude=runs ./ "$WSL_DIR"/

# Run from the Windows side of the mount so cmd.exe starts on a Windows path,
# not a \\wsl.localhost UNC path it would only warn about.
cd "$WSL_DIR"
cmd.exe /c "cd /d $WIN_DIR && set \"PATH=$CARGO_BIN;%PATH%\" && cargo $*"

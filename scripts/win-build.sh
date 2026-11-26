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
# One mirror per worktree: concurrent worktrees sharing one mirror compiled
# each other's trees (and one seat's uncommitted file was lost that way).
# The suffix is the worktree's directory name; the main checkout keeps the
# original path, so its incremental target/ survives.
#
# The cost of a per-worktree mirror is a per-worktree cargo target/, and
# Bevy's target/ is multiple gigabytes. A mirror outlives its worktree: a
# seat that finishes leaves a multi-gigabyte mirror behind, and on
# 2026-10-30 twenty-three of them had filled the C: drive to 96% (71 GB).
# Nothing in the old script ever removed one.
#
#   prune  removes the mirrors whose worktree is gone (the dead ones)
#   sweep  drops every mirror's target/ and keeps the source
#   list   shows each mirror, its worktree and whether it is still live
#
# usage:
#   bash scripts/win-build.sh [branch] <cargo args...>
#   bash scripts/win-build.sh prune [--dry-run] [--keep <name>]...
#   bash scripts/win-build.sh sweep [--dry-run]
#   bash scripts/win-build.sh list
#
# examples:
#   bash scripts/win-build.sh run -p lm-host --release -- \
#       --screenshot 'C:\Users\casey\src\shot1.png' --frames 30
#   bash scripts/win-build.sh prune --keep merge
set -euo pipefail

TOP=$(git rev-parse --show-toplevel)
NAME=$(basename "$TOP")
if [ "$NAME" = "last-mile" ]; then SUFFIX=""; else SUFFIX="-$NAME"; fi

# The Windows-side mirror root. Overridable so the same script works on a
# machine that keeps its sources somewhere else.
WSL_SRC_ROOT="${WIN_SRC_ROOT_WSL:-/mnt/c/Users/casey/src}"
WIN_SRC_ROOT="${WIN_SRC_ROOT_WIN:-C:\\Users\\casey\\src}"
WSL_DIR="$WSL_SRC_ROOT/last-mile-build$SUFFIX"
WIN_DIR="$WIN_SRC_ROOT\\last-mile-build$SUFFIX"
CARGO_BIN='C:\Users\casey\.cargo\bin'

# Metadata file inside a mirror: which worktree it belongs to. Excluded
# from the rsync so `--delete` never takes it away.
META=.win-mirror

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//; $d'
}

# The worktree a mirror was made from, or empty when the mirror predates
# the metadata file.
mirror_worktree() {
  [ -f "$1/$META" ] || return 0
  sed -n 's/^worktree=//p' "$1/$META" | head -1
}

# When the mirror was made, or empty for mirrors without metadata.
mirror_created() {
  [ -f "$1/$META" ] || return 0
  sed -n 's/^created=//p' "$1/$META" | head -1
}

# Directory names of the worktrees this repository still has.
live_worktrees() {
  git -C "$TOP" worktree list --porcelain 2>/dev/null \
    | awk '/^worktree /{print $2}' | while read -r w; do basename "$w"; done \
    | sort -u
}

# Is this mirror still backed by a worktree that exists?
#
# A mirror is live when the worktree it was made from is still on disk. The
# metadata is authoritative; for mirrors made before the metadata existed we
# fall back to matching the suffix against the live worktree names.
mirror_is_live() {
  local dir="$1" suffix="$2" live="$3" wt
  wt=$(mirror_worktree "$dir")
  if [ -n "$wt" ]; then
    [ -d "$wt" ] && return 0 || return 1
  fi
  case "$suffix" in
    "") return 0 ;; # the main checkout's own mirror
    *) printf '%s\n' "$live" | grep -qx -- "$suffix" ;;
  esac
}

cmd_list() {
  local live; live=$(live_worktrees)
  printf '%-34s %-8s %-34s %s\n' MIRROR STATE WORKTREE MIRRORED_AT
  local dir base suffix wt state
  for dir in "$WSL_SRC_ROOT"/last-mile-build*; do
    [ -d "$dir" ] || continue
    base=$(basename "$dir")
    suffix="${base#last-mile-build}"; suffix="${suffix#-}"
    wt=$(mirror_worktree "$dir"); [ -n "$wt" ] || wt="(no metadata: $suffix)"
    if mirror_is_live "$dir" "$suffix" "$live"; then state=live; else state=DEAD; fi
    printf '%-34s %-8s %-34s %s\n' "$base" "$state" "$wt" "$(mirror_created "$dir")"
  done
}

cmd_prune() {
  local dry=0
  local keep=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run|-n) dry=1; shift ;;
      --keep) keep+=("$2"); shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) echo "prune: unknown argument '$1'" >&2; exit 2 ;;
    esac
  done

  local live; live=$(live_worktrees)
  local freed=0 count=0
  local dir base suffix wt state size
  for dir in "$WSL_SRC_ROOT"/last-mile-build*; do
    [ -d "$dir" ] || continue
    base=$(basename "$dir")
    suffix="${base#last-mile-build}"; suffix="${suffix#-}"

    # Never prune the mirror this invocation is running from.
    if [ "$dir" = "$WSL_DIR" ]; then state="kept (current worktree)"
    elif printf '%s\n' "${keep[@]+"${keep[@]}"}" 2>/dev/null | grep -qx -- "$suffix"; then
      state="kept (--keep $suffix)"
    elif mirror_is_live "$dir" "$suffix" "$live"; then
      state="kept (worktree alive)"
    else
      state="PRUNE"
    fi

    case "$state" in
      PRUNE)
        if [ "$dry" = 1 ]; then
          printf 'would remove %-34s worktree gone\n' "$base"
        else
          size=$(du -sm "$dir" 2>/dev/null | cut -f1 || echo 0)
          rm -rf -- "$dir"
          printf 'removed %-34s %sM\n' "$base" "$size"
          freed=$((freed + size)); count=$((count + 1))
        fi
        ;;
      *)
        wt=$(mirror_worktree "$dir"); [ -n "$wt" ] || wt="(no metadata: $suffix)"
        printf '%-34s %s\n' "$base" "$state"
        ;;
    esac
  done
  if [ "$dry" = 1 ]; then
    echo "dry run: nothing was removed"
  else
    printf 'pruned %d mirror(s), about %sM freed\n' "$count" "$freed"
  fi
}

cmd_sweep() {
  local dry=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run|-n) dry=1; shift ;;
      -h|--help) usage; exit 0 ;;
      *) echo "sweep: unknown argument '$1'" >&2; exit 2 ;;
    esac
  done
  local freed=0 count=0
  local dir base size
  for dir in "$WSL_SRC_ROOT"/last-mile-build*; do
    [ -d "$dir/target" ] || continue
    base=$(basename "$dir")
    size=$(du -sm "$dir/target" 2>/dev/null | cut -f1 || echo 0)
    if [ "$dry" = 1 ]; then
      printf 'would sweep %-34s %sM\n' "$base" "$size"
    else
      rm -rf -- "$dir/target"
      printf 'swept %-34s %sM\n' "$base" "$size"
      freed=$((freed + size)); count=$((count + 1))
    fi
  done
  if [ "$dry" = 1 ]; then
    echo "dry run: nothing was removed"
  else
    printf 'swept %d mirror(s), about %sM freed\n' "$count" "$freed"
  fi
}

case "${1:-}" in
  prune) shift; cmd_prune "$@"; exit 0 ;;
  sweep) shift; cmd_sweep "$@"; exit 0 ;;
  list)  shift; cmd_list; exit 0 ;;
esac

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
rsync -a --delete --exclude=target --exclude=.git --exclude=runs --exclude="$META" ./ "$WSL_DIR/"

# Remember which worktree this mirror belongs to, so `prune` can tell a
# live mirror from a dead one.
{
  printf 'worktree=%s\n' "$TOP"
  printf 'repo=%s\n' "$(git rev-parse --git-common-dir)"
  printf 'created=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$WSL_DIR/$META"

# Run from the Windows side of the mount so cmd.exe starts on a Windows path,
# not a \\wsl.localhost UNC path it would only warn about.
cd "$WSL_DIR"
cmd.exe /c "cd /d $WIN_DIR && set \"PATH=$CARGO_BIN;%PATH%\" && cargo $*"

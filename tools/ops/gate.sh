#!/usr/bin/env bash
# gate.sh <worktree> <task-branch> <target-branch> <msgfile>
# Commit a seat's work, rebase on target (safe lockfile fix), run check.sh, ff-merge, push, clean up.
set -uo pipefail
WT=$1; TB=$2; TGT=$3; MSG=$4; R=/home/proart/projects/last-mile; S=$(dirname "$0")
cd "$WT" || exit 1
rm -f BRIEF.md
git add -A && git commit -q -F "$MSG" || { echo "COMMIT FAILED"; exit 1; }
if ! git rebase -q "$TGT" 2>/dev/null; then
  conf=$(git diff --name-only --diff-filter=U)
  if [ "$conf" = "Cargo.lock" ]; then "$S/fixlock.sh" "$TGT" && GIT_EDITOR=true git rebase --continue >/dev/null || { echo "LOCKFIX FAILED"; exit 1; }
  else echo "CONFLICT: $conf"; exit 2; fi
fi
out=$(bash scripts/check.sh 2>&1); echo "$out" | grep -qE 'all checks passed' || { echo "CHECK FAILED"; echo "$out" | grep -E 'FAILED|panicked|error' | head -8; exit 3; }
echo "$out" | grep -E 'OK [0-9]/[0-9]' | tail -1
win=$(bash scripts/win-build.sh build -p lm-host --release 2>&1); echo "$win" | grep -qE '^error' && { echo "WINDOWS BUILD FAILED"; echo "$win" | grep -E -A6 '^error' | head -20; exit 4; }
cd "$R" && git checkout -q "$TGT" && git merge -q --ff-only "$TB" && git push -q && git log --oneline -1 && git checkout -q phase1-core && git worktree remove --force "$WT" && git branch -q -D "$TB" && echo MERGED

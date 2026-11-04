#!/usr/bin/env bash
# merge-branch.sh <remote-branch>: rebase a pushed seat branch onto m1-integration, run check.sh AND the Windows lm-host build, ff-merge, push, clean up.
# One fixed worktree dir (lm-wt/merge) so win-build.sh's per-worktree mirror stays warm between merges. Run merges one at a time.
set -uo pipefail
B=$1; R=/home/proart/projects/last-mile; W=/home/proart/lm-wt/merge
cd $R && git fetch -q && git branch -f m1-integration origin/m1-integration
[ -d $W ] && { git worktree remove --force $W; git branch -qD mb-tmp 2>/dev/null; }
git worktree add -q -b mb-tmp $W origin/$B || exit 1
cd $W && git rebase -q m1-integration || { echo "CONFLICT $B"; git rebase --abort; exit 2; }
out=$(bash scripts/check.sh 2>&1); echo "$out" | grep -E 'OK [0-9]/[0-9]' | tail -1
echo "$out" | grep -q 'all checks passed' || { echo "CHECK FAILED $B"; echo "$out" | grep -E 'FAILED|error\[|panicked' | head -6; exit 3; }
win=$(bash scripts/win-build.sh build -p lm-host --release 2>&1); echo "$win" | grep -qE '^error' && { echo "WINDOWS BUILD FAILED $B"; echo "$win" | grep -E -A6 '^error' | head -20; exit 4; }
echo "windows lm-host builds"
cd $R && git fetch -q && git branch -f m1-integration origin/m1-integration && (cd $W && git rebase -q m1-integration) && git checkout -q m1-integration && git merge -q --ff-only mb-tmp && git push -q && git push -q origin --delete $B && git log --oneline -1 && git checkout -q phase1-core && git worktree remove --force $W && git branch -qD mb-tmp && echo "MERGED $B"

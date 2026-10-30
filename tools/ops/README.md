# tools/ops: how the director runs the seats

See docs/HANDOFF.md §6. State lives in `$LM_OPS` (default `~/lm-ops`): `queue.txt`, `running.txt`, `wt/<job>.out` for ProArt jobs. Thor jobs write `~/lm-wt/<job>.out` on the Thor.
- `sched.sh`: rate- and memory-aware zcode scheduler; exits on each clean finish for review.
- `relaunch-z.sh <thor|proart> <job> [main|flash]`: launch or resume zcode on a job's BRIEF.md, with a CONTINUE note.
- `merge-branch.sh <branch>`: rebase a pushed branch onto m1-integration, run check.sh and the Windows lm-host build, then ff-merge and push.
- `gate.sh <worktree> <branch> m1-integration <msgfile>`: commit a seat's worktree, then the same checks and merge.
- `fixlock.sh`: resolve a Cargo.lock-only rebase conflict.

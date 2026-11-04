# BRIEF: bring the esc-tune branch onto the current trunk and resolve its conflict

This worktree is branch origin/esc-tune (one commit, b048771: `escalate_gates` in policy.toml defaulting to ["move"], the d1-3B floor at 0.15, both models' choices in the escalation record; read its commit message). It conflicts with the trunk (origin/m1-integration) in crates/lm-host/src/main.rs: the trunk's push-to-talk merge (ddea09c) rewrote the host's backend wiring (backends(), plus the voice/ASR options), and esc-tune passes escalate_gates through that same wiring into EscalatingBackend::with_gates.

1. `git rebase origin/m1-integration`. Resolve every conflict **keeping both**: the trunk's working backends() and ASR wiring, and esc-tune's escalate_gates plumbing. Read both sides fully before editing.
2. Build lm-host for Windows: `bash scripts/win-build.sh build -p lm-host --release` must finish with no errors.
3. `bash scripts/check.sh` must print '==> all checks passed', and run `bash scripts/live.sh` (the Thor is at 192.168.0.78).
4. Leave the rebased commit in place. Don't push; the reviewer merges. If you change anything beyond resolving the conflict, say exactly what and why.

Decision backends are `Box<dyn SysOne + Send + Sync>`. Answer in English, with what conflicted and how each was resolved.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

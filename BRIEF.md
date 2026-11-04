# BRIEF: bring the N1/N3 branch onto the current trunk

This worktree is branch origin/timing (one commit, 3073060: per-ask wall-clock timings (N1) and the escalation fence (N3); read `git log -1`). It predates several trunk merges that touch the same files: N2/N4 refusals (Refusal grew OffPlan, Superseded, Panicked and an `escalation_late` name reserved for N3; Escalation grew a `refusal` field; per-kind refusal telemetry), the surprise signal (surprise.rs's refusal_kind match), the batched server's harness hooks (LM_SEEDS, LM_THOR_PORT, LM_CAPTURE_PACKS in talk_on_thor.rs), the in-game voices and push-to-talk.
1. `git rebase origin/m1-integration`. Resolve every conflict **keeping both sides**. N3's Refusal::EscalationLate must use the name N2 reserved, `escalation_late`; one enum, no duplicates. Add the new variant to every exhaustive match (surprise.rs refusal_kind, the score view's hollow kinds, log kinds).
2. `bash scripts/check.sh` must print '==> all checks passed'; `bash scripts/win-build.sh build -p lm-host --release` must finish with no errors; run `bash scripts/live.sh`.
3. Don't push; the reviewer merges. Report each conflict and how you resolved it. Decision backends are `Box<dyn SysOne + Send + Sync>`. Answer in English.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE: your previous run was cancelled mid-rebase. `git status` shows the rebase in progress; continue resolving from there, then finish the verification steps.

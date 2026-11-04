# BRIEF: a wording rung on the ProArt's own GPU. When the Thor is busy, can a small local model word the lines?

Casey wants work spread across the devices. Tonight the Thor is heavily loaded: the realizer (LFM2-1.2B on the Thor, :8092) words lines at 563-593 ms p50, and 55-60% of lines fall back to the phrasebook at the 1000 ms deadline (crates/lm-agents/tests/realizer_on_thor.md §7). d1-2B decisions slowed to 1.1-1.8 s. The ProArt's RTX 4050 (6 GB) is idle. It runs Ollama at http://127.0.0.1:11434 (OpenAI-compatible at /v1) with gemma2:2b, llama3.2:1b, qwen3:0.6b, qwen2.5:0.5b and qwen2.5-coder:7b already pulled. qwen2.5-coder:7b measured 23.8 tok/s. Don't pull new models without a reason you state.

1. **A/B the local models as the realizer**, with the same harness, prompt (plan only, plain words) and checks as realizer_on_thor (it reads UTTER_LLM_MODEL / UTTER_LLM_PORT / UTTER_DUMP; it may need a base-URL setting for localhost: add the smallest config change). Arms: LFM2 on the Thor (:8092) vs gemma2:2b, llama3.2:1b and qwen3:0.6b on the ProArt. Per arm, 3 seeds × 3000 ticks: wording latency p50/p95, the fallback rate at 1000 ms, the on-plan rate (target, intent, recipient), and 15 sample lines side by side.
2. **Design the ladder:** Thor LFM2 → ProArt local → phrasebook, as an `auto` realizer mode that races or falls through within the deadline. Implement it if the A/B shows a local model is on-plan ≥ 95% and meaningfully cuts the fallback rate; otherwise write down why not. The game must never wait on wording.
3. **GPU sharing:** the ProArt's GPU also renders the game (Bevy, Windows). Measure whether a local model running on it affects the host's frame rate: run a Windows host round (scripts/win-build.sh; --agents stub --talk on) with and without the local realizer, and compare the frame times if the host reports them, or note what it reports.

Write the results as a new section in crates/lm-agents/tests/realizer_on_thor.md. `bash scripts/check.sh` must print '==> all checks passed'. Decision backends are `Box<dyn SysOne + Send + Sync>`. Don't edit decisions.md. Do not commit. Answer in English.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

## Note (director): zcode is out of quota; mcode continues this. Check git status: an earlier zcode run left partial work (config.rs, utter.rs, the A/B scripts). Keep the ProArt light: its memory is tight.

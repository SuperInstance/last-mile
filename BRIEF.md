# BRIEF: refresh the hands-on acceptance guide and the README for everything added since the first guide

docs/ACCEPTANCE.md was written before most of tonight's work. Since then the trunk gained:
- agents speaking in skald voices in the game (D73; `--talk on`, the voice bank, `--voice-log`, the HUD's voice hit/tone/pending);
- push-to-talk heard by the Thor's ears (`V`; voice.toml's [asr]; `--mic-wav` as a test hook; the HUD's ears line);
- the LFM2 realizer wording lines (HUD words llm/phrasebook/late);
- the conductor's score view (`E`; `--bottom score`, `--score-seconds`);
- the `B` view flip, the centred waiting card, and closing safely mid-flow;
- D70-D72 and D74-D76 behaviour (the split pack, escalation, per-gate confidence).

Read `git log --oneline` since the guide's commit, docs/notes/*.md, and the code (crates/lm-host main.rs parse_args, keys.rs, the plugins) for the exact flags and keys; never write from memory.

1. Update docs/ACCEPTANCE.md: add sections or steps for each new feature, with the exact PowerShell command, keys and pass/fail, keeping the existing structure. Re-verify every step on Windows (scripts/win-build.sh builds into this worktree's mirror) and re-shoot screenshots into docs/notes/accept-*.png where the screen changed. Thor services: 192.168.0.78 (d1-2B :8094, ears :8102, realizer :8092, skald :8000). The Thor is heavily loaded tonight, so voices may fall back to tones; say so honestly in a "Known issues" note if you see it.
2. Update README.md's status and features sections to match what's actually on the trunk, with links to the docs.
3. `bash scripts/check.sh` must print '==> all checks passed'.

Don't change product code. Don't edit decisions.md. Do not commit. Answer in English.

## CONTINUE (handover: the previous agent on this job stopped when its provider's quota ran out; its uncommitted work is in this worktree)
You are taking over this job. Run `git status` and `git diff` to see what the previous agent already did, and read any notes or drafts it left. Then finish the brief from there: keep what is sound, fix what isn't, and don't redo finished steps. Report what you inherited and what you did.

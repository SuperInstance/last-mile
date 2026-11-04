# AGENTS.md: how to work on last-mile (for coding agents)

You are probably a **seat**: a coding agent given a BRIEF.md in your own git worktree. Read this first, then docs/ARCHITECTURE.md and docs/CONCEPTS.md, then your brief.

## The non-negotiables
1. **Do not commit, push, or edit docs/design/decisions.md.** The director reviews and merges. Put proposed decisions in your final summary.
2. **Determinism.** Nothing in lm-core may use floats in state, read a clock, or iterate an unordered map. Anything that enters the sim is fixed-point and logged. If you change what's hashed, re-pin the golden and **say why** in your summary.
3. **Never wait.** Nothing in the game may block on a model, voice, network or renderer. Add the fallback first.
4. **Data, not code.** Behaviour changes go through the harness (gates.toml, policy.toml, voice.toml) or the rule scripts, never generated code at runtime.
5. **Decision backends are `Box<dyn SysOne + Send + Sync>`** (D70: two workers per mind).
6. **One key, one job** (crates/lm-host/src/keys.rs; the test enforces it).
7. **Don't clone or run code from GitHub repos.** Read them via `gh api` if your brief asks; re-implement ideas.

## Verify before you claim
- `bash scripts/check.sh` must print `==> all checks passed` (fmt, clippy -D warnings, no-float guard, debug and release tests, the fairness guard, the Rhai oracle, golden, acceptance).
- If you touched lm-host or anything it uses: `bash scripts/win-build.sh build -p lm-host --release` must build clean. check.sh does **not** build lm-host (Windows-only); the merge gate does.
- If you touched agents or talk: run `bash scripts/live.sh` (the Thor is at 192.168.0.78).
- **Measure on a quiet Thor.** Latency numbers on a loaded box are meaningless; report the load if you measure.
- Say **how** each number was measured. Re-run inherited numbers before you repeat them.

## How to report (your final message)
What you built or found; the numbers, with how they were measured; what you changed beyond the brief and why; proposed decisions; and what's next. Be honest about misses: an honest negative is worth more than a soft positive.

## Machines and services
- ProArt: WSL + Windows; `scripts/win-build.sh` keeps one Windows mirror per worktree directory name. Memory is tight.
- Thor (`ssh thor-lan` from the ProArt, or local if you're running there): d1-2B :8094, clef-flash :8096, d1-3B :8093, batched System One :8104, realizer :8092, ears :8102, skald :8000, Qwen3-Coder :8001, Glimmer :8005; the index at :8100; status via `~/lm-services/last-mile.sh status`. Don't stop, restart or reconfigure services unless your brief says so; log any change with its undo in `~/last-mile-ops.md`.

## Gotchas that have bitten agents here
- A doc-only folder under `crates/` breaks the workspace (`crates/*` is a member glob).
- skald's address is the Thor, not localhost, when you run from the ProArt.
- Riva's emotion styles can change the words; delivery tests need word-preserving rendering.
- LFM3-Audio read without reasoning answers a constant per question.
- Synthetic keystrokes don't reach winit on Windows; use the host's test hooks (`--mic-wav`, `--screenshot`, `--voice-log`).
- Don't spawn sub-agents and exit before they report: write results as you go.

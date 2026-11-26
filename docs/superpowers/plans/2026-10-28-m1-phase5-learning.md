# last-mile M1 Phase 5 (Rounds + learning) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use `- [ ]`. Seat-driven: ZCode and MiniMax Code implement, and Claude reviews and commits.

**Goal:** **Round N+1 uses lessons from round N**, and the improvement is measured honestly. After each round:
- metrics are computed from the log;
- a System-2 reviewer (cloud) proposes a change to the squad's **gates, building blocks or lexicon**, and writes a **sealed prediction** of its effect;
- the change is evaluated on **held-out seeds** before it's adopted;
- the human can make change requests, recorded as deposits;
- harnesses are tagged, and the next round's header pins them.

**Architecture:**
- A new crate, `lm-learn`, holds:
  - **metrics:** pure functions over a `RoundLog` plus `ScoreView` replays;
  - **review:** builds the System-2 review prompt from metrics and moments, and parses a typed proposal;
  - **eval:** a held-out-seed A/B runner on the Phase 1 runner, plus `lm branch` (Phase 1 Task 15);
  - **ledger:** sealed predictions and verdicts (an append-only JSONL with a hash chain);
  - **deposits:** human change requests as markdown (D33).
- A new crate, `lm-router`, holds provider adapters and allotments (spec §5).
- The between-round screen in `lm-host` shows metrics, the proposal, the prediction and the verdict, and has an Adopt/Reject control.

**Tech Stack:** Rust 1.99; `ureq` (as Phase 3) for cloud calls; `serde_json`. Earlier phases' crates.

**Spec:** §5 (System 2), §10 phase 5; D6, D7, D16, D25, D27, D30, D32, D33, D39, D40, D47.

## Global Constraints

- **Lessons enter only through harness files** (`gates.toml`, `blocks/*.rhai`, `reflexes/*.rhai`, `lexicon.md`, `policy.toml`), each change committed in the agent's harness repo and pinned by the next header (D27). No hidden state.
- **Every proposal carries a sealed prediction before it's evaluated** (D32): the expected direction and size of the change in ≥1 named metric, written to the ledger with its hash **before** the eval runs. The verdict is appended afterwards.
- **Eval seeds are held out:** the seeds a lesson was learned from never appear in its evaluation (⚙ `learning.eval_seeds`, default 4 seeds drawn from a separate range).
- **The trainer's output is questions and blocks, not labels** (D47). A proposal is one of:
  - `AddGate`, `EditGate`, `RemoveGate`;
  - `AddBlock`, `EditBlock`;
  - `EditReflex`;
  - `LexiconEntry`;
  - `PolicyTweak`.
- **Cloud spending goes through `lm-router` only.** Real provider limits live in its config, never visible to agents. Every call is metered by observation: tokens and requests counted, and a 429 or "limit" error marks the pool cold (fleet/01).
- **The model-vs-code dial is reported, not maximised** (D40): per-role `dial.target`, and distance to target, per round.
- Commits as before.

## Review Focus

1. **The cloud reviewer returns an unparseable or illegal proposal** (it edits a file outside the harness, or Rhai that doesn't compile): reject it with a named reason; the round flow continues with no change → `bad_proposal_rejected`, `uncompilable_rhai_rejected`.
2. **A proposal improves the training seeds but not the held-out seeds:** the verdict is "not adopted", even if the human likes it, unless the human overrides it explicitly (the override is logged) → `heldout_failure_not_adopted`.
3. **The provider is rate-limited mid-review:** the pool goes cold, a fallback provider is tried in config order, and if none is available the review is skipped with a logged reason and the game goes on → `cold_pool_falls_back`, `all_cold_skips_review`.
4. **Two squads' proposals touch the same shared file:** impossible by construction (each agent's harness is its own dir) → `harness_dirs_disjoint`.
5. **A metric over a round with zero events** (no utterances yet in M1, no tags): defined values (0 or "n/a"), no division by zero → `metrics_on_empty_round`.

---

## Tasks (outline; each expands to TDD steps when started, in the style of phases 1–3)

| # | Task | Produces | Key tests |
|---|---|---|---|
| 1 | **Metrics** (`lm-learn::metrics`) | `RoundMetrics::from_log(&RoundLog) -> RoundMetrics`: scores, tags for/against, beacon-hold ticks per squad, decision counts by provenance, refusals, on-time/late/stale, reflex share and the model-vs-code dial per agent (D25/D40), and "moments" (ticks where the score swing or a tag cluster happened, for the reviewer). Phase 4 adds the utterance metrics (order uptake, words per successful order, misread rate, airtime) | `metrics_on_empty_round`, `dial_counts_provenance`, `moments_found_at_tag_clusters` |
| 2 | **Router** (`lm-router`) | `Router::ask(pool_pref: &[Pool], req: ChatReq) -> Result<ChatResp, RouterError>` with adapters: Z.AI, MiniMax, Kimi and Claude via their HTTP APIs (keys from `~/.config/last-mile/secrets.env`), Jev via System One, plus a stub. Per-provider allotments and an exchange table, metered by observation; cold pools; config order fallback | `allotment_enforced`, `cold_pool_falls_back`, `all_cold_skips_review`, `keys_never_logged` |
| 3 | **Ledger** (`lm-learn::ledger`) | `Ledger::seal(prediction) -> SealId` (hash-chained JSONL), `Ledger::verdict(id, measured)`; a prediction can't be edited after sealing | `sealed_before_eval`, `tamper_detected`, `verdict_requires_seal` |
| 4 | **Proposals and review** (`lm-learn::review`) | `build_review_prompt(metrics, moments, harness) -> String`; `parse_proposal(text) -> Result<Proposal, Rejection>` (a strict JSON schema; Rhai compiled with `RhaiRuntime::load`; paths confined to the harness dir); `apply(proposal, harness_dir) -> CommitId` | `bad_proposal_rejected`, `uncompilable_rhai_rejected`, `path_escape_rejected`, `apply_commits_in_harness_repo` |
| 5 | **Held-out evaluation** (`lm-learn::eval`) | `evaluate(base_harnesses, proposal, eval_seeds) -> Verdict { metric_deltas, adopted: bool }`: N seeds × (baseline vs proposal) headless rounds through the runner; adopted iff the sealed prediction's metric moves in the predicted direction on ≥ ⌈0.75 N⌉ seeds | `heldout_failure_not_adopted`, `eval_is_deterministic`, `train_and_eval_seeds_disjoint` |
| 6 | **Deposits** (`lm-learn::deposits`, D33) | `Deposit { said, inferred, diff, questions }` as markdown under `runs/deposits/`, committed with the round | `deposit_roundtrip`, `deposit_links_round_and_commit` |
| 7 | **Round flow + between-round screen** (`lm-host`) | round → metrics → review → seal → eval → screen (metrics, proposal, prediction, verdict, Adopt/Reject/Override) → tag harnesses `round-<id>` → next header pins. A human request box writes a deposit and can be turned into a proposal by the reviewer | `round_n_plus_1_uses_lesson` (headless: a stub reviewer proposes a known-good reflex edit; round N+1's header pins the new commit, and its metric improves on eval seeds); human acceptance on the ProArt |
| 8 | **Rhai-scripted rulesets** (D7) | `rules::by_name("rhai:<file>")`: Beacons re-expressed as a script with the native one as its oracle (identical hashes on 50 seeds); between-round rule changes go through deposits | `rhai_beacons_matches_native` |

**Phase 5 is done when** (spec §10): round N+1 uses lessons from round N, adopted only through a sealed prediction plus a held-out eval, and the whole chain (log → metrics → proposal → seal → verdict → commit → pinned header) can be audited from files.

### Later (M2, recorded so nobody builds it early)

- COLOR SQUAD-style seasons with breed, fleet and author innovation breaks (D32).
- Automatic reflex promotion from situation keys (D25).
- Overnight LoRA distillation of d1 per role (D39).
- The matched-pair blindness probe for gate discovery (D47).
- The image path (D41).

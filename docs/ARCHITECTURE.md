# How last-mile works

*For any agent or person joining. Read this, then CONCEPTS.md (the vocabulary), then AGENTS.md (how to work here). Decisions are numbered D1-Dn in docs/design/decisions.md; this doc cites them rather than repeating their reasoning. Last reviewed 2026-10-30.*

## 1. The shape in one picture

```
        ProArt (Windows host + WSL)                          Thor (Jetson, aarch64)
 ┌──────────────────────────────────────────┐        ┌───────────────────────────────────┐
 │ lm-host (Bevy)  draws, plays audio, keys │        │ d1-2B :8094  System One answers   │
 │   └ lm-host-model  the host loop:        │  HTTP  │ clef-flash :8096  second opinion  │
 │       sim tick ─► minds ─► packs ────────┼───────►│ batched :8104  same answers, conc.│
 │       ◄─ answers ◄─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┼◄───────┤ LFM2 :8092  wording (realizer)   │
 │       applied decisions ─► lm-core sim   │        │ LFM3-Audio :8102  ears (ASR)      │
 │       ─► hash-chained round log          │        │ skald :8000 / Riva  voices (TTS)  │
 │ lm-learn  between rounds: measure, seal, │        │ Qwen3-Coder :8001, Glimmer :8005  │
 │           test on held-out seeds, adopt  │        │   (free pi workers, not the game) │
 └──────────────────────────────────────────┘        └───────────────────────────────────┘
        Orin (archive): ingests round logs, replays them, verifies every hash (lm-archive)
```

**The rule that shapes everything: only decisions travel, and the sim never waits** (D-series on never-wait). The simulation is deterministic fixed-point on the ProArt. Models answer questions; an answer becomes a decision only when it is applied at a tick and logged. A late answer is dropped and the reflex plays. So any machine can replay any round bit-exact from its log (D69: x86-64 and aarch64 agree).

## 2. The crates, inside out

| Crate | Owns | Must never |
|---|---|---|
| **lm-core** | the deterministic sim: world/grid, rules (beacons, tags, holds), agents and lanes, the scheduler, fixed-point maths (`fixed`, `trig_table`), the RNG, hearing, utterances/talk events, the hash-chained `log`, `refusal` kinds, `script` (Rhai rule oracle), `view` (what an agent may see) | use a float in sim state; read a clock; depend on any other crate |
| **lm-proto** | wire messages, the postcard codec, version handshake | change a message without a version bump |
| **lm-agents** | cognition: `facts` (state → text facts per agent), `gates` (the closed questions), `sysone` (the System One client, backends, escalation, the per-gate confidence rule), the least-use queue, `mind` (when to think, what to ask, the talk triggers), `utter` (planning and wording lines), `surprise` (where minds struggle) | let a model answer reach the sim except through the applied-decision path |
| **lm-learn** | between rounds: metrics, proposals, the sealed-prediction ledger, held-out evaluation (parallel seeds, 16 + sign test per D78), deposits (applied changes), the flow, review | adopt without a sealed prediction |
| **lm-router** | the cloud purse: provider adapters and per-provider allotments for System Two calls | spend without an allotment |
| **lm-voice** | TTS/ASR traits; skald client, voice bank and prefetch (D73); the ASR HTTP client (ears); the intelligibility gate | block the game on audio |
| **lm-vox** | Vox Score: the speech-performance protocol (L0 frames → L1 gestures → L2 phrase → L2.5 marks → L3 intent), analyzer, encoder, LPC renderer | use non-deterministic maths where it feeds L2/L3 |
| **lm-render** | rendering helpers shared by the host (upscale, fog, view math) | touch sim state |
| **lm-host-model** | the host loop without Bevy (testable): input mapping, the map, piano roll, the conductor's score view, voices in the host, the between-round screen's model | depend on Bevy |
| **lm-host** | the Windows Bevy app: plugins for map, HUD, audio, input, between screen, 3D view, score; the single key binding table `keys.rs` | own logic that could live in lm-host-model; bind one key to two jobs |
| **lm-tools** | the `lm` CLI: run, replay, diff, golden, learn, voice, vox | — |
| **lm-archive** | the Orin side: ingest, replay, verify | — |

Dependency direction: lm-core ← everything; lm-host depends on everything; nothing depends on lm-host.

## 3. One tick, end to end

1. **Sim** (`lm-core::sim`) advances one fixed tick: movement, hearing, rules (beacon holds, tags/stuns), and talk airtime. State is hashed into the log segment.
2. **Minds** (`lm-agents::mind`) decide whether each agent thinks this tick (budgets, D68: members at 1 Hz by default) and which **pack** to ask: the **move pack** every think; the **talk pack** only on a trigger (sight change, order heard, threat, timer; D70).
3. **Facts** (`lm-agents::facts`) render what the agent may know as short text. Facts set the target the model reasons about (the engagement study found facts.rs, not the harness, sets the target).
4. **The pack** (gates from `assets/harness/default/gates.toml`) goes to the System One backend over HTTP (`sysone`): choice, noul (presence) and score questions. The Thor answers with per-option probabilities. The cost is about 11 ms per option, scored sequentially per question (D70); the batched server runs a pack's questions concurrently (identical answers).
5. **The confidence rule is per gate** (D72): an unsure gate drops only its own decision (Answers::unsure). Escalation to clef-flash happens only for the move gate by default (D75) and is fenced by the decision's apply tick (N3, pending merge).
6. **Applied decisions** land at a tick via the least-use queue and the reflex fallback (a late answer plays the reflex). Every refusal is named and logged (N2), and every ask ends in exactly one state: accepted, late or refused (N4).
7. **Talk:** a planned line (`utter`) is worded by the realizer (LFM2 with a deadline, else the phrasebook) and voiced by the bank (skald WAV if ready, else a tone). Audio is presentation only.
8. **The human:** push-to-talk audio goes to the ears (LFM3-Audio); the transcript becomes a human line and an order on the score. Keys possess, follow and hold (keys.rs).
9. **The round log** is hash-chained (protocol v3); the golden tests pin final state hashes.

## 4. Between rounds

`lm-learn::flow`: measure the round (metrics) → make a proposal (stub reviewer or router to a cloud model) → **seal a prediction** (metric, direction, min delta) → evaluate on held-out seeds (in parallel; 16 seeds with ceil(3N/4) qualified AND sign test p < 0.05 per D78, at 1500 ticks; shorter rounds measure nothing) → verdict → the human adopts, rejects or overrides on the between-round screen → a deposit (applied change) pins the next round. The surprise table shows where minds struggled.

## 5. Determinism: the boundary

- **Inside** (hashed, replayed): lm-core state, applied decisions, refusals, talk events, adopted harness changes, and in future paint and ghost fields.
- **Outside** (presentation and telemetry, never hashed): wall-clock timings (N1), audio, voice bank contents, camera, HUD, model latencies, the surprise table.
- Moving something inside means fixed-point, logging, and re-pinning the golden **with a reason**.

## 6. Where to change what

| To change | Edit | Then check |
|---|---|---|
| what agents are asked | `assets/harness/default/gates.toml` (wording is behaviour) | talk_on_thor / urgency / engagement harnesses on the Thor |
| what agents know | `crates/lm-agents/src/facts.rs` | the same, plus decisiveness |
| when they think or talk | `policy.toml`, `mind.rs` | the think budget; talk_soak |
| the game's rules | `lm-core::rules` + `assets/rules/*.rhai` (must match tick for tick) | the golden re-pin with reason; the Rhai oracle |
| voices | `assets/harness/default/voice.toml` | `lm voice`; the live skald lane |
| keys | `crates/lm-host/src/keys.rs` | `no_key_has_two_jobs` |
| what's learned and how it's judged | `lm-learn` (flow, eval) | SEEDFARM bench |

## 7. Where things are heading

See docs/GRAND-PLAN.md (vision, pillars, milestones), docs/ROADMAP-AHEAD.md (schemas drafted ahead: Vibe blocks, sealed proposals, paint state, personality patches, the question ledger, ghost fields) and the designs. The architecture stays: deterministic core, questions answered by local models, intent written by the cloud between rounds, everything applied as logged data.

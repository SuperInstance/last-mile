# last-mile Milestone 1: design spec

**Date:** 2026-10-28 · **Status:** written; awaiting user review
**Decision log:** [`docs/design/decisions.md`](../../design/decisions.md) (D1–D20) · **Research:** [`docs/research/forclaude2/`](../../research/forclaude2/00-overview.md)

## 1. Goal

A thin, working path through every tier of a distributed squad game:
- 2–4 squads of 4 self-directed AI agents talk to each other in a procedurally generated area and compete over rounds.
- A human plays as any squad member or spectates.
- The ProArt sequences and renders. The Thor thinks for the agents. The Orin archives and verifies. Cloud subscriptions provide slow, budgeted thinking.

M1 proves the architecture: **decisions-only deterministic lockstep, agents that read the score, budgets as gameplay, utterance scores, and bit-exact replay.** Depth comes in later milestones.

**Principle (D19):** every value with more than one workable choice is a **parameter** with a default. Defaults are listed in §11.

## 2. Architecture

```
ProArt (Windows, native)            Thor (aarch64 Linux)              Orin (aarch64 Linux)
┌──────────────────────────┐        ┌───────────────────────────┐     ┌────────────────────┐
│ lm-host                  │ QUIC   │ lm-agents                 │     │ lm-archive         │
│  sequencer + authority   │◄──────►│  mirror sim (lm-core)     │     │  mirror sim        │
│  lm-core sim             │        │  per-agent ScoreView      │     │  log store         │
│  input (human)           │        │  summarizer, think policy │     │  replay verifier   │
│  views: 2D map, piano    │        │  fair-share model queue   │     └────────▲───────────┘
│   roll, 3D (retro-modern)│        │  model clients (NIM/vLLM) │              │
│  audio playback          │        │  Riva TTS/ASR client      │   decision stream
└──────────┬───────────────┘        │  vector DB client         │   + round header
           │                         │  harness store (git)      │              │
           └─────────────────────────┤ lm-router (cloud purse)   ├──────────────┘
                                     └───────────────────────────┘
```

**Crates (one Cargo workspace):**
| Crate | Runs on | Responsibility |
|---|---|---|
| `lm-core` | all | Fixed-point deterministic sim: seed worldgen (L0 chunks), movement and collision, line of sight and visibility, hearing, General Action Set, utterance score expansion, reflex script runtime, budget ledger rules, fair-share ordering rules, event log, state hash, `view()` |
| `lm-proto` | all | Message types, postcard codec, version/capability handshake, QUIC transport helpers |
| `lm-host` | ProArt | Sequencer, authoritative sim loop, Bevy app (views, input, audio), round flow |
| `lm-agents` | Thor | Agent host: mirror sim, cognition loop, model queue, model clients, harness store, memory |
| `lm-router` | Thor | Cloud provider adapters, per-provider allotments, exchange table, hard limits |
| `lm-archive` | Orin | Log ingestion, storage, headless replay verification |
| `lm-tools` | dev | CLI: run headless rounds, replay a log, diff state hashes, export `.mid` (later) |

**Dependency rule:** `lm-core` depends on nothing networked or rendering-related, and it builds with no Bevy. Everything else depends on `lm-core` + `lm-proto`. This is what lets the same sim run headless on ARM.

## 3. The score (approved section 2, D20)

- **Time:** the sim tick (⚙ default 30 Hz) is the only authoritative clock. Rendering interpolates between ticks.
- **Fixed point:** positions = `(chunk: IVec3, offset: i32 units of 1/1024 m)`; angles are `u16`; trig comes from tables. **No floats anywhere in `lm-core` sim state or logic.**
- **Round header:** `{proto_version, seed, world_params, ruleset_hash, roster[agent → squad, role, voice, harness_commit], budgets, exchange_table, params, start_tick}`. Membership is frozen for the round.
- **Decision:** `{apply_tick, source, seq, kind, provenance}`, where kind is one of `ActionOn | ActionOff | Control | Program | Utterance | Intent | Marker`. Provenance records reflex, system1 (model, tokens, latency), system2 (provider, tokens) or human.
- **Scheduling:** AI decisions request `apply_tick ≥ now + lead` (⚙ 3 ticks). The sequencer stamps the final tick: on time → as requested; late → next tick, with lateness logged; older than the stale window (⚙ 15 ticks) → dropped. **The sim never waits.**
- **Integrity:** a state hash every ⚙ 30 ticks; hash-chained log segments. Replay = header + stamped decision stream, bit-exact.
- **Wire:** postcard over QUIC (`quinn`), one reliable ordered stream per peer for decisions, plus a separate stream for bulk payloads (TTS audio, later bake results).

## 4. World and game rules (M1)

- **World:** a bounded procedural area (⚙ default 8×8 chunks of 32 m) generated from the seed: terrain heights, building blocks, walls and cover, all on a coarse grid so collision and line of sight stay cheap in fixed point.
- **Default mode, "Beacons"** (the ruleset is a parameter and a Rhai script):
  - ⚙ 3 beacons; squads score per tick a beacon is held.
  - Contact is **tagging** (a stun-and-respawn timer, not lethal).
  - Fog of war: you only know what your squad can see or has been told.
  - The win score or round time limit is a parameter.
  - Scouting, callouts and coordinated holds are rewarded, so **talk matters**.
- **Visibility and hearing are numbers** (D13): per-cell light/visibility values; utterances reach listeners by channel and range, attenuated through walls (⚙ ranges per channel: shout, voice, whisper, radio).
- **Fairness:** the per-tick processing order of agents and squads rotates (the MIDI Maze lesson).

## 5. Agents (approved section 3)

**Agent = data:**
- identity (name, squad, role, voice)
- **harness**: a git repo on the Thor, containing `prompts/`, `reflexes/*.rhai`, `tools/`, `policy.toml`, `lexicon.md`
- memory (a vector DB namespace with a quota)
- lineage (commits)

The leader is a role flag plus the squad-plan duty.

**Cognition loop (per agent, on the Thor):**
1. `ScoreView` from the mirror sim at the think rate (⚙ member 4 Hz, leader 1 Hz).
2. **Summarize.** ⚙ `summarizer = deterministic | model`; M1 default is deterministic, from `lm-core`.
3. **Decide whether to think.** This comes from the harness `policy.toml`: thresholds on novelty, threat, addressed-by-name, and remaining budget.
4. **Think.** Enqueue on the local model's **least-use-first queue**:
   - priority = this-round usage (⚙ unit tokens | gpu_ms; ⚙ squad weight; ⚙ decay),
   - batching window ⚙ 20 ms,
   - structured/constrained output against the decision schema.
5. **Act.** Decisions with `apply_tick` go to the sequencer.

**Reflexes:**
- Rhai scripts (floats disabled, operation limit = fuel per tick ⚙) run **inside `lm-core` on every node**, so they cost no traffic and replay deterministically.
- They sit behind a `ScriptRuntime` trait (⚙ `rhai | wasm` later).
- Out of fuel → the script is cut off for that tick, and the default motor behavior continues.

**System 2:**
- `lm-router` spends per-provider allotments at the exchange rate.
- Mid-round use is budgeted, and results become `Intent` decisions.
- After the round: **one review per squad** produces a lexicon/doctrine diff, which is committed to the leader's and members' harness `lexicon.md` and stored in the vector DB.
- *(Full self-re-harness sessions are M2.)*

**Budgets and HUD:**
- Per agent per round: System-1 usage (queue priority), cloud allotments by provider, reflex fuel, context allowance, storage quota.
- The ledger lives in `lm-core` rules plus router accounting, and every spend is logged.
- **Real limits live only in `lm-router`** (provider windows, API-key caps). Agents see game allotments only.

**Utterances (M1 scope of research/09):**
- System 1 emits `text + per-word emphasis + pace + affect/intent tags`.
- `lm-core` expands that into score lanes deterministically.
- Agents receive the score, degraded by the hearing physics.
- The LLM projection is an annotated-text form.

## 6. ProArt client (section 4)

- **Native Windows** Bevy app; WSL is used only for tooling.
- wgpu `HighPerformance` adapter, so it runs on the 4050 rather than the iGPU.
- Sim and render threads pinned to the full Zen 5 cores (⚙ affinity mask).

**Views, built in order:**
1. **2D overhead map:** agents, beacons, sight cones, utterances as speech ticks. This is the MIDIcam/spectator view.
2. **Piano roll:** a timeline lane per agent showing actions, utterances, think requests (with queue wait and latency bars), and budget spend. It's also the M1 debugger.
3. **3D retro-modern:**
   - low-poly, vertex colors, palette, ⚙ internal resolution with upscale (⚙ `integer | dlss`),
   - L0 chunks only in M1,
   - **clean proxy-friendly output** (flat material classes) for future neural renderers.

**Human:**
- *Possess/release* any agent (the same decision interface as agents, D10 lesson 10), or spectate (free camera, follow an agent).
- Push-to-talk to Riva ASR → utterance (prosody capture is M2).

**Audio:** TTS requested from the Thor only for utterances the human's listener position can hear. A cached bark library covers short lines. ⚙ voice per agent.

**VRAM budget:** pooled allocations, sized by parameters. The default split is in research/11.

## 7. Orin archive (section 5)

`lm-archive` subscribes to the header plus the stamped decision stream and stores logs on NVMe. It re-runs each round headless and **verifies every state hash**. A mismatch is reported to the ProArt HUD and flagged in the log. The asset bank is an M2 stub (a content-addressed directory layout only).

## 8. Failure handling

| Failure | Behavior |
|---|---|
| Thor unreachable or slow | All agents run reflex-only; the game continues; the HUD shows "agents on reflex"; reconnect resyncs the mirror |
| A model call misses its deadline | Dropped by the stale window; reflex continues; lateness logged |
| Cloud provider down or quota exhausted | System-2 requests fail fast with a logged refusal; no mid-round effect beyond missing advice |
| Orin unreachable | The ProArt buffers logs locally (⚙ cap) and flushes on reconnect |
| Desync (hash mismatch on a mirror) | Logged with the first divergent tick; the mirror requests a **snapshot** from the ProArt and resumes; the round continues (the ProArt is the authority) |
| Reflex script error or fuel exhaustion | That agent's script is disabled for the tick (error) or cut off (fuel); default motor behavior; logged |
| Malformed or out-of-version message | Rejected at the handshake or decode; the peer is disconnected with a reason |

## 9. Testing

- **Determinism:**
  - golden replays (log → identical hash sequence) in CI on x86,
  - the **same logs replayed on the Thor/Orin (ARM)** as a cross-arch check,
  - property tests for fixed-point math and trig tables.
- **Sim unit tests:** movement and collision, line of sight, hearing attenuation, beacon scoring, rotation fairness.
- **Protocol:** encode/decode round-trips, handshake version mismatch, the stale/late stamping rules.
- **Agents:**
  - a **deterministic stub model backend** (scripted responses) so the cognition loop, queue ordering, batching and budget accounting are testable without GPUs,
  - fair-share queue ordering tests (least-use-first, squad weight, decay).
- **Router:** allotment and exchange math; hard-limit enforcement; failure paths with fake providers.
- **Soak:** headless rounds back to back, with flat memory and no hash mismatches.
- **Integration:** a headless 2-squad round with stub models on one machine; then the real LAN round.

## 10. Build phases inside M1 (each one is a usable stopping point)

1. **Core:** workspace, `lm-core` (fixed point, worldgen L0, sim, rules, view, log/hash, Rhai reflexes), `lm-proto`, `lm-tools` headless runner plus replay. *Done when:* headless 2-squad rounds with reflex-only agents replay bit-exact.
2. **Host plus 2D:** the `lm-host` sequencer, the 2D map and piano roll, human possess/spectate, local agents on reflex. *Done when:* you can play a round on the ProArt.
3. **Thor agent host:** mirror sim over QUIC, cognition loop, least-use-first queue, model client (stub, then a real local model), budgets plus HUD. *Done when:* agents think on the Thor and late thoughts fall back cleanly.
4. **Talk:** utterance scores, hearing physics, Riva TTS for audible lines, barks, push-to-talk ASR. *Done when:* squads coordinate by talking and you hear them.
5. **Rounds and learning:** the round flow, between-round screen (order uptake, words per successful order, misread rate, airtime), `lm-router` with a System-2 squad review → lexicon in the harness plus vector DB. *Done when:* round N+1 uses lessons from round N.
6. **3D plus archive:** the retro-modern 3D view (L0), Orin `lm-archive` verification. *Done when:* the full M1 loop runs on all three boxes.

**Out of M1 (planned later):** self-re-harness sessions and gates (M2); pre-render L1–L3, the asset bank and 👍/👎 on assets (M2); human prosody capture and formant voices (M2); Studio mode, A/B and MIDI control surfaces (M3+); EdenLive replays (spike first).

## 11. Parameters (defaults)

| Parameter | Default | Where |
|---|---|---|
| `sim.tick_hz` | 30 | header |
| `sched.min_lead_ticks` | 3 | header |
| `sched.stale_window_ticks` | 15 | header |
| `log.hash_interval_ticks` | 30 | header |
| `world.size_chunks` / `world.chunk_m` | 8×8 / 32 | header |
| `rules.script` | `beacons.rhai` | header |
| `rules.beacons` / win score / time limit | 3 / ruleset / ruleset | header |
| `squads.count` / `squads.size` | 2 / 4 | header |
| `hearing.range_m` (shout/voice/whisper/radio) | 60/25/5/∞ (squad) | header |
| `agent.think_hz` (member/leader) | 4 / 1 | header |
| `agent.summarizer` | deterministic | header |
| `queue.usage_unit` / `squad_weight` / `decay` / `batch_ms` | tokens / 0.5 / none / 20 | header |
| `script.runtime` / `fuel_per_tick` | rhai / 10 000 ops | header |
| `budget.*` (per provider, context, storage) | set per round | header |
| `router.exchange_table`, provider limits | user config | Thor config (never visible to agents) |
| `render.internal_res` / `upscale` | 960×540 / integer | client config |
| `render.vram_pools` | research/11 split | client config |
| `cpu.affinity` | Zen 5 cores | client config |
| `archive.buffer_cap` | 1 GB | client config |

## 12. Open items (don't block phase 1)

- The Thor model inventory (the user's spec sheet) decides the default System-1 model, summarizer model, Riva voices and vector DB.
- Provider terms for subscription-backed automated use (research/12).
- Windows toolchain: **checked.** Rust 1.99 is installed in WSL, and native `cargo` exists on Windows (`C:\Users\casey\.cargo\bin`). Plan: develop and test `lm-core`/`lm-proto` in WSL; build and run `lm-host` with Windows `cargo` from a Windows-side checkout (or a Windows path), not across `\\wsl.localhost` (UNC paths break `cmd`).

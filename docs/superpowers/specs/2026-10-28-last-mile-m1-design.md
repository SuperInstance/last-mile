# last-mile Milestone 1: design spec

**Date:** 2026-10-28 · **Status:** written and refined with fleet research (D23–D45, including PoC results); awaiting user review
**Decision log:** [`docs/design/decisions.md`](../../design/decisions.md) (D1–D45) · **Research:** [`docs/research/forclaude2/`](../../research/forclaude2/00-overview.md), [`docs/research/fleet/`](../../research/fleet/00-overview.md)

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
- **Decision:** `{apply_tick, source, seq, kind, provenance}`, where kind is one of `ActionOn | ActionOff | Control | Program | Utterance | Intent | Marker | Roll | Lane`. Provenance records reflex, system1 (model, tokens, latency, situation key), system2 (provider, tokens) or human.
- **Roll (D23):** `{menu_id, options, p: [u16], seed_stream}`. `lm-core` rolls it at `apply_tick` with the agent's seeded RNG stream and applies the chosen option. Every mirror rolls identically. Field names follow the quilt's `ROLL` shape (D36).
- **Lane (D38):** `{gate, values: [u8]}`: one presence-mode sample of a gate's probabilities. `lm-core` conditions lanes in fixed point (EMA ⚙ α, then a deadband with hysteresis ⚙ width). An action changes only when a lane leaves its deadband.
- **Intent TTL and safe branch (D26, D45):** `Intent` decisions carry `ttl_ticks` (⚙ 300), an optimistic plan (it may cross unknown/fog cells), and a **safe branch** (known-free cells only, ending in cover or a hold). When an intent expires, is refused or arrives late, the agent follows its safe branch. Paths come from integer Jump Point Search on the coarse grid.
- **Scheduling:** AI decisions request `apply_tick ≥ now + lead`. ⚙ `sched.lead_mode = adaptive` (D34): lead = clamp(measured one-way p99 + margin, ⚙ min 3, ⚙ max 9 ticks). With `fixed`, the lead is the minimum. The sequencer stamps the final tick: on time → as requested; late → next tick, with lateness logged; older than the stale window (⚙ 15 ticks) → dropped. **The sim never waits.**
- **Integrity:** a state hash every ⚙ 30 ticks; hash-chained log segments. Replay = header + stamped decision stream, bit-exact.
- **Wire:** postcard over QUIC (`quinn`), one reliable ordered stream per peer for decisions, plus a separate stream for bulk payloads (TTS audio, later bake results).
- **Connection direction (D34):** the ProArt **dials out** to the Thor and the Orin (WSL2 sits behind NAT, and inbound connections to Windows need firewall rules). Which node dials is independent of which node is the authority. ⚙ `net.sequencer_node` (default `proart` when a human plays; `thor` is allowed for agent-only rounds, D36).

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
- **harness**: a git repo on the Thor, containing `prompts/`, `reflexes/*.rhai`, `blocks/*.rhai` (building blocks as levels, D40), `gates.toml` (typed questions, D37), `tools/`, `policy.toml`, `lexicon.md`
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
   - **System-1 is a decision model (D37):** state (the typed summary) + the agent's gates (bool / choice / score; options generated from the sim's legal menu) → `p[]` per gate.
     - actions use **argmax with hysteresis over conditioned lanes** (D43); seeded rolls are only for exploration (⚙ `s1.explore_temp`, default 0) and for procgen; scores are rounded; booleans are thresholded with hysteresis.
     - the decision model picks the speech act, addressee, intent and affect; the wording comes from ⚙ `utter.realizer = template | llm | auto`.
   - **Modes (D38):** ⚙ `s1.mode = point | presence | auto`. Point mode is the policy-triggered think. Presence mode evaluates the whole squad's gates as one batch at ⚙ `s1.presence_hz` and emits `Lane` decisions. `auto` uses presence mode while measured batch p99 + transit fits the period.
   - **Pre-drafting (D31):** an agent addressed by a teammate may enqueue its reply when the utterance *starts*, and target `apply_tick` just after it ends.
5. **Act.** Decisions with `apply_tick` go to the sequencer.
   - Output that fails the schema is a **refusal** (D26): logged, no decision, the reflex keeps playing. There are never guessed defaults.
   - Each decision carries its **situation key** (D25), a hash of the summary features the policy used. This is the raw material for reflex promotion.

**Models (D35/D37, provisional until spikes S1/S2 and Casey's spec sheet):**
- member gates: **Liquid d1-2B** (local on the Thor);
- leader gates: **Clef-flash** (local on the Thor, or the Workers AI pool);
- optional cloud gate pool: TypeSafe Jev (Clef is Jev-API compatible);
- utterance wording when `llm`: LFM2-1.2B (Thor :8092);
- between-round operator: Qwen3-Coder-30B-A3B (:8001);
- between-round judge: Clef-27B or cloud System-2.

A stub backend (scripted `p[]`) covers phases 1–2.

All are parameters. During a round, think requests are quilt *reaction/conversation* class, which preempts the Thor's production tenants (D36).

**Reflexes:**
- Rhai scripts (floats disabled, operation limit = fuel per tick ⚙) run **inside `lm-core` on every node**, so they cost no traffic and replay deterministically.
- They sit behind a `ScriptRuntime` trait (⚙ `rhai | wasm` later).
- Out of fuel → the script is cut off for that tick, and the default motor behavior continues.

**System 2:**
- `lm-router` spends per-provider allotments at the exchange rate.
- Mid-round use is budgeted, and results become `Intent` decisions.
- After the round: **one review per squad** produces a lexicon/doctrine diff, which is committed to the leader's and members' harness `lexicon.md` and stored in the vector DB.
- The reviewer writes a **sealed prediction** of the diff's effect before it's evaluated on **held-out seeds** (D32).
- Harnesses are tagged `round-<id>` after each round, and the next header pins their commits (D27).
- M1 logs every gate answer with its state hash and outcome, which is the training data for the trainer/student loop (D39).
- *(Full self-re-harness sessions, automatic reflex promotion, gate authoring, overnight LoRA distillation, the image path (D41) and COLOR SQUAD-style seasons are M2.)*

**Budgets and HUD:**
- Per agent per round: System-1 usage (queue priority), cloud allotments by provider, reflex fuel, context allowance, storage quota, **airtime** (D31).
- When a budget runs out: ⚙ `budget.on_exhaust = reflex | freeze` (D24).
- The HUD also shows the **model-vs-code dial**: the share of decision weight decided by code (reflexes, blocks, templates) versus a model, against a per-role ⚙ `dial.target` (D40). Reflex share is its extreme end (D25).
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
- Push-to-talk to Riva ASR → utterance (human prosody capture is **M1**, not M2: `communicator` is reused as a local service on the ProArt — D53). Negations in commands are refused, not interpreted (D29).
- Optional **tutor ladder** (⚙ `human.tutor = off | ladder`): verbs unlock as the human uses earlier ones (D29).

**Audio:**
- TTS is requested from the Thor only for utterances the human's listener position can hear.
- A cached bark library covers short lines, keyed by normalized text, voice and affect (D25).
- ⚙ voice per agent.
- **Half-duplex (D28):** agent speech yields to the human's push-to-talk. Held frames are dropped, not queued. ⚙ hang 400 ms, ⚙ max talk 6 s.

**Between rounds:** each human change request (rules or parameters) is saved as a markdown deposit with the words said, what was inferred, the exact diff, and open questions. It's committed with the round (D33).

**VRAM budget:** pooled allocations, sized by parameters. The default split is in research/11.

## 7. Orin archive (section 5)

`lm-archive` subscribes to the header plus the stamped decision stream and stores logs on NVMe. It re-runs each round headless and **verifies every state hash**. A mismatch is reported to the ProArt HUD and flagged in the log. The asset bank is an M2 stub (a content-addressed directory layout only).

## 8. Failure handling

| Failure | Behavior |
|---|---|
| Thor unreachable or slow | All agents run reflex-only (or the ProArt-local tier if ⚙ `agent.s1_fallback = proart_local`); the game continues; the HUD shows "agents on reflex"; reconnect resyncs the mirror |
| Model output fails the schema | A refusal: logged, no decision, the reflex continues (D26) |
| Intent expires | Reflex defaults resume; logged (D26) |
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
  - property tests for fixed-point math and trig tables,
  - a **golden-vector suite** (fixed point, trig, Rhai reflexes, dice rolls) checked on x86 and ARM (D26; thor-firstline's parity vectors).
- **Kill test:** SIGKILL the host and a mirror mid-round, restart, and confirm the resumed decision stream and hashes are identical (continuum-runtime).
- **Sim unit tests:** movement and collision, line of sight, hearing attenuation, beacon scoring, rotation fairness.
- **Protocol:** encode/decode round-trips, handshake version mismatch, the stale/late stamping rules.
- **Agents:**
  - a **deterministic stub model backend** (scripted responses) so the cognition loop, queue ordering, batching and budget accounting are testable without GPUs,
  - fair-share queue ordering tests (least-use-first, squad weight, decay).
- **Router:** allotment and exchange math; hard-limit enforcement; failure paths with fake providers.
- **Soak:** headless rounds back to back, with flat memory and no hash mismatches.
- **Learning:** evaluation seeds are held out from the seeds that produced a lesson, and never mixed (D32).
- **Integration:** a headless 2-squad round with stub models on one machine; then the real LAN round.

## 10. Build phases inside M1 (each one is a usable stopping point)

1. **Core:** workspace, `lm-core` (fixed point, worldgen L0, sim, rules, view, decision menus and rolls, lane conditioning, Rhai reflexes and blocks, log/hash), `lm-proto`, `lm-tools` headless runner plus replay, golden vectors. *Done when:* headless 2-squad rounds with reflex-only agents replay bit-exact.
2. **Host plus 2D:** the `lm-host` sequencer, the 2D map and piano roll, human possess/spectate, local agents on reflex. *Done when:* you can play a round on the ProArt.
3. **Thor agent host:** mirror sim over QUIC (the ProArt dials), adaptive lead, cognition loop, least-use-first queue, decision-model client (stub, then d1-2B; point and presence modes after spike S1), refusals and intent TTL, budgets plus HUD with reflex share. *Done when:* agents think on the Thor and late thoughts fall back cleanly.
4. **Talk:** utterance scores, hearing physics, Riva TTS for audible lines, barks, push-to-talk ASR, human prosody capture (the local `communicator` service, D53), half-duplex gate, pre-drafted replies, optional tutor ladder. *Done when:* squads coordinate by talking and you hear them.
5. **Rounds and learning:** the round flow, between-round screen (order uptake, uptake by speaker, words per successful order, misread rate, airtime, reflex share), `lm-router` with a System-2 squad review → lexicon in the harness plus vector DB, sealed predictions with held-out eval seeds, harness tags, change-request deposits. *Done when:* round N+1 uses lessons from round N.
6. **3D plus archive:** the retro-modern 3D view (L0), Orin `lm-archive` verification. *Done when:* the full M1 loop runs on all three boxes.

**Out of M1 (planned later):** self-re-harness sessions and gates (M2); pre-render L1–L3, the asset bank and 👍/👎 on assets (M2); human formant voices (M2); Studio mode, A/B and MIDI control surfaces (M3+); EdenLive replays (spike first).

## 11. Parameters (defaults)

| Parameter | Default | Where |
|---|---|---|
| `sim.tick_hz` | 30 | header |
| `sched.lead_mode` / `min_lead_ticks` / `max_lead_ticks` | adaptive / 3 / 9 | header |
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
| `agent.s1_output` / `menu_max` | distribution (actions), direct (text) / 8 | header |
| `s1.backend` | d1 (member) / clef_flash (leader); stub in tests | Thor config |
| `s1.mode` / `presence_hz` | auto / 10 (PoC A, D42) | header |
| `lane.alpha` / `lane.deadband` | 1/4 / 0.10 (PoC A) | header |
| `s1.explore_temp` | 0 | header |
| `sched.lead_estimator` | host-level, N=32, +10 ms margin (PoC B, D44) | header |
| `gates.max_questions` | 8 per agent (Clef limit 64) | harness |
| `utter.realizer` | auto | header |
| `dial.target` | per role | harness |
| `s1.image_path` | off | header |
| `agent.s1_fallback` | reflex | header |
| `agent.models` (member / leader / operator) | LFM2-1.2B / qwen3.5:9b / Qwen3-Coder-30B | Thor config |
| `intent.default_ttl_ticks` | 300 | header |
| `budget.on_exhaust` / `budget.airtime_s` | reflex / set per round | header |
| `reflex.promote_after` / `decay` / `evict_on_contradiction` | 3 / ruleset / true (promotion in M2) | header |
| `memory.full_window_ticks` / `archive.downmix_after_rounds` | 1 800 / 20 | Thor / Orin config |
| `audio.duplex` / `hang_ms` / `max_talk_ms` | half / 400 / 6 000 | client config |
| `human.tutor` | ladder | client config |
| `learning.eval_seeds` | 4 held-out seeds | header |
| `net.sequencer_node` / `net.dial` / ports | proart / proart_dials / not 8788 | config |
| `quilt.heartbeat_s` | 60 (off until the quilt bridge exists) | client config |

## 12. Open items (don't block phase 1)

- The Thor model inventory: **partly answered** by the quilt's Stage 0 audit (fleet/01, D35). The spec sheet still decides the Riva port and voices, the vector DB, and any replacement for the D35 defaults.
- Spikes S1/S2 (decision-model latency on the Thor and the 4050) set `s1.mode = auto` thresholds and confirm D37's model picks. Check d1's schema/API on its model card.
- Network: wired Ethernet for the Thor is recommended (its Wi-Fi is the p99 storm source). The adaptive lead covers it until then.
- Provider terms for subscription-backed automated use (research/12).
- Windows toolchain: **checked.** Rust 1.99 is installed in WSL, and native `cargo` exists on Windows (`C:\Users\casey\.cargo\bin`). Plan: develop and test `lm-core`/`lm-proto` in WSL; build and run `lm-host` with Windows `cargo` from a Windows-side checkout (or a Windows path), not across `\\wsl.localhost` (UNC paths break `cmd`).

# last-mile M1 Phase 3 (Agents think) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use `- [ ]`. Seat-driven in this repo: ZCode and MiniMax Code implement, and Claude reviews and commits.

**Goal:** Agents think. Each agent's cognition loop:
- turns its fog-of-war `ScoreView` into **code-computed facts**;
- asks its **gates** (typed questions) to a **decision model** (d1-3B on the Thor by default) through a **least-use-first queue** under **budgets**;
- turns the answers into scheduled `Lane`/`Intent` decisions, falling back cleanly (refusal → reflex, late → stale drop, expired → safe branch).

Rounds still replay bit-exact without any model, because model answers are logged decisions.

**Architecture:**
- A new crate, `lm-agents`, holds:
  - **facts:** a deterministic ScoreView → plain-language + structured facts (D49);
  - **gates:** `gates.toml` → System One questions, and answers → decisions;
  - **sysone:** a System One HTTP client with persistent connections (D50) and backends for the Thor (local), Jev and a stub;
  - **queue:** least-use-first with budgets (D16);
  - **mind:** the cognition loop with point/presence/auto modes (D38/D42);
  - **harness:** a per-agent store pinned by commit (D27).
- `lm-agents` talks to the host through a `DecisionSink` trait:
  - in-process (a channel into `HostLoop::submit`) for M1;
  - `lm-proto` over a socket later.
- **Where `lm-agents` runs is a parameter** (D51): the ProArt by default, since models are reached over HTTP either way.

**Tech Stack:** Rust 1.99; `ureq` 2.x (blocking HTTP with keep-alive) **or** `reqwest` blocking, whichever the seat verifies works cleanly on both x86 and aarch64; `serde_json`; `toml`. Phase 1 and Phase 2 crates.

**Spec:** spec §5 (agents), §8 (failure), §10 phase 3. Decisions D16, D23–D27, D31, D37–D44, D47, D49–D51. Evidence: `docs/research/poc/{thor,jev,clef,presence,student}/`.

## Global Constraints

- **`lm-core` stays model-free.** Nothing in `lm-agents` may be required to replay a round: every model output enters the sim only as a logged `Decision` with `Provenance::System1 { model, tokens, latency_ms, situation_key }` or `System2 {…}`.
- **Facts are computed by code, not models** (D49): distances, LOS, path lengths and bearings are code. The model sees plain-language facts and answers judgement questions. No raw grids are sent to models in M1.
- **Refusal is an outcome** (D26): a failed HTTP call, a non-2xx status, a malformed answer, or confidence below the model's threshold → no decision, a logged refusal, and the reflex keeps playing. **Never fabricate a default answer.**
- **Never average models** (Thor handoff §3.6). Each backend has its own confidence threshold (⚙ `sysone.<backend>.min_confidence`).
- **Never wait:** the sim thread never blocks on a model. Requests carry a deadline; answers that arrive after `apply_tick + stale_window` are dropped by the sequencer.
- **Persistent connections** to every endpoint (D50). One client per backend, reused.
- **Secrets:** the Jev key is read from `~/.config/last-mile/secrets.env` (`TYPESAFE_API_KEY=`) at runtime, never logged, never sent anywhere but `api.typesafe.ai`. The Thor endpoints are unauthenticated LAN endpoints (`http://192.168.0.78:809x`); never send the key there.
- **Every tunable is a parameter** (D19), in `AgentsConfig` (`agents.toml`), with its defaults listed in Task 1.
- Commits as in Phase 1.

## Review Focus

1. **The Thor is unreachable or slow** (Wi-Fi spike, model reload): agents fall back to reflexes within one deadline, with no backlog storm when it returns (the queue drops expired requests) → `thor_down_falls_back`, `no_backlog_after_outage` (Task 5).
2. **A model answer names an option that isn't on the menu any more** (the menu changed between ask and answer): refuse, never map it to a neighbouring option → `stale_menu_answer_refused` (Task 2).
3. **One agent hogs the model:** least-use-first must still serve the others → `hog_does_not_starve_others` (Task 4).
4. **A budget hits zero mid-round:** `on_exhaust = reflex` stops asking; `freeze` freezes; no further HTTP calls → `exhausted_budget_makes_no_calls` (Task 4).
5. **The answer's confidence is missing** (the Thor handoff §5.3): derive it from the probabilities as the largest normalised share, never treat it as 1.0 → `missing_confidence_derived` (Task 3).

---

## File Structure

```
crates/lm-agents/
  Cargo.toml
  src/lib.rs           AgentsConfig (+ defaults) and module tree
  src/facts.rs         Facts::from_view(&ScoreView, &World) -> Facts { text: String, key: u64, fields: BTreeMap }
  src/gates.rs         GateSet (from gates.toml), to_questions(&Menu), answers_to_decisions()
  src/sysone.rs        SysOne trait; ThorBackend, JevBackend, StubBackend; Answer types
  src/queue.rs         LeastUseQueue, Usage, Budgets
  src/mind.rs          Mind (per-agent loop), Mode::{Point, Presence, Auto}, RttEstimator
  src/harness.rs       Harness dir: gates.toml, policy.toml, reflexes/*.rhai; commit pinning
  src/sink.rs          DecisionSink trait; ChannelSink (in-process)
assets/harness/default/{gates.toml,policy.toml}
crates/lm-host-model/src/host_loop.rs   (modify) accept decisions from a ChannelSink each tick
```

### Task 1: Config and facts

**Interfaces:**
- `AgentsConfig` (serde, `Default`), loaded from an optional `agents.toml`:

| field | default | field | default |
|---|---|---|---|
| `node` | `"proart"` | `backend_member` | `"thor-d1-3b"` |
| `backend_leader` | `"thor-d1-3b"` | `thor_base` | `"http://192.168.0.78"` |
| `ports` | `{d1-omni-600m: 8095, d1-2b: 8094, d1-3b: 8093, clef-flash: 8096}` | `min_confidence` | `{thor-d1-3b: 0.55, jev: 0.5}` |
| `think_hz_member` | 4 | `think_hz_leader` | 1 |
| `mode` | `"auto"` | `presence_hz` | 10 |
| `max_questions` | 3 (d1 cost ≈ 10–50 ms per question) | `request_timeout_ms` | 400 |
| `squad_weight` | 0.5 | `budget_calls_per_round` | 400 per agent |
| `on_exhaust` | `"reflex"` | | |

- `Facts::from_view(view: &ScoreView, world: &World, me_squad: u8) -> Facts`:
  - **plain sentences** in a fixed order: my position; each beacon (with holder, path length and bearing, all computed by code); each fresh enemy (distance, LOS yes/no); allies within 4 cells; my status;
  - `key` = FNV-1a 64 over the canonical `fields` (D25 situation key).
- [ ] Tests:
  - `facts_are_deterministic` (same view → same text and key);
  - `facts_state_numbers_not_grids` (no grid characters; contains "cells away");
  - `stale_enemies_omitted` (age > 6 is not mentioned);
  - `key_changes_with_situation`.

### Task 2: Gates

**Interfaces:**
- `gates.toml`: a list of `{ id, type: "choice"|"noul"|"score", instructions, criteria? , options_from: "menu"|"fixed" }`.
- `GateSet::to_questions(&self, menu: &Menu) -> serde_json::Value`: System One `questions`. Menu options become `choice` criteria with stable keys `"opt0".."optN"` plus a human description per option.
- `answers_to_decisions(set, answers, menu, agent, now, lead) -> Result<Vec<Decision>, Refusal>`. The `choice` gate `"move"` becomes a `Lane { gate, menu_id, values }`, with the probabilities quantised to u8 in menu order.
- `assets/harness/default/gates.toml`: one `choice` gate over the menu ("What should you do now?") and one `noul` gate ("Should you call out to your squad?"; it's logged only in phase 3, and utterances arrive in phase 4).
- [ ] Tests:
  - `menu_becomes_choice_criteria`;
  - `answers_quantise_in_menu_order`;
  - `stale_menu_answer_refused` (the answer's key set ≠ the current menu → `Refusal::MenuChanged`);
  - `unknown_gate_refused`.

### Task 3: The System One client

**Interfaces:**
- `trait SysOne { fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal>; fn name(&self) -> &str; }`
- `ThorBackend::new(base, port, model)` (keep-alive agent, no auth), `JevBackend::from_secrets()` (bearer key from the secrets file), and `StubBackend::scripted(Vec<(Duration, Result<Answers, Refusal>)>)` for tests.
- `Answers { per_gate: BTreeMap<String, Answer>, model: String, input_tokens: u32, latency_ms: u32 }`, where `Answer::{Choice { probs: BTreeMap<String, f32>, confidence: f32 }, Noul(f32), Score { probs, score, confidence }}`.
- `Refusal::{Timeout, Http(u16), Malformed(String), LowConfidence(f32), MenuChanged, Budget}`.
- [ ] Tests (the stub, plus a local mock HTTP server using `std::net::TcpListener` in the test):
  - `parses_thor_shape` and `parses_jev_shape` (fixtures copied from `docs/research/poc/thor/` and `jev/` logs);
  - `missing_confidence_derived`;
  - `timeout_is_refusal`;
  - `non_2xx_is_refusal`;
  - `low_confidence_is_refusal`;
  - `connection_reused` (the mock counts accepted TCP connections: 1 for 5 calls).
- [ ] An **opt-in live test**, `#[ignore]`, run with `cargo test -- --ignored live_thor`: ask d1-3B one facts pack and assert a 200 with a choice.

### Task 4: The least-use-first queue and budgets

**Interfaces:**
- `LeastUseQueue::new(cfg)`, `push(req: ThinkReq)`, `pop_batch(now) -> Vec<ThinkReq>` (batches the window, ordered by `usage_this_round × squad_weight^…` ascending, ties by agent id), `record(agent, tokens, ms)`.
- `Budgets { calls_left: BTreeMap<u16, u32> }`, `try_spend(agent) -> Result<(), Refusal::Budget>`.
- Expired requests (`deadline < now`) are dropped at pop, never sent.
- [ ] Tests:
  - `least_used_goes_first`;
  - `hog_does_not_starve_others` (agent 0 pushes 50 requests and agents 1–3 one each; all of 1–3 are served within the first 4 pops);
  - `expired_requests_dropped`;
  - `exhausted_budget_makes_no_calls` (with a stub that counts calls).

### Task 5: The cognition loop (Mind) and fallbacks

> **Amended by D55 (thinking time is gameplay):** the Mind keeps a `TierTable`. Each backend (reflex, `thor-d1-3b`, `thor-tev1`, `jev`, …) has a measured latency estimate (its own `RttEstimator`, p50 and p99 in ticks) and a configured quality rank. `policy.toml` maps the situation to a tier choice: for example, fresh threat → the fastest tier whose p99 lands within the threat's horizon; no threat and a beacon plan → the best-quality tier whose p99 fits `max_think_ticks`. Every request records `{tier, asked, expected_land}`, and every landed decision records the actual landing tick (provenance `latency_ms`). Tests: `urgent_picks_fast_tier`, `calm_picks_quality_tier`, `tier_estimates_adapt` (a stub that slows down shifts choices to the faster tier). The piano roll (phase 2) draws thought bars from these records.

**Interfaces:**
- `Mind::new(agent, role, harness, cfg)`, `on_tick(&mut self, now, view, world, menu) -> Option<ThinkReq>` (the policy decides: novelty via a change in the facts key, a fresh threat, the think rate), `on_answer(&mut self, req, result) -> Vec<Decision>` (via `answers_to_decisions` with `lead = lead_ticks(p, rtt.p99())`).
- `RttEstimator` (a window of 32, D44).
- `Mode::Auto`: presence while the measured batch p99 + transit < `1000/presence_hz` ms, else point mode (D38).
- A worker thread runs the HTTP calls. The sim thread only pushes and drains channels.
- [ ] Tests (stub backend with scripted latencies):
  - `fast_model_produces_lane_decisions` (stub at 20 ms: the decisions arrive, are stamped on time and change the agent's lane command);
  - `thor_down_falls_back` (every call times out → refusals logged, zero decisions, agents still move by reflex, the round completes);
  - `no_backlog_after_outage` (outage then recovery: queue length ≤ agents);
  - `auto_switches_modes` (latency 200 ms → point; 20 ms → presence);
  - `round_replays_without_models` (a round played with the stub, then `lm replay` with no backend gives identical hashes).

### Task 6: Harness store

**Interfaces:**
- `Harness::open(dir) -> Harness` reads `gates.toml`, `policy.toml` and `reflexes/*.rhai`; `Harness::commit_id() -> String` (the `git rev-parse HEAD` of that dir, or a content hash if it isn't a git repo).
- `HarnessSet::for_roster(root, roster) -> …`; `Header` builders fill `RosterEntry.harness_commit` and embed the reflex sources (D27).
- [ ] Tests:
  - `default_harness_loads`;
  - `commit_pinned_in_header`;
  - `content_hash_when_not_git`.

### Task 7: Integration and done-when

- `lm-host-model::HostLoop` gains `attach_agents(Agents)`: each tick it calls every `Mind::on_tick`, pushes `ThinkReq`s to the worker, drains answered decisions into `submit_agent(…)` with `arrived = now`, and records refusals as stats.
- `lm-host --agents thor|stub|off` runs it live. The HUD shows think requests, refusals, queue length and per-agent usage.
- [ ] Tests:
  - `stub_round_end_to_end` (900 ticks; decisions logged with `System1` provenance; replay exact);
  - an **opt-in live** `live_round_on_thor` (`#[ignore]`): 300 ticks against d1-3B, with at least 1 decision applied and no panics.
- [ ] Human acceptance: `lm-host --seed 1 --agents thor`. Watch the HUD's think counter climb and agents change course on lane decisions. Then pull the Wi-Fi: agents keep playing on reflexes.
- [ ] Commit `feat(agents): agents think on the Thor's decision models — phase 3 done-when met`.

**Phase 3 is done when** (spec §10): agents think on the Thor's models, late or failed thoughts fall back cleanly, and the round still replays bit-exact without any model.

### Out of this plan (handoffs)

- **Cross-architecture golden check:** ask the Thor's agent (in a `forthor` handoff) to `git clone` the repo, `cargo run -p lm-tools --release -- golden`, and report `OK 3/3` on aarch64. This is the D10/D26 x86↔ARM proof.
- **QUIC transport and a Thor-resident `lm-agents`:** only if measured latency says the extra hop matters (D51).

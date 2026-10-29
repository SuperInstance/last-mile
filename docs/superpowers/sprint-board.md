# Sprint board

*Live status of the seat-driven build. Claude orchestrates: it writes a brief per task and runs ZCode (`zcode`, Z.AI GLM) and MiniMax Code (`mcode`) jobs in parallel git worktrees. It reviews each diff, runs `scripts/check.sh` itself, and commits with the seat named. Updated as tasks move.*

**Legend:** ✅ merged · 🔨 in progress · ⏳ queued (waiting on a dependency) · 🧪 experiment

## Phase 1: Core ([plan](plans/2026-10-28-m1-phase1-core.md)), branch `phase1-core`

| Task | What | Seat | Status | Notes |
|---|---|---|---|---|
| 1 | Workspace, guardrails, fixed-point math, hash, RNG | MiniMax | ✅ | Trig table made a `static`; host-libm tests gated to x86 |
| 2 | Params, header, validation | ZCode | ✅ | Validation also rejects every zero divisor |
| 3 | Mirrored, connected worldgen | ZCode | ✅ | 500 seeds connected |
| 4 | LOS + Jump Point Search | ZCode | ✅ | Corrected pruning rules from prototype P1; equals A* on 200/200 maps |
| 5 | Decisions + never-wait sequencer | MiniMax | ✅ | Margin moved into `Params.lead_margin_ms` |
| 6 | Lane conditioner | MiniMax | ✅ | |
| 7 | Agents, motor, safe branches | ZCode | ✅ | |
| 8 | Fog-of-war ScoreView, menus | ZCode | ✅ | |
| 9 | Beacons ruleset | MiniMax | ✅ | |
| 10 | Rhai reflexes | MiniMax | ✅ | `goto` is a Rhai keyword, so it's quoted |
| 11 | Sim step + state hash | MiniMax | ✅ | Found that the reflex never made contact → fixed (`5f65529`) |
| — | Determinism audit (read-only) | ZCode | ✅ | No divergence; 2 risks and 4 nits fixed (`0c500b0`) |
| 12 | Hash-chained round log | MiniMax | ✅ | |
| 13 | lm-proto codec + handshake | MiniMax | ✅ | |
| 14 | `lm` CLI: run/replay/diff/golden; end-to-end proof | MiniMax | 🔨 | Golden hashes regenerate after the reflex fix |
| 15 | `lm branch` (the permanent test) | — | ⏳ after 14 | From Grok's review |

## Phase 2: Host + 2D ([plan](plans/2026-10-28-m1-phase2-host.md)), branch `phase2-host`

| Task | What | Seat | Status | Notes |
|---|---|---|---|---|
| 1 | Bevy 0.20.1 spike + Windows build script + API notes | ZCode | 🔨 | Builds on Windows from a mirrored copy; verified by screenshot PNG |
| 2 | lm-tools as a library + host loop | — | ⏳ after P1-14 | |
| 3 | Input → decisions, possession | — | ⏳ after P2-2 | |
| 4 | Map model (fog, interpolation) | — | ⏳ after P2-2 | Can run in parallel with 3 and 5 |
| 5 | Piano roll + replay scrubber | — | ⏳ after P2-2 | |
| 6 | Bevy app wiring | — | ⏳ after P2-1, 3–5 | |
| 7 | Recording, replay mode, done-when | — | ⏳ after 6 | Casey's acceptance on the ProArt |

## Phase 3: Agents think ([plan](plans/2026-10-28-m1-phase3-agents.md)), branch `phase3-agents`

| Task | What | Seat | Status | Notes |
|---|---|---|---|---|
| — | Scaffold `lm-agents` (modules declared up front) | Claude | ✅ | Lets tasks run in parallel without lib.rs collisions |
| 1 | Config + code-computed facts (D49) | ZCode | 🔨 | |
| 2 | Gates → System One questions → decisions | — | ⏳ after 3 | Needs the Answer types |
| 3 | System One client (Thor/Jev/stub), keep-alive, refusals | MiniMax | 🔨 | |
| 4 | Least-use-first queue + budgets | Claude | ✅ | One pending request per agent, so no backlog by construction |
| 5 | Mind loop: point/presence/auto, fallbacks | — | ⏳ after 1–3 | |
| 6 | Harness store pinned by commit | — | ⏳ | |
| 7 | Integration into the host; done-when | — | ⏳ after P2 | |

## Side experiments

| Item | Where | Seat | Status |
|---|---|---|---|
| X1: host-agnostic record/replay/branch/diff on two hosts | [decision-daw](https://github.com/SuperInstance/decision-daw) | ZCode built ✅ · MiniMax red-team ⏳ | 🧪 built: 20/20 replays, branch prefix byte-identical |
| Thor requests (d1-3B parallel slots, embeddings, Riva/vector DB info) | `C:\Users\casey\forthor1.md` | Thor's agent | waiting on the Thor |

## Throughput rules

- At most two jobs per seat at a time. Merge conflicts are usually one `lib.rs` line, and Claude resolves them at merge.
- Each brief carries "Notes from merged tasks" with the real names, so seats don't guess interfaces.
- Nothing merges without `scripts/check.sh` green in debug and release, run by Claude.

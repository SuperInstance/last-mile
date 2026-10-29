# E4c — the Thor's local decision models, and "facts, not grids" (2026-10-28)

The Thor serves four decision models over the System One API (`POST /v1/systemone`, no auth, LAN):
- d1-omni-600M on :8095
- d1-2B on :8094
- d1-3B on :8093
- clef-flash on :8096

Details are in [THOR-HANDOFF.md](THOR-HANDOFF.md), written by the Thor's agent. Same 40 scenes and questions as E4.

## Reading a text grid (FRAME) vs the controls

| model | cond | enemy_visible | action | bearing | reachable | p50 ms |
|---|---|---|---|---|---|---|
| d1-omni-600M | frame | 0.38 | 0.40 | 0.30 | 0.17 | 83 |
| d1-omni-600M | blind | 0.62 | 0.40 | 0.20 | 0.97 | 51 |
| d1-3B | frame | 0.62 | 0.45 | 0.15 | 0.97 | 179 |
| d1-3B | gates (grid + facts) | 0.78 | 0.53 | 0.25 | 1.00 | 205 |
| majority | | 0.62 | 0.40 | 0.35 | 0.97 | |

**The small local models can't read a text grid.** The 600M does worse with the frame than blind.

## FACTS: code-computed facts in plain language, no grid

| model | enemy_visible | action | bearing | reachable | p50 ms (from ProArt) |
|---|---|---|---|---|---|
| d1-omni-600M | 0.62 | 0.40 | 0.23 | 0.68 | 52 |
| **d1-3B** | **0.97** | **0.97** | 0.38 | **1.00** | 153 |
| **Jev (cloud)** | **1.00** | **1.00** | 0.82 | **1.00** | 229 |

- **Facts beat grids.** Raw spatial input confuses every decision model we tried. Code states the facts, and the model applies judgement and rules: d1-3B goes from 0.45 to **0.97** on action.
- **Arithmetic stays in code.** Bearing (comparing two offsets) defeats d1-3B (0.38). Code computes it; that's the calculator in "calculator vs storyteller".
- **d1-omni-600M** can't apply even a stated rule here. It's only usable for trivial direct questions, and only after its calibration is checked against a stronger model (the Thor handoff §3.6: the models disagree wildly).

## Wi-Fi cost (`wifi.py`)

| measurement | value |
|---|---|
| 1-question call from the ProArt, new HTTP connection each time | p50 22.0 ms, p90 26.6, p95 29.0 |
| same call measured on the Thor itself | 9.7 ms |
| ProArt → Thor ping | avg 6.0 ms, max 13.3 |
| **p99** | **~1,037 ms** (2 of 200 calls) |

- At the median, Wi-Fi plus connection setup adds about 12 ms per call.
- The p99 spikes are TCP connection-setup losses waiting out the 1 s retransmit timer.
- Persistent connections (the planned QUIC link) remove the 1 s spikes, and Ethernet removes most of the jitter.
- Per-question model time (~10 ms on the 600M, ~50 ms on d1-3B) dominates once connections persist.

## Concurrency (`par.py`)

| endpoint | 8 sequential | 8 concurrent (wall) | per-call range |
|---|---|---|---|
| d1-3B :8093 | 773 ms (97/call) | 638 ms | 167–635 ms |
| d1-omni-600M :8095 | 271 ms (34/call) | 161 ms | 55–160 ms |

d1-3B effectively serializes requests, so with 16 agents squad-wide thinking runs at about 1 Hz. The request for parallel slots is in REQUESTS-TO-THOR.md.

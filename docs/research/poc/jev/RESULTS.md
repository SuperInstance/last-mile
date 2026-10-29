# E4 — Jev reading a text-rendered tactical grid (real model, 2026-10-28)

Method: syzygy-curriculum's controls. 40 seeded 12×12 grids (walls, agent A, 0–2 enemies, beacon), rendered as plain character text. Ground truth is computed by code (Bresenham LOS, BFS). One Jev call (`jev-latest`, 4 questions: 2 noul, 2 choice) per scene per condition. Called from the ProArt over the WAN.

| condition | enemy_visible | action | beacon_bearing | beacon_reachable | p50 ms | p95 ms |
|---|---|---|---|---|---|---|
| FRAME (the grid) | 0.85 | 0.72 | 0.75 | 0.97 | 235 | 291 |
| BLIND (no grid) | 0.62 | 0.23 | 0.15 | 0.03 | 236 | 354 |
| SHUFFLED (wrong grid) | 0.55 | 0.25 | 0.15 | 0.97 | 234 | 302 |
| GATES (grid + code-computed LOS/BFS facts) | 0.85 | 0.88 | 0.82 | 0.97 | 246 | 1307 |
| majority-class baseline | 0.62 | 0.40 | 0.35 | 0.97 | | |

Reading:
- **It really reads the frame.** Lift over blind is +0.49 on action and +0.60 on bearing, and shuffled collapses to blind. This is position-reading, like syzygy-curriculum's finding.
- **Code-computed gates help** (action 0.72 → 0.88, the E2 effect with a real model), but **don't reach 100%** even when the facts are handed over. So last-mile shouldn't ask a model to re-derive what code already knows: code computes the gates, and the model answers only judgement questions over them (D47).
- `beacon_reachable` is uninformative here (97% of maps are reachable). The blind model says "no" (0.03), a prior worth knowing.
- **~235 ms p50 over the WAN, ~690 input tokens per call.** Jev is a **point-mode and teacher-side** model, not a presence-mode student. Local d1 fills that role (D37).
- The key never leaves `~/.config/last-mile/secrets.env` except in the Authorization header sent to api.typesafe.ai.

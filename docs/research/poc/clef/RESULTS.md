# E4b — Clef-flash (Workers AI) on the same text-grid test (2026-10-28)

Same 40 scenes, questions and controls as [E4 / Jev](../jev/RESULTS.md). Clef-flash ran via Workers AI through a throwaway `wrangler dev --remote` Worker (`worker-index.js`, `wrangler.toml`); nothing was deployed. Clef accepts Jev's request shape (`choice`/`noul` with `criteria`) and answers with `probabilities` and `confidence`; `output_tokens` is 0.

| condition | enemy_visible | action | beacon_bearing | beacon_reachable | p50 ms | p95 ms |
|---|---|---|---|---|---|---|
| FRAME | 0.62 | 0.40 | 0.55 | 0.30 | 263 | 792 |
| BLIND | 0.62 | 0.40 | 0.15 | 0.03 | 256 | 1087 |
| SHUFFLED | 0.62 | 0.40 | 0.15 | 0.30 | 253 | 391 |
| GATES (+ code-computed facts) | 0.85 | 0.70 | 0.75 | 0.50 | 265 | 811 |
| majority | 0.62 | 0.40 | 0.35 | 0.97 | | |
| *Jev, FRAME (for comparison)* | *0.85* | *0.72* | *0.75* | *0.97* | *235* | *291* |

Reading:
- Clef-flash **barely reads a raw text grid**: action and visibility sit at the majority baseline, and only the bearing shows lift (0.55 vs blind 0.15). Jev reads it much better.
- **With code-computed gate facts, Clef-flash ≈ Jev on the frame** (action 0.70, visibility 0.85, bearing 0.75). This is the strongest real-model evidence for D47: **code computes the gates, the decision model judges over them**. Which model you use matters far less than whether its questions are well posed.
- Clef-flash leans hard toward "not reachable" (0.30 on a 97%-reachable set), so its booleans need calibration or code.
- The latency here includes the dev-proxy hop. It doesn't reflect local Clef-flash on the Thor.

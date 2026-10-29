Done — `sim.py` is written, ran with `python3` (16s, stdlib only, deterministic), and all four measurements came back positive. Full output is above; here are the results and the reading.

**Setup.** 24×16 map, WFC-style solver over 8 tile types with symmetric adjacency rules (doors only in walls, beacons only in open ground, spawns in wall niches) plus one end-of-map hard rule (every door set into a wall/spawn). Every collapse is a typed decision: p[] over allowed tiles, rolled with `Random(crc32(seed, attempt, step))`. Baseline p is uniform; learned p is a softmax over 46 local features (neighbour support by type, beacon proximity, open-area bucket, tile identity, and tile×context products). Hidden clicker: likes 2–4 cover near each beacon, dislikes water touching spawns, likes width-2 corridors, dislikes dead ends; thumbs up iff score ≥ the reference-batch median (θ = −69.10).

**Results.**

| measurement | outcome |
|---|---|
| Learning curve (mean taste, first-5 → last-5 batches) | uniform −63.09 → −61.80 (flat, as it must be); learned −61.65 → −24.28 |
| Thumbs-up rate | 60% → 99% (chance 50%) |
| Determinism (same seed + same weights) | 3/3 replays byte-identical, incl. replaying an in-batch map from its seed — PASS |
| Contradiction rate | uniform 0.26 restarts/map (21% of maps), learned 0.45 (32%), max 5, zero exhausted solvers |
| Delta regen (10 trials) | 100% success, 80% of the 6×6 region's tiles changed, only 9.4% of the map touched, taste −5.40 → +0.36, 8/10 maps improved |

The curve is unambiguous from batch ~9 on: learned stays between −20 and −43 while uniform oscillates around −60, and every learned map from batch 20 onward gets a thumb up. In the sample before/after maps you can see the repair is surgical — the six marked rows change while all other rows are byte-identical.

**READING** (as printed by the script):

1. The same decision channel that recognizes also generates: with one bit of feedback per map, learned taste goes −61.65 → −24.28 across batches while uniform sits at −63.09 → −61.80, and thumbs-up rises 60% → 99% (chance 50%).
2. The learner recovers structure it was never shown: weight on cover-near-beacon is +0.11 and on water-next-to-spawn −0.14 — the hidden clicker's two strongest likes/dislikes, reconstructed from whole-map thumbs alone.
3. Determinism holds: replaying (seed, weights) reproduces byte-identical hashes (PASS), because every dice is `Random(crc32(seed, attempt, step))` and cell order is pure min-entropy.
4. Contradictions stay cheap: 0.26 restarts/map uniform vs 0.45 learned — a sharper p[] asks more of the constraint graph (door-near-wall weight −0.08), and restarts absorb it (max 5).
5. Delta regen works: freezing 90.6% of the map and re-rolling one disliked 6×6 succeeds 100% of the time, changes 80% of the region, and moves taste −5.40 → 0.36 (8/10 improved) without touching the rest.
6. Verdict on the open question: yes, in miniature — "decision model + seeded dice" gives generation, learning from thumbs, and targeted repair with one mechanism; runtime 16s, stdlib only, zero nondeterminism.

One honest caveat baked into the numbers: the learned weights are small (±0.1 range) because credit assignment is crude — one thumb per map spread evenly across ~380 decisions via reward-weighted feature averages. The score gain comes mostly from the heavily-penalized terms (water-spawn pairs, dead ends) where even a small nudge compounds over hundreds of decisions; a per-decision credit scheme would be the obvious next step, but for a throwaway PoC the signal is already clear.
EXIT 0

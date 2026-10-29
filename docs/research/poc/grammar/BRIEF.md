# E3 — Generation as a stream of typed decisions (THROWAWAY PoC)

Open question: can the same "decision model + seeded dice" that recognizes also GENERATE: procedural rooms where every placement is a typed question, a taste model is learned from a human's thumbs up/down, and only the disliked region is regenerated?
Write `sim.py` HERE ONLY: Python 3 stdlib, deterministic seeds, no network. Run it and print tables plus a 6-line reading.

## Generator
- A 24x16 tile map of rooms, built by a wave-function-collapse-style constraint solver over 8 tile types (floor, wall, door, cover, window, water, beacon, spawn) with adjacency rules you define.
- At each collapse step the choice among the allowed tiles is made by a "decision model": p[] over the allowed options, then a seeded roll (rng = Random(hash(seed, step))).
  - Baseline: uniform p.
  - Learned: p = softmax of a linear taste model over local features (neighbour counts by type, distance to the nearest beacon, open-area size bucket).

## Hidden human taste (the "clicker")
- A hidden scoring function over maps, e.g. likes 2–4 cover tiles near each beacon, dislikes water touching spawns, likes corridors of width 2, dislikes dead ends.
- The human gives thumbs up/down on whole maps: up if score >= the median of a reference batch. No other signal.

## Learning
- After each batch of 20 generated maps, update the taste model from thumbs (any simple method: e.g. reward-weighted feature averages, or a perceptron on per-decision features from liked vs disliked maps).
- Run 30 batches.

## Measure
- mean hidden-taste score per batch: uniform vs learned (learning curve)
- determinism: the same seed + same taste weights → a byte-identical map (hash)
- DELTA REGEN: take a liked map and mark one 6x6 region "disliked". Regenerate only that region (keep the rest fixed as constraints).
  - report the fraction of tiles regenerated, the success rate (constraints satisfiable), and the taste score before/after.
- contradiction rate of the solver (restarts needed) for uniform vs learned

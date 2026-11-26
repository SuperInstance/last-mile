# The seed farm

The between-round flow seals a prediction for each proposal ("hold_ticks_squad0
rises by at least 1") and judges it on held-out seeds. Until now that was
**4 seeds**, played one after the other: thin statistics, where a verdict can
turn on luck. This note is what the Thor (14 Arm Neoverse cores) says about
farming the judgement out to many seeds at once.

Everything here was measured with the bench in `examples/seedfarm.rs`
(`cargo run --release -p lm-learn --example seedfarm -- timing 300` and
friends). The bench writes nothing into a flow record or a ledger, keeps no
wall-clock value anywhere the chain could see one, and every number below can
be read again by running it twice.

## What changed in the crate

* **The seeds are judged in parallel** (`eval.rs`). `std::thread::scope`, no
  new dependency — rayon would have been a new crate for a job the standard
  library already does. The seeds are split into contiguous blocks, one block
  per worker, and the blocks are reassembled in seed order; a worker that
  panics is resumed, not swallowed. The verdict bound is
  `dyn Fn(u64) -> Header + Send + Sync`, which is why the header closures the
  flow builds are safe to share across workers.
* **The parallel run is the serial run, bit for bit.** The sim is
  deterministic and bit-exact on aarch64 (D69), each seed is judged on its own,
  and only the schedule differs. `tests/eval.rs` holds the two to each other:
  same verdict, same deltas in the same order, and — the strong form — the
  same per-tick hash chains for both rounds of every seed
  (`the_parallel_evaluation_plays_the_same_rounds`). `evaluate_serial` is the
  serial reference the tests use.
* **The seed count is a parameter**: `FlowConfig::with_eval_seeds(n)` and
  `lm learn --eval-seeds N`. The default **is 16** now
  (`DEFAULT_EVAL_SEEDS`) — the recommendation below, applied on 2026-10-30 —
  and the seeds of a round are unchanged at any count ≤ 1000 (the round of
  seed 1 still judges 1_001_000..1_001_003). A count above 1000 is given its
  own block of the seed space, so two rounds never judge the same seed at
  any count.
* A prediction judged on no seeds adopts nothing (it used to adopt
  vacuously); `a_prediction_with_no_seeds_is_not_adopted` pins that.

## How fast, on this Thor

Wall time of the held-out evaluation (baseline and candidate, all seeds,
including the metric check), 14 workers. Each row's parallel and serial
numbers come from the same run.

**At `--ticks 300`** (the quick headless round):

| seeds | parallel | serial | speed-up |
|---:|---:|---:|---:|
| 4 | 0.43 s | 1.73 s | 4.0x |
| 16 | 0.89 s | 6.84 s | 7.7x |
| 64 | 2.19 s | 27.25 s | 12.5x |
| 256 | 8.13 s | 103.11 s | 12.7x |
| 1024 | 33.45 s | 412.56 s | 12.3x |

**At the default learn ticks (1_500)**:

| seeds | parallel | serial | speed-up |
|---:|---:|---:|---:|
| 4 | 2.31 s | 9.26 s | 4.0x |
| 16 | 4.77 s | 37.68 s | 7.9x |
| 64 | 12.83 s | 150.44 s | 11.7x |
| 256 | 45.30 s | — | — |
| 1024 | 174.40 s | — | — |

Serial runs beyond 64 seeds at 1_500 ticks were not timed: the table above
says what they cost (very roughly 5x the 300-tick column).

End to end, the whole learning round (`lm learn --ticks 300`, this Thor,
parallel eval): **0.58 s** at the default 4 seeds (three runs: 0.58 / 0.58 /
0.60 s), 0.96 s at 16 seeds, 2.37 s at 64, 7.64 s at 256. The evaluation is
the bulk of the round; everything else (the round itself, the review, the
records) is ~0.15 s.

### The window, and what fits in it

The window assumed here: **the pause the host already tolerates between
rounds**, which the brief puts at 1.2–1.6 s for the 300-tick flow before this
change (that flow judged 4 seeds serially, ~1.7 s of evaluation alone; in
practice the whole round ran at roughly 1.2 s). With the evaluation now
parallel, the same budget buys:

| budget for the learning step | seeds at 300 ticks | seeds at 1_500 ticks |
|---:|---:|---:|
| 1.6 s (today's pause, kept) | ~35 | ~3 |
| 2.5 s | ~70 | 4 |
| 8 s | 256 | ~35 |
| 15 s | ~450 | ~70 |
| 50 s | — | 256 |

So: **within today's pause, the farm fits roughly 8 times as many seeds at
300 ticks; at the default 1_500 ticks, today's pause still fits only the
current 4.** 256 seeds fit in 8 s at 300 ticks, or 45 s at 1_500.

## Does it matter? The verdict experiment

Thirty proposals the flow would make — reflex edits, lexicon entries, gates,
policy tweaks, each with the claim a reviewer seals with it — run through the
real flow (`run_learning_round`, the stub reviewer speaking the proposal),
judged once on 4 held-out seeds and once on 256. The 4 seeds are the first 4
of the 256 (the bench asserts it), so the only thing that differs is how many
seeds the verdict was read from.

**First, where the experiment had to run.** At `--ticks 300` the flow's
claimable metrics are identically zero — `hold_ticks_squad0`, `tags_for_squad0`
and `margin_squad0` are 0 on every seed (squad 0 holds nothing before ~1_000
ticks; the probe in the bench documents this). All 30 proposals therefore
reject at both counts at 300 ticks: 0 flips, but only because there is nothing
to judge. The experiment below ran at the flow's default 1_500 ticks, where
the metric lives (baseline ≈ 135 hold ticks).

**The criterion.** The truth about a proposal is taken to be its 256-seed
verdict, read with two statistics over the per-seed deltas:

* a **sign test** — under the null "the change does nothing", each seed's
  delta is as likely up as down, and the p-value is how unlikely the observed
  count of deltas moving the claimed way is (exact binomial tail at 1/2,
  zeros excluded);
* a **bootstrap interval** — the seeds are resampled with replacement 2_000
  times (a seeded generator, so the interval is reproducible) and the
  2.5th/97.5th percentiles of the qualified fraction are reported.

A flip is a **false adopt** when the 4 seeds adopted and the 256 rejected (the
change helps on too few seeds to pass), and a **false reject** when the 4
seeds rejected a change the 256 seeds adopt with the sign test behind it.

### The verdicts at 1_500 ticks

| # | proposal | claim | 4 seeds | 256 seeds | qualified | 95% of it | sign p | flip |
|---:|---|---|---|---|---:|---|---:|---|
| 1 | camp after 30 ticks | up by 1 | adopt | adopt | 91% | [87, 94] | 0.0000 | - |
| 2 | camp after 60 ticks | up by 1 | adopt | adopt | 89% | [85, 93] | 0.0000 | - |
| 3 | camp after 90 ticks | up by 1 | adopt | adopt | 90% | [86, 94] | 0.0000 | - |
| 4 | camp after 120 ticks | up by 1 | adopt | adopt | 92% | [89, 95] | 0.0000 | - |
| 5 | camp after 150 ticks | up by 1 | adopt | adopt | 88% | [84, 92] | 0.0000 | - |
| 6 | camp after 180 ticks | up by 1 | adopt | adopt | 91% | [88, 95] | 0.0000 | - |
| 7 | camp every 2 ticks | up by 1 | reject | reject | 64% | [58, 70] | 0.0000 | - |
| 8 | camp every 3 ticks | up by 1 | adopt | adopt | 80% | [75, 85] | 0.0000 | - |
| 9 | camp every 4 ticks | up by 1 | reject | reject | 54% | [48, 60] | 0.0000 | - |
| 10 | camp every 5 ticks | up by 1 | adopt | reject | 59% | [53, 65] | 0.0000 | **false adopt** |
| 11 | camp every 6 ticks | up by 1 | adopt | reject | 56% | [50, 62] | 0.0000 | **false adopt** |
| 12 | camp every 8 ticks | up by 1 | reject | reject | 42% | [36, 48] | 0.0000 | - |
| 13 | stand still after 30 ticks | down by 1 | reject | reject | 19% | [14, 23] | 0.0000 | - |
| 14 | stand still after 60 ticks | down by 1 | reject | reject | 20% | [15, 25] | 0.0000 | - |
| 15 | stand still after 120 ticks | down by 1 | adopt | reject | 19% | [14, 24] | 0.0000 | **false adopt** |
| 16 | stand still after 240 ticks | down by 1 | reject | reject | 15% | [11, 20] | 0.0000 | - |
| 17 | agent 0 camps on the beacon | up by 1 | adopt | adopt | 89% | [85, 93] | 0.0000 | - |
| 18 | agent 1 camps on the beacon | up by 1 | adopt | reject | 59% | [54, 65] | 0.0000 | **false adopt** |
| 19 | agent 2 camps on the beacon | up by 1 | adopt | adopt | 96% | [94, 98] | 0.0000 | - |
| 20 | agent 3 camps on the beacon | up by 1 | adopt | adopt | 91% | [87, 94] | 0.0000 | - |
| 21 | camp, at least 5 more ticks | up by 5 | adopt | adopt | 92% | [88, 95] | 0.0000 | - |
| 22 | camp, at least 20 more ticks | up by 20 | adopt | adopt | 87% | [82, 91] | 0.0000 | - |
| 23 | camp, at least 35 more ticks | up by 35 | adopt | adopt | 82% | [78, 87] | 0.0000 | - |
| 24 | camp, at least 45 more ticks | up by 45 | adopt | adopt | 79% | [73, 84] | 0.0000 | - |
| 25 | camp, at least 55 more ticks | up by 55 | reject | reject | 57% | [51, 63] | 0.0000 | - |
| 26 | camp, at least 70 more ticks | up by 70 | reject | reject | 38% | [32, 45] | 0.0000 | - |
| 27 | camp, at least 100 more ticks | up by 100 | reject | reject | 0% | [0, 0] | 0.0000 | - |
| 28 | a lexicon entry for squad 0 | up by 1 | reject | reject | 0% | [0, 0] | 1.0000 | - |
| 29 | a gate for agent 0 | up by 1 | reject | reject | 0% | [0, 0] | 1.0000 | - |
| 30 | the policy of agent 0 | up by 1 | reject | reject | 0% | [0, 0] | 1.0000 | - |

(The metric is `hold_ticks_squad0` throughout; "up by N" is the sealed
direction and bar. The sign test's 0.0000 on rows 13–16 is exact: those
changes move the metric on only ~19% of seeds and **down on every seed they
move** — 48 of 48 nonzero deltas, p ≈ 2⁻⁴⁸ — which is precisely why row 15
fooled the 4 seeds: the change acts rarely, and the first four seeds were all
ones it acts on.)

**4 of 30 verdicts flip; all four are false adopts; there are no false
rejects.** The four (rows 10, 11, 15, 18) help on 56–59% of seeds — or act on
only 19% — and needed exactly 3 of their first 4 seeds to qualify, and got
them. The 4-seed rule adopts a change that truly helps only ~60% of seeds
46% of the time (see the table below); four rolls of that dice came up
adopt. Nothing that deserves adoption was denied it: every change the 256
seeds adopt (qualified 79–96%, every one with the sign test at p ≈ 0) was
also adopted by its 4 seeds.

## The rule, and what it should be

The current pass rule is a count: the delta meets the sealed bar on at least
⌈3N/4⌉ of N seeds. How often a change that truly helps a fraction q of seeds
passes that count:

| true fraction q | 4 seeds | 16 seeds | 64 seeds | 256 seeds |
|---:|---:|---:|---:|---:|
| 0.50 (luck) | 0.312 | 0.038 | 0.000 | 0.000 |
| 0.60 | 0.475 | 0.167 | 0.009 | 0.000 |
| 0.70 | 0.652 | 0.450 | 0.233 | 0.045 |
| 0.75 (the bar) | 0.738 | 0.630 | 0.567 | 0.534 |
| 0.80 | 0.819 | 0.798 | 0.875 | 0.979 |
| 0.90 | 0.948 | 0.983 | 1.000 | 1.000 |

Read as an error table: at 4 seeds a luck change (q = 0.5) is adopted 31% of
the time, and a just-below-the-bar change (q = 0.6) almost half the time —
the four false adopts above are exactly this row. At 16 seeds those risks
fall to 4% and 17%; at 64 to ~0. The price is on the other side of the bar: a
change that truly helps exactly 3 of 4 seeds is a coin flip at every N, and
one that helps 80% loses a little at 16 (80% vs 82% at 4) before recovering
(88% at 64, 98% at 256).

**The pass rule the farm proposes: keep the 3/4 count, and add the sign
test.** Adopt iff (a) the delta meets the sealed bar on ⌈3N/4⌉ seeds, **and**
(b) the sign test on the deltas' direction has p < 0.05. The count carries
"how often", the sign test carries "which way": it refuses a change that
meets the bar on many seeds while badly hurting others, at a cost of one
exact binomial tail. It binds only from N = 8 upward — at N = 4 the fairest
possible sample (4 of 4) gives p = 0.0625 — so the rule is well-defined at
today's default and gains a second gate exactly when the seed count makes
statistics possible. On this experiment's 256-seed data the rule changes no
verdict: everything adopted has fraction ≥ 79% and p ≈ 0; everything rejected
fails the count.

**Adopted on 2026-10-30.** The rule above is the rule the flow judges by
now (`crates/lm-learn/src/sign.rs` for the sign test, decided in exact
integer arithmetic; `crates/lm-learn/src/eval.rs` for the pass rule), and
the default count is 16. The bench (`examples/seedfarm.rs`) now reads the
library's sign test rather than its own, so the numbers it prints are the
numbers the verdicts are decided by.

## Recommendation

* **Raise the default seed count from 4 to 16** (`DEFAULT_EVAL_SEEDS`), and
  adopt the pass rule above when the count rises. At 16 seeds the false
  adopt of a q ≈ 0.6 change drops from ~47% to ~17%, and the rare-actor kind
  (row 15) from 2.4% to ~0, while a true q ≥ 0.8 change is adopted as often
  as before. Cost: 0.96 s end to end at 300 ticks (inside today's pause),
  4.9 s of evaluation at the default 1_500 ticks.
* **If the host can afford a ~13 s pause between rounds, prefer 64** (12.8 s
  of evaluation at 1_500 ticks): false adopts under 1%, and the sign test
  binds from 40 of 64 seeds. 256 seeds (45 s) buy certainty but almost
  nothing over 64 for proposals like these.
* **Keep judging at the default 1_500 ticks.** At 300 ticks every claimable
  metric is identically zero — the flow there rejects everything, and no
  seed count fixes that; the horizon does. A quick 300-tick round is a fine
  smoke test, but a verdict worth sealing needs the full horizon.
* The seed block layout already keeps rounds disjoint at any count ≤ 1000,
  and gives counts above that their own block, so raising the default needs
  no other change in the flow.


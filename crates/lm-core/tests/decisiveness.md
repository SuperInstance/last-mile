# Decisiveness sweep (D60 follow-up, recorded as D63)

```bash
cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture
```

Default reflex agents vs default reflex agents, 4 point-symmetric beacons (D61), seeds 1..=100 per cell.

| win_score | time_limit_ticks | squad 0 wins | squad 1 wins | draws | draw % | median end tick | draws before the limit |
|---|---|---|---|---|---|---|---|
| 900  | 9000  | 20 | 17 | 63 | 63% | 2984 | 63 |
| 900  | 12000 | 20 | 17 | 63 | 63% | 2984 | 63 |
| 1200 | 9000  | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1200 | 12000 | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1500 | 9000  | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1500 | 12000 | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1800 | 9000  | 21 | 16 | 63 | 63% | 4784 | 63 |
| 1800 | 12000 | 21 | 16 | 63 | 63% | 4784 | 63 |

## Reading

- **Every draw is a mirror tie, not a timeout.** All 63 draws per cell end before the time limit, at the same tick for both squads. The map is point-symmetric (D61), and two identical deterministic reflexes play the same game reflected, so both squads reach win_score on the same tick.
- **win_score and time_limit_ticks don't change decisiveness at all.** The draw rate is 63% in every cell; win_score only moves the end tick (about 600 ticks per +300 points).
- D60's "draws at the 9000-tick limit" is corrected by this. The tuning lever isn't the scoring parameters: something has to break the mirror. That could be agents that think (the model tiers, talk, learning), seeded per-squad variation, or a tie-break rule.

## Re-measured for the paint (M2a): unchanged, cell for cell

The paint (M2a) is new hashed world state: a tag writes a splat on the ground, and the paint dries on its own. It has no effect on the rules yet — `slow_q8` and `bounce_q8` are placeholders at 0 — so the sweep must come out exactly where it was.

It does. The table above was **re-measured on this machine** on both trees, not inherited: the base commit (`3392d7f`) and the paint tree, same sweep, same 100 seeds, same cells. Every number is identical — the win/loss/draw counts, the draw rate, the median end tick, the early draws, and the per-round counters (tags, sight ticks, score margin, mean scores, lowest squad score, collapsed rounds, rounds with a fight). The two runs' outputs diff clean.

So the paint is stateful and visual, and it is not yet a lever: the mirror still holds, exactly as the table says.

| win_score | time_limit_ticks | squad 0 wins | squad 1 wins | draws | draw % | median end tick | draws before the limit |
|---|---|---|---|---|---|---|---|
| 900  | 9000  | 20 | 17 | 63 | 63% | 2984 | 63 |
| 900  | 12000 | 20 | 17 | 63 | 63% | 2984 | 63 |
| 1200 | 9000  | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1200 | 12000 | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1500 | 9000  | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1500 | 12000 | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1800 | 9000  | 21 | 16 | 63 | 63% | 4784 | 63 |
| 1800 | 12000 | 21 | 16 | 63 | 63% | 4784 | 63 |


## The paint's rule, swept (M2b)

The paint has a rule now: an agent standing in the **other** team's paint is
slowed, and the paint pushes it back every N-th tick of contact. The rule is
in `crates/lm-core/src/rules/paint.rs`, in the rules layer, and the script
that mirrors the native rules (`assets/rules/beacons.rhai`) writes the same
rule in Rhai — the oracle `rhai_beacons_matches_native_with_the_paint_on`
plays 25 seeds with the knobs turned and the two have to agree tick for
tick.

The rule is two knobs, and the sweep plays them:

```bash
DECISIVENESS_CELL=1800x9000 \
DECISIVENESS_PAINT=0x0,64x0,128x0,192x0,0x64,64x64,128x64,192x64 \
DECISIVENESS_HARNESS=assets/harness/variants/advance \
cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture
```

`bounce_q8 = 64` is the "on" of the sweep: a push-back every **fourth** tick
of contact. `bounce_q8 = 0` is off, and with both knobs at zero the paint
plays no rule at all — the round is the round it was. The cells are the
round's defaults (win_score 1800, time_limit 9000) and 100 seeds each.

### The advance doctrine (and the shipped reflex, which is the same game here)

| slow_q8 | bounce_q8 | squad 0 wins | squad 1 wins | draws | draw % | median end tick | tags/round | collapsed rounds | rounds with a fight |
|---|---|---|---|---|---|---|---|---|---|
| 0   | 0  | 21 | 16 | 63 | 63.0% | 4784 | 0.62 | 2 | 35 |
| 64  | 0  | 21 | 16 | 63 | 63.0% | 4783 | 0.61 | 2 | 35 |
| 128 | 0  | 20 | 17 | 63 | 63.0% | 4783 | 0.64 | 2 | 35 |
| 192 | 0  | 21 | 16 | 63 | 63.0% | 4784 | 0.65 | 2 | 35 |
| 0   | 64 | 19 | 18 | 63 | 63.0% | 4781 | 0.74 | 3 | 35 |
| 64  | 64 | 19 | 18 | 63 | 63.0% | 4783 | 0.72 | 2 | 35 |
| 128 | 64 | 18 | 19 | 63 | 63.0% | 4783 | 0.78 | 2 | 35 |
| 192 | 64 | 19 | 18 | 63 | 63.0% | 4784 | 0.79 | 4 | 35 |

The shipped reflex (no `DECISIVENESS_HARNESS`) gives the same numbers, cell
for cell: the `advance` variant's reflex plays the same game as the shipped
one on these seeds.

### The shared-ghost doctrine

| slow_q8 | bounce_q8 | squad 0 wins | squad 1 wins | draws | draw % | median end tick | tags/round | collapsed rounds | rounds with a fight |
|---|---|---|---|---|---|---|---|---|---|
| 0   | 0  | 64 | 36 | 0 | 0.0% | 4578 | 5.45 | 45 | 97 |
| 64  | 0  | 67 | 33 | 0 | 0.0% | 4561 | 5.41 | 45 | 97 |
| 128 | 0  | 67 | 33 | 0 | 0.0% | 4565 | 5.58 | 43 | 97 |
| 192 | 0  | 67 | 33 | 0 | 0.0% | 4577 | 5.55 | 44 | 97 |
| 0   | 64 | 73 | 27 | 0 | 0.0% | 4393 | 5.62 | 51 | 97 |
| 64  | 64 | 67 | 33 | 0 | 0.0% | 4576 | 5.97 | 46 | 97 |
| 128 | 64 | 65 | 35 | 0 | 0.0% | 4525 | 5.79 | 47 | 97 |
| 192 | 64 | 66 | 34 | 0 | 0.0% | 4538 | 5.57 | 50 | 97 |

### The strongest push-back, for the record

`bounce_q8 = 255` is a push-back on **every** tick of contact — the paint as
a wall. It was measured on its own, in the same 100 seeds:

| harness | slow_q8 | bounce_q8 | squad 0 wins | squad 1 wins | draws | draw % | median end tick | tags/round | collapsed rounds |
|---|---|---|---|---|---|---|---|---|---|
| advance | 0 | 255 | 19 | 18 | 63 | 63.0% | 4781 | 0.78 | 4 |
| shared-ghost | 0 | 255 | 62 | 38 | 0 | 0.0% | 4464 | 5.62 | 48 |

It says the same thing louder: the draws do not move and the collapses go up
(2 → 4 and 45 → 48).

### Reading

- **The draw rate does not move.** 63% in every cell of the advance
  doctrine, 0% in every cell of the shared-ghost doctrine — the same numbers
  as with the paint rules off. The mirror still holds: the paint is not the
  lever that breaks it.
- **The fights change.** The tags per round rise from 0.62 to 0.79 with the
  push-back (advance) and from 5.45 to 5.97 (shared-ghost): the paint
  shoves agents around and the rounds have more contact. The win split
  moves with it (21–16 becomes 19–18).
- **The collapses go up.** With the push-back on, the collapsed rounds rise
  from 2 to 3–4 (advance) and from 45 to 46–51 (shared-ghost), and the
  lowest squad score of the sweep (5) is in the `0 × 64` cell. A paint that
  pushes an agent back can hold a squad off its own beacon.
- **Why so little of it.** The paint is rare: a tag is rare. The advance
  doctrine sees 0.62 tags a round and the shared-ghost doctrine 5.45, and a
  splat lasts 150 ticks and covers nine cells. There is very little of it on
  the ground for a rule to act on.

### The default: 0/0

No cell lowers the draws, and several raise the collapsed rounds, so the
defaults stay at `slow_q8 = 0, bounce_q8 = 0` — the paint is a mark on the
ground, not a rule. The brief's rule ("pick defaults that lower draws
without raising collapsed rounds; if no cell does, ship 0/0 and say so")
says so.

The golden does not move, and this is measured, not assumed: `lm golden`
prints `OK 3/3` with the paint rules off. The round is bit-for-bit the round
it was, because the paint's state is hashed only when the paint's rule is
used — the same "hashed only when it is in use" rule the airtime counter
follows.

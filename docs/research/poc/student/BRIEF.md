# E2 — Trainer, student, and "the right questions" (THROWAWAY PoC)

Open question: a fast student model distilled from a slow teacher, can it match the teacher, and how much does the TEACHER's choice of questions/features ("gates", "building blocks") matter, versus raw input? When should a model be replaced by plain code?
Write `sim.py` HERE ONLY: Python 3 stdlib, deterministic seeds, no network. Run it and print tables plus a 6-line reading.

## Task
- 16x16 tactical grid: walls (25% random), one agent, 0–3 enemies, one beacon.
- Correct action, from a TEACHER = exact oracle:
  - HIDE if any enemy has line of sight (Bresenham over walls) within 6 cells;
  - else ADVANCE if a BFS path to the beacon is <= 10;
  - else SCOUT.
- The teacher's labels have 5% label noise (it's a slow, imperfect judge).

## Students (all trained only on teacher labels, multinomial logistic regression in pure Python, SGD, fixed seeds)
- S-raw: input = the 256 cells one-hot-ish (wall/enemy/beacon/empty) relative to the agent (egocentric 9x9 crop is OK).
- S-gates: input = answers to authored gates (features the "higher model" wrote as building blocks):
  - enemy_visible_within_6 (bool), n_enemies_visible, nearest_enemy_dist_bucket, path_to_beacon_len_bucket, cover_adjacent (bool).
- S-gates-minus: S-gates without the LOS gate (the teacher forgot the key question).
- CODE: no model, a hand rule built from the gates (the same rule as the teacher, without noise).

## Measure
- held-out accuracy vs the noiseless truth at training sizes 50, 200, 1000, 5000 (separate eval seeds, never trained on)
- inference cost proxy: multiply-adds per decision
- the "dial": for S-gates at 5000, the fraction of eval cases where its argmax equals CODE's output. If >= 99%, report "model can be replaced by code here".
- an OUTSIDE-SPACE test: change the truth to depend on something no gate captures (e.g. HIDE also when an enemy is exactly 2 cells away diagonally). Show how S-gates plateaus and S-raw does or doesn't, i.e. the boundary of the declared space.

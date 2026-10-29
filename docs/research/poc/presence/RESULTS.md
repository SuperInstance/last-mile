Done. `sim.py` is a single 394-line stdlib-only, deterministic simulator; it ran clean (exit 0, ~30 s) and the full 42-config × 3-seed sweep with the determinism assert passing.

**Results table** (top of the 42 rows, full table + Pareto front in `out.md`):

```
config                  acc%  lag_mn lag_p95 flip/a-min call/a-min  skip%  srv/min  front
COND hz30 a0.25 d0.05  95.27     5.6       8       2.85     1645.6    8.6  1645.6   *
COND hz30 a0.25 d0.1   94.99     6.1       9       1.18     1646.4    8.5  1646.4   *
COND hz15 a0.25 d0.05  93.37     8.0      12       2.01      899.6    0.0   899.6   *
COND hz10 a0.5 d0.2    92.05     8.1      14       5.44      600.0    0.0   600.0   *
COND hz5 a0.5 d0.2     88.17    13.3      25       3.02      300.0    0.0   300.0   *
RAW hz30               84.08     2.9       4     221.26     1645.4    8.6  1645.4
RAW hz5                82.61     5.9      13      40.05      300.0    0.0   300.0
POINT argmax           83.20     4.6       8      33.84      254.8    0.0  1560.6   *
POINT roll             56.01     6.0      17      92.99      254.8    0.0  1560.6
```

**5-line reading**

1. **Conditioning beats call rate** — at ~300 calls/agent/min, presence + lane + deadband reaches 88.2% acc and 3.0 flips/agent-min, while point mode (255 calls/agent/min) gets 83.2% and 33.8 flips; the lane refuses the model's bad answers, raw argmax cannot.
2. **Presence is also cheaper on the model** — point mode needs 1561 calls/min (26 batches/s, only 2.75 agents per batch) for 83%, presence@5 Hz needs 300 calls/min (5/s) for 88%: per-agent requests simply don't batch.
3. **Raw presence is the worst of the family** — with no memory every confidently-wrong answer flips the action (221 flips/agent-min at 30 Hz vs 2.9 conditioned) and accuracy caps near 84%.
4. **Buying accuracy with rate has a tail** — 30 Hz + α=1/4 tops the table at 95.3% but skips 8.6% of periods (service > 33 ms, not saturation; the server caps at ~45 batches/s) at 1646 calls/agent-min, while α=1/8 collapses to 67% at 5 Hz because the lane can't track a 4 s truth change.
5. **The 24-of-42 Pareto front is a call-budget ladder** (5 Hz/300 → 88%, 15 Hz/900 → 93%, 30 Hz/1646 → 95%); point mode stays on the front only because it is cheapest per agent (255), and "roll" is a dead end (56% acc, 93 flips).

**Interpretation calls I made** (brief was ambiguous; each is documented in the module docstring): the 20 ms micro-batch window closes 20 ms after the first request and dispatches then; "the rest is split randomly" means random Dirichlet-style weights over the three non-holder options; one transport sample per call, applied to the response path; and argmax ties break uniformly at random (seeded) so a uniform-noise answer carries no information instead of always naming option 0 — a deterministic first-index tie-break injects a spurious option-0 prior.

Files: [sim.py](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/presence/sim.py) · [out.md](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/presence/out.md) (full table, Pareto front, reading).
 d0.1  83.93    20.9      32       0.43      600.0    0.0   600.0    16.7   *
RAW hz15               83.80     3.5       6     120.99      899.6    0.0   899.6     3.1   
RAW hz10               83.48     4.1       8      80.45      600.0    0.0   600.0     3.6   
POINT argmax           83.20     4.6       8      33.84      254.8    0.0  1560.6     4.1   *
RAW hz5                82.61     5.9      13      40.05      300.0    0.0   300.0     4.9   
COND hz5 a0.25 d0.2    80.40    25.7      44       0.40      300.0    0.0   300.0    19.7   *
COND hz10 a0.125 d0.2  79.36    27.4      42       0.36      600.0    0.0   600.0    20.6   *
COND hz5 a0.125 d0.05  77.29    30.2      50       0.49      300.0    0.0   300.0    22.9   
COND hz5 a0.125 d0.1   74.11    35.0      59       0.44      300.0    0.0   300.0    26.0   
COND hz5 a0.125 d0.2   67.19    45.7      79       0.31      300.0    0.0   300.0    32.4   *
POINT roll             56.01     6.0      17      92.99      254.8    0.0  1560.6     4.8   

Pareto front over (accuracy up, spurious flips down, calls down):
  COND hz30 a0.25 d0.05 acc 95.27%  flips   2.85/a-min  calls  1645.6/a-min  srv 1645.6/min
  COND hz30 a0.25 d0.1  acc 94.99%  flips   1.18/a-min  calls  1646.4/a-min  srv 1646.4/min
  COND hz30 a0.25 d0.2  acc 94.04%  flips   0.49/a-min  calls  1640.5/a-min  srv 1640.5/min
  COND hz15 a0.25 d0.05 acc 93.37%  flips   2.01/a-min  calls   899.6/a-min  srv 899.6/min
  COND hz30 a0.125 d0.05 acc 92.91%  flips   0.48/a-min  calls  1648.9/a-min  srv 1648.9/min
  COND hz15 a0.25 d0.1  acc 92.80%  flips   0.91/a-min  calls   899.5/a-min  srv 899.5/min
  COND hz30 a0.125 d0.1 acc 92.09%  flips   0.48/a-min  calls  1647.5/a-min  srv 1647.5/min
  COND hz10 a0.5 d0.2   acc 92.05%  flips   5.44/a-min  calls   600.0/a-min  srv 600.0/min
  COND hz10 a0.25 d0.05 acc 91.38%  flips   1.80/a-min  calls   600.0/a-min  srv 600.0/min
  COND hz15 a0.25 d0.2  acc 91.15%  flips   0.52/a-min  calls   899.4/a-min  srv 899.4/min
  COND hz10 a0.25 d0.1  acc 90.52%  flips   0.83/a-min  calls   600.0/a-min  srv 600.0/min
  COND hz30 a0.125 d0.2 acc 90.04%  flips   0.47/a-min  calls  1645.0/a-min  srv 1645.0/min
  COND hz15 a0.125 d0.05 acc 89.30%  flips   0.49/a-min  calls   899.5/a-min  srv 899.5/min
  COND hz5 a0.5 d0.2    acc 88.17%  flips   3.02/a-min  calls   300.0/a-min  srv 300.0/min
  COND hz10 a0.25 d0.2  acc 88.13%  flips   0.46/a-min  calls   600.0/a-min  srv 600.0/min
  COND hz15 a0.125 d0.1 acc 87.93%  flips   0.44/a-min  calls   899.7/a-min  srv 899.7/min
  COND hz5 a0.25 d0.05  acc 86.01%  flips   1.40/a-min  calls   300.0/a-min  srv 300.0/min
  COND hz15 a0.125 d0.2 acc 84.68%  flips   0.38/a-min  calls   899.6/a-min  srv 899.6/min
  COND hz5 a0.25 d0.1   acc 84.46%  flips   0.72/a-min  calls   300.0/a-min  srv 300.0/min
  COND hz10 a0.125 d0.1 acc 83.93%  flips   0.43/a-min  calls   600.0/a-min  srv 600.0/min
  POINT argmax          acc 83.20%  flips  33.84/a-min  calls   254.8/a-min  srv 1560.6/min
  COND hz5 a0.25 d0.2   acc 80.40%  flips   0.40/a-min  calls   300.0/a-min  srv 300.0/min
  COND hz10 a0.125 d0.2 acc 79.36%  flips   0.36/a-min  calls   600.0/a-min  srv 600.0/min
  COND hz5 a0.125 d0.2  acc 67.19%  flips   0.31/a-min  calls   300.0/a-min  srv 300.0/min

Reading:
  Conditioning beats call rate: at ~300 calls/agent/min, presence + lane + deadband reaches 88.2% acc and 3.0 flips/agent-min, while point mode (255 calls/agent/min) gets 83.2% and 33.8 flips - the lane refuses the model's bad answers, raw argmax cannot.
  Presence is also cheaper on the model: point mode needs 1561 calls/min (26 batches/s, only 2.75 agents per batch) to deliver 83%, presence@5Hz needs 300 calls/min (5/s) for 88% - per-agent requests simply do not batch.
  Raw presence is the worst of the family: with no memory every confidently-wrong answer flips the action, 221 flips/agent-min at 30Hz vs 2.9 conditioned, and accuracy is capped near 84%.
  Buying accuracy with rate has a tail: 30Hz + alpha=1/4 tops the table at 95.3% but skips 8.6% of periods (service > 33ms, not saturation - the server caps at ~45 batches/s) at 1646 calls/agent-min, while alpha=1/8 collapses to 67% at 5Hz because the lane cannot track a 4s truth change.
  So the 24-of-42 Pareto front is a call-budget ladder (5Hz/300 -> 88%, 15Hz/900 -> 93%, 30Hz/1646 -> 95%); point mode stays on the front only because it is cheapest per agent (255), and 'roll' is a dead end (56% acc, 93 flips).
EXIT 0

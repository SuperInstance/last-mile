Done. `sim.py` is written, run, and validated. Everything lives in this directory; nothing outside was touched (two scratch files I'd written in `/tmp` for a determinism check were removed).

**Files**
- [sim.py](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/lead/sim.py) — the simulator (stdlib only, no network, seeded)
- [out.md](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/lead/out.md) — the captured run output (this file was pre-existing and empty)

**Results — all decisions** (seed 20241028; 38400 decisions, 13 storms = 32.1 s = 5.4% of the run)

| policy | mean lead ms | on-time % | late % | drop % | ex.lat mean | ex.lat p99 | on-time%/lead ms |
|---|---|---|---|---|---|---|---|
| FIXED L=2 | 66.7 | 95.70 | 4.30 | 0.00 | 28.05 | 69.46 | 1.4355 |
| FIXED L=3 | 100.0 | 98.28 | 1.72 | 0.00 | 14.61 | 38.18 | 0.9828 |
| FIXED L=4 | 133.3 | 99.90 | 0.10 | 0.00 | 2.38 | 5.80 | 0.7492 |
| FIXED L=6 | 200.0 | 100.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.5000 |
| FIXED L=9 | 300.0 | 100.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.3333 |
| ADAPT N=32 m=0 | 101.8 | 99.71 | 0.29 | 0.00 | 11.77 | 36.51 | 0.9796 |
| ADAPT N=32 m=10 | 101.9 | 99.77 | 0.23 | 0.00 | 10.23 | 36.74 | 0.9792 |
| ADAPT N=128 m=0 | 102.6 | 99.67 | 0.33 | 0.00 | 12.85 | 37.72 | 0.9715 |
| ADAPT N=128 m=10 | 102.7 | 99.72 | 0.28 | 0.00 | 12.01 | 38.05 | 0.9707 |

**Results — storm periods only** (2060 decisions)

| policy | mean lead ms | on-time % | late % | ex.lat mean | ex.lat p99 |
|---|---|---|---|---|---|
| FIXED L=2 | 66.7 | 28.74 | 71.26 | 30.91 | 69.58 |
| FIXED L=3 | 100.0 | 67.91 | 32.09 | 14.61 | 38.18 |
| FIXED L=4 | 133.3 | 98.06 | 1.94 | 2.38 | 5.80 |
| FIXED L=6 | 200.0 | 100.00 | 0.00 | 0.00 | 0.00 |
| ADAPT N=32 m=10 | 130.0 | 95.73 | 4.27 | 10.23 | 36.74 |
| ADAPT N=128 m=10 | 129.1 | 94.76 | 5.24 | 12.01 | 38.05 |

**5-line reading**

1. Best on_time per ms of lead (the brief's literal ratio) is **FIXED L=2**: 95.70% on time for 66.7 ms = 1.4355 %-pts/ms, ahead of FIXED L=3 (0.9828).
2. That ratio is bought with tail damage, not saved — L=2 leaves 4.30% of decisions late carrying 28.0 ms mean / 69.5 ms p99 extra lateness, and only 28.7% on time inside storms; the player feels exactly the latency the ratio hides.
3. On the real frontier the adaptive policy wins: **ADAPT N=32 m=10** is 99.77% on time for 101.9 ms, dominating FIXED L=3 (98.28% for 100.0 ms) and landing −0.12 pts of FIXED L=4 (99.90% for 133.3 ms) for 31.4 ms less player latency.
4. Why it wins: base p99 42 ms maps to 2 ticks and is clamped up to 3, so lead is 3 ticks in calm traffic and escalates to 4 only while a storm is live (5.7% of decisions); the 9-tick clamp is unreachable because p99_est never exceeds the 110 ms storm ceiling (ceil((110+10)/33.3) = 4), and the one-RTT echo lag costs it 4.3% of storm decisions.
5. Pick ADAPT N=32 m=10 (N=32 reacts faster than N=128; margin 10 ms buys +0.07 pts); caveats — per-agent windows are worse (99.39% at 105.1 ms), and STALE never fires (worst lateness ~2.2 ticks vs a 15-tick limit), so the drop path is untested here and drop% is 0 everywhere.

**Verification performed**
- An independent re-implementation of the brief's stamping rules (not importing `sim.py`) reproduces every fixed-lead on-time/late/drop figure exactly (95.70 / 98.28 / 99.90 / 100.00).
- Generated latency quantiles match the measured targets (p50 2.70, p90 12.00, p99 42.47, max 49.99 ms).
- Byte-identical output across repeated runs; a second seed (7) preserves the same policy ordering.

**Modelling choices I had to make** (all documented in `sim.py`'s docstring): the adaptive window is a single host-level estimator shared by the 16 agents on the remote host, so N=32 ≈ 0.5 s and N=128 ≈ 2 s of history — `--scope agent` runs the per-agent reading; p99_est uses linear-interpolation percentiles; cold start uses the 3-tick clamp minimum; "extra lateness" is logged lateness (arrival − requested tick time).
IXED L=9         9t: 100.0%          
ADAPT N=32 m=0    3t: 94.7%   4t: 5.3%
ADAPT N=32 m=10   3t: 94.3%   4t: 5.7%
ADAPT N=128 m=0   3t: 92.2%   4t: 7.8%
ADAPT N=128 m=10  3t: 91.8%   4t: 8.2%

Pareto frontier (on-time up, mean lead down): FIXED L=2, FIXED L=3, ADAPT N=32 m=0, ADAPT N=32 m=10, FIXED L=4, FIXED L=6
Sensitivity: per-agent windows instead of one host-level estimator: best ADAPT N=32 m=10 at 99.39% on time for 105.1 ms - host scope is better

============================================================================================================
READING
============================================================================================================
1. Best on_time per ms of lead (the brief's literal ratio): FIXED L=2, 95.70% on time for 66.7 ms = 1.4355 %-pts/ms, ahead of FIXED L=3 (0.9828).
2. That ratio is bought with tail damage, not saved: FIXED L=2 leaves 4.30% of decisions late carrying 28.0 ms mean / 69.5 ms p99 extra lateness, and only 28.7% on time inside storms - the player feels exactly the latency that the ratio hides.
3. On the real frontier the adaptive policy wins: ADAPT N=32 m=10 is 99.77% on time for 101.9 ms, dominating FIXED L=3 (98.28% for 100.0 ms) and landing -0.12 pts of FIXED L=4 (99.90% for 133.3 ms) for 31.4 ms less player latency.
4. Why it wins: base p99 of 42 ms maps to 2 ticks, clamped up to 3, so the lead is 3 ticks for calm traffic and only escalates to 4 while a storm is live (5.7% of decisions); the 9-tick clamp is unreachable because p99_est never exceeds the 110 ms storm ceiling (ceil((110+10)/33.3) = 4), and the one-RTT echo lag costs it 4.3% of storm decisions.
5. Pick ADAPT N=32 m=10 (N=32 reacts faster than N=128; margin 10 ms buys +0.07 pts). Caveats: per-agent windows instead of one host-level estimator: best ADAPT N=32 m=10 at 99.39% on time for 105.1 ms - host scope is better; and STALE never fires - the worst lateness seen is ~2.2 ticks against a 15-tick limit - so the drop path is untested by this scenario and drop% is 0 everywhere.
EXIT 0

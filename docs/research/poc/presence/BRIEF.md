# PoC A — point mode vs presence mode for a fast decision model (THROWAWAY)

Write `sim.py` in this directory only: Python 3 stdlib only, deterministic (random.Random(seed)), no network.
Run it, then print a results table plus your 5-line reading of it. Do not modify anything outside this directory.

## Model of the world
- 16 agents, sim at 30 ticks/s, 600 s per run.
- Each agent has a hidden TRUE best option among 4. It changes at random times (exponential gaps, mean 4 s).
- The game sees the change instantly, but an agent only acts through decisions.

## Stub decision model (stands in for Liquid d1 / Cloudflare Clef-flash)
- One call = one batch of agents. It returns, per agent, p[4] (sums to 1).
- 85% of calls are informative: the true option gets mass q~U(0.45,0.85), and the rest is split randomly.
- 10% are confidently wrong: a random wrong option gets mass U(0.5,0.8).
- 5% are uniform noise.
- Batch latency (ms): lognormal, median 21, sigma 0.35 (~16 ms vendor x 1.3 for multiple questions).
- Transport one-way latency (ms), sampled by piecewise-linear interpolation of these measured quantiles:
  p0 1.5, p50 2.7, p90 12, p95 21, p99 42, p100 50.
- The decision applies at tick ceil(now + batch + transport) or now + LEAD_TICKS (3), whichever is later.
- The model server handles one batch at a time (FIFO), so queueing is real.

## Modes to compare
1. POINT: each agent requests a think at 4 Hz, plus an extra think 2 ticks after its truth changes (event detection). Batch requests arriving within 20 ms. Action = argmax of the last returned p (also try "roll": sample from p with the seeded RNG).
2. PRESENCE-RAW: all 16 agents in one batch at presence_hz in {5,10,15,30}; action = argmax of each fresh p.
3. PRESENCE-CONDITIONED: same rates. Keep a lane per option per agent in FIXED POINT (ints, Q16): lane += (p_q16 - lane) * ALPHA (ALPHA in {1/2, 1/4, 1/8}, as shifts).
   - Switch the action from current c to option o only if lane[o] - lane[c] > DEADBAND (DEADBAND in {0.05, 0.10, 0.20} of 1.0 in Q16): hysteresis.
   - If the model can't keep up (the batch is still running when the next period is due), skip that period. This is "auto" falling back.

## Metrics per configuration (averaged over agents, 3 seeds)
- accuracy: % of ticks where action == truth
- lag_mean / lag_p95: ticks from a truth change until the action first equals the new truth (count it as unresolved if the truth changes again first)
- spurious_flips_per_agent_min: action changes NOT followed within 1 tick by matching truth (jitter)
- calls_per_agent_min: compute cost
- skipped_periods %
- A determinism check: run one config twice with the same seed, and assert the logs are identical (hash them).

Print one table sorted by accuracy, then mark the Pareto front over (accuracy up, spurious flips down, calls down).

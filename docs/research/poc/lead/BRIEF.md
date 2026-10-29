# PoC B — fixed vs adaptive scheduling lead over a Wi-Fi link with tails (THROWAWAY)

Write `sim.py` in this directory only: Python 3 stdlib, deterministic seeds, no network. Run it, then print a results table and a 5-line reading.
Do not modify anything outside this directory.

## Setup
- The sequencer ticks at 30 Hz for 600 s. 16 agents on a remote host each send a decision at 4 Hz (with random phase).
- A decision is created at time t_send and requests apply_tick = current_tick + LEAD.
- One-way latency (ms) is sampled by piecewise-linear interpolation of measured quantiles: p0 1.5, p50 2.7, p90 12, p95 21, p99 42, p100 50.
- STORMS: with rate 1 per 60 s, a storm lasts U(1,4) s. During a storm, latency = U(30,110) ms (correlated tail; the measured box has an 80–112 ms p99 RTT on Thor paths).
- Sequencer stamping:
  - arrival <= requested tick time → on time;
  - otherwise late → applied the next tick (lateness logged);
  - if lateness > STALE (15 ticks) → dropped.

## Policies
- FIXED lead in {2,3,4,6,9} ticks.
- ADAPTIVE: the sender keeps a rolling window of the last N one-way samples (N in {32,128}; assume it learns each decision's latency from an echo, delayed by one RTT). lead = clamp(ceil((p99_est + margin)/33.3 ms), 3, 9), with margin in {0, 10 ms}.

## Metrics
- on_time %, late %, dropped %, mean lead used (ms): the latency cost the player feels for agents
- mean and p99 extra lateness among late decisions
- the same metrics restricted to storm periods

Print a table, and say which policy gives the best on_time per ms of lead.

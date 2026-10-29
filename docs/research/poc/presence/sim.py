#!/usr/bin/env python3
"""PoC A - point mode vs presence mode for a fast decision model (THROWAWAY).

One file, Python 3 stdlib only, deterministic (random.Random seeds), no network.

Modes
  POINT  every agent thinks at 4 Hz, plus an extra think 2 ticks after its truth changes
         (event detection).  Action = argmax of the last returned p (or "roll": sample
         from p with the seeded RNG).
  RAW    one presence batch per period (all 16 agents).  Action = argmax(fresh p).
  COND   same cadence, but a per-agent per-option lane in FIXED POINT (Q16) smooths p and
         a deadband gates the action switch.  A period is skipped when the model server is
         still busy when the next period is due ("auto" fallback).
Model stub: 85% informative (true option gets q~U(0.45,0.85), rest split at random), 10%
confidently wrong (a wrong option gets U(0.5,0.80)), 5% uniform noise.  The server handles
one batch at a time (FIFO); service = lognormal(median 21ms, sigma 0.35); one-way transport
latency is interpolated from the measured quantiles.  A decision lands at tick
max(now+LEAD_TICKS, ceil((now+queue+service+transport)*30)).  Requests arriving inside a 20
ms micro-batch window join one call.  Argmax ties break uniformly at random (seeded), so a
uniform-noise answer carries no information instead of always naming option 0.
"""
import hashlib
import heapq
import math
import random
from bisect import bisect_left, bisect_right

# ------------------------------------------------------------------ constants
N_AGENTS, N_OPTIONS = 16, 4
TICK_HZ, RUN_SECONDS = 30, 600
N_TICKS = TICK_HZ * RUN_SECONDS
LEAD_TICKS, BATCH_WINDOW_S = 3, 0.020
POINT_HZ, TRUTH_GAP_S = 4.0, 4.0
SEEDS = (11, 22, 33)
Q16 = 1 << 16
BATCH_MEDIAN_MS, BATCH_SIGMA = 21.0, 0.35
TQ = ((0.0, 1.5), (50.0, 2.7), (90.0, 12.0), (95.0, 21.0), (99.0, 42.0), (100.0, 50.0))
READING = [
    "Conditioning beats call rate: at ~300 calls/agent/min, presence + lane + deadband reaches 88.2% acc and 3.0 flips/agent-min, while point mode (255 calls/agent/min) gets 83.2% and 33.8 flips - the lane refuses the model's bad answers, raw argmax cannot.",
    "Presence is also cheaper on the model: point mode needs 1561 calls/min (26 batches/s, only 2.75 agents per batch) to deliver 83%, presence@5Hz needs 300 calls/min (5/s) for 88% - per-agent requests simply do not batch.",
    "Raw presence is the worst of the family: with no memory every confidently-wrong answer flips the action, 221 flips/agent-min at 30Hz vs 2.9 conditioned, and accuracy is capped near 84%.",
    "Buying accuracy with rate has a tail: 30Hz + alpha=1/4 tops the table at 95.3% but skips 8.6% of periods (service > 33ms, not saturation - the server caps at ~45 batches/s) at 1646 calls/agent-min, while alpha=1/8 collapses to 67% at 5Hz because the lane cannot track a 4s truth change.",
    "So the 24-of-42 Pareto front is a call-budget ladder (5Hz/300 -> 88%, 15Hz/900 -> 93%, 30Hz/1646 -> 95%); point mode stays on the front only because it is cheapest per agent (255), and 'roll' is a dead end (56% acc, 93 flips).",
]

# ------------------------------------------------------------------- helpers
def transport_ms(u):
    """u in [0,1) -> one-way transport latency (ms), piecewise linear in percentile."""
    p = u * 100.0
    for (p0, v0), (p1, v1) in zip(TQ, TQ[1:]):
        if p <= p1:
            return v0 + (v1 - v0) * (p - p0) / (p1 - p0)
    return TQ[-1][1]

def _split(rng, p, mass, holder):
    """Split `mass` at random over the options other than `holder`."""
    others = [i for i in range(N_OPTIONS) if i != holder]
    w = [rng.random() for _ in others]
    s = sum(w)
    for i, wi in zip(others, w):
        p[i] = mass * wi / s

def sample_p(rng, truth_val):
    """One model answer p[4] (sums to 1) for an agent whose true option is truth_val."""
    p = [0.0] * N_OPTIONS
    r = rng.random()
    if r < 0.85:                                     # informative
        q = 0.45 + rng.random() * 0.40
        p[truth_val] = q
        _split(rng, p, 1.0 - q, truth_val)
    elif r < 0.95:                                   # confidently wrong
        o = (truth_val + 1 + rng.randrange(N_OPTIONS - 1)) % N_OPTIONS
        m = 0.50 + rng.random() * 0.30
        p[o] = m
        _split(rng, p, 1.0 - m, o)
    else:                                            # uniform noise
        p = [0.25] * N_OPTIONS
    return p

def to_q16(p):
    """float p[4] -> int p[4] in Q16, summing to exactly 65536."""
    q = [int(round(x * Q16)) for x in p]
    j = max(range(N_OPTIONS), key=lambda i: q[i])
    q[j] += Q16 - sum(q)
    return q

def argmax(p, rng=None):
    """Index of the largest p; ties break uniformly at random (seeded)."""
    b = 0
    for i in range(1, N_OPTIONS):
        if p[i] > p[b]:
            b = i
    if rng is None:
        return b
    n = 1
    for i in range(N_OPTIONS):
        if i != b and p[i] == p[b]:
            n += 1
    if n == 1:
        return b
    k = rng.randrange(n)
    for i in range(N_OPTIONS):
        if i != b and p[i] == p[b]:
            if k == 0:
                return i
            k -= 1
    return b

def sample_from(p, rng):
    """Sample an option from distribution p."""
    u, acc = rng.random(), 0.0
    for i, x in enumerate(p):
        acc += x
        if u <= acc:
            return i
    return N_OPTIONS - 1

def percentile(vals, p):
    if not vals:
        return float("nan")
    s = sorted(vals)
    k = (len(s) - 1) * p
    lo, hi = math.floor(k), math.ceil(k)
    return s[int(k)] if lo == hi else s[lo] + (s[hi] - s[lo]) * (k - lo)

class Truth:
    """Hidden truth trajectory.  Identical for every config at a given seed."""
    def __init__(self, seed):
        rng = random.Random(seed)
        self.init, self.events, self.eticks = [], [], []
        for _ in range(N_AGENTS):
            cur = rng.randrange(N_OPTIONS)
            self.init.append(cur)
            evs, t = [], rng.expovariate(1.0 / TRUTH_GAP_S)
            while t < RUN_SECONDS:
                tick = math.ceil(t * TICK_HZ)
                if tick >= N_TICKS:
                    break
                cur = (cur + 1 + rng.randrange(N_OPTIONS - 1)) % N_OPTIONS
                evs.append((tick, cur))
                t += rng.expovariate(1.0 / TRUTH_GAP_S)
            self.events.append(evs)
            self.eticks.append([e[0] for e in evs])

    def at_time(self, a, t):
        """True option of agent `a` at time t (seconds)."""
        tick = int(t * TICK_HZ)
        i = bisect_right(self.eticks[a], tick) - 1
        return self.init[a] if i < 0 else self.events[a][i][1]

# ---------------------------------------------------------------- simulation
def run_config(cfg, seed, truth):
    """One 600 s run.  Returns action timelines and counters."""
    rng = random.Random(seed * 1000003 + cfg["id"] * 7919)
    roll_rng = random.Random(seed * 31 + cfg["id"] * 131 + 17)
    phase_rng = random.Random(seed * 977 + 5)
    mode = cfg["mode"]
    shift = {0.5: 1, 0.25: 2, 0.125: 3}[cfg.get("alpha", 0.5)]
    dead = int(round(cfg.get("db", 0.0) * Q16))
    heap, seq = [], 0

    def push(t, kind, payload):
        nonlocal seq
        seq += 1
        heapq.heappush(heap, (t, seq, kind, payload))

    # ---- schedule the work ------------------------------------------------
    if mode == "point":
        for a in range(N_AGENTS):
            ph = phase_rng.uniform(0.0, 1.0 / POINT_HZ)
            k = 0
            while ph + k / POINT_HZ < RUN_SECONDS:
                push(ph + k / POINT_HZ, "req", a)
                k += 1
        for a in range(N_AGENTS):
            for tick, _v in truth.events[a]:          # event detection think
                push((tick + 2) / TICK_HZ, "req", a)
    else:
        hz = cfg["hz"]
        k = 0
        while k / hz < RUN_SECONDS:
            push(k / hz, "period", None)
            k += 1

    # ---- state ------------------------------------------------------------
    server_free, open_batch, dispatch_id = 0.0, None, 0
    action = [None] * N_AGENTS
    lanes = [[0] * N_OPTIONS for _ in range(N_AGENTS)]
    acts = [[] for _ in range(N_AGENTS)]
    last_tick = [-1] * N_AGENTS
    applied_id = [-1] * N_AGENTS
    calls = [0] * N_AGENTS
    batches = skipped = periods = 0

    def dispatch(t, agents):
        """Send one batch to the (single-slot, FIFO) model server."""
        nonlocal server_free, dispatch_id, batches
        service = BATCH_MEDIAN_MS * math.exp(BATCH_SIGMA * rng.gauss(0.0, 1.0)) / 1000.0
        finish = max(t, server_free) + service
        server_free = finish
        ready = finish + transport_ms(rng.random()) / 1000.0
        apply_tick = max(int(t * TICK_HZ) + LEAD_TICKS, math.ceil(ready * TICK_HZ))
        dispatch_id += 1
        batches += 1
        ps = [sample_p(rng, truth.at_time(a, t)) for a in agents]
        for a in agents:
            calls[a] += 1
        push(apply_tick / TICK_HZ, "apply", (apply_tick, agents, ps, dispatch_id))

    # ---- event loop -------------------------------------------------------
    while heap:
        t, _s, kind, payload = heapq.heappop(heap)
        if kind == "req":
            if open_batch is None:
                open_batch = (t + BATCH_WINDOW_S, [payload])
                push(open_batch[0], "flush", open_batch[1])
            else:
                open_batch[1].append(payload)
        elif kind == "flush":
            agents = payload
            open_batch = None
            dispatch(t, agents)
        elif kind == "period":
            periods += 1
            if server_free > t + 1e-12:
                skipped += 1                      # model busy -> "auto" skips the period
            else:
                dispatch(t, list(range(N_AGENTS)))
        else:                                      # apply
            apply_tick, agents, ps, did = payload
            for a, p in zip(agents, ps):
                if did < applied_id[a]:
                    continue                      # stale answer, a newer batch already landed
                applied_id[a] = did
                at = max(apply_tick, last_tick[a])
                last_tick[a] = at
                if mode == "cond":
                    pq = to_q16(p)
                    lane = lanes[a]
                    for o in range(N_OPTIONS):
                        lane[o] += (pq[o] - lane[o]) >> shift
                    best = argmax(lane, rng)
                    c = action[a]
                    if c is None:
                        action[a] = best
                    elif lane[best] - lane[c] > dead:
                        action[a] = best
                elif mode == "point" and cfg["decision"] == "roll":
                    action[a] = sample_from(p, roll_rng)
                else:
                    action[a] = argmax(p, rng)
                if not acts[a] or acts[a][-1][1] != action[a]:
                    acts[a].append((at, action[a]))
    return {"acts": acts, "calls": calls, "batches": batches,
            "skipped": skipped, "periods": periods}

# ------------------------------------------------------------------ metrics
def agent_stats(truth, a, acts, calls):
    """Per-agent metrics from the action timeline."""
    tticks = [0] + [e[0] for e in truth.events[a]]
    tvals = [truth.init[a]] + [e[1] for e in truth.events[a]]
    aticks = [t for t, _ in acts]

    # accuracy: two-pointer merge of truth and action change points
    correct, ti, ai = 0, 0, 0
    cur_t, cur_a, prev = tvals[0], None, 0
    while ti < len(tticks) or ai < len(aticks):
        take_t = ai >= len(aticks) or (ti < len(tticks) and tticks[ti] <= aticks[ai])
        nxt = tticks[ti] if take_t else aticks[ai]
        if nxt > prev and cur_a is not None and cur_a == cur_t:
            correct += nxt - prev
        if take_t:
            cur_t, ti = tvals[ti], ti + 1
        else:
            cur_a, ai = acts[ai][1], ai + 1
        prev = nxt
    if N_TICKS > prev and cur_a is not None and cur_a == cur_t:
        correct += N_TICKS - prev

    # lag: truth change -> action first equals the new truth
    lags, unresolved = [], 0
    for j in range(len(truth.events[a])):
        tc, v = tticks[j + 1], tvals[j + 1]
        nxt = tticks[j + 2] if j + 2 < len(tticks) else N_TICKS
        found = None
        k = bisect_right(aticks, tc) - 1
        if k >= 0 and acts[k][1] == v:
            found = tc
        else:
            for k2 in range(bisect_right(aticks, tc), bisect_left(aticks, nxt)):
                if acts[k2][1] == v:
                    found = aticks[k2]
                    break
        if found is None or found >= nxt:
            unresolved += 1                      # truth changed again first
        else:
            lags.append(found - tc)

    # spurious flips: action change not matched by truth within 1 tick
    spurious = 0
    for i in range(1, len(acts)):
        ta = acts[i][0]
        if ta + 1 >= N_TICKS:
            continue
        if tvals[bisect_right(tticks, ta + 1) - 1] != acts[i][1]:
            spurious += 1
    return {"acc": correct / N_TICKS, "lags": lags, "unresolved": unresolved,
            "episodes": len(truth.events[a]), "spurious": spurious, "calls": calls}

def evaluate(cfg, truths):
    """Average the per-agent metrics over agents and seeds."""
    accs, lags, flips, calls, eps, unres = [], [], [], [], 0, 0
    skipped = periods = batches = 0
    per_min = RUN_SECONDS / 60.0
    for seed in SEEDS:
        res = run_config(cfg, seed, truths[seed])
        truth = truths[seed]
        for a in range(N_AGENTS):
            st = agent_stats(truth, a, res["acts"][a], res["calls"][a])
            accs.append(st["acc"])
            lags.extend(st["lags"])
            flips.append(st["spurious"] / per_min)
            calls.append(st["calls"] / per_min)
            eps += st["episodes"]
            unres += st["unresolved"]
        skipped += res["skipped"]
        periods += res["periods"]
        batches += res["batches"]
    return {"acc": sum(accs) / len(accs), "lag_mean": sum(lags) / len(lags) if lags else float("nan"),
            "lag_p95": percentile(lags, 0.95), "flips": sum(flips) / len(flips),
            "calls": sum(calls) / len(calls),
            "skip": 100.0 * skipped / periods if periods else 0.0,
            "srv": batches / per_min / len(SEEDS), "unres": 100.0 * unres / eps if eps else 0.0}

def log_hash(res):
    h = hashlib.sha256()
    for a in range(N_AGENTS):
        h.update(repr(res["acts"][a]).encode())
        h.update(b"|")
    h.update(f"{res['skipped']},{res['periods']},{res['batches']},{res['calls']}".encode())
    return h.hexdigest()

# --------------------------------------------------------------------- main
def build_configs():
    cfgs = [dict(id=0, mode="point", decision="argmax", label="POINT argmax"),
            dict(id=1, mode="point", decision="roll", label="POINT roll")]
    for hz in (5, 10, 15, 30):
        cfgs.append(dict(id=len(cfgs), mode="raw", hz=hz, label=f"RAW hz{hz}"))
    for hz in (5, 10, 15, 30):
        for alpha in (0.5, 0.25, 0.125):
            for db in (0.05, 0.10, 0.20):
                cfgs.append(dict(id=len(cfgs), mode="cond", hz=hz, alpha=alpha, db=db,
                                 label=f"COND hz{hz} a{alpha:g} d{db:g}"))
    return cfgs

def dominates(x, y):
    """x is at least as good as y on (accuracy up, flips down, calls down)."""
    return (x["acc"] >= y["acc"] and x["flips"] <= y["flips"] and x["calls"] <= y["calls"]
            and (x["acc"] > y["acc"] or x["flips"] < y["flips"] or x["calls"] < y["calls"]))

def main():
    truths = {s: Truth(s) for s in SEEDS}
    rows = [(cfg, evaluate(cfg, truths)) for cfg in build_configs()]

    # determinism check: same config + same seed twice -> identical logs
    probe = rows[12][0]
    h1 = log_hash(run_config(probe, SEEDS[0], truths[SEEDS[0]]))
    h2 = log_hash(run_config(probe, SEEDS[0], truths[SEEDS[0]]))
    assert h1 == h2, "determinism check FAILED"
    print(f"determinism: {probe['label']} seed {SEEDS[0]} run twice -> sha256 {h1[:16]} identical\n")

    rows.sort(key=lambda r: -r[1]["acc"])
    front = {id(cfg) for cfg, r in rows
             if not any(dominates(o, r) for oc, o in rows if oc is not cfg)}
    hdr = (f"{'config':<21}{'acc%':>7}{'lag_mn':>8}{'lag_p95':>8}{'flip/a-min':>11}"
           f"{'call/a-min':>11}{'skip%':>7}{'srv/min':>8}{'unres%':>8}   front")
    print(hdr)
    print("-" * len(hdr))
    for cfg, r in rows:
        print(f"{cfg['label']:<21}{r['acc']*100:7.2f}{r['lag_mean']:8.1f}{r['lag_p95']:8.0f}"
              f"{r['flips']:11.2f}{r['calls']:11.1f}{r['skip']:7.1f}{r['srv']:8.1f}"
              f"{r['unres']:8.1f}   {'*' if id(cfg) in front else ''}")
    print("\nPareto front over (accuracy up, spurious flips down, calls down):")
    for cfg, r in rows:
        if id(cfg) in front:
            print(f"  {cfg['label']:<21} acc {r['acc']*100:5.2f}%  flips {r['flips']:6.2f}/a-min"
                  f"  calls {r['calls']:7.1f}/a-min  srv {r['srv']:5.1f}/min")
    if READING:
        print("\nReading:")
        for line in READING:
            print(f"  {line}")

if __name__ == "__main__":
    main()

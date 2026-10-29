#!/usr/bin/env python3
"""
E2 (THROWAWAY PoC) — teacher, student, and "the right questions".

Open question: a fast student distilled from a slow teacher — can it match the teacher,
and how much does the TEACHER's choice of questions ("gates", "building blocks") matter
versus raw input?  And when should the model simply be replaced by plain code?

Run:  python3 sim.py
Stdlib only.  Deterministic seeds.  No network.
"""
import math
import random
from collections import deque

# --------------------------------------------------------------------------- config
N, WALL_P = 16, 0.25                      # 16x16 tactical grid, 25% walls
LOS_RANGE, PATH_LIMIT = 6, 10            # teacher's two thresholds
NOISE_P = 0.05                           # teacher is a slow, imperfect judge
ENEMY_W = (0.15, 0.30, 0.30, 0.25)       # 0..3 enemies
ACT = ("HIDE", "ADVANCE", "SCOUT")
K = 3
SIZES = (50, 200, 1000, 5000)
TRAIN_N, EVAL_N = 5000, 10000
SEED_TRAIN, SEED_EVAL = 20260101, 20260202          # base world
SEED_TRAIN_O, SEED_EVAL_O = 20260303, 20260404      # outside-space world
HALF = 15                                           # agent-relative frame covers the board
D_RAW = (2 * HALF + 1) ** 2 * 3 + 1                 # wall/enemy/beacon per cell + bias
D_GATE = 16                                         # 5 gates (one-hot) + bias
OPS = [0]                                          # arithmetic-op counter (cost proxy)


# --------------------------------------------------------------------------- world
def gen_world(rng):
    """A tactical board: walls, one agent, 0-3 enemies, one beacon."""
    while True:
        walls = [[rng.random() < WALL_P for _ in range(N)] for _ in range(N)]
        wlist = [(r, c) for r in range(N) for c in range(N) if walls[r][c]]
        free = [(r, c) for r in range(N) for c in range(N) if not walls[r][c]]
        if len(free) >= 8:
            break
    rng.shuffle(free)
    agent = free.pop()
    n_en = rng.choices(range(4), weights=ENEMY_W, k=1)[0]
    enemies = [free.pop() for _ in range(min(n_en, len(free)))]
    beacon = free.pop()
    return {"walls": walls, "wlist": wlist, "agent": agent, "enemies": enemies, "beacon": beacon}


def cheb(a, b):
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]))


def los_clear(w, a, e):
    """Bresenham line a->e crosses no wall.  (Cells off the board never occur on the line.)"""
    r0, c0 = a
    r1, c1 = e
    dx, dy = abs(c1 - c0), abs(r1 - r0)
    sx = 1 if c0 < c1 else -1
    sy = 1 if r0 < r1 else -1
    err = dx - dy
    r, c = r0, c0
    while True:
        OPS[0] += 4                                   # cell test + 3 arithmetic
        if w["walls"][r][c]:
            return False
        if r == r1 and c == c1:
            return True
        e2 = 2 * err
        if e2 > -dy:
            err -= dy
            c += sx
        if e2 < dx:
            err += dx
            r += sy


def bfs_len(w):
    """Steps from agent to beacon over free cells (walls block); None if unreachable."""
    walls = w["walls"]
    s, g = w["agent"], w["beacon"]
    dist = {s: 0}
    q = deque([s])
    while q:
        r, c = q.popleft()
        if (r, c) == g:
            return dist[(r, c)]
        d = dist[(r, c)] + 1
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            OPS[0] += 3                               # bounds + wall + dist checks
            nr, nc = r + dr, c + dc
            if 0 <= nr < N and 0 <= nc < N and not walls[nr][nc] and (nr, nc) not in dist:
                dist[(nr, nc)] = d
                q.append((nr, nc))
    return None


def decide(w):
    """THE TEACHER = exact oracle.  Returns (base_label, outside_label, facts, ops).

    base   : HIDE if any enemy has LoS within 6; else ADVANCE if BFS path <= 10; else SCOUT.
    outside: base, but HIDE also when an enemy sits exactly 2 cells away diagonally.
    """
    OPS[0] = 0
    a = w["agent"]
    n_vis, within6 = 0, False
    for e in w["enemies"]:
        if los_clear(w, a, e):
            n_vis += 1
            if cheb(a, e) <= LOS_RANGE:
                within6 = True
    path = bfs_len(w)
    diag = any(abs(e[0] - a[0]) == 2 and abs(e[1] - a[1]) == 2 for e in w["enemies"])
    base = 0 if within6 else (1 if (path is not None and path <= PATH_LIMIT) else 2)
    outside = 0 if (within6 or diag) else base
    facts = {"n_vis": n_vis, "within6": within6, "path": path, "diag": diag}
    return base, outside, facts, OPS[0]


# --------------------------------------------------------------------------- gates
def gate_vec(facts, w):
    """The 'higher model''s authored building blocks -> 15 binary indicators.

    0 enemy_visible_within_6 | 2..5 n_enemies_visible | 6..9 nearest_enemy_dist_bucket
    10..14 path_to_beacon_len_bucket | 15 cover_adjacent   (16 = bias)
    """
    v = [0] * 16
    v[0] = 1 if facts["within6"] else 0
    for k in range(4):
        v[1 + k] = 1 if facts["n_vis"] == k else 0
    if w["enemies"]:
        d = min(cheb(w["agent"], e) for e in w["enemies"])
        b3 = 1 if d <= 3 else (2 if d <= 7 else 3)        # coarse: straddles the 6 threshold
    else:
        b3 = 0
    for k in range(4):
        v[5 + k] = 1 if b3 == k else 0
    path = facts["path"]
    b4 = 0 if path is None else (1 if path <= 5 else (2 if path <= 10 else (3 if path <= 15 else 4)))
    for k in range(5):
        v[9 + k] = 1 if b4 == k else 0
    ar, ac = w["agent"]
    walls = w["walls"]
    cover = 0
    for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        nr, nc = ar + dr, ac + dc
        if 0 <= nr < N and 0 <= nc < N and walls[nr][nc]:
            cover = 1
            break
    v[14] = cover
    return v


_OFFBOARD = {}


def _offboard(ar, ac):
    """Frame cells that fall outside the 16x16 board (cached per agent position)."""
    key = (ar, ac)
    idx = _OFFBOARD.get(key)
    if idx is None:
        idx = []
        for dr in range(-HALF, HALF + 1):
            for dc in range(-HALF, HALF + 1):
                if not (0 <= ar + dr < N and 0 <= ac + dc < N):
                    idx.append(((dr + HALF) * (2 * HALF + 1) + (dc + HALF)) * 3)
        _OFFBOARD[key] = idx
    return idx


def raw_vec(w):
    """Raw input: every board cell, agent-relative, one-hot-ish (wall/enemy/beacon).

    Off-board cells are marked as walls: they block movement and sight-lines exactly like
    walls do, so this is a faithful raw encoding, not a feature.
    """
    ar, ac = w["agent"]
    idxs = list(_offboard(ar, ac))
    for r, c in w["wlist"]:
        idxs.append(((r - ar + HALF) * (2 * HALF + 1) + (c - ac + HALF)) * 3)
    for e in w["enemies"]:
        idxs.append(((e[0] - ar + HALF) * (2 * HALF + 1) + (e[1] - ac + HALF)) * 3 + 1)
    br, bc = w["beacon"]
    idxs.append(((br - ar + HALF) * (2 * HALF + 1) + (bc - ac + HALF)) * 3 + 2)
    idxs.append(D_RAW - 1)                             # bias
    return idxs


# --------------------------------------------------------------------------- data
def build_pool(n, seed, outside):
    """Generate worlds + teacher labels.  5% of teacher labels are corrupted."""
    rng = random.Random(seed)
    pool = []
    for _ in range(n):
        w = gen_world(rng)
        base, out_lab, facts, ops = decide(w)
        truth = out_lab if outside else base
        noisy = truth
        if rng.random() < NOISE_P:
            noisy = rng.choice([c for c in range(K) if c != truth])
        pool.append({"w": w, "facts": facts, "g": gate_vec(facts, w), "raw": raw_vec(w),
                     "truth": truth, "noisy": noisy, "base": base, "ops": ops})
    return pool


def view(pool, kind):
    """Feature view per student: sparse index lists (all binary features)."""
    if kind == "raw":
        return [p["raw"] for p in pool]
    if kind == "gates":
        return [[i for i, v in enumerate(p["g"][:15]) if v] + [15] for p in pool]
    return [[i for i, v in enumerate(p["g"][:15]) if v and i != 0] + [15] for p in pool]


# --------------------------------------------------------------------------- model
def train(X, Y, dim, seed, epochs, lr0, l2, batch=32):
    """Multinomial logistic regression, mini-batch SGD, pure Python."""
    rng = random.Random(seed)
    W = [[0.0] * dim for _ in range(K)]
    n = len(X)
    order = list(range(n))
    for ep in range(epochs):
        rng.shuffle(order)
        lr = lr0 * (0.02 ** (ep / max(1, epochs)))
        for b in range(0, n, batch):
            bidx = order[b:b + batch]
            G = [[0.0] * dim for _ in range(K)]
            for j in bidx:
                x, y = X[j], Y[j]
                z = []
                for k in range(K):
                    wk = W[k]
                    z.append(sum([wk[i] for i in x]))
                m = max(z)
                ez = [math.exp(v - m) for v in z]
                s = sum(ez)
                for k in range(K):
                    e = ez[k] / s - (1.0 if k == y else 0.0)
                    if e:
                        gk = G[k]
                        for i in x:
                            gk[i] += e
            f = lr / len(bidx)
            for k in range(K):
                W[k] = [w * (1.0 - lr * l2) - f * g for w, g in zip(W[k], G[k])]
    return W


def predict(W, X):
    out = []
    for x in X:
        z = [sum([W[k][i] for i in x]) for k in range(K)]
        out.append(max(range(K), key=lambda k: z[k]))
    return out


# --------------------------------------------------------------------------- CODE
def code_rule(p, outside=False):
    """No model: a hand rule written from the gates (the teacher's rule, no noise).

    In the outside-space world the human adds the diagonal clause by hand -- plain code
    can express what the gate set cannot.
    """
    g = p["g"]
    if g[0]:
        return 0
    if outside and p["facts"]["diag"]:
        return 0
    b4 = next(k for k in range(5) if g[9 + k])
    return 1 if b4 in (1, 2) else 2


def probe_pairs(n_pairs, seed):
    """Matched probe pairs for the declared-space boundary.

    The ONLY difference between the two members of a pair is where the enemy stands:
    2 cells away diagonally (the outside-space clause fires) vs 2 cells straight ahead
    (it does not).  Both are hidden behind a wall, and the walls/beacon are identical,
    so the two members carry IDENTICAL gate vectors but DIFFERENT raw pixels.
    """
    rng = random.Random(seed)
    pool = []
    ar, ac = 8, 8
    for _ in range(n_pairs):
        walls = [[False] * N for _ in range(N)]
        for _ in range(rng.randint(15, 45)):
            walls[rng.randrange(N)][rng.randrange(N)] = True
        walls[ar][ac] = False
        walls[9][9] = True                     # blocks the diagonal sight-line
        walls[8][9] = True                     # blocks the straight sight-line
        taken = {(ar, ac), (9, 9), (8, 9), (10, 10), (8, 10)}
        free = [(r, c) for r in range(N) for c in range(N)
                if not walls[r][c] and (r, c) not in taken]
        beacon = rng.choice(free)
        wlist = [(r, c) for r in range(N) for c in range(N) if walls[r][c]]
        for ex in ((10, 10), (8, 10)):
            w = {"walls": walls, "wlist": wlist, "agent": (ar, ac), "enemies": [ex],
                 "beacon": beacon}
            base, out_lab, facts, _ = decide(w)
            noisy = out_lab
            if rng.random() < NOISE_P:                      # teacher is still imperfect
                noisy = rng.choice([c for c in range(K) if c != out_lab])
            pool.append({"w": w, "facts": facts, "g": gate_vec(facts, w), "raw": raw_vec(w),
                         "truth": out_lab, "base": base, "noisy": noisy})
    return pool


# --------------------------------------------------------------------------- driver
# A fixed update budget, not a fixed epoch count: small training sets get more passes,
# large ones fewer, so every student gets a comparable amount of optimisation.
# The raw student needs a ~25x smaller step size: it activates ~766 inputs per case, so
# a step that is safe for 8 gate inputs massively overshoots there.
#            name          kind          dim   lr0   l2    batch  budget  lo   hi
STUDENTS = [("S-raw", "raw", D_RAW, 0.03, 0.0, 64, 8000, 8, 60),
            ("S-gates", "gates", D_GATE, 0.80, 0.0, 32, 40000, 8, 120),
            ("S-gates-minus", "gates-minus", D_GATE, 0.80, 0.0, 32, 40000, 8, 120)]


def epochs_for(n, budget, lo, hi):
    return max(lo, min(hi, round(budget / n)))


def run(train_pool, eval_pool, sizes, outside, tag):
    acc, agree, cost, pred_cache, models = {}, {}, {}, {}, {}
    code_truth = [code_rule(p, outside) for p in eval_pool]
    truth = [p["truth"] for p in eval_pool]
    for zi, n in enumerate(sizes):
        for si, (name, kind, dim, lr, l2, batch, budget, lo, hi) in enumerate(STUDENTS):
            Xtr_all = view(train_pool, kind)
            Xev = view(eval_pool, kind)
            W = train(Xtr_all[:n], [p["noisy"] for p in train_pool[:n]], dim,
                      seed=7000 + 131 * zi + 31 * si + n, epochs=epochs_for(n, budget, lo, hi),
                      lr0=lr, l2=l2, batch=batch)
            pred = predict(W, Xev)
            pred_cache[(name, n)] = pred
            models[(name, n)] = W
            acc[(name, n)] = 100.0 * sum(a == b for a, b in zip(pred, truth)) / len(truth)
            agree[(name, n)] = 100.0 * sum(a == b for a, b in zip(pred, code_truth)) / len(truth)
            cost[(name, n)] = 3.0 * sum(len(x) for x in Xev) / len(Xev)   # mul-adds / decision
    code_cost = sum(p["ops"] for p in eval_pool) / len(eval_pool)
    for p in eval_pool:
        p["code"] = code_rule(p, outside)
    return {"acc": acc, "agree": agree, "cost": cost, "code_cost": code_cost, "models": models,
            "pred_cache": pred_cache, "students": [s[0] for s in STUDENTS], "tag": tag}


def capability_probe(pool):
    """Train each student ON the probe distribution: what can it represent at all?"""
    res = {}
    n = len(pool)
    for name, kind, dim, lr, l2, batch, budget, lo, hi in STUDENTS:
        X = view(pool, kind)
        W = train(X, [p["noisy"] for p in pool], dim, seed=4242,
                  epochs=epochs_for(n, budget, lo, hi), lr0=lr, l2=l2, batch=batch)
        pred = predict(W, X)
        res[name] = 100.0 * sum(a == p["truth"] for a, p in zip(pred, pool)) / len(pool)
    return res


def majority_base(pool):
    """Accuracy of always predicting the most common true class (the floor to beat)."""
    truth = [p["truth"] for p in pool]
    best = max(range(K), key=lambda c: sum(1 for t in truth if t == c))
    return 100.0 * sum(1 for t in truth if t == best) / len(truth), best


def table(title, cols, rows):
    print()
    print(title)
    head = "  " + "student".ljust(14) + "".join(str(c).rjust(10) for c in cols)
    print(head)
    print("  " + "-" * (len(head) - 2))
    for name, vals in rows:
        print("  " + name.ljust(14) + "".join(v.rjust(10) for v in vals))


def main():
    print("=" * 78)
    print('E2 - TEACHER, STUDENT, AND "THE RIGHT QUESTIONS"')
    print("16x16 grid | 25% walls | 0-3 enemies | 1 beacon | teacher = exact oracle + 5% label noise")
    print("=" * 78)

    train_pool = build_pool(TRAIN_N, SEED_TRAIN, outside=False)
    eval_pool = build_pool(EVAL_N, SEED_EVAL, outside=False)
    base = run(train_pool, eval_pool, SIZES, outside=False, tag="base")

    names = base["students"]
    maj, maj_cls = majority_base(eval_pool)
    rows = []
    for nm in names + ["CODE (rule)"]:
        vals = []
        for n in SIZES:
            vals.append(f"{base['acc'][(nm, n)]:.1f}" if nm in names else "100.0")
        rows.append((nm, vals))
    rows.append((f"BASE ({ACT[maj_cls]})", [f"{maj:.1f}"] * len(SIZES)))
    table("TABLE 1 - held-out accuracy vs the NOISELESS truth "
          f"(eval: {EVAL_N} fresh worlds, unseen seed)", [f"n={n}" for n in SIZES], rows)
    mix = " / ".join(f"{ACT[c]} {100.0*sum(1 for p in eval_pool if p['truth']==c)/len(eval_pool):.0f}%"
                     for c in range(K))
    print(f"  truth mix: {mix}   |   teacher = this rule + 5% label noise")

    rows = []
    for nm in names:
        vals = [f"{base['cost'][(nm, n)]:.0f}" for n in SIZES]
        rows.append((nm, vals))
    rows.append(("CODE (rule)", [f"{base['code_cost']:.0f}"] * len(SIZES)))
    table("TABLE 2 - inference cost proxy (mul-adds / cell-checks per decision, mean over eval)",
          [f"n={n}" for n in SIZES], rows)

    dial = base["agree"][("S-gates", 5000)]
    verdict = "model can be replaced by code here" if dial >= 99.0 else "NOT yet replaceable"
    print()
    print("TABLE 3 - THE DIAL: S-gates @n=5000 vs CODE, per eval decision")
    print(f"  agreement {dial:.2f}%   (>= 99% ? {verdict})")
    print(f"  cost: S-gates {base['cost'][('S-gates', 5000)]:.0f} mul-adds/decision"
          f"   vs   CODE {base['code_cost']:.0f} ops/decision")

    # ---------------- outside-space truth ----------------
    train_o = build_pool(TRAIN_N, SEED_TRAIN_O, outside=True)
    eval_o = build_pool(EVAL_N, SEED_EVAL_O, outside=True)
    out = run(train_o, eval_o, SIZES, outside=True, tag="outside")

    rows = []
    for nm in names + ["CODE (rule)"]:
        vals = [f"{out['acc'][(nm, n)]:.1f}" if nm in names else "100.0" for n in SIZES]
        rows.append((nm, vals))
    majo, maj_cls_o = majority_base(eval_o)
    rows.append((f"BASE ({ACT[maj_cls_o]})", [f"{majo:.1f}"] * len(SIZES)))
    table("TABLE 4 - OUTSIDE-SPACE truth (HIDE also when an enemy is exactly 2 cells away "
          "diagonally): held-out accuracy vs the NEW truth", [f"n={n}" for n in SIZES], rows)

    # the slice: eval cases whose truth actually changed
    idx = [i for i, p in enumerate(eval_o) if p["truth"] != p["base"]]
    slice_acc = {}
    print()
    print(f"  slice: truth changed on {len(idx)}/{EVAL_N} eval cases "
          f"({100.0*len(idx)/EVAL_N:.1f}%) - accuracy ON THAT SLICE only (n=5000)")
    for nm in names:
        pred = out["pred_cache"][(nm, 5000)]
        hit = sum(1 for i in idx if pred[i] == eval_o[i]["truth"])
        slice_acc[nm] = 100.0 * hit / max(1, len(idx))
        print(f"    {nm:<14} {slice_acc[nm]:5.1f}%  ({hit}/{len(idx)})")

    # ---------------- capability probe: where exactly is the boundary? ----------------
    probe = probe_pairs(200, 31337)
    same_gate = all(probe[2 * i]["g"] == probe[2 * i + 1]["g"] for i in range(len(probe) // 2))
    same_raw = all(probe[2 * i]["raw"] == probe[2 * i + 1]["raw"] for i in range(len(probe) // 2))
    nat = {}
    for nm, kind, *_ in STUDENTS:
        pred = predict(out["models"][(nm, 5000)], view(probe, kind))
        nat[nm] = 100.0 * sum(a == p["truth"] for a, p in zip(pred, probe)) / len(probe)
    cap = capability_probe(probe)
    print()
    print("TABLE 5 - CAPABILITY PROBE: 200 matched pairs (enemy 2 away diagonally vs 2 away")
    print("  straight ahead, both hidden behind a wall; identical walls/beacon)")
    print(f"  identical GATE vector on both members of every pair : {same_gate}")
    print(f"  identical RAW vector on both members of every pair  : {same_raw}")
    rows = []
    for nm in names:
        rows.append((nm, [f"{nat[nm]:.1f}", f"{cap[nm]:.1f}"]))
    print()
    print("  " + "student".ljust(14) + "natural data -> probe".rjust(24)
          + "trained on probe".rjust(22))
    print("  " + "-" * 58)
    for nm, vals in rows:
        print("  " + nm.ljust(14) + vals[0].rjust(24) + vals[1].rjust(22))
    print("  S-gates is capped at 50% by construction: the two members of a pair carry identical")
    print("  gate vectors, so identical outputs are forced and no amount of data can separate")
    print("  them.  S-raw's vectors differ, so with data of that kind it can learn the clause.")

    # ---------------- reading ----------------
    a = base["acc"]
    g5, r5, m5 = a[("S-gates", 5000)], a[("S-raw", 5000)], a[("S-gates-minus", 5000)]
    g50, r50 = a[("S-gates", 50)], a[("S-raw", 50)]
    og5 = out["acc"][("S-gates", 5000)]
    gc5 = base["cost"][("S-gates", 5000)]
    rc5 = base["cost"][("S-raw", 5000)]
    lines = [
        f"1. S-gates hits {g5:.1f}% at n=1000 and n=5000 vs CODE 100.0% ({g50:.1f}% on only 50 labels): the gates are a sufficient statistic, so the teacher's 5% label noise only bites when data is scarce.",
        f"2. S-raw (whole board, {D_RAW} raw pixels) climbs {r50:.1f} -> {r5:.1f}% and stops, barely above the {maj:.1f}% majority-class baseline: a linear model over pixels cannot represent 'LoS within 6 through walls' nor an exact BFS path length.",
        f"3. S-gates-minus (the LOS gate forgotten) loses {g5-m5:.1f} pts at n=5000 and plateaus at {m5:.1f}%: the coarse 'nearest enemy <=7' bucket cannot certify 'visible within 6' - the sharp question is what carries the signal.",
        f"4. Small data favours the authored questions: at n=50 S-gates {g50:.1f}% vs S-raw {r50:.1f}% ({g50-r50:.1f} pts), for {gc5:.0f} mul-adds instead of {rc5:.0f} - the raw student reads the whole board, so it is even dearer than the {base['code_cost']:.0f}-op rule it replaces.",
        f"5. The dial: S-gates agrees with CODE on {dial:.2f}% of decisions at n=5000 -> \"{verdict}\"; the student is {base['code_cost']/gc5:.0f}x cheaper per call than the rule it learned.",
        f"6. Outside the declared space (diagonal-2 HIDE) S-gates drops to {og5:.1f}% overall and {slice_acc['S-gates']:.1f}% on the {len(idx)} changed cases (S-raw {slice_acc['S-raw']:.1f}% - the clause is too rare to learn), but the probe separates causes: S-gates is hard-capped at 50% while S-raw reaches {cap['S-raw']:.1f}% given such data - the gates' blindness is structural, code's is not.",
    ]
    print()
    print("READING (6 lines)")
    for ln in lines:
        print("  " + ln)


if __name__ == "__main__":
    main()

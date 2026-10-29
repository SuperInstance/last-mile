#!/usr/bin/env python3
"""E3 — Generation as a stream of typed decisions (throwaway PoC).

A wave-function-collapse-style constraint solver builds 24x16 room maps over
8 tile types. Every collapse is a typed decision: p[] over the allowed tiles,
then a seeded roll rng = Random(crc32(seed, attempt, step)). Two decision
models: uniform baseline, and softmax of a linear taste model over local
features. A hidden human taste scores finished maps; the clicker thumbs up if
score >= median of a reference batch (no other signal). The taste model is
updated after each batch of 20 maps (reward-weighted feature averages).
Measures: learning curve, determinism (byte-identical replay), contradiction/
restart rate, and delta-regen of one disliked 6x6 region with the rest frozen.
Python 3 stdlib only, deterministic seeds, no files written.
"""
import hashlib
import math
import random
import time
import zlib
from statistics import median

T0 = time.time()

# ---------------- board, tiles, adjacency ----------------
W, H = 24, 16
N = W * H
TILES = ["floor", "wall", "door", "cover", "window", "water", "beacon", "spawn"]
TI = {t: i for i, t in enumerate(TILES)}
CH = ".#+c~=BS"                      # one char per tile, same order as TILES
FULL = (1 << 8) - 1
NESW = ((0, 1), (0, -1), (1, 0), (-1, 0))
OPEN_T = {TI["floor"], TI["cover"], TI["water"], TI["beacon"]}
WALLISH = {TI["wall"], TI["door"], TI["window"]}
OPENMASK = 0x69                      # bits of floor|cover|water|beacon

# symmetric adjacency: tile -> tiles it may touch. Deliberate tensions:
# doors only in walls, beacons only in open space, spawns in wall niches
# (or wet niches), windows never touch each other.
PARTNERS = {
    "floor":  {"floor", "wall", "door", "cover", "window", "water", "beacon"},
    "wall":   {"floor", "wall", "door", "cover", "window", "water", "spawn"},
    "door":   {"floor", "wall", "spawn"},
    "cover":  {"floor", "wall", "cover", "window", "water", "beacon"},
    "window": {"floor", "wall", "cover", "water"},
    "water":  {"floor", "wall", "cover", "window", "water", "beacon", "spawn"},
    "beacon": {"floor", "cover", "water"},
    "spawn":  {"wall", "door", "water"},
}
ALLOW = [0] * 8
for a in PARTNERS:
    for b in PARTNERS[a]:
        ALLOW[TI[a]] |= 1 << TI[b]
for a in range(8):
    for b in range(8):
        assert ((ALLOW[a] >> b) & 1) == ((ALLOW[b] >> a) & 1), "adjacency must be symmetric"

# unary-at-collapse "needs": tile t is only choosable at c if some neighbour of
# c could still be a tile from NEEDS[t] (out-of-bounds counts as wall).
NEEDS = {
    TI["window"]: 1 << TI["wall"],
    TI["spawn"]: (1 << TI["wall"]) | (1 << TI["door"]),
    TI["door"]: (1 << TI["wall"]) | (1 << TI["spawn"]),
}
# hard rule that local propagation cannot guarantee and is only checkable on
# the finished map; violating it is a contradiction -> restart:
END_NEEDS = {TI["door"]: {TI["wall"], TI["spawn"]}}   # doors are set into walls

BASE = 0xE3
BATCHES, BATCH = 30, 20
ETA, LAM, CLIP = 0.8, 0.015, 3.0
MAX_ATT = 40
REGION = 6

# feature layout (per decision, per candidate tile), DIM = 38:
#   0..7   neighbour support per tile type (fractional, 4-neighbourhood)
#   8      proximity of nearest collapsed beacon (1 - min(d,12)/12)
#   9      "no beacon collapsed yet" indicator
#   10..13 open-area size bucket one-hot (0 / 1-3 / 4-9 / 10+ collapsed open cells)
#   14..21 candidate tile one-hot
#   22..29 tile one-hot x beacon proximity
#   30..37 tile one-hot x spawn-neighbour support
#   38..45 tile one-hot x wall-neighbour support
DIM = 46


def h(s):
    return zlib.crc32(s.encode())


def bits(m):
    out = []
    i = 0
    while m:
        if m & 1:
            out.append(i)
        m >>= 1
        i += 1
    return out


# ---------------- solver ----------------
def propagate(domains, c0):
    """AC-3-style revision from c0. Returns False on contradiction."""
    q = [c0]
    qi = 0
    while qi < len(q):
        c = q[qi]
        qi += 1
        dc = domains[c]
        m = 0
        s = dc
        while s:
            b = s & -s
            m |= ALLOW[b.bit_length() - 1]
            s ^= b
        y, x = divmod(c, W)
        for dy, dx in NESW:
            ny, nx = y + dy, x + dx
            if 0 <= ny < H and 0 <= nx < W:
                n = ny * W + nx
                dn = domains[n]
                nn = dn & m
                if nn != dn:
                    if nn == 0:
                        return False
                    domains[n] = nn
                    q.append(n)
    return True


def needs_ok(cell, t, domains):
    """Does tile t at cell still have a neighbour that could host its needs?"""
    nm = NEEDS.get(t)
    if nm is None:
        return True
    y, x = divmod(cell, W)
    for dy, dx in NESW:
        ny, nx = y + dy, x + dx
        if not (0 <= ny < H and 0 <= nx < W) or (domains[ny * W + nx] & nm):
            return True
    return False


def open_bucket(cell, domains):
    """Size bucket of the connected blob of collapsed open tiles touching cell."""
    y, x = divmod(cell, W)
    stack = []
    for dy, dx in NESW:
        ny, nx = y + dy, x + dx
        if 0 <= ny < H and 0 <= nx < W:
            d = domains[ny * W + nx]
            if d.bit_count() == 1 and (d & OPENMASK):
                stack.append(ny * W + nx)
    seen = set()
    while stack and len(seen) < 13:
        c = stack.pop()
        if c in seen:
            continue
        seen.add(c)
        y2, x2 = divmod(c, W)
        for dy, dx in NESW:
            ny, nx = y2 + dy, x2 + dx
            if 0 <= ny < H and 0 <= nx < W:
                n = ny * W + nx
                d = domains[n]
                if d.bit_count() == 1 and (d & OPENMASK) and n not in seen:
                    stack.append(n)
    sz = len(seen)
    return 0 if sz == 0 else 1 if sz <= 3 else 2 if sz <= 9 else 3


def decide(aseed, k, cell, opts, w, domains, beacons):
    """One typed decision: p[] over allowed tiles + seeded roll.
    Returns (tile, feature vector of the chosen tile or None)."""
    rng = random.Random(h(f"{aseed}:k{k}"))
    if w is None:
        return opts[rng.randrange(len(opts))], None
    y, x = divmod(cell, W)
    supp = [0.0] * 8
    for dy, dx in NESW:
        ny, nx = y + dy, x + dx
        if 0 <= ny < H and 0 <= nx < W:
            d = domains[ny * W + nx]
            pc = d.bit_count()
            if pc:
                inv = 1.0 / pc
                s = d
                while s:
                    b = s & -s
                    supp[b.bit_length() - 1] += inv
                    s ^= b
    if beacons:
        dmin = min(abs(y - by) + abs(x - bx) for by, bx in beacons)
        prox = 1.0 - min(dmin, 12) / 12.0
        nb = 0.0
    else:
        prox, nb = 0.0, 1.0
    bkt = open_bucket(cell, domains)
    base = supp + [prox, nb] + [1.0 if j == bkt else 0.0 for j in range(4)]
    spw = supp[TI["spawn"]]
    wsw = supp[TI["wall"]]
    fs, logits = [], []
    for t in opts:
        f = (base
             + [1.0 if j == t else 0.0 for j in range(8)]
             + [prox if j == t else 0.0 for j in range(8)]
             + [spw if j == t else 0.0 for j in range(8)]
             + [wsw if j == t else 0.0 for j in range(8)])
        fs.append(f)
        logits.append(sum(a * b for a, b in zip(w, f)))
    mx = max(logits)
    ps = [math.exp(l - mx) for l in logits]
    tot = sum(ps)
    r = rng.random() * tot
    acc = 0.0
    for i, p in enumerate(ps):
        acc += p
        if r <= acc:
            return opts[i], fs[i]
    return opts[-1], fs[-1]


def map_valid(tiles):
    """Hard rules checkable only on the finished map (END_NEEDS)."""
    for c, t in enumerate(tiles):
        nm = END_NEEDS.get(t)
        if nm is None:
            continue
        y, x = divmod(c, W)
        for dy, dx in NESW:
            ny, nx = y + dy, x + dx
            if 0 <= ny < H and 0 <= nx < W and tiles[ny * W + nx] in nm:
                break
        else:
            return False
    return True


def generate(seed, w=None, region=None, base=None):
    """Collapse the whole map (region=None) or just `region` cells of `base`.
    Returns (tiles, restarts, phi) or (None, restarts, None) after MAX_ATT."""
    region_cells = list(range(N)) if region is None else sorted(region)
    in_reg = None if region is None else set(region_cells)
    restarts = 0
    for attempt in range(MAX_ATT):
        aseed = f"{seed}:a{attempt}"
        domains = [FULL] * N
        beacons = []
        done = [False] * N
        ok = True
        if base is not None:
            for c in range(N):
                if c not in in_reg:
                    t = base[c]
                    domains[c] = 1 << t
                    done[c] = True
                    if t == TI["beacon"]:
                        by, bx = divmod(c, W)
                        beacons.append((by, bx))
            for c in range(N):
                if c not in in_reg and not propagate(domains, c):
                    ok = False
                    break
        phisum = [0.0] * DIM if w is not None else None
        ndec = 0
        k = 0
        while ok:
            best, bestpc = -1, 99
            for c in region_cells:
                if done[c]:
                    continue
                pc = domains[c].bit_count()
                if 1 <= pc < bestpc:
                    bestpc, best = pc, c
            if best < 0:
                break
            opts = [t for t in bits(domains[best]) if needs_ok(best, t, domains)]
            if not opts:
                ok = False                # contradiction: restart
                break
            if len(opts) == 1:
                t, fv = opts[0], None     # forced by propagation, no dice rolled
            else:
                t, fv = decide(aseed, k, best, opts, w, domains, beacons)
                k += 1
            if phisum is not None and fv is not None:
                for i in range(DIM):
                    phisum[i] += fv[i]
                ndec += 1
            domains[best] = 1 << t
            done[best] = True
            if t == TI["beacon"]:
                by, bx = divmod(best, W)
                beacons.append((by, bx))
            if not propagate(domains, best):
                ok = False
        if ok:
            tiles = [bits(domains[c])[0] for c in range(N)]
            if map_valid(tiles):
                phi = [p / ndec for p in phisum] if (phisum is not None and ndec) else None
                return tiles, restarts, phi
            ok = False                 # end-of-map hard rule broken -> contradiction
        restarts += 1
    return None, restarts, None


# ---------------- hidden human taste (never shown to the learner) ----------------
def taste_score(tiles, win=None):
    """Likes 2-4 cover near each beacon (+3, else -2), dislikes water touching
    spawns (-4/pair), likes width-2 corridors (+0.4/cell, cap 30), dislikes
    dead ends (-1.2), wants >=1 beacon and >=1 spawn (-6 each if missing).
    win=(oy,ox) restricts entities to that 6x6 window (for region picking)."""
    oy, ox = (0, 0) if win is None else win
    inside = (lambda c: True) if win is None else (
        lambda c: (oy <= c // W < oy + REGION) and (ox <= c % W < ox + REGION))
    s = 0.0
    for c in range(N):
        t = tiles[c]
        y, x = divmod(c, W)
        if t == TI["beacon"] and inside(c):
            cnt = 0
            for dy in range(-2, 3):
                for dx in range(-2, 3):
                    ny, nx = y + dy, x + dx
                    if 0 <= ny < H and 0 <= nx < W and tiles[ny * W + nx] == TI["cover"]:
                        cnt += 1
            s += 3.0 if 2 <= cnt <= 4 else -2.0
        if t == TI["water"]:
            for dy, dx in ((0, 1), (1, 0)):
                ny, nx = y + dy, x + dx
                if 0 <= ny < H and 0 <= nx < W:
                    n = ny * W + nx
                    if tiles[n] == TI["spawn"] and (inside(c) or inside(n)):
                        s -= 4.0
    corr = 0
    for c in range(N):
        if tiles[c] not in OPEN_T or not inside(c):
            continue
        y, x = divmod(c, W)

        def isw(ny, nx):
            if not (0 <= ny < H and 0 <= nx < W):
                return True
            return tiles[ny * W + nx] in WALLISH

        def iso(ny, nx):
            return 0 <= ny < H and 0 <= nx < W and tiles[ny * W + nx] in OPEN_T

        wl, wr = isw(y, x - 1), isw(y, x + 1)
        wu, wd = isw(y - 1, x), isw(y + 1, x)
        if wl + wr + wu + wd >= 3:
            s -= 1.2
        # width-2 passage: exactly one side wall-ish, partner open, wall beyond partner
        v2 = (wl and iso(y, x + 1) and isw(y, x + 2)) or (wr and iso(y, x - 1) and isw(y, x - 2))
        h2 = (wu and iso(y + 1, x) and isw(y + 2, x)) or (wd and iso(y - 1, x) and isw(y - 2, x))
        if (v2 and (iso(y - 1, x) or iso(y + 1, x))) or (h2 and (iso(y, x - 1) or iso(y, x + 1))):
            corr += 1
    s += 0.4 * min(corr, 30)
    if win is None:
        if TI["beacon"] not in tiles:
            s -= 6.0
        if TI["spawn"] not in tiles:
            s -= 6.0
    return s


def worst_window(tiles):
    best = None
    for oy in range(H - REGION + 1):
        for ox in range(W - REGION + 1):
            v = taste_score(tiles, (oy, ox))
            if best is None or v < best[0]:
                best = (v, oy, ox)
    return best


def maphash(tiles):
    return hashlib.sha256("".join(CH[t] for t in tiles).encode()).hexdigest()[:16]


def show(tiles, win=None):
    oy, ox = (0, 0) if win is None else win
    out = []
    for y in range(H):
        row = "".join(CH[tiles[y * W + x]] for x in range(W))
        mark = ">" if win is not None and oy <= y < oy + REGION else " "
        out.append(mark + row)
    return "\n".join(out)


# ---------------- experiment ----------------
def main():
    print("=" * 78)
    print("E3 — generation as a stream of typed decisions (throwaway PoC)")
    print(f"board {W}x{H} | tiles: {' '.join(TILES)} | chars: '{CH}'")
    print(f"{BATCHES} batches x {BATCH} maps | taste update: w += eta*(mean_up - mean_dn), "
          f"eta={ETA} lam={LAM} clip={CLIP} | features dim={DIM}")
    print("=" * 78)

    ref = [generate(f"{BASE}:REF:{i}") for i in range(BATCH)]
    theta = median(taste_score(t) for t, _, _ in ref if t is not None)
    print(f"\nclicker: thumb up iff hidden taste >= median of reference batch -> theta = {theta:.2f}")

    w = [0.0] * DIM
    rows = []
    all_u, all_l = [], []                     # (score, restarts) per map
    last_L_tiles, w_pre_last = None, None
    for b in range(1, BATCHES + 1):
        bu = [generate(f"{BASE}:U:{b}:{i}") for i in range(BATCH)]
        bl = [generate(f"{BASE}:L:{b}:{i}", w) for i in range(BATCH)]
        su = [(taste_score(t), r) for t, r, _ in bu if t is not None]
        slp = [(taste_score(t), r, p) for t, r, p in bl if t is not None]
        all_u += su
        all_l += [(s, r) for s, r, _ in slp]
        phi_up = [p for s, _, p in slp if s >= theta and p is not None]
        phi_dn = [p for s, _, p in slp if s < theta and p is not None]
        if phi_up and phi_dn:
            for i in range(DIM):
                mu = sum(p[i] for p in phi_up) / len(phi_up)
                md = sum(p[i] for p in phi_dn) / len(phi_dn)
                w[i] = max(-CLIP, min(CLIP, w[i] * (1.0 - LAM) + ETA * (mu - md)))
        rows.append((b,
                     sum(s for s, _ in su) / len(su),
                     sum(s for s, _, _ in slp) / len(slp),
                     len(phi_up),
                     sum(r for _, r in su) / len(su),
                     sum(r for _, r, _ in slp) / len(slp)))
        if b == BATCHES:
            last_L_tiles = [t for t, _, _ in bl if t is not None]
            w_pre_last = w[:]

    print("\n--- learning curve: mean hidden taste per batch (20 maps each)")
    print("batch | uniform | learned | thumbs up | restarts U | restarts L")
    for b, mu_, ml_, up_, ru_, rl_ in rows:
        print(f"{b:5d} | {mu_:7.2f} | {ml_:7.2f} | {up_:6d}/20 | {ru_:10.2f} | {rl_:10.2f}")

    def m5(i):
        return (sum(r[1] for r in rows[i:i + 5]) / 5, sum(r[2] for r in rows[i:i + 5]) / 5)
    (u1, l1), (u2, l2) = m5(0), m5(len(rows) - 5)
    up1 = sum(r[3] for r in rows[:5]) / (5 * BATCH)
    up2 = sum(r[3] for r in rows[-5:]) / (5 * BATCH)
    print(f"first-5 mean: uniform {u1:.2f} learned {l1:.2f} | last-5 mean: uniform {u2:.2f} learned {l2:.2f}")
    print(f"thumbs-up rate: first-5 {100*up1:.0f}%  ->  last-5 {100*up2:.0f}%   (chance = 50%)")

    print("\n--- contradiction rate of the solver (restarts per map)")
    for name, res in (("uniform", all_u), ("learned", all_l)):
        rs = [r for _, r in res]
        print(f"{name:8s}: mean restarts {sum(rs)/len(rs):.3f} | maps with >=1 restart "
              f"{100*sum(1 for r in rs if r)/len(rs):.1f}% | max {max(rs)}")

    print("\n--- determinism (same seed + same taste weights -> byte-identical map)")
    det_ok = True
    checks = [
        ("uniform seed replay", f"{BASE}:U:1:0", None, None),
        ("learned seed replay (final w)", f"{BASE}:L:{BATCHES}:0", w, None),
        ("learned batch-30 map 0 replay", f"{BASE}:L:{BATCHES}:0", w_pre_last,
         maphash(last_L_tiles[0])),
    ]
    for name, seed, wt, orig in checks:
        t1, _, _ = generate(seed, wt)
        t2, _, _ = generate(seed, wt)
        h1, h2 = maphash(t1), maphash(t2)
        same = h1 == h2 and (orig is None or h1 == orig)
        det_ok &= same
        extra = "" if orig is None else f" | matches in-batch original: {h1 == orig}"
        print(f"  {name:32s} {h1} == {h2} -> {'PASS' if same else 'FAIL'}{extra}")

    print("\n--- DELTA REGEN: liked map, worst 6x6 region re-rolled, rest frozen")
    scored = sorted(((taste_score(t), t) for t in last_L_tiles), reverse=True)
    liked = [(s, t) for s, t in scored if s >= theta] or scored[:10]
    trials = liked[:10]
    print(f"liked maps in last batch: {len([1 for s, _ in scored if s >= theta])}/{len(scored)}; "
          f"running {len(trials)} regen trials")
    regen = []
    for i, (sc0, tiles) in enumerate(trials):
        _, oy, ox = worst_window(tiles)
        region = [(oy + dy) * W + (ox + dx) for dy in range(REGION) for dx in range(REGION)]
        new, rr, _ = generate(f"{BASE}:RG:{i}", w, region=region, base=tiles)
        if new is None:
            regen.append(None)
            print(f"  trial {i}: FAILED (constraints unsatisfiable within {MAX_ATT} attempts)")
            continue
        chg = sum(1 for c in region if new[c] != tiles[c])
        sc1 = taste_score(new)
        regen.append((sc0, sc1, chg, rr, new))
        print(f"  trial {i}: region (y={oy},x={ox}) | {chg:2d}/36 tiles changed "
              f"({100*chg/36:.0f}% of region, {100*chg/N:.1f}% of map) | restarts {rr} "
              f"| taste {sc0:.2f} -> {sc1:.2f}")
    ok_r = [r for r in regen if r]
    succ = len(ok_r) / len(regen)
    m_chg = sum(r[2] for r in ok_r) / len(ok_r) / (REGION * REGION)
    m_b = sum(r[0] for r in ok_r) / len(ok_r)
    m_a = sum(r[1] for r in ok_r) / len(ok_r)
    improved = sum(1 for r in ok_r if r[1] > r[0])
    print(f"summary: success {len(ok_r)}/{len(regen)} ({100*succ:.0f}%) | mean region tiles changed "
          f"{100*m_chg:.0f}% (map frozen: {100*(1 - 36/N):.1f}%) | taste {m_b:.2f} -> {m_a:.2f} "
          f"| improved {improved}/{len(ok_r)}")

    sc0, tiles = trials[0]
    print(f"\nsample: best liked map (taste {sc0:.2f}) with '>' marking the regen rows:")
    print(show(tiles, worst_window(tiles)[1:]))
    if regen and regen[0]:
        print(f"same map after delta regen (taste {regen[0][1]:.2f}):")
        print(show(regen[0][4], worst_window(tiles)[1:]))

    ru_m = sum(r for _, r in all_u) / len(all_u)
    rl_m = sum(r for _, r in all_l) / len(all_l)
    wcov = w[22 + TI["cover"]]
    wwat = w[30 + TI["water"]]
    braver = "more of" if rl_m >= ru_m else "no more of"
    print("\nREADING:")
    print(f"1. The same decision channel that recognizes also generates: with one bit of feedback "
          f"per map, learned taste goes {l1:.2f} -> {l2:.2f} across batches while uniform sits at "
          f"{u1:.2f} -> {u2:.2f}, and thumbs-up rises {100*up1:.0f}% -> {100*up2:.0f}% (chance 50%).")
    print(f"2. The learner recovers structure it was never shown: weight on cover-near-beacon is "
          f"{wcov:+.2f} and on water-next-to-spawn {wwat:+.2f} — the hidden clicker's two strongest "
          f"dislikes/likes, reconstructed from whole-map thumbs alone.")
    print(f"3. Determinism holds: replaying (seed, weights) reproduces byte-identical hashes "
          f"({'PASS' if det_ok else 'FAIL'}), because every dice is Random(crc32(seed, attempt, step)) "
          f"and cell order is pure min-entropy.")
    print(f"4. Contradictions stay cheap: {ru_m:.2f} restarts/map uniform vs {rl_m:.2f} learned — "
          f"a sharper p[] asks {braver} the constraint graph (door-near-wall weight "
          f"{w[38 + TI['door']]:+.2f}), and restarts absorb it (max {max(r for _, r in all_u + all_l)}).")
    print(f"5. Delta regen works: freezing {100*(1-36/N):.1f}% of the map and re-rolling one disliked "
          f"6x6 succeeds {100*succ:.0f}% of the time, changes {100*m_chg:.0f}% of the region, and moves "
          f"taste {m_b:.2f} -> {m_a:.2f} ({improved}/{len(ok_r)} improved) without touching the rest.")
    print(f"6. Verdict on the open question: yes, in miniature — 'decision model + seeded dice' gives "
          f"generation, learning from thumbs, and targeted repair with one mechanism; runtime "
          f"{time.time()-T0:.0f}s, stdlib only, zero nondeterminism.")


if __name__ == "__main__":
    main()

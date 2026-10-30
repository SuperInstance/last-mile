#!/usr/bin/env python3
"""The Hodge decomposition on the attention gate's real answers, and what it
tells us that the escalation policy does not.

Corpora
  A. attention_capture.json -- 60 real answers captured from the Thor's
     System One endpoint (d1-2b :8094, clef-flash :8096) on the shipped
     default-harness `attention` gate: 3 rosters x 5 situations x 2 models
     x 2 repeats. See capture_attention.py.
  B. ../../../../docs/research/poc/thor/e4-d1-3B-log.json -- the repo's own
     logged Thor gate answers (E4 poc), choice gates with option probabilities.

Run:  python3 analyse_attention.py > results.txt
"""

import itertools
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hodge import analyse, analyse_bipartite

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
THRESHOLD = 0.5   # the shipped D71 escalation threshold


def load_capture(path=None):
    path = path or os.path.join(HERE, "attention_capture.json")
    recs = []
    for r in json.load(open(path)):
        if not r.get("ok"):
            continue
        recs.append({
            "roster": r["roster"], "roster_size": r["roster_size"],
            "situation": r["situation"], "model": r["model"], "rep": r["rep"],
            "choice": r["choice"], "confidence": float(r["confidence"] or 0.0),
            "probs": {k: float(v) for k, v in r["probs"].items()},
            "options": sorted(r["probs"].keys()),
        })
    return recs


def rank_agreement(pa, pb, options):
    """Kendall tau of the two orderings over the shared options."""
    shared = [o for o in options if o in pa and o in pb]
    if len(shared) < 2:
        return None
    conc = disc = 0
    for a, b in itertools.combinations(shared, 2):
        sa, sb = pa[a] - pa[b], pb[a] - pb[b]
        if abs(sa) < 1e-15 or abs(sb) < 1e-15:
            continue
        if (sa > 0) == (sb > 0):
            conc += 1
        else:
            disc += 1
    n = conc + disc
    return (conc - disc) / n if n else None


def fmt(r):
    return (f"grad={r['gradient_energy']:.6e} curl={r['curl_energy']:.6e} "
            f"harm={r['harmonic_energy']:.6e} | h0={r['h0_ratio']:.6f} "
            f"h1={r['h1_ratio']:.6f} h2={r['h2_ratio']:.6f} | "
            f"maxcirc={r['max_abs_circulation']:.6f} tris={r['n_triangles']} "
            f"comp={r['n_components']} edges={r['n_edges']}")


def hodge_of(recs):
    return analyse([{"probs": r["probs"]} for r in recs],
                   [r["options"] for r in recs])


def main():
    print("=" * 78)
    print("CORPUS A -- the Thor's real answers to the shipped `attention` gate")
    print("=" * 78)
    recs = load_capture()
    print(f"{len(recs)} answers captured, "
          f"{len(set((r['roster'], r['situation'], r['model'], r['rep']) for r in recs))} distinct")

    r8 = [r for r in recs if r["roster"] == "r8"]
    print("\nA1. attention gate, roster r8 (10 options), ALL 20 answers")
    print("    ", fmt(hodge_of(r8)))

    only_a = [r for r in r8 if r["model"] == "d1-2b"]
    print("\nA2. attention gate, roster r8, d1-2b only (10 answers)")
    print("    ", fmt(hodge_of(only_a)))

    sit = "contested"
    both = [r for r in r8 if r["situation"] == sit]
    print(f"\nA3. attention gate, roster r8, situation '{sit}', both models (4 answers)")
    print("    ", fmt(hodge_of(both)))

    print("\nA4. attention gate, ALL rosters (r8/r4/r2) -- the option sets differ")
    print("    ", fmt(hodge_of(recs)))

    sel = [r for r in recs if r["roster"] in ("r8", "r4")]
    print("\nA5. attention gate, rosters r8+r4 (r2's agents drop out)")
    print("    ", fmt(hodge_of(sel)))

    print("\nA6. every single answer on its own (60 of them): the largest curl")
    worst = max(((hodge_of([r])["curl_energy"], r) for r in recs), key=lambda t: t[0])
    print(f"     max curl over all 60 single answers: {worst[0]:.6e}")
    print(f"     (that answer: roster {worst[1]['roster']}, {worst[1]['situation']}, "
          f"{worst[1]['model']})")

    print()
    print("=" * 78)
    print("THE BIPARTITE (HodgeRank heterogeneous) FORM: agents vs candidates")
    print("=" * 78)
    print("Every answer is an AGENT; every option is a CANDIDATE. The curl then")
    print("measures whether the AGENTS disagree about the ORDER of the options.")
    print()
    print(f"{'roster':7} {'situation':16} {'agents':>7} {'cands':>6} "
          f"{'h1 ratio':>9} {'h2 ratio':>9} {'comp':>5} "
          f"{'same choice':>12} {'same order':>11}")
    for roster in ("r8", "r4", "r2"):
        for sit in sorted({r["situation"] for r in recs}):
            sel = [r for r in recs if r["roster"] == roster and r["situation"] == sit]
            if not sel:
                continue
            b = analyse_bipartite([{"probs": r["probs"]} for r in sel],
                                  [r["options"] for r in sel], scores="rank")
            a = [r for r in sel if r["model"] == "d1-2b"][0]
            c = [r for r in sel if r["model"] == "clef-flash"][0]
            same_choice = "yes" if a["choice"] == c["choice"] else "NO"
            shared = sorted(set(a["options"]) & set(c["options"]))
            oa = sorted(shared, key=lambda o: -a["probs"][o])
            oc = sorted(shared, key=lambda o: -c["probs"][o])
            same_order = "yes" if oa == oc else "NO"
            print(f"{roster:7} {sit:16} {b['n_agents']:>7} {b['n_candidates']:>6} "
                  f"{b['h1_ratio']:9.6f} {b['h2_ratio']:9.6f} {b['n_components']:>5} "
                  f"{same_choice:>12} {same_order:>11}")
    print()
    print("  (the two models are the only agents when both answered that situation;")
    print("   the repeats are byte-identical so they add agents, not information)")
    print("  'same choice' compares the argmax; 'same order' compares the whole")
    print("  ranking of the shared options. The curl tracks the second, not the")
    print("  first -- it measures whether the two models lie on ONE scale, which")
    print("  is a stricter thing than whether they happen to agree on top.")

    print()
    print("=" * 78)
    print("COULD THE SHIPPED hodge-consensus RUN ON THIS GATE AT ALL?")
    print("=" * 78)
    print("hodge-consensus enumerates simple cycles by DFS with max_depth = 8")
    print("(src/decomposition.rs: dfs_cycles(..., 8)) and then projects the")
    print("residual onto each one (project_onto_cycles, O(cycles x edges)).")
    print("The attention gate has 10 options; a dense candidate graph on 10")
    print("nodes is K_10. Counting the cycles of K_10:")
    import math
    total, capped = 0, 0
    for k in range(3, 11):
        c = math.comb(10, k) * math.factorial(k - 1) // 2
        total += c
        if k <= 8:
            capped += c
    print(f"    simple cycles of length 3..10 in K_10 : {total:,}")
    print(f"    of those, length 3..8 (the DFS cap)  : {capped:,}")
    print(f"    edges in the dense candidate graph  : {10 * 9 // 2} undirected "
          f"({10 * 9} directed)")
    print(f"    projection work at O(cycles x edges) : "
          f"{capped * 90:,} inner operations, and {capped:,} cycle vectors")

    print()
    print("=" * 78)
    print("THE ESCALATION POLICY vs THE HODGE SIGNAL")
    print("=" * 78)
    print(f"Policy (D71): primary confidence < {THRESHOLD} -> ask clef-flash, use its answer.\n")
    print(f"{'roster':7} {'situation':16} {'d1 choice':10} {'clef choice':11} "
          f"{'d1 conf':>8} {'escal?':>6} {'agree':>6} {'kendall':>8}")
    disagree_no_escalation = []
    agree_but_escalated = []
    for roster in ("r8", "r4", "r2"):
        for sit in sorted({r["situation"] for r in recs}):
            a = [r for r in recs if r["roster"] == roster and r["situation"] == sit
                 and r["model"] == "d1-2b"]
            b = [r for r in recs if r["roster"] == roster and r["situation"] == sit
                 and r["model"] == "clef-flash"]
            if not a or not b:
                continue
            a, b = a[0], b[0]
            esc = a["confidence"] < THRESHOLD
            same = a["choice"] == b["choice"]
            tau = rank_agreement(a["probs"], b["probs"],
                                  sorted(set(a["options"]) & set(b["options"])))
            print(f"{roster:7} {sit:16} {a['choice']:10} {b['choice']:11} "
                  f"{a['confidence']:8.3f} {'YES' if esc else 'no':>6} "
                  f"{'yes' if same else 'NO':>6} "
                  f"{('%.2f' % tau) if tau is not None else '  n/a':>8}")
            if not same and not esc:
                disagree_no_escalation.append((roster, sit, a["choice"], b["choice"], a["confidence"]))
            if same and esc:
                agree_but_escalated.append((roster, sit, a["choice"], a["confidence"]))

    n = len({(r["roster"], r["situation"]) for r in recs})
    print(f"\nSituations compared: {n}")
    print(f"MODELS DISAGREE but the policy does NOT escalate: {len(disagree_no_escalation)}")
    for x in disagree_no_escalation:
        print(f"    {x[0]} {x[1]}: d1 says {x[2]}, clef says {x[3]}, d1 confidence {x[4]:.3f}")
    print(f"MODELS AGREE but the policy escalates anyway (a wasted second call): {len(agree_but_escalated)}")
    for x in agree_but_escalated:
        print(f"    {x[0]} {x[1]}: both say {x[2]}, d1 confidence {x[3]:.3f}")

    print("\nConfidence distribution (the escalation threshold is "
          f"{THRESHOLD}):")
    for m in ("d1-2b", "clef-flash"):
        cs = sorted(r["confidence"] for r in recs if r["model"] == m)
        below = sum(1 for c in cs if c < THRESHOLD)
        print(f"    {m:11} n={len(cs):3} min={cs[0]:.3f} median={cs[len(cs)//2]:.3f} "
              f"max={cs[-1]:.3f}  below {THRESHOLD}: {below}/{len(cs)}")

    print()
    print("=" * 78)
    print("CORPUS B -- the repo's own logged Thor answers (E4 poc)")
    print("=" * 78)
    p = os.path.join(REPO, "docs", "research", "poc", "thor", "e4-d1-3B-log.json")
    logs = json.load(open(p))
    print(f"{p}: {len(logs)} logged scenes")
    for gate in sorted({g for l in logs for g in l["answers"]}):
        obs, sets = [], []
        for l in logs:
            a = l["answers"].get(gate)
            if not a or a.get("type") != "choice":
                continue
            obs.append({"probs": {k: float(v) for k, v in a["probabilities"].items()}})
            sets.append(sorted(a["probabilities"].keys()))
        if not obs:
            continue
        r = analyse(obs, sets, label=gate)
        print(f"  gate '{gate}': n={len(obs):3}  {fmt(r)}")
        worst1 = max(abs(analyse([o], [s])["curl_energy"]) for o, s in zip(obs, sets))
        print(f"      max curl over the {len(obs)} single answers taken alone: {worst1:.3e}")


if __name__ == "__main__":
    main()

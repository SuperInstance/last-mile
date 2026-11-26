#!/usr/bin/env python3
"""Is the confidence threshold the best escalation trigger?

The shipped policy (D71) escalates when the primary model's confidence is
below 0.5. The confidence is the largest normalised share of the option
distribution (crates/lm-agents/src/sysone.rs, derived_confidence) -- or
whatever number the model itself reports.

This asks a narrower question than the Hodge one: among the statistics you
can compute from the PRIMARY answer alone (so before you spend a second call),
which one best predicts that the second model will answer differently?

The label is "the two models chose different options", taken from the capture
in attention_capture.json. Nothing here needs the second model's answer at
decision time; the label is only used to score the triggers afterwards.
"""

import itertools
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def stats(probs):
    """The one-call statistics, all computed from the primary answer alone."""
    ps = sorted(probs.values(), reverse=True)
    n = len(ps)
    top = ps[0]
    second = ps[1] if n > 1 else 0.0
    total = sum(ps)
    p = [x / total for x in ps] if total > 0 else ps
    H = -sum(x * math.log(x) for x in p if x > 0)
    Hmax = math.log(n) if n > 1 else 1.0
    return {
        "confidence": top,                       # the shipped one (max share)
        "margin": top - second,                 # gap between the top two
        "norm_entropy": H / Hmax if Hmax > 0 else 0.0,
        "eff_options": math.exp(H),             # how many options are live
        "top_minus_rest": top - (sum(ps[1:]) / (n - 1)) if n > 1 else top,
    }


def load_pairs():
    recs = [r for r in json.load(open(os.path.join(HERE, "attention_capture.json")))
            if r.get("ok")]
    pairs = []
    for roster in ("r8", "r4", "r2"):
        for sit in sorted({r["situation"] for r in recs}):
            a = [r for r in recs if r["roster"] == roster and r["situation"] == sit
                 and r["model"] == "d1-2b"]
            b = [r for r in recs if r["roster"] == roster and r["situation"] == sit
                 and r["model"] == "clef-flash"]
            if not a or not b:
                continue
            a, b = a[0], b[0]
            pairs.append({
                "roster": roster, "situation": sit,
                "primary": a, "second": b,
                "disagree": a["choice"] != b["choice"],
                "stats": stats({k: float(v) for k, v in a["probs"].items()}),
            })
    return pairs


def auc(pairs, key):
    """Area under the ROC of the statistic, where 'disagree' is the positive."""
    pos = [p["stats"][key] for p in pairs if p["disagree"]]
    neg = [p["stats"][key] for p in pairs if not p["disagree"]]
    if not pos or not neg:
        return float("nan"), 0, 0
    hits = 0.0
    for a in pos:
        for b in neg:
            hits += 1.0 if a > b else (0.5 if a == b else 0.0)
    return hits / (len(pos) * len(neg)), len(pos), len(neg)


def sweep(pairs, key, direction):
    """Every threshold, with what it would have cost and caught."""
    vals = sorted({round(p["stats"][key], 6) for p in pairs})
    rows = []
    for t in vals:
        if direction == "low":
            fire = lambda s: s < t
        else:
            fire = lambda s: s > t
        tp = sum(1 for p in pairs if p["disagree"] and fire(p["stats"][key]))
        fp = sum(1 for p in pairs if not p["disagree"] and fire(p["stats"][key]))
        fn = sum(1 for p in pairs if p["disagree"] and not fire(p["stats"][key]))
        rows.append({"threshold": t, "calls": tp + fp, "tp": tp, "fp": fp, "fn": fn})
    return rows


def cheapest_full_catch(pairs, key, direction):
    """The fewest second calls that still catch every disagreement."""
    rows = sweep(pairs, key, direction)
    full = [r for r in rows if r["fn"] == 0]
    if not full:
        return None
    return min(full, key=lambda r: (r["calls"], r["threshold"]))


def frontier(pairs, key, direction, nd):
    """The call-cost frontier: the fewest calls for each number caught."""
    rows = sweep(pairs, key, direction)
    best = {}
    for r in rows:
        caught = r["tp"]
        if caught not in best or r["calls"] < best[caught]["calls"]:
            best[caught] = r
    return [best[k] for k in sorted(best)]


def main():
    pairs = load_pairs()
    print("=" * 78)
    print("IS confidence < 0.5 THE RIGHT ESCALATION TRIGGER?")
    print("=" * 78)
    n = len(pairs)
    nd = sum(1 for p in pairs if p["disagree"])
    print(f"{n} situations; the two models chose DIFFERENT options in {nd} of them.\n")

    print("Is the reported confidence the largest normalised share?")
    same = 0
    for p in pairs:
        probs = {k: float(v) for k, v in p["primary"]["probs"].items()}
        if abs(max(probs.values()) - p["primary"]["confidence"]) < 1e-9:
            same += 1
    print(f"  {same}/{n} primary answers: reported confidence == max share\n")

    print(f"{'roster':7} {'situation':16} {'disagree':>9} | "
          f"{'conf':>6} {'margin':>7} {'nEnt':>6} {'effOpt':>7}")
    for p in pairs:
        s = p["stats"]
        print(f"{p['roster']:7} {p['situation']:16} "
              f"{('YES' if p['disagree'] else 'no'):>9} | "
              f"{s['confidence']:6.3f} {s['margin']:7.3f} "
              f"{s['norm_entropy']:6.3f} {s['eff_options']:7.2f}")

    print("\nHow well does each one-call statistic separate 'the models differ'")
    print("from 'the models agree'? (AUC, 1.0 is perfect; 0.5 is a coin)")
    print("AUC is reported in the direction that PREDICTS a disagreement, so a")
    print("statistic that is high when the models agree gets 1-AUC.\n")
    print(f"{'statistic':16} {'AUC':>6} {'oriented':>9} "
          f"{'calls to catch all':>18} {'wasted':>7} {'missed':>7}")
    for key, direction in (("confidence", "low"), ("margin", "high"),
                           ("norm_entropy", "high"), ("eff_options", "high"),
                           ("top_minus_rest", "high")):
        a, np_, nn = auc(pairs, key)
        oriented = a if direction == "high" else 1.0 - a
        best = cheapest_full_catch(pairs, key, direction)
        if best is None:
            print(f"  {key:16} {oriented:6.3f} {'n/a':>9} "
                  f"{'never catches all':>18} {'-':>7} {'-':>7}")
        else:
            print(f"  {key:16} {oriented:6.3f} {'1-AUC' if direction=='low' else 'AUC':>9} "
                  f"{best['calls']:>9} of {n} (t>={best['threshold']:.3f}) "
                  f"{best['fp']:>7} {best['fn']:>7}")

    print("\nThe shipped rule, for comparison:")
    calls = sum(1 for p in pairs if p["stats"]["confidence"] < 0.5)
    tp = sum(1 for p in pairs if p["disagree"] and p["stats"]["confidence"] < 0.5)
    fp = sum(1 for p in pairs if not p["disagree"] and p["stats"]["confidence"] < 0.5)
    fn = sum(1 for p in pairs if p["disagree"] and p["stats"]["confidence"] >= 0.5)
    print(f"  confidence < 0.50 -> {calls}/{n} second calls, {tp}/{nd} disagreements "
          f"caught, {fp} wasted, {fn} missed")

    print("\nThe call-cost frontier for each statistic (fewest calls for each")
    print("number of disagreements caught):")
    for key, direction in (("confidence", "low"), ("norm_entropy", "high"),
                           ("eff_options", "high")):
        fr = frontier(pairs, key, direction, nd)
        cells = "  ".join(f"{r['tp']} caught/{r['calls']} calls" for r in fr)
        print(f"  {key:14} {cells}")

    print("\nA perfect trigger that needs no second call would be impossible:")
    print("  by construction it cannot be better than catching all "
          f"{nd} disagreements with 0 wasted calls.")


if __name__ == "__main__":
    main()

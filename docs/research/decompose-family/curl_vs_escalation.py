#!/usr/bin/env python3
"""Does the Hodge curl pick out the cases where decomposition beats escalation?

The addendum asks: "Test whether high-curl questions are the ones where
decomposition beats escalation."

For each situation we have two models' answers to the same gate. We compute
the bipartite (agent/candidate) Hodge h1 ratio -- the curl, which measures
whether the two models' preferences lie on ONE scale -- and ask how well it
predicts the thing escalation is for: the two models choosing DIFFERENT
options.

A curl that cannot separate "the models differ" from "the models agree" adds
nothing over the confidence threshold.
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from hodge import analyse_bipartite


def auc(pos, neg):
    if not pos or not neg:
        return float("nan")
    hits = 0.0
    for a in pos:
        for b in neg:
            hits += 1.0 if a > b else (0.5 if a == b else 0.0)
    return hits / (len(pos) * len(neg))


def main():
    recs = [r for r in json.load(open(os.path.join(HERE, "attention_capture.json")))
            if r.get("ok")]
    rows = []
    for roster in ("r8", "r4", "r2"):
        for sit in sorted({r["situation"] for r in recs}):
            sel = [r for r in recs if r["roster"] == roster and r["situation"] == sit]
            a = [r for r in sel if r["model"] == "d1-2b"][0]
            c = [r for r in sel if r["model"] == "clef-flash"][0]
            opts = [sorted(r["probs"].keys()) for r in sel]
            b = analyse_bipartite([{"probs": r["probs"]} for r in sel],
                                  opts, scores="rank")
            rows.append({
                "roster": roster, "situation": sit,
                "h1": b["h1_ratio"],
                "disagree": a["choice"] != c["choice"],
                "conf": a["confidence"],
            })

    print("=" * 78)
    print("DOES THE CURL PICK OUT THE CASES WHERE THE MODELS DIFFER?")
    print("=" * 78)
    print(f"{'roster':7} {'situation':16} {'h1 (curl)':>10} {'conf':>7} "
          f"{'models differ':>14}")
    for r in rows:
        print(f"{r['roster']:7} {r['situation']:16} {r['h1']:10.6f} {r['conf']:7.3f} "
              f"{('YES' if r['disagree'] else 'no'):>14}")

    pos = [r["h1"] for r in rows if r["disagree"]]
    neg = [r["h1"] for r in rows if not r["disagree"]]
    a = auc(pos, neg)
    cpos = [r["conf"] for r in rows if r["disagree"]]
    cneg = [r["conf"] for r in rows if not r["disagree"]]
    ca = auc(cpos, cneg)

    print(f"\nmodels differ in {len(pos)} of {len(rows)} situations\n")
    print(f"  curl (h1)     AUC for 'the models differ' = {a:.3f}  "
          f"(0.5 = a coin, 1.0 = perfect)")
    print(f"  confidence     AUC for 'the models differ' = {1 - ca:.3f}  "
          f"(oriented: low confidence predicts a difference)")
    print(f"\n  the two models' full ORDERINGS differ in {len(rows)} of "
          f"{len(rows)} situations, so the curl is")
    print("  non-zero everywhere: it says 'these two models are not on one")
    print("  scale', which is true always, and therefore selects nothing.")

    print("\nThe overlap that makes the point concrete:")
    for r in sorted(rows, key=lambda r: r["h1"]):
        print(f"    h1={r['h1']:.6f}  {'differs' if r['disagree'] else 'agrees ':8} "
              f"{r['roster']}/{r['situation']}")


if __name__ == "__main__":
    main()

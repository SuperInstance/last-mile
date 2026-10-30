#!/usr/bin/env python3
"""Cross-check the numbers quoted in docs/research/decompose-family.md against
the committed result files. Run from docs/research/decompose-family/.
"""
import json
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
NOTE = os.path.join(HERE, "..", "decompose-family.md")
RESULTS = os.path.join(HERE, "results.txt")
TRIGGERS = os.path.join(HERE, "escalation_results.txt")
CURL = os.path.join(HERE, "curl_vs_escalation.txt")

note = open(NOTE).read()
res = open(RESULTS).read()
trig = open(TRIGGERS).read()
curl = open(CURL).read()

checks = []


def check(name, present_in, present_in_note=None, note_text=note):
    checks.append((name, present_in, present_in_note))


# --- numbers quoted in the note that must appear in the result files -----
# The note rounds to 3 significant figures, so compare numerically.
def num(token):
    return float(token)


def close(a, b, sig=3):
    """True if a and b agree to `sig` significant figures."""
    if a == b:
        return True
    if b == 0:
        return abs(a) < 1e-30
    fa = abs(a)
    rel = abs(a - b) / max(fa, abs(b))
    return rel < 10 ** (-(sig - 1)) * 1.5


pairs = [
    ("A1 curl", "3.441098e-30", "3.44e-30", res),
    ("A2 curl", "5.283837e-31", "5.28e-31", res),
    ("A3 curl", "5.490826e-32", "5.49e-32", res),
    ("A6 max single-answer curl", "8.201452e-33", "8.20e-33", res),
    ("A4 mixed-roster curl", "1.111328e+02", "1.11e+02", res),
    ("corpus B action curl", "3.455375e-28", "3.46e-28", res),
    ("corpus B bearing curl", "3.108112e-28", "3.11e-28", res),
]
for name, full, rounded, hay in pairs:
    okv = full in hay and close(num(full), num(rounded))
    checks.append((f"{name}: {rounded} in results and matches {full}", okv, rounded in note))

for name, token, hay in [
    ("A4 h1 0.199169", "0.199169", res),
    ("A4 max circulation 9.307372", "9.307372", res),
    ("A5 h1 0.103050", "0.103050", res),
    ("A5 max circulation 4.461091", "4.461091", res),
    ("curl AUC 0.250", "0.250", curl),
    ("confidence AUC 0.714", "0.714", curl),
    ("eff_options 12 of 15", "12 of 15", trig),
]:
    checks.append((f"{name}", token in hay, token in note))

# the shipped rule: 13 of 15 calls in the results file, "13 of 15" in the note
checks.append(("shipped rule 13 of 15 calls",
               "13/15 second calls" in trig, "13 of 15" in note))
checks.append(("shipped rule caught 7/7", "7/7 disagreements caught" in trig,
               "all 7 disagreements" in note))
checks.append(("shipped rule wasted 6", "6 wasted" in trig, "6 pointless" in note))


# --- structural claims ---------------------------------------------------
recs = [r for r in json.load(open(os.path.join(HERE, "attention_capture.json"))) if r.get("ok")]
checks.append(("60 answers captured", len(recs) == 60, "60 answers" in note))
mism = 0
for i in range(0, len(recs), 2):
    if recs[i]["probs"] != recs[i + 1]["probs"]:
        mism += 1
checks.append(("all 30 repeat pairs identical", mism == 0, "30 repeat pairs" in note))

disagree = 0
for roster in ("r8", "r4", "r2"):
    for sit in sorted({r["situation"] for r in recs}):
        a = [r for r in recs if r["roster"] == roster and r["situation"] == sit and r["model"] == "d1-2b"][0]
        b = [r for r in recs if r["roster"] == roster and r["situation"] == sit and r["model"] == "clef-flash"][0]
        if a["choice"] != b["choice"]:
            disagree += 1
checks.append(("7 of 15 situations disagree", disagree == 7, "differ in 7" in note))

below = sum(1 for r in recs if r["model"] == "d1-2b" and r["confidence"] < 0.5)
checks.append(("26/30 d1-2b answers below 0.5", below == 26, "26 of 30" in note))
below_c = sum(1 for r in recs if r["model"] == "clef-flash" and r["confidence"] < 0.5)
checks.append(("30/30 clef-flash answers below 0.5 (data)", below_c == 30,
               "clef-flash" in note))

# K_10 cycle counts quoted in the note
import math
total = sum(math.comb(10, k) * math.factorial(k - 1) // 2 for k in range(3, 11))
capped = sum(math.comb(10, k) * math.factorial(k - 1) // 2 for k in range(3, 9))
checks.append(("K_10 total cycles 556,014", total == 556014, "556,014" in note))
checks.append(("K_10 capped cycles 172,974", capped == 172974, "172,974" in note))
checks.append(("projection work 15.6M", abs(capped * 90 - 15567660) < 5, "15.6 million" in note))

ok = True
for name, in_results, in_note in checks:
    status = "PASS" if (in_results and in_note) else "FAIL"
    if status == "FAIL":
        ok = False
    print(f"  [{status}] {name}   (in results: {in_results}, in note: {in_note})")
print("\nALL CHECKS PASS" if ok else "\nSOME CHECKS FAILED")
raise SystemExit(0 if ok else 1)

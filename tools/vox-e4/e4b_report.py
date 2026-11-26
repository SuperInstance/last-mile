"""E4b: the report — per voice, the word-identical subset, and late fusion.

Three things this computes that E4's `evaluate.py` does not:

1. **Per voice.** Every number E4 reported was pooled over three speakers.
   The dials are profile-relative, so the per-speaker numbers are the ones
   that say whether the protocol works for a speaker it has never heard.
2. **The word-identical subset.** Riva's styles change the words as the ears
   hear them: in 10 of the 60 (line, speaker) groups the same line comes
   back as different words in different styles ("Hold the line." heard as
   "Oh, the line." in Angry and "Halt the line." in Sad). A dials-only arm
   cannot be reading the words, but a result that only holds on the clips
   whose words changed is not a result about the dials either, so both
   subsets are reported.
3. **Late fusion.** The two channels are answered separately and combined
   afterwards by a rule, instead of being concatenated into one state.

    python tools/vox-e4/e4b_report.py --dataset evidence/vox-e4b \\
        --e4 evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path

DELIVERY_CLASSES = ["calm", "ordinary", "urgent"]
INTENT_CLASSES = ["order", "question"]


def load(path: str) -> list[dict]:
    return list(csv.DictReader(Path(path).open(encoding="utf-8")))


def accuracy(pairs: list[tuple[str, str]]) -> float:
    if not pairs:
        return 0.0
    return sum(1 for t, p in pairs if t == p) / len(pairs)


def binary_urgent(pairs: list[tuple[str, str]]) -> float:
    return accuracy([("urgent" if t == "urgent" else "not",
                       ("urgent" if p == "urgent" else "not") if p else "")
                      for t, p in pairs])


def probs(row: dict) -> dict[str, float]:
    try:
        raw = json.loads(row.get("probabilities") or "{}")
    except json.JSONDecodeError:
        return {}
    return {k: float(v) for k, v in raw.items()}


def word_identical_groups(manifest: dict[str, dict],
                          transcripts: dict[str, dict]) -> set[str]:
    """The (line, speaker) groups where every style heard the same words.

    A group is the same line spoken by the same speaker in every style. When
    the styles all come back as the same words, the delivery is the only
    thing that varies; when they do not, the words vary too.
    """
    groups: dict[tuple[str, str], set[str]] = defaultdict(set)
    for path, row in manifest.items():
        heard = transcripts.get(path, {}).get("heard_text", "")
        groups[(row["text"], row["speaker"])].add(heard)
    keep = {key for key, heard in groups.items() if len(heard) == 1}
    return {path for path, row in manifest.items()
            if (row["text"], row["speaker"]) in keep}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4b")
    parser.add_argument("--e4", default="evidence/vox-e4",
                        help="the E4 evidence, for the baseline arms")
    args = parser.parse_args()

    manifest = {r["file"]: r for r in load(f"{args.dataset}/manifest.csv")
                if not r.get("error")}
    transcripts = {r["file"]: r for r in load(f"{args.dataset}/transcripts.csv")}
    results = load(f"{args.dataset}/arm_results.csv")
    e4_results = load(f"{args.e4}/arm_results.csv")

    identical_groups = word_identical_groups(manifest, transcripts)
    exact_clips = {p for p, r in transcripts.items() if r.get("exact") == "1"}
    print(f"{len(manifest)} clips; {len(identical_groups)} in word-identical "
          f"(line, speaker) groups; {len(exact_clips)} heard exactly")

    # --- index the answers ------------------------------------------------
    by_arm: dict[tuple[str, str], dict[str, dict]] = defaultdict(dict)
    for row in results + e4_results:
        by_arm[(row["arm"], row["gate"])][row["file"]] = row

    arms = sorted({r["arm"] for r in results})
    print(f"\n{'arm':22s} {'gate':18s} {'n':>4s} {'acc':>7s} {'base':>6s} "
          f"{'bin':>7s}  per voice (Mia / Jason / Aria)")
    print("-" * 104)

    summary: dict = {}
    for arm in arms:
        summary[arm] = {}
        for gate in ("urgency", "order_or_question", "pitch_end"):
            rows = [r for r in results if r["arm"] == arm and r["gate"] == gate]
            if not rows:
                continue
            if gate == "urgency":
                rows = [r for r in rows if r["truth"] in DELIVERY_CLASSES]
            pairs = [(r["truth"], r["pred"]) for r in rows]
            per_voice = {}
            for speaker in ("Mia", "Jason", "Aria"):
                sub = [(r["truth"], r["pred"]) for r in rows if r["speaker"] == speaker]
                per_voice[speaker] = round(accuracy(sub), 4)
            entry = {
                "n": len(pairs),
                "accuracy": round(accuracy(pairs), 4),
                "per_voice": per_voice,
                "answered": len([1 for _, p in pairs if p]),
            }
            if gate == "urgency":
                entry["binary_urgent_vs_not"] = round(binary_urgent(pairs), 4)
                base = 0.5
            else:
                base = 0.5
            # the word-identical subsets
            ident = [(r["truth"], r["pred"]) for r in rows
                     if r["file"] in identical_groups]
            exact = [(r["truth"], r["pred"]) for r in rows
                     if r["file"] in exact_clips]
            entry["word_identical_groups"] = {"n": len(ident),
                                              "accuracy": round(accuracy(ident), 4)}
            entry["word_identical_clips"] = {"n": len(exact),
                                             "accuracy": round(accuracy(exact), 4)}
            summary[arm][gate] = entry
            voice_text = " / ".join(f"{per_voice[s]*100:.1f}%" for s in
                                    ("Mia", "Jason", "Aria"))
            print(f"{arm:22s} {gate:18s} {len(pairs):4d} "
                  f"{accuracy(pairs)*100:6.1f}% {base*100:5.1f}% "
                  f"{entry.get('binary_urgent_vs_not', 0)*100:6.1f}%  {voice_text}")

    # --- the word-identical subsets, spelled out --------------------------
    print("\n=== the word-identical subsets (order/question) ===")
    print(f"   {len(identical_groups)} clips are in (line, speaker) groups where "
          f"every style heard the same words;")
    print(f"   {len(exact_clips)} clips were heard exactly as they were rendered.")
    for arm in arms:
        rows = [r for r in results if r["arm"] == arm and r["gate"] == "order_or_question"]
        if not rows:
            continue
        allp = [(r["truth"], r["pred"]) for r in rows]
        ident = [(r["truth"], r["pred"]) for r in rows if r["file"] in identical_groups]
        exact = [(r["truth"], r["pred"]) for r in rows if r["file"] in exact_clips]
        print(f"   {arm:22s} all {accuracy(allp)*100:5.1f}% (n={len(allp):3d})  "
              f"word-identical groups {accuracy(ident)*100:5.1f}% (n={len(ident):3d})  "
              f"heard-exactly {accuracy(exact)*100:5.1f}% (n={len(exact):3d})")

    # --- the confusion that shows a constant answer -----------------------
    print("\n=== constant-answer check (the E4 dials arms answered one way) ===")
    for arm in arms:
        for gate in ("urgency", "order_or_question"):
            rows = [r for r in results if r["arm"] == arm and r["gate"] == gate]
            if not rows:
                continue
            counts: dict[str, int] = defaultdict(int)
            for r in rows:
                counts[r["pred"] or "(none)"] += 1
            top, n = max(counts.items(), key=lambda kv: kv[1])
            print(f"  {arm:22s} {gate:18s} {top}: {n}/{len(rows)} "
                  f"({n/len(rows)*100:.1f}%)")

    # --- late fusion -------------------------------------------------------
    print("\n=== late fusion: two gate answers, combined by a rule ===")
    print("   (the words gate and the dials gate are asked separately)")
    fusion_arms = [("l2-v3", "words"), ("l2-v3+words", "words")]
    for dials_arm in ("l2-v3", "l2-v3-num", "l2-v3-cls", "l2-v3-vocab"):
        for gate in ("urgency", "order_or_question"):
            dials = by_arm.get((dials_arm, gate))
            words = by_arm.get(("words", gate))
            if not dials or not words:
                continue
            shared = sorted(set(dials) & set(words))
            if gate == "urgency":
                shared = [f for f in shared
                          if manifest[f]["style"] in ("Calm", "Neutral", "Angry")]
            if not shared:
                continue
            t = [manifest[f]["intent"] if gate == "order_or_question"
                 else {"Calm": "calm", "Neutral": "ordinary",
                       "Angry": "urgent"}[manifest[f]["style"]] for f in shared]

            def decide(f: str, rule: str) -> str:
                pd = probs(dials[f])
                pw = probs(words[f])
                classes = DELIVERY_CLASSES if gate == "urgency" else INTENT_CLASSES
                if rule == "route":
                    src = pw if gate == "order_or_question" else pd
                elif rule == "maxconf":
                    cd = max(pd.values()) if pd else 0.0
                    cw = max(pw.values()) if pw else 0.0
                    src = pd if cd >= cw else pw
                elif rule == "product":
                    src = {c: pd.get(c, 0.0) * pw.get(c, 0.0) for c in classes}
                elif rule.startswith("blend"):
                    w = float(rule.split(":")[1])
                    src = {c: w * pd.get(c, 0.0) + (1 - w) * pw.get(c, 0.0)
                           for c in classes}
                else:
                    raise ValueError(rule)
                if not src:
                    return ""
                return max(src, key=lambda c: src[c])

            for rule in ("route", "maxconf", "product", "blend:0.5",
                         "blend:0.7", "blend:0.3"):
                pairs = [(truth, decide(f, rule))
                         for truth, f in zip(t, shared)]
                line = (f"  {dials_arm:12s} {gate:18s} {rule:10s} "
                        f"n={len(pairs):3d} acc {accuracy(pairs)*100:5.1f}%")
                if gate == "urgency":
                    line += f"  binary {binary_urgent(pairs)*100:5.1f}%"
                print(line)

    # --- what each channel is worth on its own ----------------------------
    print("\n=== the two channels, for reference ===")
    for arm in ("words", "l2", "l2-v3"):
        for gate in ("urgency", "order_or_question"):
            rows = [r for r in (results + e4_results)
                    if r["arm"] == arm and r["gate"] == gate]
            if not rows:
                continue
            if gate == "urgency":
                rows = [r for r in rows if r["truth"] in DELIVERY_CLASSES]
            pairs = [(r["truth"], r["pred"]) for r in rows]
            line = f"  {arm:10s} {gate:18s} acc {accuracy(pairs)*100:5.1f}%"
            if gate == "urgency":
                line += f"  binary {binary_urgent(pairs)*100:5.1f}%"
            print(line)

    Path(f"{args.dataset}/e4b_report.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nwrote {args.dataset}/e4b_report.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

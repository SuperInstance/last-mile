"""E6(a) and E6(b): do the marks agree with LFM3, and with the intended label?

Reads the marks (from ``e6_marks.py``) and LFM3's per-family answers (from
``e6_lfm3_families.py``) and reports, **per family**:

(a) the agreement with LFM3-Audio's own answer about the same clip, with
    Cohen's kappa against the chance agreement two independent raters would
    reach by chance; and
(b) the agreement with the clip's **intended label** — the Riva style that
    was rendered, read through an explicit mapping from style to mark, and
    the corpus's order/question label for the contour family.

A family that lands below chance is reworked or dropped, per design §11.

    python tools/vox-e4/e6_agreement.py --marks evidence/vox-e6/marks.csv \
        --lfm3 evidence/vox-e6/lfm3_families.csv --out evidence/vox-e6
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref.marks import FAMILIES, FAMILY_MARKS  # noqa: E402

#: The intended label: what each Riva style means, read in the marks' own
#: vocabulary. This table is a *stated expectation* — it comes from the
#: plain meaning of the style names, not from the measurements — so that the
#: marks can be scored against something they were not fitted to.
STYLE_TO_MARK: dict[str, dict[str, str]] = {
    "Angry":   {"dynamics": "shouted", "articulation": "pressed",
                "contour": "falling", "pace": "rushed", "colour": "bright"},
    "Happy":   {"dynamics": "strong",  "articulation": "pressed",
                "contour": "lifting", "pace": "brisk",  "colour": "bright"},
    "Calm":    {"dynamics": "soft",    "articulation": "drawn",
                "contour": "flat",    "pace": "measured", "colour": "dark"},
    "Neutral": {"dynamics": "level",   "articulation": "even",
                "contour": "flat",    "pace": "regular", "colour": "plain"},
    "Default": {"dynamics": "level",   "articulation": "even",
                "contour": "falling", "pace": "regular", "colour": "plain"},
    "Sad":     {"dynamics": "soft",    "articulation": "drawn",
                "contour": "flat",    "pace": "measured", "colour": "plain"},
}

#: The E4/E4b delivery vocabulary, and the marks it implies for dynamics.
#: This is the *clean* label: the corpus's own calm/ordinary/urgent classes.
DELIVERY_TO_DYNAMICS = {
    "calm": {"soft", "hushed"},
    "ordinary": {"level"},
    "urgent": {"strong", "shouted"},
}

STYLE_TO_DELIVERY = {
    "Calm": "calm",
    "Neutral": "ordinary",
    "Angry": "urgent",
}


def cohens_kappa(pairs: list[tuple[str, str]]) -> float:
    """Cohen's kappa over the clips both raters answered."""
    if not pairs:
        return 0.0
    n = len(pairs)
    first = Counter(a for a, _ in pairs)
    second = Counter(b for _, b in pairs)
    observed = sum(1 for a, b in pairs if a == b) / n
    expected = sum((first[k] / n) * (second[k] / n) for k in set(first) | set(second))
    if expected >= 1.0:
        return 0.0
    return (observed - expected) / (1.0 - expected)


def wilson_interval(successes: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """A 95% Wilson interval, for a proportion near the ends."""
    if n == 0:
        return (0.0, 0.0)
    p = successes / n
    denominator = 1.0 + z * z / n
    centre = (p + z * z / (2 * n)) / denominator
    margin = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / denominator
    return (max(0.0, centre - margin), min(1.0, centre + margin))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--marks", default="evidence/vox-e6/marks.csv")
    parser.add_argument("--lfm3", default="evidence/vox-e6/lfm3_families.csv")
    parser.add_argument("--out", default="evidence/vox-e6")
    args = parser.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    marks_rows = list(csv.DictReader(Path(args.marks).open(encoding="utf-8")))
    by_file = {row["file"]: row for row in marks_rows}

    lfm3_rows = list(csv.DictReader(Path(args.lfm3).open(encoding="utf-8"))) \
        if Path(args.lfm3).exists() else []
    lfm3_by = {(row["file"], row["family"]): row for row in lfm3_rows}

    summary: dict = {"clips": len(marks_rows), "families": {}, "part_b": {}}

    # ---- (a) agreement with LFM3 -----------------------------------------
    print(f"{'family':13s} {'n':>5s} {'answered':>9s} {'agree':>7s} "
          f"{'kappa':>7s} {'chance':>7s} {'ci95':>16s}")
    print("-" * 72)
    for family in FAMILIES:
        pairs: list[tuple[str, str]] = []
        unanswered = 0
        errors = 0
        latencies = []
        for row in marks_rows:
            answer = lfm3_by.get((row["file"], family))
            if answer is None:
                continue
            if answer["lfm3"]:
                pairs.append((row[family], answer["lfm3"]))
            else:
                unanswered += 1
                if answer["error"].startswith("error"):
                    errors += 1
            if answer["latency_ms"]:
                latencies.append(float(answer["latency_ms"]))
        if not pairs:
            continue
        agree = sum(1 for a, b in pairs if a == b)
        rate = agree / len(pairs)
        kappa = cohens_kappa(pairs)
        k = len(FAMILY_MARKS[family])
        low, high = wilson_interval(agree, len(pairs))
        summary["families"][family] = {
            "n_pairs": len(pairs),
            "n_unanswered": unanswered,
            "n_errors": errors,
            "agreement": round(rate, 4),
            "agreement_ci95": [round(low, 4), round(high, 4)],
            "cohens_kappa": round(kappa, 4),
            "chance_uniform": round(1.0 / k, 4),
            "vocabulary_size": k,
            "mark_distribution": dict(Counter(a for a, _ in pairs)),
            "lfm3_distribution": dict(Counter(b for _, b in pairs)),
            "confusion": {a: dict(Counter(b for x, b in pairs if x == a))
                          for a in sorted({x for x, _ in pairs})},
            "median_latency_ms": round(sorted(latencies)[len(latencies)//2], 1)
                                if latencies else 0.0,
            "by_prompt_style": {},
        }
        for style in sorted({answer.get("prompt_style") or "reasoned"
                             for answer in (lfm3_by.get((r["file"], family))
                                            for r in marks_rows)
                             if answer}):
            style_pairs = [(r[family], lfm3_by[(r["file"], family)]["lfm3"])
                           for r in marks_rows
                           if (r["file"], family) in lfm3_by
                           and lfm3_by[(r["file"], family)]["lfm3"]
                           and (lfm3_by[(r["file"], family)].get("prompt_style")
                                or "reasoned") == style]
            if style_pairs:
                summary["families"][family]["by_prompt_style"][style] = {
                    "n": len(style_pairs),
                    "agreement": round(
                        sum(1 for a, b in style_pairs if a == b) / len(style_pairs), 4),
                }
        print(f"{family:13s} {len(pairs):5d} {len(pairs):4d}/{len(pairs)+unanswered:<4d} "
              f"{rate*100:6.1f}% {kappa:+7.3f} {1.0/k*100:6.1f}% "
              f"[{low*100:.1f},{high*100:.1f}]")

    # ---- (b) agreement with the intended label ---------------------------
    print()
    print(f"{'family':13s} {'n':>5s} {'agree':>7s} {'kappa':>7s}  by style")
    print("-" * 72)
    for family in FAMILIES:
        pairs = []
        by_style: dict[str, list[int]] = defaultdict(list)
        for row in marks_rows:
            expected = STYLE_TO_MARK.get(row["style"], {}).get(family)
            if expected is None:
                continue
            pairs.append((row[family], expected))
            by_style[row["style"]].append(
                1 if row[family] == expected else 0)
        if not pairs:
            continue
        agree = sum(1 for a, b in pairs if a == b)
        rate = agree / len(pairs)
        kappa = cohens_kappa(pairs)
        low, high = wilson_interval(agree, len(pairs))
        summary["part_b"][family] = {
            "n": len(pairs),
            "agreement": round(rate, 4),
            "agreement_ci95": [round(low, 4), round(high, 4)],
            "cohens_kappa": round(kappa, 4),
            "chance_uniform": round(1.0 / len(FAMILY_MARKS[family]), 4),
            "by_style": {style: {"n": len(v),
                                 "agreement": round(sum(v)/len(v), 4)}
                         for style, v in sorted(by_style.items())},
        }
        detail = "  ".join(f"{s}={sum(v)}/{len(v)}" for s, v in sorted(by_style.items()))
        print(f"{family:13s} {len(pairs):5d} {rate*100:6.1f}% {kappa:+7.3f}  {detail}")

    # ---- (b2) the clean delivery label, for dynamics ---------------------
    print()
    print("dynamics vs the corpus's own delivery class (Calm/Neutral/Angry only):")
    delivery_pairs = []
    for row in marks_rows:
        delivery = STYLE_TO_DELIVERY.get(row["style"])
        if delivery is None:
            continue
        expected = DELIVERY_TO_DYNAMICS[delivery]
        delivery_pairs.append((row["dynamics"], delivery, expected))
    agree = sum(1 for mark, _, expected in delivery_pairs if mark in expected)
    rate = agree / len(delivery_pairs)
    per_class = defaultdict(lambda: [0, 0])
    for mark, delivery, expected in delivery_pairs:
        per_class[delivery][1] += 1
        if mark in expected:
            per_class[delivery][0] += 1
    print(f"  n={len(delivery_pairs)}  agreement={rate*100:.1f}%  "
          f"kappa={cohens_kappa([(m, d) for m, d, _ in delivery_pairs]):+.3f}")
    for delivery, (hits, total) in sorted(per_class.items()):
        print(f"    {delivery:9s} {hits:3d}/{total:3d} = {hits/total*100:.1f}%")
    summary["part_b"]["dynamics_vs_delivery"] = {
        "n": len(delivery_pairs),
        "agreement": round(rate, 4),
        "by_class": {k: {"n": v[1], "agreement": round(v[0]/v[1], 4)}
                     for k, v in sorted(per_class.items())},
    }

    # ---- (b3) contour against the order/question label -------------------
    print()
    print("contour against the order/question label:")
    read = {"falling": "order", "questioning": "question",
            "lifting": "question", "rise-fall": "question", "flat": "order"}
    pairs = [(read[row["contour"]], row["intent"]) for row in marks_rows]
    agree = sum(1 for a, b in pairs if a == b)
    print(f"  every mark read as a speech act: {agree}/{len(pairs)} = "
          f"{agree/len(pairs)*100:.1f}%")
    strict = [(read[row["contour"]], row["intent"]) for row in marks_rows
              if row["contour"] in ("falling", "questioning")]
    strict_agree = sum(1 for a, b in strict if a == b)
    print(f"  only the decisive marks (falling/questioning): {strict_agree}/"
          f"{len(strict)} = {strict_agree/len(strict)*100:.1f}%")
    carry = {}
    for mark in FAMILY_MARKS["contour"]:
        rows = [row for row in marks_rows if row["contour"] == mark]
        if rows:
            carry[mark] = {
                "n": len(rows),
                "question_share": round(
                    sum(1 for r in rows if r["intent"] == "question")/len(rows), 4),
            }
    for mark, data in sorted(carry.items(), key=lambda kv: -kv[1]["n"]):
        print(f"    {mark:11s} n={data['n']:3d} question-share={data['question_share']:.3f}")
    summary["part_b"]["contour_vs_intent"] = {
        "all_marks_read_as_speech_act": round(agree/len(pairs), 4),
        "decisive_marks_only": round(strict_agree/len(strict), 4) if strict else 0.0,
        "decisive_n": len(strict),
        "carry": carry,
    }

    (out_dir / "agreement_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nwrote {out_dir/'agreement_summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Turn the E4 results into the markdown tables used in the report.

    python tools/vox-e4/report_table.py --dataset evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter

ARM_ORDER = ["l2-abs", "l2", "lfm3", "words", "l2-abs+words", "l2+words",
             "oracle-words"]
ARM_LABEL = {
    "l2-abs": "L2 dials only (absolute)",
    "l2": "L2 dials only (profile-relative)",
    "lfm3": "LFM3 only (audio, no gate)",
    "words": "words only",
    "l2-abs+words": "L2 (absolute) + words",
    "l2+words": "L2 (profile-relative) + words",
    "oracle-words": "oracle words (rendered text)",
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    args = parser.parse_args()

    summary = json.load(open(f"{args.dataset}/summary.json", encoding="utf-8"))
    rows = list(csv.DictReader(open(f"{args.dataset}/arm_results.csv",
                                    encoding="utf-8")))

    print("### E4 results\n")
    print("| arm | gate | n | accuracy | chance | urgent-vs-not | median latency |")
    print("|---|---|---|---|---|---|---|")
    for arm in ARM_ORDER:
        if arm not in summary["arms"]:
            continue
        for gate, label, chance in (("urgency", "urgency", "33.3%"),
                                    ("order_or_question", "order/question", "50.0%")):
            s = summary["arms"][arm][gate]
            binary = s.get("binary_urgent_vs_not")
            binary_text = (f"{binary['accuracy']*100:.1f}%" if binary else "—")
            print(f"| {ARM_LABEL[arm]} | {label} | {s['n']} | "
                  f"**{s['accuracy']*100:.1f}%** | {chance} | {binary_text} | "
                  f"{s['median_latency_ms']:.0f} ms |")

    print("\n### Confusion matrices\n")
    for arm in ARM_ORDER:
        if arm not in summary["arms"]:
            continue
        for gate in ("urgency", "order_or_question"):
            s = summary["arms"][arm][gate]
            print(f"**{ARM_LABEL[arm]} — {gate}**\n")
            matrix = s["confusion"]
            classes = list(matrix.keys())
            print("| true \\ predicted | " + " | ".join(classes) + " |")
            print("|---" * (len(classes) + 1) + "|")
            for truth in classes:
                cells = " | ".join(str(matrix[truth][pred]) for pred in classes)
                print(f"| **{truth}** | {cells} |")
            print()

    print("### ASR accuracy by style\n")
    for style, v in summary["asr_exact_by_style"].items():
        print(f"* `{style}`: {v['exact']}/{v['n']} ({v['rate']*100:.1f}%)")
    print()
    print(f"overall: {summary['totals']['asr_exact_overall']*100:.1f}% exact "
          f"over {summary['totals']['clips']} clips")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

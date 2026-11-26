"""Remove duplicate answers from the E4 results table.

The gate arms are deterministic, so a re-run produces identical rows; the
only reason duplicates appear is that a results file was extended with a
second arm. Keeping one row per (arm, gate, clip) keeps the counts honest.
"""

from __future__ import annotations

import argparse
import csv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    args = parser.parse_args()
    path = f"{args.dataset}/arm_results.csv"

    rows = list(csv.DictReader(open(path, encoding="utf-8")))
    print(f"before: {len(rows)} rows")

    seen: dict[tuple[str, str, str], str] = {}
    kept = []
    disagreements = 0
    for row in rows:
        key = (row["arm"], row["gate"], row["file"])
        if key in seen:
            if seen[key] != row["pred"]:
                disagreements += 1
                print(f"  DISAGREEMENT on {key}: {seen[key]!r} vs {row['pred']!r}")
            continue
        seen[key] = row["pred"]
        kept.append(row)

    print(f"after: {len(kept)} rows, {disagreements} disagreements")
    with open(path, "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(kept)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

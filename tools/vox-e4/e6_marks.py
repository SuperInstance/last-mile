"""E6 step 1: the marks for every clip in the E4 corpus.

Quantizes the marks layer (design §11) from the measured, profile-relative L2
fields, using the same speaker profiles the E4b L2 line was measured against,
so the marks and the line are two encodings of *the same measurements* and
the comparison in ``e6_gate.py`` is like for like.

Also re-renders the v3 L2 line from the analyzer and checks it against
``evidence/vox-e4b/l2.csv``: if the analyzer still reproduces E4b's line
exactly, the marks sit on the same measurement.

    python tools/vox-e4/e6_marks.py --dataset evidence/vox-e4b \
        --clip-root ~/lm-evidence/vox-e4 --out evidence/vox-e6
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref.cli import analyze_clip  # noqa: E402
from vox_ref.marks import (FAMILIES, FAMILY_MARKS, MarkConfig,  # noqa: E402
                            marks_line, quantize_families, quantize_marks)
from vox_ref.profile import SpeakerProfile  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4b",
                        help="the dataset directory (manifest, l2.csv, profiles.json)")
    parser.add_argument("--clip-root", default=None,
                        help="prefix for the manifest's clip paths")
    parser.add_argument("--out", default="evidence/vox-e6")
    parser.add_argument("--config", default=None,
                        help="marks threshold config JSON")
    args = parser.parse_args()

    dataset = Path(args.dataset)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    clip_root = Path(args.clip_root) if args.clip_root else None

    manifest = [row for row in
                csv.DictReader((dataset / "manifest.csv").open(encoding="utf-8"))
                if not row.get("error")]
    profiles = {name: SpeakerProfile.from_json(json.dumps(data))
                for name, data in
                json.loads((dataset / "profiles.json").read_text(encoding="utf-8")).items()}
    old_l2 = {row["file"]: row for row in
              csv.DictReader((dataset / "l2.csv").open(encoding="utf-8"))}
    config = MarkConfig.load(args.config) if args.config else MarkConfig.default()

    def clip_path(row: dict) -> str:
        return str(clip_root / row["file"]) if clip_root else row["file"]

    rows = []
    mismatches = []
    started = time.perf_counter()
    for n, row in enumerate(manifest, start=1):
        profile = profiles[row["speaker"]]
        result = analyze_clip(clip_path(row), profile)
        patch = result["patch"]
        families = quantize_families(patch, profile, config)
        marks = quantize_marks(patch, profile, config)

        # The analyzer must still reproduce E4b's line exactly, or the marks
        # are being compared against a line that was never measured.
        line = patch.to_line_v3()
        reference = old_l2.get(row["file"], {}).get("l2_v3", "")
        if line != reference:
            mismatches.append({"file": row["file"], "now": line, "reference": reference})

        out = {
            "file": row["file"],
            "speaker": row["speaker"],
            "style": row["style"],
            "intent": row["intent"],
            "text": row["text"],
            "marks": marks_line(marks),
            "l2_v3": line,
        }
        for family in FAMILIES:
            out[family] = families[family]
        out.update({
            "velocity_db": patch.velocity_db,
            "tempo_syl_per_s": patch.tempo_syl_per_s,
            "duration_s": patch.duration_s,
            "contour_median_norm": patch.contour_median_norm,
            "contour_class": patch.contour_class,
            "eop_tone": patch.eop_tone,
            "eop_final_offset_ratio": patch.eop_final_offset_ratio,
            "brightness_offset_hz": patch.brightness_offset_hz,
            "decay_ratio": (patch.decay_ms / patch.profile_decay_ms
                            if patch.profile_decay_ms > 0 else 1.0),
            "sustain_offset_db": patch.sustain_db - patch.profile_sustain_db,
            "tempo_ratio": (patch.tempo_syl_per_s / profile.tempo_syl_per_s
                            if profile.tempo_syl_per_s > 0 else 1.0),
        })
        rows.append(out)
        if n % 80 == 0 or n == len(manifest):
            print(f"  {n}/{len(manifest)}")

    elapsed = time.perf_counter() - started
    fields = list(rows[0].keys())
    with (out_dir / "marks.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)

    # The vocabulary as it actually came out of the corpus.
    summary = {
        "clips": len(rows),
        "seconds_per_clip": round(elapsed / max(1, len(rows)), 4),
        "l2_v3_mismatches_vs_e4b": len(mismatches),
        "l2_v3_mismatch_examples": mismatches[:5],
        "config": config.to_dict(),
        "families": list(FAMILIES),
        "vocabulary": {name: list(FAMILY_MARKS[name]) for name in FAMILIES},
        "counts": {name: {} for name in FAMILIES},
    }
    for row in rows:
        for family in FAMILIES:
            summary["counts"][family][row[family]] = (
                summary["counts"][family].get(row[family], 0) + 1)
    (out_dir / "marks_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8")

    print(f"analysed {len(rows)} clips in {elapsed:.1f}s "
          f"({elapsed/max(1,len(rows))*1000:.0f} ms per clip)")
    print(f"L2 v3 line reproduced E4b exactly on "
          f"{len(rows) - len(mismatches)}/{len(rows)} clips")
    print(f"wrote {out_dir/'marks.csv'}")
    print()
    for family in FAMILIES:
        counts = summary["counts"][family]
        ordered = ", ".join(f"{mark}={counts.get(mark, 0)}"
                             for mark in FAMILY_MARKS[family])
        print(f"  {family:13s} {ordered}")
    print()
    for row in rows[:5]:
        print(f"  {row['style']:8s} {row['intent']:8s} {row['marks']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

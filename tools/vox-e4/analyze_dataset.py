"""Analyse every clip in the E4 dataset and write the L2 lines.

Also builds the per-speaker profiles: the calibration the design calls a
speaker profile (design §7). Everything the L2 line says about level and
brightness is measured against that profile, so "loud" means loud *for this
speaker*.

Two L2 lines are written per clip:

  l2_abs      the design's format as first implemented: envelope classes come
              from fixed constants (v1)
  l2_rel      the same measurement with every class made relative to the
              speaker's own profile (v2)

The difference between them is one of the findings of E4, so both are kept.

A third line is written per clip:

  l2_v3       the revised line (E4b): the fields that earned their place on
              the corpus, profile-relative throughout, plus the end of the
              phrase (the boundary tone and the measurements around it).

    python tools/vox-e4/analyze_dataset.py --dataset evidence/vox-e4
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
from vox_ref.l2 import (V3_FIELDS_CLASS, V3_FIELDS_NO_CONTOUR,  # noqa: E402
                        V3_FIELDS_NUM)
from vox_ref.profile import build_profile  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    parser.add_argument("--out", default=None,
                        help="write l2.csv and profiles.json here "
                             "(default: the dataset directory)")
    parser.add_argument("--clip-root", default=None,
                        help="prefix for the manifest's clip paths, for when "
                             "the audio lives outside the repository")
    parser.add_argument("--extended", action="store_true",
                        help="write the extended L2 line")
    args = parser.parse_args()

    dataset = Path(args.dataset)
    out_dir = Path(args.out) if args.out else dataset
    out_dir.mkdir(parents=True, exist_ok=True)
    clip_root = Path(args.clip_root) if args.clip_root else None

    manifest = list(csv.DictReader((dataset / "manifest.csv").open(encoding="utf-8")))
    manifest = [row for row in manifest if not row.get("error")]
    print(f"{len(manifest)} clips to analyse")

    def clip_path(row: dict) -> str:
        if clip_root is None:
            return row["file"]
        return str(clip_root / row["file"])

    # First pass: no profile, so every clip is measured on its own terms.
    results = []
    started = time.perf_counter()
    for n, row in enumerate(manifest, start=1):
        results.append((row, analyze_clip(clip_path(row))))
        if n % 80 == 0 or n == len(manifest):
            print(f"  {n}/{len(manifest)} analysed")
    elapsed = time.perf_counter() - started

    # Per-speaker profiles, from that speaker's own clips.
    by_speaker: dict[str, list] = {}
    tempos: dict[str, list[float]] = {}
    gestures_by_speaker: dict[str, list] = {}
    for row, result in results:
        by_speaker.setdefault(row["speaker"], []).append(result["frames"])
        tempos.setdefault(row["speaker"], []).append(result["patch"].tempo_syl_per_s)
        gestures_by_speaker.setdefault(row["speaker"], []).append(result["gestures"])
    patches_by_speaker: dict[str, list] = {}
    for row, result in results:
        patches_by_speaker.setdefault(row["speaker"], []).append(result["patch"])

    profiles = {}
    for speaker, clips in by_speaker.items():
        profile = build_profile(clips, tempos[speaker], patches_by_speaker[speaker],
                                gestures_by_speaker[speaker])
        profiles[speaker] = profile
        print(f"profile {speaker}: ref={profile.reference_hz:.1f} Hz "
              f"peak={profile.level_peak_db:.1f} dB centroid={profile.centroid_hz:.0f} Hz "
              f"tempo={profile.tempo_syl_per_s:.2f}/s "
              f"decay={profile.decay_median_ms:.0f} ms "
              f"release={profile.release_median_ms:.0f} ms "
              f"gesture={profile.gesture_median_ms:.0f} ms "
              f"final_gesture={profile.final_gesture_median_ms:.0f} ms")

    (out_dir / "profiles.json").write_text(
        json.dumps({k: json.loads(v.to_json()) for k, v in profiles.items()}, indent=2),
        encoding="utf-8",
    )

    # Second pass: with the profile in place, vel and bright are relative.
    rows = []
    for row, _ in results:
        profile = profiles[row["speaker"]]
        result = analyze_clip(clip_path(row), profile)
        patch = result["patch"]
        out = {
            "file": row["file"],
            "speaker": row["speaker"],
            "style": row["style"],
            "intent": row["intent"],
            "text": row["text"],
            "gestures": len(result["gestures"]),
        }
        out.update(patch.as_row())
        out["l2_abs"] = patch.to_line(extended=args.extended, relative=False)
        out["l2_rel"] = patch.to_line(extended=args.extended, relative=True)
        out["l2"] = out["l2_rel"]
        out["l2_v3"] = patch.to_line_v3()
        # The three encodings of the same fields, and the control that
        # leaves the pitch-movement fields out.
        out["l2_v3_num"] = patch.to_line_v3(fields=V3_FIELDS_NUM)
        out["l2_v3_cls"] = patch.to_line_v3(fields=V3_FIELDS_CLASS)
        out["l2_v3_nocontour"] = patch.to_line_v3(fields=V3_FIELDS_NO_CONTOUR)
        rows.append(out)

    fields = list(rows[0].keys())
    with (out_dir / "l2.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)

    print(f"analysed {len(rows)} clips in {elapsed:.1f}s "
          f"({elapsed/max(1,len(rows))*1000:.0f} ms per clip)")
    print(f"wrote {out_dir/'l2.csv'} and {out_dir/'profiles.json'}")
    print("\n  v1 (absolute):")
    for row in rows[:3]:
        print("   ", row["l2_abs"])
    print("  v2 (profile-relative):")
    for row in rows[:3]:
        print("   ", row["l2_rel"])
    print("  v3 (revised):")
    for row in rows[:3]:
        print("   ", row["l2_v3"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

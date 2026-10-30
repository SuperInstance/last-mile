"""Command line for the Vox Score reference analyzer.

    python -m vox_ref analyze clip.wav [--profile p.json] [--extended]
    python -m vox_ref profile a.wav b.wav ... --out p.json
    python -m vox_ref batch clips/ --csv patches.csv --l0-dir l0/
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

from .l0 import analyze_l0, frames_to_csv
from .l1 import apply_profile, segment_gestures
from .l2 import summarize
from .profile import SpeakerProfile, build_profile
from .wavio import read_wav


def analyze_clip(path: str | Path, profile: SpeakerProfile | None = None) -> dict:
    """Analyse one clip and return the L0 frames, L1 gestures and L2 patch."""
    x, sr = read_wav(path)
    reference_hz = profile.reference_hz if profile else 0.0
    frames = analyze_l0(x, sr, reference_hz=reference_hz)
    gestures = segment_gestures(frames, sr)
    level_peak = profile.level_peak_db if profile else -60.0
    centroid = profile.centroid_hz if profile else 0.0
    apply_profile(gestures, level_peak)
    patch = summarize(
        frames,
        gestures,
        sr,
        profile_reference_hz=reference_hz,
        profile_level_peak_db=level_peak,
        profile_centroid_hz=centroid,
        profile=profile,
    )
    return {
        "path": str(path),
        "sr": sr,
        "frames": frames,
        "gestures": gestures,
        "patch": patch,
    }


def _load_profile(args) -> SpeakerProfile | None:
    if getattr(args, "profile", None):
        return SpeakerProfile.load(args.profile)
    return None


def cmd_analyze(args) -> int:
    profile = _load_profile(args)
    for path in args.wavs:
        result = analyze_clip(path, profile)
        patch = result["patch"]
        print(patch.to_line(extended=args.extended, relative=args.relative))
        if args.json:
            payload = {
                "path": str(path),
                "l2": patch.to_line(extended=args.extended, relative=args.relative),
                "patch": patch.as_row(),
                "gestures": [g.as_row() for g in result["gestures"]],
            }
            print(json.dumps(payload), file=sys.stderr)
        if args.l0_csv:
            out = Path(args.l0_csv)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(frames_to_csv(result["frames"]), encoding="utf-8")
    return 0


def cmd_profile(args) -> int:
    profile = _load_profile(args)
    clips = []
    tempos = []
    for path in args.wavs:
        result = analyze_clip(path, profile)
        clips.append(result["frames"])
        tempos.append(result["patch"].tempo_syl_per_s)
    built = build_profile(clips, tempos)
    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        built.save(args.out)
        print(built.to_json(), end="")
    else:
        print(built.to_json(), end="")
    return 0


def cmd_batch(args) -> int:
    profile = _load_profile(args)
    paths = sorted(Path(args.directory).rglob("*.wav"))
    if args.pattern:
        paths = [p for p in paths if args.pattern in p.name]
    rows = []
    for path in paths:
        result = analyze_clip(path, profile)
        patch = result["patch"]
        row = {"path": str(path),
               "l2": patch.to_line(extended=args.extended, relative=args.relative)}
        row.update(patch.as_row())
        rows.append(row)
        if args.l0_dir:
            out_dir = Path(args.l0_dir)
            out_dir.mkdir(parents=True, exist_ok=True)
            (out_dir / (path.stem + ".l0.csv")).write_text(
                frames_to_csv(result["frames"]), encoding="utf-8"
            )
    if args.csv:
        out = Path(args.csv)
        out.parent.mkdir(parents=True, exist_ok=True)
        if rows:
            with out.open("w", newline="", encoding="utf-8") as handle:
                writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
                writer.writeheader()
                writer.writerows(rows)
        print(f"{len(rows)} clips -> {out}", file=sys.stderr)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="vox_ref", description="Vox Score reference analyzer")
    sub = parser.add_subparsers(dest="command", required=True)

    analyze = sub.add_parser("analyze", help="analyse one or more clips")
    analyze.add_argument("wavs", nargs="+")
    analyze.add_argument("--profile", help="speaker profile JSON")
    analyze.add_argument("--extended", action="store_true", help="extra L2 detail")
    analyze.add_argument("--relative", action="store_true",
                          help="profile-relative L2 classes (v2)")
    analyze.add_argument("--json", action="store_true", help="also emit JSON on stderr")
    analyze.add_argument("--l0-csv", help="write the L0 frames to this CSV")
    analyze.set_defaults(func=cmd_analyze)

    profile = sub.add_parser("profile", help="build a speaker profile from clips")
    profile.add_argument("wavs", nargs="+")
    profile.add_argument("--out", help="write the profile JSON here")
    profile.set_defaults(func=cmd_profile)

    batch = sub.add_parser("batch", help="analyse a directory of clips")
    batch.add_argument("directory")
    batch.add_argument("--profile", help="speaker profile JSON")
    batch.add_argument("--csv", help="write the patch summary CSV here")
    batch.add_argument("--l0-dir", help="write per-clip L0 CSVs here")
    batch.add_argument("--pattern", help="only clips whose name contains this")
    batch.add_argument("--extended", action="store_true")
    batch.add_argument("--relative", action="store_true")
    batch.set_defaults(func=cmd_batch)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())

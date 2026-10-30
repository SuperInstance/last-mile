"""E4b: which L2 fields earn their place, and which carry the intent.

The question this answers is not "does the measurement work" — E4 already
answered that — but "which fields should the L2 line carry". A field earns
its place when it separates the intents on its own, on speakers it has
never seen.

Two things are measured for every candidate field:

  * **AUC** — how well the field alone ranks a question above an order
    (0.5 is chance). AUC is the honest measure of a single continuous
    field; accuracy would depend on a threshold picked after the fact.
  * **leave-one-speaker-out** nearest-centroid accuracy over a field set —
    what the same fields are worth when a classifier reads them directly,
    which is the ceiling the text line is measured against.

The end-of-phrase features are also swept over their window (200, 250 and
300 ms), because the brief allows any of the three and the choice should
be made on the data.

    python tools/vox-e4/e4b_select_fields.py --dataset evidence/vox-e4b \\
        --clip-root ~/lm-evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref import eop  # noqa: E402
from vox_ref.cli import analyze_clip  # noqa: E402
from vox_ref.profile import SpeakerProfile  # noqa: E402

#: Which Riva style stands for which delivery.
STYLE_TO_DELIVERY = {
    "Calm": "calm",
    "Neutral": "ordinary",
    "Angry": "urgent",
}

#: The candidate fields, and where each one comes from.
CANDIDATES = [
    "velocity_db",          # the loudness, against the profile
    "brightness_offset_hz",  # the timbre, against the profile
    "tempo_syl_per_s",
    "duration_s",
    "decay_ms",
    "sustain_db",
    "release_ms",
    "attack_ms",
    "contour_norm",         # the pitch movement, in ranges
    "contour_median_norm",  # ... measured with medians
    "eop_slope_cents_per_s",  # the F0 slope over the last 250 ms
    "eop_rise_ratio",        # the final rise, in ranges
    "eop_final_offset_ratio",  # where the pitch ended, in ranges
    "eop_lengthening",       # the final lengthening
]


def auc(pos: list[float], neg: list[float]) -> float:
    """The area under the ROC curve for one field: how well it ranks.

    Ties get their average rank. Without that, a field that is zero for
    96% of the clips gets whatever AUC the row order happens to give it,
    which is not a measurement of anything.
    """
    pos = np.asarray([v for v in pos if v is not None and not np.isnan(v)])
    neg = np.asarray([v for v in neg if v is not None and not np.isnan(v)])
    if pos.size == 0 or neg.size == 0:
        return float("nan")
    allv = np.concatenate([pos, neg])
    order = np.argsort(allv, kind="mergesort")
    ranks = np.empty(allv.size, dtype=float)
    i = 0
    while i < allv.size:
        j = i
        while j + 1 < allv.size and allv[order[j + 1]] == allv[order[i]]:
            j += 1
        ranks[order[i:j + 1]] = (i + j) / 2.0 + 1.0
        i = j + 1
    u1 = ranks[: pos.size].sum()
    return float((u1 - pos.size * (pos.size + 1) / 2) / (pos.size * neg.size))


def nearest_centroid(x_tr: np.ndarray, y_tr: np.ndarray, x_te: np.ndarray) -> np.ndarray:
    cents = {c: x_tr[y_tr == c].mean(0) for c in np.unique(y_tr)}
    dists = {c: np.linalg.norm(x_te - v, axis=1) for c, v in cents.items()}
    return np.array([min(dists, key=lambda c: dists[c][i]) for i in range(len(x_te))])


def loso(x: np.ndarray, y: np.ndarray, sp: np.ndarray, mask: np.ndarray) -> tuple[float, list[float]]:
    """Leave-one-speaker-out nearest-centroid accuracy, standardised on the
    training speakers only."""
    ok = mask & ~np.isnan(x).any(1)
    per = []
    for s in sorted(set(sp)):
        te = ok & (sp == s)
        tr = ok & (sp != s)
        if not te.any() or not tr.any():
            continue
        mu = x[tr].mean(0)
        sd = x[tr].std(0)
        sd[sd == 0] = 1.0
        pred = nearest_centroid((x[tr] - mu) / sd, y[tr], (x[te] - mu) / sd)
        per.append(float((pred == y[te]).mean()))
    return (float(np.mean(per)) if per else float("nan")), per


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4b")
    parser.add_argument("--clip-root", default=None,
                        help="prefix for the manifest's clip paths")
    parser.add_argument("--out", default=None,
                        help="write the selection as JSON here")
    args = parser.parse_args()

    dataset = Path(args.dataset)
    rows = list(csv.DictReader((dataset / "l2.csv").open(encoding="utf-8")))
    manifest = {r["file"]: r for r in
                csv.DictReader((dataset / "manifest.csv").open(encoding="utf-8"))}
    profiles = {k: SpeakerProfile.from_json(json.dumps(v)) for k, v in
                json.loads((dataset / "profiles.json").read_text(encoding="utf-8")).items()}
    clip_root = Path(args.clip_root) if args.clip_root else None

    def clip_path(row: dict) -> str:
        return str(clip_root / row["file"]) if clip_root else row["file"]

    # --- the slope window sweep ------------------------------------------
    # The brief allows 200-300 ms; the analyzer's default is 250 ms.
    slope_sweep: dict[str, dict] = {}
    for window_s in (0.20, 0.25, 0.30):
        vals = []
        for row in rows:
            profile = profiles[row["speaker"]]
            res = analyze_clip(clip_path(row), profile)
            end = eop.measure(res["frames"], res["gestures"], res["sr"],
                              window_s=window_s,
                              pitch_range_cents=profile.pitch_range_cents,
                              final_gesture_median_s=profile.final_gesture_median_ms / 1000.0,
                              gesture_median_s=profile.gesture_median_ms / 1000.0)
            vals.append(end.slope_cents_per_s)
        q = [v for v, r in zip(vals, rows) if r["intent"] == "question"]
        o = [v for v, r in zip(vals, rows) if r["intent"] == "order"]
        slope_sweep[f"{int(window_s * 1000)}ms"] = {
            "auc_order_vs_question": round(auc(q, o), 4),
        }

    # --- per-field AUC ----------------------------------------------------
    def field(name: str) -> np.ndarray:
        return np.array([float(r[name]) for r in rows])

    intent = np.array([r["intent"] for r in rows])
    style = np.array([r["style"] for r in rows])
    speaker = np.array([r["speaker"] for r in rows])
    delivery = np.array([STYLE_TO_DELIVERY.get(s, "skip") for s in style])
    urgency_mask = delivery != "skip"

    y_question = (intent == "question").astype(int)
    y_delivery = np.array([{"calm": 0, "ordinary": 1, "urgent": 2}.get(d, -1)
                           for d in delivery])
    y_urgent = (delivery == "urgent").astype(int)

    per_field = {}
    for name in CANDIDATES:
        v = field(name)
        per_field[name] = {
            "auc_question_vs_order": round(auc(v[intent == "question"],
                                               v[intent == "order"]), 4),
            "auc_by_speaker": {s: round(auc(v[(intent == "question") & (speaker == s)],
                                             v[(intent == "order") & (speaker == s)]), 4)
                               for s in sorted(set(speaker))},
            "loso_urgency_3class": round(loso(v[:, None], y_delivery, speaker,
                                               urgency_mask)[0], 4),
            "loso_urgency_binary": round(loso(v[:, None], y_urgent, speaker,
                                               urgency_mask)[0], 4),
            "constant": bool(np.nanstd(v) < 1e-9),
        }

    # --- field sets --------------------------------------------------------
    def set_loso(keys: list[str], labels: np.ndarray, mask: np.ndarray) -> dict:
        x = np.column_stack([field(k) for k in keys])
        acc, per = loso(x, labels, speaker, mask)
        return {"fields": keys, "loso": round(acc, 4),
                "by_speaker": {s: round(a, 4) for s, a in zip(sorted(set(speaker)), per)}}

    sets = {
        "order_question": [
            set_loso(["contour_median_norm"], y_question, np.ones(len(rows), bool)),
            set_loso(["contour_median_norm", "eop_final_offset_ratio"], y_question,
                     np.ones(len(rows), bool)),
            set_loso(["contour_median_norm", "eop_final_offset_ratio", "eop_slope_cents_per_s"],
                     y_question, np.ones(len(rows), bool)),
            set_loso(["contour_median_norm", "eop_final_offset_ratio", "eop_rise_ratio"],
                     y_question, np.ones(len(rows), bool)),
            set_loso(["contour_median_norm", "eop_final_offset_ratio", "velocity_db",
                      "brightness_offset_hz", "tempo_syl_per_s"], y_question,
                     np.ones(len(rows), bool)),
        ],
        "urgency": [
            set_loso(["velocity_db", "brightness_offset_hz"], y_delivery, urgency_mask),
            set_loso(["velocity_db", "tempo_syl_per_s"], y_delivery, urgency_mask),
            set_loso(["velocity_db", "brightness_offset_hz", "tempo_syl_per_s",
                      "duration_s"], y_delivery, urgency_mask),
            set_loso(["velocity_db", "brightness_offset_hz", "tempo_syl_per_s",
                      "duration_s", "decay_ms", "sustain_db", "release_ms", "attack_ms"],
                     y_delivery, urgency_mask),
        ],
    }

    report = {
        "clips": len(rows),
        "slope_window_sweep": slope_sweep,
        "per_field": per_field,
        "field_sets": sets,
    }

    print(f"{len(rows)} clips")
    print("\n=== the F0 slope over the last N ms (order vs question) ===")
    for k, v in slope_sweep.items():
        print(f"  slope over {k:6s} AUC {v['auc_order_vs_question']:.3f}")

    print("\n=== per-field AUC, order vs question (0.5 = chance) ===")
    print(f"  {'field':26s} {'AUC':>6s} {'Mia':>6s} {'Jason':>6s} {'Aria':>6s} "
          f"{'urg3':>6s} {'urgbin':>7s}")
    for name in CANDIDATES:
        f = per_field[name]
        by = f["auc_by_speaker"]
        print(f"  {name:26s} {f['auc_question_vs_order']:6.3f} "
              f"{by['Mia']:6.3f} {by['Jason']:6.3f} {by['Aria']:6.3f} "
              f"{f['loso_urgency_3class']:6.3f} {f['loso_urgency_binary']:7.3f}")

    print("\n=== leave-one-speaker-out nearest centroid, field sets ===")
    for task, entries in sets.items():
        print(f"  {task}:")
        for e in entries:
            print(f"    {e['loso']:.3f}  {'+'.join(e['fields'])}")

    if args.out:
        Path(args.out).write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"\nwrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

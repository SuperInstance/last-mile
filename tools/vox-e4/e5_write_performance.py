"""E5: can the decision model write a performance?

The loop, end to end:

  1. d1-2B is given a line and a situation, and a closed menu of delivery
     presets. It picks one.
  2. The preset is rendered by skald with the matching voice style.
  3. The render is analysed again by the reference analyzer.
  4. A classifier, trained only on measured L2 features and fitted on the E4
     data, says which class the render landed in.
  5. The class is compared with the class the model intended.

A control renders the *intended* preset directly, so the round-trip
accuracy can be separated into "the model chose wrong" and "the render and
the classifier lost the class on the way".

    python tools/vox-e4/e5_write_performance.py --dataset evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
import time
import urllib.request
from collections import Counter
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref.cli import analyze_clip  # noqa: E402
from vox_ref.profile import SpeakerProfile  # noqa: E402

D1_2B = "http://localhost:8094/v1/systemone"
SKALD = "http://localhost:8000"

#: The features the classifier sees. All of them are measured L2 channels.
FEATURES = ["velocity_db", "tempo_syl_per_s", "attack_ms", "duration_s",
            "contour_change_cents", "brightness_offset_hz"]

#: The preset menu. Each preset is one delivery, and each delivery is one
#: Riva voice style, because that is the only dimension skald can render.
PRESETS = {
    "calm": {"style": "Calm", "voice": "Magpie-Multilingual.EN-US.{speaker}.Calm",
             "gloss": "quiet, measured and unhurried; for a situation with no pressure"},
    "ordinary": {"style": "Neutral", "voice": "Magpie-Multilingual.EN-US.{speaker}.Neutral",
                 "gloss": "normal pace and normal volume; for a routine situation"},
    "urgent": {"style": "Angry", "voice": "Magpie-Multilingual.EN-US.{speaker}.Angry",
               "gloss": "loud and sharp; for something that must happen right now"},
    "cheerful": {"style": "Happy", "voice": "Magpie-Multilingual.EN-US.{speaker}.Happy",
                 "gloss": "light and upbeat; for good news"},
    "grave": {"style": "Sad", "voice": "Magpie-Multilingual.EN-US.{speaker}.Sad",
             "gloss": "slow and heavy; for bad news"},
    "plain": {"style": "Default", "voice": "Magpie-Multilingual.EN-US.{speaker}",
              "gloss": "without emphasis; for a plain factual report"},
}

#: The delivery classes the brief names, and which presets belong to them.
PRESET_TO_CLASS = {"calm": "calm", "ordinary": "ordinary", "urgent": "urgent",
                   "cheerful": "ordinary", "grave": "ordinary", "plain": "ordinary"}

#: Riva style to the three delivery classes.
STYLE_TO_CLASS = {"Calm": "calm", "Neutral": "ordinary", "Angry": "urgent",
                  "Default": "ordinary", "Happy": "ordinary", "Sad": "ordinary"}

#: Twelve situations: a line to say and the context that decides how it
#: should be said. The target is the delivery the context calls for.
SCENARIOS = [
    ("Hold the line.",
     "The beacon expires in ten seconds and the squad has to move now.",
     "urgent"),
    ("Hold the line.",
     "The area is quiet, there is no contact, and the commander has the "
     "situation under control. Wait for the next order.",
     "calm"),
    ("Move to the beacon.",
     "Nothing is known about the enemy. Proceed and report.",
     "ordinary"),
    ("Move to the beacon.",
     "The beacon is about to expire. The squad has to move now.",
     "urgent"),
    ("We need that supply crate.",
     "The drop is late and the squad has no ammo left. This is the priority.",
     "urgent"),
    ("We need that supply crate.",
     "The convoy is on schedule and the crates are already counted. Confirm "
     "for the log.",
     "plain"),
    ("Wait for my signal.",
     "Radio discipline is being enforced and there is nothing to do yet.",
     "calm"),
    ("Do you see the beacon?",
     "The supply drop landed early and the squad got what it needed.",
     "cheerful"),
    ("Where is the enemy?",
     "Two of our people were lost this morning and the camp is still being "
     "cleared.",
     "grave"),
    ("Can you hear me?",
     "Nothing is happening and the channel is quiet.",
     "ordinary"),
    ("Pick up the radio.",
     "The squad is in trouble and the line is about to go hot.",
     "urgent"),
    ("Stay on the road.",
     "There is a good stretch of road ahead and no sign of trouble.",
     "cheerful"),
    ("Move to the beacon.",
     "The beacon expires in under a minute and the squad is still in the open. "
     "This is the last chance to make it.",
     "urgent"),
    ("Fall back to cover.",
     "Three hostiles are closing on the squad right now and ammo is low.",
     "urgent"),
    ("Fall back to cover.",
     "The area is secure and the squad has been told to stand down and wait "
     "for the next order.",
     "calm"),
    ("Get behind the barricade.",
     "The squad is pinned down under fire and needs cover immediately.",
     "urgent"),
    ("Get behind the barricade.",
     "The barricade is intact, the squad is in good shape and the routine "
     "check is logged.",
     "plain"),
    ("Take the east passage.",
     "The east passage is the only way out and the squad has to be through "
     "it before the corridor closes.",
     "urgent"),
    ("Take the east passage.",
     "There is nothing pressing; the east passage is simply the shortest "
     "way and the squad is not in a hurry.",
     "ordinary"),
    ("Are you in position?",
     "The drop went perfectly and the squad is calling it a good haul.",
     "cheerful"),
    ("Who has the map?",
     "A squad member was lost on the last run and the camp is still being "
     "searched.",
     "grave"),
    ("Watch the south bridge.",
     "The south bridge is about to fall and the squad has to cross now.",
     "urgent"),
    ("Watch the south bridge.",
     "The south bridge is quiet and the squad has nothing better to do.",
     "calm"),
]


def majority(values: list[str]) -> str:
    """The most common value, ties broken by first appearance."""
    counts = Counter(v for v in values if v and not v.startswith("error"))
    if not counts:
        return ""
    best = max(counts.values())
    for value in values:
        if counts.get(value) == best:
            return value
    return ""


def post_json(url: str, payload: dict, timeout: float = 120.0) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def render(voice: str, text: str, path: Path) -> bool:
    payload = {"input": text, "voice": voice, "response_format": "wav"}
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        f"{SKALD}/v1/audio/speech", data=data,
        headers={"Content-Type": "application/json"}, method="POST",
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        body = response.read()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(body)
    return True


class NearestCentroid:
    """A nearest-centroid classifier over standardised L2 features."""

    def __init__(self, centroids: dict[str, np.ndarray], mean: np.ndarray,
                 scale: np.ndarray):
        self.centroids = centroids
        self.mean = mean
        self.scale = scale

    @classmethod
    def fit(cls, rows: list[dict], label_key: str) -> "NearestCentroid":
        matrix = np.array([[float(r[f]) for f in FEATURES] for r in rows])
        mean = matrix.mean(axis=0)
        scale = matrix.std(axis=0) + 1e-9
        standardised = (matrix - mean) / scale
        centroids = {}
        for label in sorted({r[label_key] for r in rows}):
            mask = np.array([r[label_key] == label for r in rows])
            centroids[label] = standardised[mask].mean(axis=0)
        return cls(centroids, mean, scale)

    def predict(self, row: dict) -> str:
        vector = (np.array([float(row[f]) for f in FEATURES]) - self.mean) / self.scale
        return min(self.centroids,
                   key=lambda label: float(np.linalg.norm(vector - self.centroids[label])))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    parser.add_argument("--speaker", default="Mia")
    parser.add_argument("--out", default=None)
    parser.add_argument("--samples", type=int, default=3,
                        help="renders per preset; Riva is not deterministic")
    args = parser.parse_args()
    dataset = Path(args.dataset)

    l2_rows = list(csv.DictReader((dataset / "l2.csv").open(encoding="utf-8")))
    profiles = {k: SpeakerProfile.from_json(json.dumps(v))
                for k, v in json.loads((dataset / "profiles.json").read_text()).items()}

    # The classifier is fitted on the E4 data only, on the *other* speakers,
    # so the E5 renders are out of sample for it as well. Its accuracy on the
    # held-out speaker's own E4 clips is the ceiling the round trip inherits.
    held_out = [r for r in l2_rows if r["speaker"] != args.speaker]
    own = [r for r in l2_rows if r["speaker"] == args.speaker]
    by_style = NearestCentroid.fit(held_out, "style")
    reference = sum(1 for r in own if by_style.predict(r) == r["style"]) / len(own)
    print(f"classifier fitted on {len(held_out)} clips from the other speakers; "
          f"its accuracy on {args.speaker}'s own E4 clips is {reference*100:.1f}%")

    # The same classifier over the three delivery classes, which is the
    # vocabulary the brief asks about.
    held_out_c = [dict(r, delivery=STYLE_TO_CLASS[r["style"]]) for r in held_out]
    by_class = NearestCentroid.fit(held_out_c, "delivery")
    own_c = [dict(r, delivery=STYLE_TO_CLASS[r["style"]]) for r in own]
    reference_c = sum(1 for r in own_c
                      if by_class.predict(r) == r["delivery"]) / len(own_c)
    print(f"same classifier over the three delivery classes: "
          f"{reference_c*100:.1f}% on {args.speaker}'s own E4 clips")

    loso = []
    for speaker in sorted({r["speaker"] for r in l2_rows}):
        train = [r for r in l2_rows if r["speaker"] != speaker]
        test = [r for r in l2_rows if r["speaker"] == speaker]
        model = NearestCentroid.fit(train, "style")
        hits = sum(1 for r in test if model.predict(r) == r["style"])
        loso.append(hits / len(test))
    print(f"classifier (6 styles) leave-one-speaker-out accuracy: "
          f"{np.mean(loso)*100:.1f}%  per speaker "
          f"{['%.3f' % v for v in loso]}")

    profile = profiles[args.speaker]
    out_dir = dataset / "e5"
    out_dir.mkdir(parents=True, exist_ok=True)

    rows = []
    for index, (line, situation, target) in enumerate(SCENARIOS):
        criteria = {name: f"{name}: {spec['gloss']}"
                    for name, spec in PRESETS.items()}
        state = (f"An agent must say a line. The line is: \"{line}\" "
                 f"The situation: {situation}")
        questions = {"delivery": {"type": "choice",
                                  "instructions": "Which delivery preset should "
                                                 "this line be spoken with?",
                                  "criteria": criteria}}
        payload = {"model": "d1-2B", "state": state, "questions": questions}
        started = time.perf_counter()
        answer = post_json(D1_2B, payload).get("answers", {}).get("delivery", {})
        latency = (time.perf_counter() - started) * 1000.0
        picked = answer.get("choice", "")
        confidence = answer.get("confidence", 0.0)

        record = {
            "index": index,
            "line": line,
            "situation": situation,
            "target_preset": target,
            "target_class": PRESET_TO_CLASS[target],
            "picked_preset": picked,
            "picked_class": PRESET_TO_CLASS.get(picked, ""),
            "gate_confidence": round(float(confidence), 4),
            "gate_latency_ms": round(latency, 1),
        }

        # Render what the model chose, and what it should have chosen.
        # Riva's synthesis is not deterministic, so each preset is rendered
        # `--samples` times and the majority classification is taken.
        for label, preset in (("chosen", picked), ("control", target)):
            if preset not in PRESETS:
                record[f"{label}_style"] = ""
                record[f"{label}_measured"] = ""
                record[f"{label}_measured_class"] = ""
                record[f"{label}_measured_class3"] = ""
                continue
            spec = PRESETS[preset]
            voice = spec["voice"].format(speaker=args.speaker)
            styles_seen: list[str] = []
            classes_seen: list[str] = []
            classes3_seen: list[str] = []
            l2_lines: list[str] = []
            for sample in range(args.samples):
                path = out_dir / f"{index:02d}_{label}_{preset}_{sample}.wav"
                try:
                    render(voice, line, path)
                    result = analyze_clip(path, profile)
                    patch = result["patch"]
                    styles_seen.append(by_style.predict(patch.as_row()))
                    classes3_seen.append(by_class.predict(patch.as_row()))
                    classes_seen.append(
                        "urgent" if styles_seen[-1] == "Angry"
                        else "calm" if styles_seen[-1] == "Calm" else "ordinary")
                    l2_lines.append(patch.to_line(relative=True))
                except Exception as exc:  # noqa: BLE001
                    styles_seen.append(f"error: {exc}")
            record[f"{label}_style"] = spec["style"]
            record[f"{label}_measured"] = majority(styles_seen)
            record[f"{label}_measured_class"] = majority(classes_seen)
            record[f"{label}_measured_class3"] = majority(classes3_seen)
            record[f"{label}_samples"] = len(styles_seen)
            record[f"{label}_l2"] = l2_lines[0] if l2_lines else ""
        rows.append(record)
        print(f"[{index:02d}] target={target:9s} picked={picked:9s} "
              f"-> measured {record.get('chosen_measured','')} "
              f"(class {record.get('chosen_measured_class3','')})")

    fields = sorted({k for row in rows for k in row})
    out_path = args.out or str(dataset / "e5_results.csv")
    with open(out_path, "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)

    pick_hits = sum(1 for r in rows if r["picked_preset"] == r["target_preset"])
    class_hits = sum(1 for r in rows
                     if r.get("chosen_measured_class") == r["target_class"])
    control_hits = sum(1 for r in rows
                       if r.get("control_measured_class") == r["target_class"])
    style_hits = sum(1 for r in rows
                     if r.get("chosen_measured") == r.get("chosen_style"))
    control_style_hits = sum(1 for r in rows
                             if r.get("control_measured") == r.get("control_style"))
    class3_hits = sum(1 for r in rows
                      if r.get("chosen_measured_class3") == r["target_class"])
    control3_hits = sum(1 for r in rows
                        if r.get("control_measured_class3") == r["target_class"])
    print()
    print(f"scenarios: {len(rows)}")
    print(f"model picked the intended preset: {pick_hits}/{len(rows)} "
          f"({pick_hits/len(rows)*100:.1f}%)")
    print(f"round trip landed in the intended class: {class_hits}/{len(rows)} "
          f"({class_hits/len(rows)*100:.1f}%)")
    print(f"round trip landed in the intended class, three-class classifier: "
          f"{class3_hits}/{len(rows)} ({class3_hits/len(rows)*100:.1f}%)")
    print(f"round trip recovered the exact style: {style_hits}/{len(rows)} "
          f"({style_hits/len(rows)*100:.1f}%)")
    print(f"control (intended preset rendered directly): {control_hits}/{len(rows)} "
          f"({control_hits/len(rows)*100:.1f}%) class, "
          f"{control_style_hits}/{len(rows)} style, "
          f"{control3_hits}/{len(rows)} three-class")
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

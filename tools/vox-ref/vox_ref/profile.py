"""The speaker profile: design §7's per-player calibration.

The profile is a small, human-readable set of statistics. Every L0 and L1
measurement is expressed relative to it, so "loud" means loud *for this
player*. The profile is a plain JSON file: a player can open it and see, and
edit it, how the squad has learned to hear them.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np

from .l0 import L0Frame


@dataclass
class SpeakerProfile:
    """Per-speaker calibration. All levels are dB; all pitches are hertz."""

    reference_hz: float = 0.0        # median pitch of the speaker
    pitch_range_cents: float = 0.0   # robust pitch spread
    level_median_db: float = -60.0   # median frame level
    level_peak_db: float = -60.0     # typical peak frame level
    centroid_hz: float = 0.0         # median spectral centroid (brightness reference)
    sustain_median_db: float = -60.0 # typical sustain level
    decay_median_ms: float = 0.0     # typical envelope decay time
    release_median_ms: float = 0.0   # typical envelope release time
    attack_ms: float = 0.0           # median gesture attack
    tempo_syl_per_s: float = 0.0     # median speaking tempo
    gesture_median_ms: float = 0.0   # median gesture length
    final_gesture_median_ms: float = 0.0  # median length of the last gesture
    clip_count: int = 0

    def to_json(self) -> str:
        return json.dumps(asdict(self), indent=2, sort_keys=True) + "\n"

    @classmethod
    def from_json(cls, text: str) -> "SpeakerProfile":
        data = json.loads(text)
        known = {k: v for k, v in data.items() if k in cls.__dataclass_fields__}
        return cls(**known)

    @classmethod
    def load(cls, path: str | Path) -> "SpeakerProfile":
        return cls.from_json(Path(path).read_text(encoding="utf-8"))

    def save(self, path: str | Path) -> None:
        Path(path).write_text(self.to_json(), encoding="utf-8")


def build_profile(clips: list[list[L0Frame]], tempos: list[float] | None = None,
                  patches: list | None = None,
                  gestures: list[list] | None = None) -> SpeakerProfile:
    """Derive a profile from a set of analysed clips.

    ``clips`` is a list of per-clip L0 frame lists. ``patches`` is the
    matching list of L2 patches, used for the envelope medians. ``gestures``
    is the matching list of gesture lists, used for the length of a syllable
    and of the last syllable — the reference the end-of-phrase measurements
    are made against. The statistics are medians, which makes a single bad
    clip harmless.
    """
    if not clips:
        return SpeakerProfile()

    f0s: list[float] = []
    levels: list[float] = []
    peaks: list[float] = []
    centroids: list[float] = []
    for frames in clips:
        voiced = [f for f in frames if f.voiced and f.f0_hz > 0]
        if voiced:
            f0s.extend(f.f0_hz for f in voiced)
            levels.extend(f.rms_db for f in frames if f.rms_db > -120.0)
            peaks.append(max(f.rms_db for f in frames))
            body = [f.centroid_hz for f in frames if f.centroid_hz > 0.0]
            if body:
                centroids.append(float(np.median(body)))

    reference_hz = float(np.median(f0s)) if f0s else 0.0
    if f0s and reference_hz > 0:
        cents = 1200.0 * np.log2(np.asarray(f0s) / reference_hz)
        pitch_range = float(np.percentile(cents, 90) - np.percentile(cents, 10))
    else:
        pitch_range = 0.0

    level_median = float(np.median(levels)) if levels else -60.0
    level_peak = float(np.median(peaks)) if peaks else -60.0
    centroid = float(np.median(centroids)) if centroids else 0.0

    sustain_values = [p.sustain_db for p in (patches or []) if p.sustain_db > -100.0]
    decay_values = [p.decay_ms for p in (patches or []) if p.decay_ms > 0.0]
    release_values = [p.release_ms for p in (patches or []) if p.release_ms > 0.0]
    attack_values = [p.attack_ms for p in (patches or []) if p.attack_ms > 0.0]

    tempo_values = tempos if tempos else []
    tempo = float(np.median(tempo_values)) if tempo_values else 0.0

    gesture_values: list[float] = []
    final_gesture_values: list[float] = []
    for gesture_list in (gestures or []):
        for gesture in gesture_list:
            if gesture.duration_s > 0:
                gesture_values.append(gesture.duration_s)
        if gesture_list:
            last = gesture_list[-1]
            if last.duration_s > 0:
                final_gesture_values.append(last.duration_s)

    return SpeakerProfile(
        reference_hz=round(reference_hz, 4),
        pitch_range_cents=round(pitch_range, 3),
        level_median_db=round(level_median, 3),
        level_peak_db=round(level_peak, 3),
        centroid_hz=round(centroid, 3),
        sustain_median_db=round(float(np.median(sustain_values)), 3)
                              if sustain_values else -60.0,
        decay_median_ms=round(float(np.median(decay_values)), 3)
                           if decay_values else 0.0,
        release_median_ms=round(float(np.median(release_values)), 3)
                             if release_values else 0.0,
        attack_ms=round(float(np.median(attack_values)), 3)
                     if attack_values else 0.0,
        tempo_syl_per_s=round(tempo, 4),
        gesture_median_ms=round(float(np.median(gesture_values)) * 1000.0, 3)
                              if gesture_values else 0.0,
        final_gesture_median_ms=round(float(np.median(final_gesture_values)) * 1000.0, 3)
                                  if final_gesture_values else 0.0,
        clip_count=len(clips),
    )

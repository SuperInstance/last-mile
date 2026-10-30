"""L1: the gesture layer of Vox Score.

A gesture is one syllable-sized burst of speech: an onset, a nucleus, an
offset, and the expression attached to it (pitch target and glide, duration,
velocity, attack and release). This is the layer MIDI would carry as "note on
plus per-note expression".
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from .l0 import L0Frame


@dataclass
class Gesture:
    """One syllable-scale gesture."""

    index: int
    onset_s: float
    nucleus_s: float
    offset_s: float
    duration_s: float
    attack_ms: float
    release_ms: float
    pitch_target_cents: float
    pitch_glide_cents: float
    velocity_db: float
    peak_db: float
    noise_burst: int

    def as_row(self) -> dict:
        return {
            "index": self.index,
            "onset_s": round(self.onset_s, 4),
            "nucleus_s": round(self.nucleus_s, 4),
            "offset_s": round(self.offset_s, 4),
            "duration_s": round(self.duration_s, 4),
            "attack_ms": round(self.attack_ms, 2),
            "release_ms": round(self.release_ms, 2),
            "pitch_target_cents": round(self.pitch_target_cents, 2),
            "pitch_glide_cents": round(self.pitch_glide_cents, 2),
            "velocity_db": round(self.velocity_db, 2),
            "peak_db": round(self.peak_db, 2),
            "noise_burst": self.noise_burst,
        }


def _smooth(values: np.ndarray, width: int) -> np.ndarray:
    if width <= 1 or values.size == 0:
        return values.astype(np.float64)
    kernel = np.ones(width) / float(width)
    padded = np.pad(values.astype(np.float64), (width // 2, width - 1 - width // 2), mode="edge")
    return np.convolve(padded, kernel, mode="valid")[: values.size]


def segment_gestures(frames: list[L0Frame], sr: int,
                     min_gesture_ms: float = 60.0,
                     min_peak_separation_ms: float = 120.0) -> list[Gesture]:
    """Split the frame stream into syllable-scale gestures.

    The gesture nuclei are the peaks of the smoothed level envelope; the
    boundaries are the valleys between them. Peak-picking with a prominence
    requirement is used rather than an absolute threshold, because a fixed
    threshold either swallows the gap between syllables or splits one
    syllable into two depending on how loud the clip is.
    """
    if len(frames) < 3:
        return []

    times = np.array([f.time_s for f in frames])
    levels = np.array([f.rms_db for f in frames])
    hop_s = float(times[1] - times[0]) if len(times) > 1 else 0.01

    smooth = _smooth(levels, max(1, int(round(0.030 / hop_s))))
    floor = float(np.percentile(smooth, 10))
    ceiling = float(np.max(smooth))
    span = ceiling - floor
    if span < 6.0:
        return []

    speech = np.nonzero(smooth > floor + 0.15 * span)[0]
    if speech.size < 3:
        return []

    region_start = int(speech[0])
    region_end = int(speech[-1])
    peak_floor = floor + 0.30 * span

    peaks: list[int] = []
    for i in range(region_start + 1, region_end):
        if (smooth[i] >= smooth[i - 1] and smooth[i] > smooth[i + 1]
                and smooth[i] > peak_floor):
            peaks.append(i)
    if not peaks:
        peaks = [region_start + int(np.argmax(smooth[region_start:region_end + 1]))]

    # Merge peaks that are too close together, keeping the louder one.
    separation = max(1, int(round(min_peak_separation_ms / 1000.0 / hop_s)))
    merged: list[int] = []
    for peak in peaks:
        if merged and peak - merged[-1] < separation:
            if smooth[peak] > smooth[merged[-1]]:
                merged[-1] = peak
        else:
            merged.append(peak)
    peaks = merged

    gestures: list[Gesture] = []
    min_len = max(1, int(round(min_gesture_ms / 1000.0 / hop_s)))
    for k, peak in enumerate(peaks):
        if k == 0:
            onset_idx = region_start
        else:
            onset_idx = int(np.argmin(smooth[peaks[k - 1]: peak + 1])) + peaks[k - 1]
        if k == len(peaks) - 1:
            offset_idx = region_end
        else:
            offset_idx = int(np.argmin(smooth[peak: peaks[k + 1] + 1])) + peak
        if offset_idx - onset_idx + 1 < min_len:
            continue

        window = frames[onset_idx: offset_idx + 1]
        if not window:
            continue
        onset_s = times[onset_idx]
        nucleus_s = times[peak]
        offset_s = times[offset_idx]
        duration = max(0.0, offset_s - onset_s)
        attack_ms = max(0.0, (nucleus_s - onset_s) * 1000.0)
        release_ms = max(0.0, (offset_s - nucleus_s) * 1000.0)

        voiced = [f for f in window if f.voiced and f.f0_hz > 0]
        if voiced:
            cents = np.array([f.f0_cents for f in voiced])
            pitch_target = float(np.median(cents))
            k3 = max(1, len(cents) // 3)
            glide = float(np.median(cents[-k3:]) - np.median(cents[:k3]))
        else:
            pitch_target = 0.0
            glide = 0.0

        peak_db = float(np.max([f.rms_db for f in window]))
        noise_burst = 0
        onset_frames = window[: max(1, int(round(0.040 / hop_s)))]
        if onset_frames:
            onset_noise = float(np.mean([f.hnr_db for f in onset_frames]))
            body_noise = float(np.mean([f.hnr_db for f in window]))
            if onset_noise < body_noise - 6.0:
                noise_burst = 1

        gestures.append(
            Gesture(
                index=len(gestures),
                onset_s=onset_s,
                nucleus_s=nucleus_s,
                offset_s=offset_s,
                duration_s=duration,
                attack_ms=attack_ms,
                release_ms=release_ms,
                pitch_target_cents=pitch_target,
                pitch_glide_cents=glide,
                velocity_db=0.0,
                peak_db=peak_db,
                noise_burst=noise_burst,
            )
        )
    return gestures


def apply_profile(gestures: list[Gesture], profile_level_peak_db: float) -> None:
    """Express each gesture's velocity relative to the speaker profile."""
    for gesture in gestures:
        gesture.velocity_db = gesture.peak_db - profile_level_peak_db


def find_pauses(gestures: list[Gesture], frames: list[L0Frame],
                min_pause_ms: float = 120.0) -> list[float]:
    """Durations of the silences between gestures, in milliseconds."""
    if len(gestures) < 2:
        return []
    pauses: list[float] = []
    for prev, nxt in zip(gestures, gestures[1:]):
        gap = (nxt.onset_s - prev.offset_s) * 1000.0
        if gap >= min_pause_ms:
            pauses.append(gap)
    return pauses

"""L2: the phrase layer of Vox Score.

L2 is the **patch summary** — the handful of named axes a decision model
reads, written as one short text line. It is the layer that has to survive
being handed to a small model as a prompt, so it is terse, absolute in
vocabulary and relative in value (everything is measured against the speaker
profile).

The line format is the one in design §4:

    vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

import numpy as np

from . import dsp
from .l0 import L0Frame
from .l1 import Gesture, find_pauses

#: A pitch change smaller than this is called flat rather than rising or falling.
FLAT_CENTS = 60.0

#: A contour whose maximum sits later than this fraction is a rise-fall.
RISE_FALL_PEAK = 0.55

#: Minimum vibrato depth, in cents, before vibrato is reported at all.
VIBRATO_FLOOR_CENTS = 15.0

#: Minimum tremolo depth, in dB, before tremolo is reported at all.
TREMOLO_FLOOR_DB = 1.5


@dataclass
class Patch:
    """The measured L2 patch, before it is rendered as a line.

    The class fields are the *labels*. The raw measurements they were
    derived from are kept alongside, because the labels depend on whether
    they are absolute (v1) or relative to the speaker profile (v2).
    """

    # Loudness envelope: raw measurements, plus the classes they were
    # labelled with when the profile was available.
    attack_ms: float = 0.0
    decay_ms: float = 0.0
    sustain_db: float = -60.0
    release_ms: float = 0.0
    peak_db: float = -60.0
    decay_class: str = "med"
    sustain_class: str = "mid"
    release_class: str = "med"

    # The speaker's own medians, when a profile was supplied.
    profile_decay_ms: float = 0.0
    profile_release_ms: float = 0.0
    profile_sustain_db: float = -60.0

    # Brightness envelope.
    brightness_attack_ms: float = 0.0
    brightness_decay_class: str = "med"
    brightness_sustain_class: str = "mid"
    brightness_release_class: str = "med"

    # Contour.
    contour_class: str = "flat"
    contour_change_cents: float = 0.0

    # Modulation.
    vibrato_cents: float = 0.0
    vibrato_hz: float = 0.0
    tremolo_db: float = 0.0
    tremolo_hz: float = 0.0

    # Rate and level.
    tempo_syl_per_s: float = 0.0
    velocity_db: float = 0.0
    velocity_class: str = "normal"

    # Timing.
    duration_s: float = 0.0
    speech_s: float = 0.0
    pause_count: int = 0
    pause_total_ms: float = 0.0

    # Brightness level relative to the profile.
    brightness_class: str = "mid"
    brightness_offset_hz: float = 0.0

    def as_row(self) -> dict:
        row = asdict(self)
        return {k: (round(v, 3) if isinstance(v, float) else v) for k, v in row.items()}

    def to_line(self, extended: bool = False, relative: bool = False) -> str:
        """Render the patch as the L2 text line.

        ``relative=False`` gives the absolute form (v1): envelope classes
        come from fixed constants, and the level is a bare number.
        ``relative=True`` gives the profile form (v2): the envelope classes
        say how this delivery compares with this speaker's own typical
        delivery, and the level carries a word as well as a number.
        """
        if relative:
            decay, sustain, release = self._relative_classes()
        else:
            decay, sustain, release = self._absolute_classes()

        vel = f"vel {self.velocity_db:+.0f}dB"
        if relative:
            vel = f"vel {self.velocity_class} {self.velocity_db:+.0f}dB"

        parts = [
            "vox: "
            f"atk {self.attack_ms:.0f}ms dec {decay} "
            f"sus {sustain} rel {release}"
        ]

        contour = f"contour {self.contour_class}"
        if extended:
            contour += f" {self.contour_change_cents:+.0f}c"
        parts.append(contour)

        parts.append(f"vib {self._vibrato_token()}")
        parts.append(f"trem {self._tremolo_token()}")
        parts.append(f"bright {self.brightness_class}")

        if extended:
            parts.append(
                f"batk {self.brightness_attack_ms:.0f}ms bdec {self.brightness_decay_class} "
                f"bsus {self.brightness_sustain_class} brel {self.brightness_release_class}"
            )

        parts.append(f"tempo {self.tempo_syl_per_s:.1f}/s")
        parts.append(vel)

        if self.pause_count:
            parts.append(f"pause {self.pause_count}x{self.pause_total_ms:.0f}ms")

        return " | ".join(parts)

    def _absolute_classes(self) -> tuple[str, str, str]:
        """Envelope classes from fixed constants (the v1 form)."""
        decay_s = self.decay_ms / 1000.0
        decay = "fast" if decay_s <= 0.12 else ("med" if decay_s <= 0.30 else "slow")
        offset = self.sustain_db - self.peak_db
        sustain = "low" if offset <= -12.0 else ("mid" if offset <= -5.0 else "high")
        release_s = self.release_ms / 1000.0
        release = "hard" if release_s <= 0.08 else ("med" if release_s <= 0.20 else "soft")
        return decay, sustain, release

    def _relative_classes(self) -> tuple[str, str, str]:
        """Envelope classes relative to the speaker's own medians (v2)."""
        if self.profile_decay_ms > 0:
            ratio = self.decay_ms / self.profile_decay_ms
            decay = "fast" if ratio < 0.70 else ("med" if ratio < 1.40 else "slow")
        else:
            decay = self._absolute_classes()[0]
        if self.profile_sustain_db > -100.0:
            offset = self.sustain_db - self.profile_sustain_db
            sustain = "low" if offset <= -4.0 else ("mid" if offset < 4.0 else "high")
        else:
            sustain = self._absolute_classes()[1]
        if self.profile_release_ms > 0:
            ratio = self.release_ms / self.profile_release_ms
            release = "hard" if ratio < 0.70 else ("med" if ratio < 1.40 else "soft")
        else:
            release = self._absolute_classes()[2]
        return decay, sustain, release

    def _vibrato_token(self) -> str:
        if self.vibrato_cents < VIBRATO_FLOOR_CENTS:
            return "none"
        depth = "low" if self.vibrato_cents < 35.0 else "high"
        return f"{depth} {self.vibrato_cents:.0f}c@{self.vibrato_hz:.1f}hz"

    def _tremolo_token(self) -> str:
        if self.tremolo_db < TREMOLO_FLOOR_DB:
            return "none"
        depth = "low" if self.tremolo_db < 3.5 else "high"
        return f"{depth} {self.tremolo_db:.1f}db@{self.tremolo_hz:.1f}hz"


def _utterance_adsr(times: np.ndarray, levels: np.ndarray,
                    gesture_attacks: list[float]) -> dict:
    """Measure the loudness envelope of one utterance.

    The attack comes from the gesture layer (the rise into each syllable
    nucleus), because a synthesised clip usually starts straight at the
    speech and an utterance-level leading edge is therefore unobservable.
    The decay, sustain and release come from the utterance envelope.

    Returns raw measurements; the class labels are assigned separately,
    because they depend on whether the reference is absolute or the
    speaker's own profile.
    """
    if levels.size < 3:
        return {"attack_ms": 0.0, "decay_ms": 0.0, "sustain_db": -60.0,
                "release_ms": 0.0}

    floor = float(np.percentile(levels, 10))
    peak = float(np.max(levels))
    span = peak - floor
    if span <= 1.0:
        return {"attack_ms": 0.0, "decay_ms": 0.0, "sustain_db": peak,
                "release_ms": 0.0}

    peak_idx = int(np.argmax(levels))

    # Decay: how long until the level settles 6 dB below the peak.
    decay_target = peak - 6.0
    decay_s = float(times[-1] - times[peak_idx])
    decay_end = levels.size - 1
    for i in range(peak_idx, levels.size):
        if levels[i] <= decay_target:
            decay_s = float(times[i] - times[peak_idx])
            decay_end = i
            break

    # Release: from the last frame still well above the floor to the end.
    release_target = floor + 0.35 * span
    release_s = 0.0
    last_loud = levels.size - 1
    for i in range(levels.size - 1, peak_idx, -1):
        if levels[i] <= release_target:
            release_s = float(times[-1] - times[i])
            last_loud = i
            break

    sustain_slice = levels[decay_end: max(decay_end + 1, last_loud)]
    sustain_db = float(np.median(sustain_slice)) if sustain_slice.size else peak

    attack_ms = float(np.median(gesture_attacks)) if gesture_attacks else 0.0

    return {"attack_ms": attack_ms, "decay_ms": decay_s * 1000.0,
            "sustain_db": sustain_db, "release_ms": release_s * 1000.0}


def _classify_adsr(raw: dict, profile) -> tuple[str, str, str]:
    """Decay / sustain / release labels, absolute or profile-relative."""
    decay_ms = raw["decay_ms"]
    sustain_db = raw["sustain_db"]
    release_ms = raw["release_ms"]

    if profile is not None and profile.decay_median_ms > 0:
        ratio = decay_ms / profile.decay_median_ms
        decay_class = "fast" if ratio < 0.70 else ("med" if ratio < 1.40 else "slow")
    else:
        decay_s = decay_ms / 1000.0
        decay_class = "fast" if decay_s <= 0.12 else ("med" if decay_s <= 0.30 else "slow")

    if profile is not None and profile.sustain_median_db > -100.0:
        offset = sustain_db - profile.sustain_median_db
        sustain_class = "low" if offset <= -4.0 else ("mid" if offset < 4.0 else "high")
    else:
        peak = max(raw["sustain_db"], raw.get("peak_db", raw["sustain_db"]))
        offset = sustain_db - peak
        sustain_class = "low" if offset <= -12.0 else ("mid" if offset <= -5.0 else "high")

    if profile is not None and profile.release_median_ms > 0:
        ratio = release_ms / profile.release_median_ms
        release_class = "hard" if ratio < 0.70 else ("med" if ratio < 1.40 else "soft")
    else:
        release_s = release_ms / 1000.0
        release_class = "hard" if release_s <= 0.08 else ("med" if release_s <= 0.20 else "soft")

    return decay_class, sustain_class, release_class


def _velocity_class(velocity_db: float) -> str:
    """Loudness class relative to the speaker's typical peak.

    3 dB is the usual threshold for a change of loudness that a listener
    notices without being asked to look for it.
    """
    if velocity_db <= -3.0:
        return "soft"
    if velocity_db >= 3.0:
        return "loud"
    return "normal"


def _contour_class(cents: np.ndarray) -> tuple[str, float]:
    """Classify the pitch contour as rise, fall, flat or rise-fall."""
    if cents.size < 3:
        return "flat", 0.0
    n = cents.size
    third = max(1, n // 3)
    first = float(np.mean(cents[:third]))
    last = float(np.mean(cents[-third:]))
    diff = last - first
    if abs(diff) < FLAT_CENTS:
        return "flat", diff
    max_pos = float(np.argmax(cents)) / float(n - 1)
    if max_pos > RISE_FALL_PEAK:
        return "rise-fall", diff
    return ("rise" if diff > 0 else "fall"), diff


def _modulation(frames: list[L0Frame], gestures: list[Gesture], sr: int,
                profile_level_peak_db: float) -> tuple[float, float, float, float]:
    """Vibrato (cents, Hz) and tremolo (dB, Hz) of the utterance."""
    best_vib_depth = 0.0
    best_vib_hz = 0.0
    best_trem_depth = 0.0
    best_trem_hz = 0.0

    for gesture in gestures:
        window = [f for f in frames
                  if gesture.onset_s - 1e-6 <= f.time_s <= gesture.offset_s + 1e-6]
        if len(window) < 8:
            continue
        voiced = [f for f in window if f.voiced and f.f0_hz > 0]
        if len(voiced) >= 8:
            span = voiced[-1].time_s - voiced[0].time_s
            if span >= 0.15:
                cents = np.array([f.f0_cents for f in voiced])
                times = np.array([f.time_s for f in voiced])
                # Detrend so the LFO is not confused with the global contour.
                if times[-1] > times[0]:
                    slope, intercept = np.polyfit(times, cents, 1)
                    resid = cents - (slope * times + intercept)
                else:
                    resid = cents - float(np.mean(cents))
                depth, rate = dsp.lfo_estimate(resid, sr)
                if depth > best_vib_depth:
                    best_vib_depth = depth
                    best_vib_hz = rate

        levels = np.array([f.rms_db for f in window])
        times = np.array([f.time_s for f in window])
        if times.size >= 8 and times[-1] > times[0]:
            slope, intercept = np.polyfit(times, levels, 1)
            resid = levels - (slope * times + intercept)
            depth, rate = dsp.lfo_estimate(resid, sr)
            if depth > best_trem_depth:
                best_trem_depth = depth
                best_trem_hz = rate

    return best_vib_depth, best_vib_hz, best_trem_depth, best_trem_hz


def summarize(frames: list[L0Frame], gestures: list[Gesture], sr: int,
              profile_reference_hz: float = 0.0,
              profile_level_peak_db: float = -60.0,
              profile_centroid_hz: float = 0.0,
              profile=None) -> Patch:
    """Measure the L2 patch from the frame and gesture layers.

    ``profile`` is an optional :class:`~vox_ref.profile.SpeakerProfile`. When
    one is given, the envelope classes are relative to that speaker's own
    typical delivery instead of to fixed constants.
    """
    patch = Patch()
    if not frames:
        return patch

    times = np.array([f.time_s for f in frames])
    levels = np.array([f.rms_db for f in frames])
    centroids = np.array([f.centroid_hz for f in frames])

    smooth_width = max(1, int(round(0.030 / (times[1] - times[0]))) if len(times) > 1 else 1)
    smooth_levels = _moving_average(levels, smooth_width)
    smooth_centroids = _moving_average(centroids, smooth_width)

    raw = _utterance_adsr(times, smooth_levels, [g.attack_ms for g in gestures])
    raw["peak_db"] = float(np.max(levels))
    decay, sustain, release = _classify_adsr(raw, profile)
    patch.attack_ms = raw["attack_ms"]
    patch.decay_ms = raw["decay_ms"]
    patch.sustain_db = raw["sustain_db"]
    patch.release_ms = raw["release_ms"]
    patch.peak_db = raw["peak_db"]
    patch.decay_class = decay
    patch.sustain_class = sustain
    patch.release_class = release
    if profile is not None:
        patch.profile_decay_ms = profile.decay_median_ms
        patch.profile_release_ms = profile.release_median_ms
        patch.profile_sustain_db = profile.sustain_median_db

    bright_raw = _utterance_adsr(times, smooth_centroids, [g.attack_ms for g in gestures])
    bright_raw["peak_db"] = float(np.max(smooth_centroids))
    b_decay, b_sustain, b_release = _classify_adsr(bright_raw, profile)
    patch.brightness_attack_ms = bright_raw["attack_ms"]
    patch.brightness_decay_class = b_decay
    patch.brightness_sustain_class = b_sustain
    patch.brightness_release_class = b_release

    # Contour, over the voiced frames, in cents relative to the profile.
    voiced_frames = [f for f in frames if f.voiced and f.f0_hz > 0]
    if voiced_frames:
        if profile_reference_hz > 0:
            cents = np.array([f.f0_cents for f in voiced_frames])
        else:
            median_hz = float(np.median([f.f0_hz for f in voiced_frames]))
            cents = np.array([1200.0 * np.log2(max(f.f0_hz, 1e-6) / median_hz)
                               for f in voiced_frames])
        contour_class, change = _contour_class(cents)
        patch.contour_class = contour_class
        patch.contour_change_cents = change

    vib_c, vib_hz, trem_db, trem_hz = _modulation(frames, gestures, sr, profile_level_peak_db)
    patch.vibrato_cents = vib_c
    patch.vibrato_hz = vib_hz
    patch.tremolo_db = trem_db
    patch.tremolo_hz = trem_hz

    # Tempo and duration.
    speech_s = float(sum(g.duration_s for g in gestures))
    patch.speech_s = speech_s
    if gestures:
        patch.duration_s = float(gestures[-1].offset_s - gestures[0].onset_s)
        if speech_s > 0:
            patch.tempo_syl_per_s = len(gestures) / speech_s
        patch.velocity_db = max(g.peak_db for g in gestures) - profile_level_peak_db
        patch.velocity_class = _velocity_class(patch.velocity_db)

    pauses = find_pauses(gestures, frames)
    patch.pause_count = len(pauses)
    patch.pause_total_ms = float(sum(pauses))

    # Brightness level relative to the profile.
    bright_floor = float(np.percentile(smooth_centroids, 10))
    body = smooth_centroids[smooth_centroids > bright_floor + 1.0]
    if body.size:
        centroid = float(np.median(body))
    else:
        centroid = float(np.median(smooth_centroids))
    if profile_centroid_hz > 0:
        patch.brightness_offset_hz = centroid - profile_centroid_hz
        ratio = centroid / profile_centroid_hz
        if ratio >= 1.12:
            patch.brightness_class = "high"
        elif ratio <= 0.90:
            patch.brightness_class = "low"
        else:
            patch.brightness_class = "mid"
    else:
        patch.brightness_class = "mid"
    return patch


def _moving_average(values: np.ndarray, width: int) -> np.ndarray:
    if width <= 1 or values.size == 0:
        return values.astype(np.float64)
    kernel = np.ones(width) / float(width)
    padded = np.pad(values.astype(np.float64), (width // 2, width - 1 - width // 2), mode="edge")
    return np.convolve(padded, kernel, mode="valid")[: values.size]

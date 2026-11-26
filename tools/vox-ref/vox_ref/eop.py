"""The end of the phrase: what the last quarter-second of a delivery says.

The L2 patch as design §4 writes it describes the delivery as a whole: the
loudness envelope, the timbre, the pace. None of that says whether the line
was an order or a question, because an order and a question can be delivered
at the same pace, the same volume and the same brightness.

What separates them is the **boundary tone** — the movement of the pitch in
the last moments of the phrase. A question ends rising, an order ends
falling or level. That is a property of the *performance*, not of the words,
so it belongs in L2.

Four measurements, all of them relative to the speaker's own profile:

``slope``
    the F0 slope over the last 200-300 ms of speech, in cents per second.
``rise``
    the final rise: how far the pitch climbs (or falls) across the last
    gesture, in cents, and as a fraction of the speaker's own pitch range.
``lengthening``
    how long the last syllable is compared with this speaker's typical last
    syllable.
``tone``
    the boundary tone itself: rise / fall / level.

The window is 250 ms by default because that is long enough to hold a
sliding pitch contour and short enough to sit inside the final gesture of
every line in the E4 corpus. The measurements are computed from the median
of the first third and the median of the last third of the window rather
than from a least-squares fit, so a single octave error in the YIN pitch
tracker cannot move the answer.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from .l0 import L0Frame
from .l1 import Gesture

#: The default width of the final window, in seconds.
WINDOW_S = 0.25

#: The width of the shortest window the design's brief allows, in seconds.
WINDOW_MIN_S = 0.20

#: The width of the longest window the design's brief allows, in seconds.
WINDOW_MAX_S = 0.30

#: How much of the speaker's pitch range the final rise must cover before
#: the boundary tone is called a rise (or a fall), as a fraction.
RISE_THRESHOLD = 0.10

#: The fewest voiced frames a window needs before its thirds are meaningful.
MIN_VOICED_FRAMES = 6


@dataclass
class EndOfPhrase:
    """What the end of the phrase says. Every value is profile-relative."""

    #: The width of the final window, in seconds.
    window_s: float = WINDOW_S
    #: F0 slope over the final window, in cents per second.
    slope_cents_per_s: float = 0.0
    #: The final rise across the last gesture, in cents.
    rise_cents: float = 0.0
    #: The final rise as a fraction of the speaker's pitch range.
    rise_ratio: float = 0.0
    #: Where the pitch ended, relative to the speaker's usual pitch, in
    #: fractions of the speaker's pitch range. Zero means the voice ended
    #: at this speaker's usual pitch.
    final_offset_ratio: float = 0.0
    #: The last gesture's length against this speaker's typical last gesture.
    lengthening: float = 1.0
    #: The last gesture's length against this speaker's typical gesture.
    lengthening_vs_gesture: float = 1.0
    #: The boundary tone: rise / fall / level.
    tone: str = "level"
    #: How many voiced frames the final window held.
    voiced_frames: int = 0

    def as_row(self) -> dict:
        return {
            "eop_window_s": round(self.window_s, 3),
            "eop_slope_cents_per_s": round(self.slope_cents_per_s, 2),
            "eop_rise_cents": round(self.rise_cents, 2),
            "eop_rise_ratio": round(self.rise_ratio, 4),
            "eop_final_offset_ratio": round(self.final_offset_ratio, 4),
            "eop_lengthening": round(self.lengthening, 4),
            "eop_lengthening_vs_gesture": round(self.lengthening_vs_gesture, 4),
            "eop_tone": self.tone,
            "eop_voiced_frames": self.voiced_frames,
        }


def _thirds_slope(times: np.ndarray, cents: np.ndarray) -> float:
    """Pitch slope in cents per second, from the medians of the thirds.

    Robust to a stray frame: one wrong octave moves a mean, but it barely
    moves a median.
    """
    n = times.size
    if n < MIN_VOICED_FRAMES:
        return 0.0
    third = max(1, n // 3)
    first_t = times[:third]
    last_t = times[-third:]
    first_c = cents[:third]
    last_c = cents[-third:]
    span = float(last_t.mean() - first_t.mean())
    if abs(span) < 1e-6:
        return 0.0
    return float((np.median(last_c) - np.median(first_c)) / span)


def _thirds_rise(times: np.ndarray, cents: np.ndarray) -> float:
    """The rise across a window, in cents: last third's median minus first."""
    n = times.size
    if n < MIN_VOICED_FRAMES:
        return 0.0
    third = max(1, n // 3)
    return float(np.median(cents[-third:]) - np.median(cents[:third]))


def measure(frames: list[L0Frame], gestures: list[Gesture], sr: int,
            window_s: float = WINDOW_S,
            pitch_range_cents: float = 0.0,
            final_gesture_median_s: float = 0.0,
            gesture_median_s: float = 0.0,
            rise_threshold: float = RISE_THRESHOLD) -> EndOfPhrase:
    """Measure the end of the phrase of one utterance.

    ``pitch_range_cents``, ``final_gesture_median_s`` and ``gesture_median_s``
    come from the speaker profile. When they are missing the measurements
    fall back to the utterance's own values, which is self-referential but
    still reproducible.
    """
    eop = EndOfPhrase(window_s=window_s)

    voiced = [f for f in frames if f.voiced and f.f0_hz > 0]
    if len(voiced) < MIN_VOICED_FRAMES:
        return eop

    times = np.array([f.time_s for f in voiced])
    cents = np.array([f.f0_cents for f in voiced])

    # --- the final window: the last `window_s` of voiced speech -----------
    end_s = float(times[-1])
    window = times >= (end_s - window_s)
    if int(window.sum()) < MIN_VOICED_FRAMES:
        # A short or clipped line: fall back to the last frames there are.
        keep = min(len(voiced), max(MIN_VOICED_FRAMES, len(voiced) // 2))
        window = np.zeros(len(voiced), dtype=bool)
        window[-keep:] = True
    w_times = times[window]
    w_cents = cents[window]
    eop.voiced_frames = int(window.sum())
    eop.slope_cents_per_s = _thirds_slope(w_times, w_cents)

    # --- the final rise, across the last gesture --------------------------
    if gestures:
        last = gestures[-1]
        in_gesture = np.array(
            [last.onset_s - 1e-6 <= t <= last.offset_s + 1e-6 for t in times])
        if int(in_gesture.sum()) >= MIN_VOICED_FRAMES:
            g_times = times[in_gesture]
            g_cents = cents[in_gesture]
        else:
            g_times, g_cents = w_times, w_cents
        eop.rise_cents = _thirds_rise(g_times, g_cents)

        # Where the pitch *ended*, relative to this speaker's usual pitch.
        # The frame cents are already relative to the profile's reference
        # pitch, so this is the final pitch in units of the speaker's range.
        third = max(1, g_cents.size // 3)
        eop.final_offset_ratio = float(np.median(g_cents[-third:]))

        if final_gesture_median_s > 0:
            eop.lengthening = last.duration_s / final_gesture_median_s
        if gesture_median_s > 0:
            eop.lengthening_vs_gesture = last.duration_s / gesture_median_s

    # --- the rise against the speaker's own range -------------------------
    if pitch_range_cents > 0:
        eop.rise_ratio = eop.rise_cents / pitch_range_cents
        eop.final_offset_ratio = eop.final_offset_ratio / pitch_range_cents
    else:
        span = float(np.percentile(cents, 90) - np.percentile(cents, 10))
        if span > 0:
            eop.rise_ratio = eop.rise_cents / span
            eop.final_offset_ratio = eop.final_offset_ratio / span

    # --- the boundary tone -------------------------------------------------
    if eop.rise_ratio >= rise_threshold:
        eop.tone = "rise"
    elif eop.rise_ratio <= -rise_threshold:
        eop.tone = "fall"
    else:
        eop.tone = "level"

    return eop

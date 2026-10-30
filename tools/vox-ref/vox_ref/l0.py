"""L0: the frame layer of Vox Score.

One record per 10 ms frame. The channels are the ones design §4 names:
F0 (in cents relative to the speaker profile), RMS level, F1/F2/F3, spectral
tilt, voicing/noise ratio, and an onset flag.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

import numpy as np

from . import dsp


@dataclass
class L0Frame:
    """One L0 frame. All levels are dBFS unless the name says otherwise."""

    time_s: float
    f0_cents: float          # relative to the profile's reference pitch
    f0_hz: float
    rms_db: float
    f1_hz: float
    f2_hz: float
    f3_hz: float
    tilt_db_per_octave: float
    centroid_hz: float
    voicing: float           # 0..1
    hnr_db: float
    onset: int               # 0 or 1
    voiced: int              # 0 or 1


def hz_to_cents(hz: np.ndarray | float, reference_hz: float) -> np.ndarray | float:
    """Convert hertz to cents relative to ``reference_hz``."""
    if reference_hz <= 0.0:
        return np.zeros_like(hz) if isinstance(hz, np.ndarray) else 0.0
    ratio = np.maximum(np.asarray(hz, dtype=np.float64), 1e-6) / reference_hz
    return 1200.0 * np.log2(ratio)


def analyze_l0(x: np.ndarray, sr: int, reference_hz: float = 0.0,
               onset_threshold_db: float = 6.0) -> list[L0Frame]:
    """Extract the L0 frame channel set from a mono signal.

    ``reference_hz`` is the speaker profile's median pitch. When it is 0 the
    signal's own median is used, which makes the analysis self-referential
    but still reproducible.
    """
    frames, hop = dsp.frame_signal(x, sr)
    n = frames.shape[0]
    if n == 0:
        return []

    f0 = np.zeros(n)
    voicedness = np.zeros(n)
    rms = np.zeros(n)
    tilt = np.zeros(n)
    centroid = np.zeros(n)
    hnr = np.zeros(n)
    formants = np.zeros((n, 3))
    flux = np.zeros(n)

    prev_mag = None
    for i in range(n):
        frame = frames[i]
        rms[i] = dsp.rms_db(frame)
        pitch, voiced = dsp.yin_pitch(frame, sr)
        f0[i] = pitch
        voicedness[i] = voiced
        feats, prev_mag = dsp.spectrum_features(frame, sr, prev_mag)
        tilt[i] = feats.tilt_db_per_octave
        centroid[i] = feats.centroid_hz
        flux[i] = feats.flux
        # Voiced frames carry a reliable autocorrelation; unvoiced frames
        # would poison the LPC fit, so they are skipped.
        if voiced > 0.45:
            a = dsp.lpc_coefficients(frame, sr)
            res = dsp.formants_from_lpc(a, sr)
            for k in range(min(3, len(res))):
                formants[i, k] = res[k][0]
        # Harmonic-to-noise proxy from the autocorrelation depth.
        v = float(np.clip(voicedness[i], 0.001, 0.999))
        hnr[i] = float(np.clip(10.0 * np.log10(v / (1.0 - v)), -20.0, 40.0))

    # Onset detection: spectral flux above a running median threshold.
    if n >= 5:
        median_flux = float(np.median(flux))
        scale = float(np.median(np.abs(flux - median_flux))) or 1e-6
        onset_flags = (flux > median_flux + 2.0 * scale) & (rms > -60.0)
        # Keep at most one onset every 40 ms.
        min_gap = max(1, int(round(0.04 * sr / hop)))
        last = -min_gap
        for i in range(n):
            if onset_flags[i] and i - last >= min_gap:
                last = i
            else:
                onset_flags[i] = False
    else:
        onset_flags = np.zeros(n, dtype=bool)

    if reference_hz <= 0.0:
        voiced_f0 = f0[(f0 > 0) & (voicedness > 0.45)]
        reference_hz = float(np.median(voiced_f0)) if voiced_f0.size else 0.0

    out: list[L0Frame] = []
    for i in range(n):
        cents = float(dsp_hz_to_cents_scalar(f0[i], reference_hz)) if f0[i] > 0 else 0.0
        out.append(
            L0Frame(
                time_s=round(i * hop / sr, 6),
                f0_cents=round(cents, 3),
                f0_hz=round(float(f0[i]), 3),
                rms_db=round(float(rms[i]), 3),
                f1_hz=round(float(formants[i, 0]), 3),
                f2_hz=round(float(formants[i, 1]), 3),
                f3_hz=round(float(formants[i, 2]), 3),
                tilt_db_per_octave=round(float(tilt[i]), 3),
                centroid_hz=round(float(centroid[i]), 3),
                voicing=round(float(voicedness[i]), 4),
                hnr_db=round(float(hnr[i]), 3),
                onset=int(onset_flags[i]),
                voiced=int(f0[i] > 0 and voicedness[i] > 0.45),
            )
        )
    return out


def dsp_hz_to_cents_scalar(hz: float, reference_hz: float) -> float:
    """Cents from hertz relative to a reference pitch."""
    if hz <= 0.0 or reference_hz <= 0.0:
        return 0.0
    return 1200.0 * float(np.log2(hz / reference_hz))


def frames_to_csv(frames: list[L0Frame]) -> str:
    """Render L0 frames as CSV with a header row."""
    if not frames:
        return ""
    header = list(asdict(frames[0]).keys())
    lines = [",".join(header)]
    for frame in frames:
        row = asdict(frame)
        lines.append(",".join(str(row[key]) for key in header))
    return "\n".join(lines) + "\n"

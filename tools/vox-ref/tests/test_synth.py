"""E1-style checks: does the analyzer measure what it claims?

Run with:  python -m pytest tools/vox-ref/tests -q
or simply: python tools/vox-ref/tests/test_synth.py

The tolerances are the ones design §9 E1 states: F0 within +-10 cents,
formants within +-5%, onset within +-10 ms.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from vox_ref.l0 import analyze_l0  # noqa: E402
from vox_ref.l1 import segment_gestures  # noqa: E402
from vox_ref.l2 import summarize  # noqa: E402

SR = 22050


def sine(f0: float, seconds: float, sr: int = SR, amp: float = 0.5) -> np.ndarray:
    t = np.arange(int(seconds * sr)) / sr
    return amp * np.sin(2.0 * np.pi * f0 * t)


def formant_signal(formants: list[tuple[float, float, float]], seconds: float,
                   f0: float = 120.0, sr: int = SR) -> np.ndarray:
    """A pulse train through a cascade of resonators: (freq, bandwidth, gain)."""
    t = np.arange(int(seconds * sr)) / sr
    # Glottal-ish excitation: a band-limited pulse train.
    excitation = np.zeros_like(t)
    period = sr / f0
    for k in range(int(seconds * f0) + 2):
        start = int(k * period)
        if start >= excitation.size:
            break
        excitation[start] = 1.0
    out = excitation.copy()
    for freq, bw, gain in formants:
        r = np.exp(-np.pi * bw / sr)
        theta = 2.0 * np.pi * freq / sr
        a1 = 2.0 * r * np.cos(theta)
        a2 = -r * r
        b0 = (1.0 - r) * np.sqrt(1.0 - 2.0 * r * np.cos(2.0 * theta) + r * r)
        y = np.zeros_like(out)
        for i in range(out.size):
            y[i] = b0 * out[i]
            if i >= 1:
                y[i] += a1 * y[i - 1]
            if i >= 2:
                y[i] += a2 * y[i - 2]
        out = y * gain
    peak = np.max(np.abs(out))
    if peak > 0:
        out = out / peak * 0.5
    return out


def check(name: str, ok: bool, detail: str) -> bool:
    status = "PASS" if ok else "FAIL"
    print(f"[{status}] {name}: {detail}")
    return ok


def test_f0_accuracy() -> bool:
    ok = True
    for f0 in (80.0, 110.0, 150.0, 200.0, 300.0):
        x = sine(f0, 0.6)
        frames = analyze_l0(x, SR)
        measured = np.array([f.f0_hz for f in frames if f.voiced])
        if measured.size == 0:
            ok = check(f"f0 {f0:.0f} Hz", False, "no voiced frames detected") and ok
            continue
        median = float(np.median(measured))
        cents = abs(1200.0 * np.log2(median / f0))
        ok = check(f"f0 {f0:.0f} Hz", cents <= 10.0,
                   f"measured {median:.2f} Hz, error {cents:.2f} cents") and ok
    return ok


def test_formant_accuracy() -> bool:
    """Three resonators at known frequencies; the analyzer should find them."""
    targets = [(700.0, 80.0, 1.0), (1300.0, 100.0, 0.7), (2600.0, 140.0, 0.4)]
    x = formant_signal(targets, 0.8)
    frames = analyze_l0(x, SR)
    ok = True
    for k, (target, _, _) in enumerate(targets):
        attr = f"f{k+1}_hz"
        values = [getattr(f, attr) for f in frames if f.voiced and getattr(f, attr) > 0]
        if not values:
            ok = check(f"formant F{k+1} {target:.0f} Hz", False, "not detected") and ok
            continue
        measured = float(np.median(values))
        error = abs(measured - target) / target
        ok = check(f"formant F{k+1} {target:.0f} Hz", error <= 0.05,
                   f"measured {measured:.1f} Hz, error {error*100:.2f}%") and ok
    return ok


def test_onset_timing() -> bool:
    """A burst at a known time should raise an onset flag within 10 ms."""
    x = np.zeros(int(0.5 * SR))
    burst_start = 0.200
    start = int(burst_start * SR)
    x[start: start + int(0.08 * SR)] = sine(140.0, 0.08)
    frames = analyze_l0(x, SR)
    onsets = [f.time_s for f in frames if f.onset]
    if not onsets:
        return check("onset detection", False, "no onset flagged")
    nearest = min(onsets, key=lambda t: abs(t - burst_start))
    error_ms = abs(nearest - burst_start) * 1000.0
    return check("onset detection", error_ms <= 10.0,
                 f"burst at {burst_start*1000:.0f} ms, flagged at {nearest*1000:.0f} ms "
                 f"(error {error_ms:.1f} ms)")


def test_tempo() -> bool:
    """Four 150 ms bursts with silences between them: about 4 syllables."""
    x = np.zeros(int(1.2 * SR))
    for k in range(4):
        start = int((0.10 + k * 0.25) * SR)
        x[start: start + int(0.15 * SR)] = sine(150.0, 0.15)
    frames = analyze_l0(x, SR)
    gestures = segment_gestures(frames, SR)
    patch = summarize(frames, gestures, SR)
    return check("gesture count", len(gestures) == 4,
                 f"{len(gestures)} gestures, tempo {patch.tempo_syl_per_s:.2f}/s")


def test_determinism() -> bool:
    """The same bytes must give the same numbers, twice."""
    x = formant_signal([(700.0, 80.0, 1.0), (1300.0, 100.0, 0.6)], 0.5)
    a = analyze_l0(x, SR)
    b = analyze_l0(x.copy(), SR)
    same = all(
        (x1.f0_cents, x1.rms_db, x1.f1_hz, x1.f2_hz, x1.f3_hz, x1.tilt_db_per_octave,
         x1.voicing, x1.onset) ==
        (x2.f0_cents, x2.rms_db, x2.f1_hz, x2.f2_hz, x2.f3_hz, x2.tilt_db_per_octave,
         x2.voicing, x2.onset)
        for x1, x2 in zip(a, b)
    ) and len(a) == len(b)
    return check("determinism", same, "two runs over identical bytes agree")


def main() -> int:
    results = [
        test_f0_accuracy(),
        test_formant_accuracy(),
        test_onset_timing(),
        test_tempo(),
        test_determinism(),
    ]
    passed = sum(1 for r in results if r)
    print(f"\n{passed}/{len(results)} checks passed")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())

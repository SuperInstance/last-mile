"""Signal-processing primitives for the Vox Score reference analyzer.

Everything here is plain numpy, float64, and deterministic: the same input
bytes always produce the same numbers, on any machine, with no RNG and no
threading-dependent reductions. That is the property E1 (design §9) checks
and the property the protocol's bit-exact-replay claim rests on.

Frame grid: 10 ms hop (100 Hz), 40 ms analysis window. 40 ms is long enough
to resolve F0 down to ~70 Hz and to carry three formants on a 22.05 kHz
signal; 10 ms is the protocol's declared L0 rate.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

HOP_MS = 10.0
WIN_MS = 40.0

#: Search band for the pitch tracker, in hertz.
F0_MIN = 60.0
F0_MAX = 400.0


def frame_signal(x: np.ndarray, sr: int) -> tuple[np.ndarray, int]:
    """Split ``x`` into overlapping frames.

    Returns the frames ``(n_frames, win)`` and the hop size in samples. The
    signal is zero-padded at the end so that the last frame starts exactly at
    the last full hop, which keeps the frame grid a function of the sample
    count alone.
    """
    hop = int(round(sr * HOP_MS / 1000.0))
    win = int(round(sr * WIN_MS / 1000.0))
    if x.size < win:
        x = np.pad(x, (0, win - x.size))
    n_frames = 1 + (x.size - win) // hop
    needed = (n_frames - 1) * hop + win
    if x.size < needed:
        x = np.pad(x, (0, needed - x.size))
    idx = np.arange(win)[None, :] + hop * np.arange(n_frames)[:, None]
    return x[idx], hop


def hann(n: int) -> np.ndarray:
    """Periodic Hann window."""
    return 0.5 - 0.5 * np.cos(2.0 * np.pi * np.arange(n) / n)


def rms_db(x: np.ndarray) -> float:
    """Root-mean-square level of a frame, in dBFS."""
    if x.size == 0:
        return -120.0
    rms = float(np.sqrt(np.mean(np.square(x.astype(np.float64)))))
    if rms <= 1e-10:
        return -120.0
    return 20.0 * float(np.log10(rms))


def yin_pitch(frame: np.ndarray, sr: int, fmin: float = F0_MIN, fmax: float = F0_MAX,
              threshold: float = 0.15) -> tuple[float, float]:
    """YIN pitch estimate for one frame.

    Returns ``(f0_hz, voicedness)`` where voicedness is the normalised
    autocorrelation depth at the chosen lag: 1.0 is a perfectly periodic
    frame, 0.0 is noise. YIN (de Cheveigné & Kawahara, 2002) is used rather
    than raw autocorrelation because its cumulative-mean normalisation makes
    the absolute threshold meaningful across speakers.
    """
    n = frame.size
    if n < 4:
        return 0.0, 0.0

    tau_min = max(2, int(np.floor(sr / fmax)))
    tau_max = min(n - 1, int(np.ceil(sr / fmin)))
    if tau_max <= tau_min:
        return 0.0, 0.0

    f = frame.astype(np.float64)
    energy = float(np.dot(f, f))
    if energy <= 1e-12:
        return 0.0, 0.0

    # The difference function over the overlapping region:
    #   d(tau) = sum_j (x[j] - x[j+tau])^2,  j = 0 .. n-1-tau
    #         = head_energy(tau) + tail_energy(tau) - 2 * ac[tau]
    # where ac is the full autocorrelation, and the two energy terms are the
    # total energy minus the non-overlapping prefix/suffix.
    size = 1 << int(np.ceil(np.log2(2 * n)))
    spec = np.fft.rfft(f, size)
    ac = np.fft.irfft(spec * np.conj(spec), size)[: tau_max + 1]

    squared = np.square(f)
    prefix = np.concatenate(([0.0], np.cumsum(squared)))  # prefix[t] = sum_{j<t} x[j]^2
    lags = np.arange(tau_max + 1)
    head_energy = prefix[n - lags]                        # sum_{j<n-tau} x[j]^2
    tail_energy = energy - prefix[lags]                  # sum_{j>=tau} x[j]^2
    d = head_energy + tail_energy - 2.0 * ac
    d[0] = 0.0

    cum = np.cumsum(d[1:])
    denom = np.arange(1, cum.size + 1)
    d_prime = np.ones_like(d)
    d_prime[1:] = d[1:] * denom / np.maximum(cum, 1e-12)

    # YIN's "first dip below the threshold" rule: take the first *local
    # minimum* of the normalised difference that falls below the threshold.
    # Using the first threshold *crossing* instead would fire on the
    # descending slope before the dip and bias the period long.
    search = d_prime[tau_min: tau_max + 1]
    candidates = []
    for i in range(1, search.size - 1):
        if search[i] <= search[i - 1] and search[i] <= search[i + 1] and search[i] < threshold:
            candidates.append(i)
    if candidates:
        tau = int(candidates[0]) + tau_min
    else:
        tau = int(np.argmin(search)) + tau_min

    # Parabolic interpolation around the chosen lag.
    if 1 <= tau < d_prime.size - 1:
        a, b, c = d_prime[tau - 1], d_prime[tau], d_prime[tau + 1]
        denom2 = a - 2.0 * b + c
        if abs(denom2) > 1e-12:
            tau_ref = tau + 0.5 * (a - c) / denom2
        else:
            tau_ref = float(tau)
    else:
        tau_ref = float(tau)

    if tau_ref <= 0:
        return 0.0, 0.0

    index = int(min(max(round(tau_ref), 0), d_prime.size - 1))
    voicedness = float(np.clip(1.0 - d_prime[index], 0.0, 1.0))
    f0 = sr / tau_ref
    if not np.isfinite(f0) or f0 < fmin or f0 > fmax:
        return 0.0, 0.0
    return float(f0), voicedness


def levinson_durbin(r: np.ndarray, order: int) -> np.ndarray:
    """LPC coefficients from autocorrelation ``r`` via Levinson-Durbin.

    Returns ``a`` with ``a[0] == 1`` and the prediction error of order
    ``order``. This is the textbook recursion; it is written out here rather
    than imported so that the reference implementation has no dependency on a
    solver's internal conventions.
    """
    a = np.zeros(order + 1, dtype=np.float64)
    a[0] = 1.0
    e = float(r[0])
    if e <= 1e-12:
        return a
    for i in range(1, order + 1):
        acc = float(r[i])
        for j in range(1, i):
            acc += float(a[j]) * float(r[i - j])
        k = -acc / e
        a_new = a.copy()
        for j in range(1, i):
            a_new[j] = a[j] + k * a[i - j]
        a_new[i] = k
        e *= (1.0 - k * k)
        a = a_new
        if e <= 1e-12:
            break
    return a


def lpc_coefficients(frame: np.ndarray, sr: int, order: int | None = None) -> np.ndarray:
    """Pre-emphasised LPC coefficients for one frame."""
    if order is None:
        order = 2 + int(sr / 1000)
    x = frame.astype(np.float64)
    if x.size > 1:
        x = np.append(x[0], x[1:] - 0.97 * x[:-1])
    x = x * hann(x.size)
    n = x.size
    # Autocorrelation via the FFT, lags 0..order.
    size = 1 << int(np.ceil(np.log2(2 * n)))
    spec = np.fft.rfft(x, size)
    acf = np.fft.irfft(spec * np.conj(spec), size)[: n]
    if acf[0] <= 1e-12:
        return np.concatenate(([1.0], np.zeros(order)))
    acf = acf / acf[0]
    return levinson_durbin(acf, order)


def formants_from_lpc(a: np.ndarray, sr: int, order: int | None = None,
                       max_bw: float = 400.0, f_min: float = 90.0,
                       f_max: float = 8000.0) -> list[tuple[float, float]]:
    """Formant frequencies and bandwidths from LPC coefficients.

    Roots of the LPC polynomial are converted to frequency and bandwidth;
    candidates that are not a plausible vocal-tract resonance (too narrow or
    too wide, outside the band of interest) are discarded.
    """
    if order is None:
        order = a.size - 1
    if a.size < 2 or order < 1:
        return []
    poly = a[: order + 1]
    if not np.all(np.isfinite(poly)):
        return []
    try:
        roots = np.roots(poly)
    except np.linalg.LinAlgError:
        return []

    out: list[tuple[float, float]] = []
    nyquist = sr / 2.0
    for root in roots:
        if root.imag <= 0.0:
            continue
        freq = float(np.angle(root) * sr / (2.0 * np.pi))
        radius = float(np.abs(root))
        if radius <= 0.0 or radius >= 1.0:
            continue
        bw = float(-np.log(radius) * sr / np.pi)
        if bw <= 0.0 or bw > max_bw:
            continue
        if freq < f_min or freq > min(f_max, nyquist - 100.0):
            continue
        out.append((freq, bw))
    out.sort(key=lambda fb: fb[0])
    return out


@dataclass
class SpectrumFeatures:
    """Per-frame spectral descriptors."""

    tilt_db_per_octave: float
    centroid_hz: float
    flatness: float
    flux: float


def spectrum_features(frame: np.ndarray, sr: int,
                      prev_magnitude: np.ndarray | None) -> tuple[SpectrumFeatures, np.ndarray]:
    """Spectral tilt, centroid, flatness and positive flux for one frame."""
    x = frame.astype(np.float64) * hann(frame.size)
    size = 1 << int(np.ceil(np.log2(max(2, x.size))))
    mag = np.abs(np.fft.rfft(x, size))
    freqs = np.fft.rfftfreq(size, 1.0 / sr)

    band = (freqs >= 200.0) & (freqs <= 5000.0) & (freqs > 0)
    if not np.any(band):
        tilt = 0.0
    else:
        log_f = np.log2(freqs[band])
        log_m = 20.0 * np.log10(np.maximum(mag[band], 1e-10))
        slope = float(np.polyfit(log_f, log_m, 1)[0])
        tilt = slope * 6.0206  # dB per octave

    ab = (freqs >= 100.0) & (freqs <= 8000.0)
    if not np.any(ab):
        centroid = 0.0
    else:
        weights = mag[ab]
        centroid = float(np.sum(freqs[ab] * weights) / max(np.sum(weights), 1e-12))

    if mag.size:
        gm = float(np.exp(np.mean(np.log(np.maximum(mag, 1e-10)))))
        am = float(np.mean(mag))
        flatness = gm / max(am, 1e-10)
    else:
        flatness = 0.0

    if prev_magnitude is None or prev_magnitude.size != mag.size:
        flux = 0.0
    else:
        diff = mag - prev_magnitude
        flux = float(np.sum(np.maximum(diff, 0.0)) / max(np.sum(mag), 1e-12))

    return SpectrumFeatures(tilt, centroid, float(flatness), flux), mag


def lfo_estimate(signal: np.ndarray, sr: float, f_min: float = 3.0,
                 f_max: float = 8.0) -> tuple[float, float]:
    """Amplitude and frequency of an LFO inside ``signal``.

    Returns ``(depth, rate_hz)`` where depth is the peak-to-peak amplitude of
    the in-band component, in the units of ``signal``. Used for vibrato
    (cents) and tremolo (dB).
    """
    n = signal.size
    if n < 8:
        return 0.0, 0.0
    x = signal.astype(np.float64)
    x = x - float(np.mean(x))
    scale = float(np.max(np.abs(x)))
    if scale <= 1e-9:
        return 0.0, 0.0
    win = np.hanning(n)
    spec = np.abs(np.fft.rfft(x * win))
    freqs = np.fft.rfftfreq(n, 1.0 / sr)
    band = (freqs >= f_min) & (freqs <= f_max)
    if not np.any(band):
        return 0.0, 0.0
    band_spec = spec[band]
    band_freqs = freqs[band]
    idx = int(np.argmax(band_spec))
    peak = float(band_spec[idx])
    if peak <= 1e-12:
        return 0.0, 0.0
    # Normalised amplitude of the in-band component.
    depth = float(2.0 * peak / (np.sum(win) / 2.0))
    return depth, float(band_freqs[idx])

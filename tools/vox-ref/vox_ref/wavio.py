"""Minimal WAV I/O for the reference analyzer.

The `wave` module in the standard library reads PCM WAV files, which is all
the skald/Riva daemon emits (16-bit mono PCM at 22.05 kHz). Float WAVs are
handled too, because some tools in this repo write them.
"""

from __future__ import annotations

import wave
from pathlib import Path

import numpy as np


def read_wav(path: str | Path) -> tuple[np.ndarray, int]:
    """Read a WAV file as mono float64 in [-1, 1], with its sample rate."""
    path = Path(path)
    with wave.open(str(path), "rb") as handle:
        channels = handle.getnchannels()
        width = handle.getsampwidth()
        rate = handle.getframerate()
        frames = handle.readframes(handle.getnframes())

    if width == 1:
        data = np.frombuffer(frames, dtype=np.uint8).astype(np.float64)
        data = (data - 128.0) / 128.0
    elif width == 2:
        data = np.frombuffer(frames, dtype="<i2").astype(np.float64) / 32768.0
    elif width == 3:
        raw = np.frombuffer(frames, dtype=np.uint8).reshape(-1, 3).astype(np.int32)
        packed = (raw[:, 0] | (raw[:, 1] << 8) | (raw[:, 2] << 16))
        packed = np.where(packed & 0x800000, packed - 0x1000000, packed)
        data = packed.astype(np.float64) / 8388608.0
    elif width == 4:
        data = np.frombuffer(frames, dtype="<i4").astype(np.float64) / 2147483648.0
    else:
        raise ValueError(f"unsupported sample width: {width} bytes")

    if channels > 1:
        usable = (data.size // channels) * channels
        data = data[:usable].reshape(-1, channels)
        data = data.mean(axis=1)

    return np.ascontiguousarray(data), rate


def write_wav(path: str | Path, x: np.ndarray, sr: int) -> None:
    """Write mono float64 samples as 16-bit PCM."""
    clipped = np.clip(np.asarray(x, dtype=np.float64), -1.0, 1.0)
    pcm = (clipped * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(sr)
        handle.writeframes(pcm.tobytes())

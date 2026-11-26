"""Vox Score reference analyzer.

A pure-Python, numpy-only implementation of the L0/L1/L2 layers described in
`docs/design/vox-score.md`. It exists to answer two questions the design
raises but does not answer:

  * does the analyzer measure what it claims?  (E1: synthetic signals with
    known truth)
  * do the dials carry intent?               (E4: does an L2 line let a
    decision model read a delivery better than the words alone?)

Everything is deterministic: no RNG, no thread-dependent reductions, no
model downloads.
"""

from __future__ import annotations

from .l0 import L0Frame, analyze_l0, frames_to_csv
from .l1 import Gesture, segment_gestures, apply_profile, find_pauses
from .l2 import Patch, summarize
from .profile import SpeakerProfile, build_profile

__all__ = [
    "L0Frame",
    "analyze_l0",
    "frames_to_csv",
    "Gesture",
    "segment_gestures",
    "apply_profile",
    "find_pauses",
    "Patch",
    "summarize",
    "SpeakerProfile",
    "build_profile",
]

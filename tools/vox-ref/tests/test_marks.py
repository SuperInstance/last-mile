"""E6-style checks: the marks quantizer is deterministic and reads as intended.

Run with:  python -m pytest tools/vox-ref/tests -q
or simply: python tools/vox-ref/tests/test_marks.py

These are cheap checks, not the experiment. The experiment is
``docs/research/vox/E6-marks.md``; the harness is ``tools/vox-e4/e6_*.py``.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from vox_ref.l2 import Patch  # noqa: E402
from vox_ref.marks import (FAMILIES, FAMILY_MARKS, MarkConfig,  # noqa: E402
                            marks_line, quantize_families, quantize_marks)
from vox_ref.profile import SpeakerProfile  # noqa: E402

PROFILE = SpeakerProfile(
    reference_hz=220.0,
    pitch_range_cents=1500.0,
    level_peak_db=-21.0,
    centroid_hz=1630.0,
    sustain_median_db=-34.0,
    decay_median_ms=70.0,
    release_median_ms=130.0,
    attack_ms=82.0,
    tempo_syl_per_s=4.70,
)

CONFIG = MarkConfig.default()


def patch(**kwargs) -> Patch:
    """A patch with the fields the quantizer reads, at neutral values."""
    base = dict(
        velocity_db=0.0,
        velocity_class="normal",
        decay_ms=70.0,
        profile_decay_ms=70.0,
        sustain_db=-34.0,
        profile_sustain_db=-34.0,
        release_ms=130.0,
        contour_class="flat",
        contour_norm=0.0,
        contour_median_norm=0.0,
        eop_tone="level",
        tempo_syl_per_s=4.70,
        brightness_class="mid",
        brightness_offset_hz=0.0,
        duration_s=1.0,
    )
    base.update(kwargs)
    return Patch(**base)


def test_dynamics_bands():
    assert quantize_families(patch(velocity_db=-9.0), PROFILE, CONFIG)["dynamics"] == "hushed"
    assert quantize_families(patch(velocity_db=-4.0), PROFILE, CONFIG)["dynamics"] == "soft"
    assert quantize_families(patch(velocity_db=0.0), PROFILE, CONFIG)["dynamics"] == "level"
    assert quantize_families(patch(velocity_db=4.0), PROFILE, CONFIG)["dynamics"] == "strong"
    assert quantize_families(patch(velocity_db=9.0), PROFILE, CONFIG)["dynamics"] == "shouted"


def test_articulation_bands():
    assert quantize_families(patch(decay_ms=40.0), PROFILE, CONFIG)["articulation"] == "clipped"
    assert quantize_families(patch(sustain_db=-30.0), PROFILE, CONFIG)["articulation"] == "pressed"
    assert quantize_families(patch(), PROFILE, CONFIG)["articulation"] == "even"
    assert quantize_families(patch(decay_ms=150.0), PROFILE, CONFIG)["articulation"] == "drawn"


def test_contour_bands():
    # The boundary tone wins: a lift at the end is "questioning" whatever the
    # overall shape was.
    assert quantize_families(patch(eop_tone="rise", contour_median_norm=-0.9),
                             PROFILE, CONFIG)["contour"] == "questioning"
    assert quantize_families(patch(contour_class="rise-fall"),
                             PROFILE, CONFIG)["contour"] == "rise-fall"
    assert quantize_families(patch(contour_median_norm=-0.9),
                             PROFILE, CONFIG)["contour"] == "falling"
    assert quantize_families(patch(contour_median_norm=0.9),
                             PROFILE, CONFIG)["contour"] == "lifting"
    assert quantize_families(patch(contour_median_norm=0.0),
                             PROFILE, CONFIG)["contour"] == "flat"


def test_pace_is_relative_to_the_speaker():
    assert quantize_families(patch(tempo_syl_per_s=6.0), PROFILE, CONFIG)["pace"] == "rushed"
    assert quantize_families(patch(tempo_syl_per_s=5.2), PROFILE, CONFIG)["pace"] == "brisk"
    assert quantize_families(patch(tempo_syl_per_s=4.70), PROFILE, CONFIG)["pace"] == "regular"
    assert quantize_families(patch(tempo_syl_per_s=4.0), PROFILE, CONFIG)["pace"] == "measured"
    # The same absolute tempo is a different mark for a different speaker.
    slow = SpeakerProfile(tempo_syl_per_s=6.0, centroid_hz=1630.0)
    assert quantize_families(patch(tempo_syl_per_s=6.0), slow, CONFIG)["pace"] == "regular"


def test_colour_is_relative_to_the_speaker():
    assert quantize_families(patch(brightness_offset_hz=+400.0), PROFILE, CONFIG)["colour"] == "bright"
    assert quantize_families(patch(brightness_offset_hz=0.0), PROFILE, CONFIG)["colour"] == "plain"
    assert quantize_families(patch(brightness_offset_hz=-400.0), PROFILE, CONFIG)["colour"] == "dark"


def test_every_mark_is_in_the_vocabulary():
    for velocity in (-9.0, -4.0, 0.0, 4.0, 9.0):
        for tone in ("rise", "fall", "level"):
            for decay in (40.0, 70.0, 150.0):
                for tempo in (3.5, 4.7, 6.5):
                    for offset in (-400.0, 0.0, 400.0):
                        families = quantize_families(
                            patch(velocity_db=velocity, eop_tone=tone,
                                  decay_ms=decay, tempo_syl_per_s=tempo,
                                  brightness_offset_hz=offset),
                            PROFILE, CONFIG)
                        assert set(families) == set(FAMILIES)
                        for name, mark in families.items():
                            assert mark in FAMILY_MARKS[name], (name, mark)


def test_output_is_the_score_form():
    marks = quantize_marks(patch(velocity_db=4.0, eop_tone="fall",
                                 contour_median_norm=-0.9, decay_ms=40.0,
                                 tempo_syl_per_s=6.0, brightness_offset_hz=400.0),
                           PROFILE, CONFIG)
    assert marks == ["strong", "clipped", "falling", "rushed", "bright"]
    assert marks_line(marks) == "marks: [strong, clipped, falling, rushed, bright]"


def test_determinism():
    one = quantize_marks(patch(velocity_db=4.0), PROFILE, CONFIG)
    two = quantize_marks(patch(velocity_db=4.0), PROFILE, CONFIG)
    assert one == two


def test_config_round_trip_and_override():
    config = MarkConfig.default()
    config.dynamics.strong_db = 4.5
    data = config.to_dict()
    again = MarkConfig.from_dict(data)
    assert again.dynamics.strong_db == 4.5
    # A partial config keeps every other default.
    partial = MarkConfig.from_dict({"pace": {"rushed_ratio": 1.4}})
    assert partial.pace.rushed_ratio == 1.4
    assert partial.pace.brisk_ratio == MarkConfig.default().pace.brisk_ratio
    assert partial.dynamics.soft_db == MarkConfig.default().dynamics.soft_db


def main() -> int:
    tests = [value for name, value in sorted(globals().items())
             if name.startswith("test_") and callable(value)]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"  ok   {test.__name__}")
        except AssertionError as exc:
            failures += 1
            print(f"  FAIL {test.__name__}: {exc}")
    print(f"\n{len(tests) - failures}/{len(tests)} checks pass")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())

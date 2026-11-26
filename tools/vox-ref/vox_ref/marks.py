"""L2.5: the marks layer — words for nuance.

Design §11 puts a small, closed **notation** over the L2 dials, the way
musical notation sits over velocity, timing and envelope. A mark set is
written the way a score is::

    "hold" [strong, clipped, falling, brisk]

Every mark is quantized **deterministically** from the measured, profile-
relative L2 fields, and every threshold lives in a config file with defaults
(:class:`MarkConfig`). The same bytes give the same marks, always.

The vocabulary here is not §11's draft. §11's table is a draft, and two of
its families cannot be measured from the fields that survived E4b, so they
are dropped and the rest are adapted. The reasoning, with the counts that
forced it, is in ``docs/research/vox/E6-marks.md``; the short version:

* **Steadiness is dropped.** ``vib`` and ``trem`` read zero on all 340 clips
  of the E4 corpus: Riva's synthesis has no measurable vibrato or tremolo at
  a 100 Hz frame rate. A family whose marks can never fire is a family that
  teaches the reader a word that never appears.
* **Articulation loses "trailing"** (the release measurement is zero on
  327 of 340 clips) and **Pace loses "halting"** (there is no pause in any
  clip of the corpus). The other marks in both families are kept, because the
  decay and sustain ratios do vary and do track the delivery.

The families that survive are, in order:

===============  ==========================================  ====================
family           marks                                       measured from
===============  ==========================================  ====================
``dynamics``     hushed, soft, level, strong, shouted        the level vs profile
``articulation`` clipped, pressed, even, drawn               the envelope vs profile
``contour``      falling, flat, lifting, rise-fall, questioning  the pitch track
``pace``         rushed, brisk, regular, measured             the tempo vs profile
``colour``       dark, plain, bright                          the timbre vs profile
===============  ==========================================  ====================
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field, fields, is_dataclass
from pathlib import Path

from .l2 import Patch
from .profile import SpeakerProfile

#: The families, in the order they are written.
FAMILIES = ("dynamics", "articulation", "contour", "pace", "colour")

#: The marks each family can produce, in the order of the axis (low to high
#: for the ordered ones). Kept here so the legend, the tests and the report
#: all read the vocabulary from one place.
FAMILY_MARKS: dict[str, tuple[str, ...]] = {
    "dynamics": ("hushed", "soft", "level", "strong", "shouted"),
    "articulation": ("clipped", "pressed", "even", "drawn"),
    "contour": ("falling", "flat", "lifting", "rise-fall", "questioning"),
    "pace": ("rushed", "brisk", "regular", "measured"),
    "colour": ("dark", "plain", "bright"),
}

#: The families §11 proposed and this implementation does not ship, with the
#: reason. Reported by the CLI and by the E6 harness, so the omission is
#: visible rather than silent.
DROPPED_FAMILIES: dict[str, str] = {
    "steadiness": ("vibrato and tremolo read zero on all 340 clips of the "
                   "corpus, so 'steady', 'tight' and 'shaking' could never fire"),
}


@dataclass
class DynamicsThresholds:
    """Level against the speaker's own peak, in dB."""

    #: At or below this the delivery is "hushed".
    hushed_db: float = -6.0
    #: At or below this (but above ``hushed_db``) it is "soft".
    soft_db: float = -3.0
    #: At or above this it is "strong".
    strong_db: float = 3.0
    #: At or above this it is "shouted".
    shouted_db: float = 6.0


@dataclass
class ArticulationThresholds:
    """The loudness envelope against the speaker's own envelope."""

    #: At or below this decay ratio the loudness falls away quickly.
    clipped_decay_ratio: float = 0.70
    #: At or above this decay ratio the loudness holds.
    drawn_decay_ratio: float = 1.40
    #: At or above this sustain offset (dB) the voice is pushed above the
    #: speaker's usual level and held there.
    pressed_sustain_db: float = 2.0


@dataclass
class ContourThresholds:
    """The pitch track."""

    #: The band, in fractions of the speaker's pitch range, that counts as
    #: no overall pitch movement at all.
    flat_band: float = 0.20


@dataclass
class PaceThresholds:
    """The tempo against the speaker's own median tempo."""

    #: At or above this ratio the delivery is "rushed".
    rushed_ratio: float = 1.25
    #: At or above this ratio it is "brisk".
    brisk_ratio: float = 1.08
    #: Above this ratio it is "regular"; below it, "measured".
    regular_ratio: float = 0.92


@dataclass
class ColourThresholds:
    """The spectral centroid against the speaker's own centroid."""

    #: At or above this ratio the timbre is "bright".
    bright_ratio: float = 1.12
    #: At or below this ratio it is "dark".
    dark_ratio: float = 0.90


@dataclass
class MarkConfig:
    """Every threshold the quantizer uses.

    The defaults are the ones E6 measured on the E4 corpus. A config file
    only has to carry the keys it wants to change; anything missing keeps its
    default.
    """

    version: int = 1
    dynamics: DynamicsThresholds = field(default_factory=DynamicsThresholds)
    articulation: ArticulationThresholds = field(
        default_factory=ArticulationThresholds)
    contour: ContourThresholds = field(default_factory=ContourThresholds)
    pace: PaceThresholds = field(default_factory=PaceThresholds)
    colour: ColourThresholds = field(default_factory=ColourThresholds)

    # -- config files --------------------------------------------------
    @classmethod
    def default(cls) -> "MarkConfig":
        """The built-in defaults."""
        return cls()

    @classmethod
    def load(cls, path: str | Path) -> "MarkConfig":
        """Read a JSON config file, merged over the defaults."""
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        return cls.from_dict(data)

    @classmethod
    def from_dict(cls, data: dict) -> "MarkConfig":
        config = cls()
        for name, value in data.items():
            if name == "version":
                config.version = int(value)
                continue
            if not hasattr(config, name):
                raise ValueError(f"unknown marks config section {name!r}")
            section = getattr(config, name)
            if not is_dataclass(section):
                raise ValueError(f"unknown marks config key {name!r}")
            known = {f.name for f in fields(section)}
            for key, item in value.items():
                if key not in known:
                    raise ValueError(f"unknown marks config key {name}.{key}")
                setattr(section, key, type(getattr(section, key))(item))
        return config

    def to_dict(self) -> dict:
        return asdict(self)

    def save(self, path: str | Path) -> None:
        Path(path).write_text(
            json.dumps(self.to_dict(), indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    def __str__(self) -> str:  # pragma: no cover - debugging aid
        return json.dumps(self.to_dict(), indent=2, sort_keys=True)


# -- the quantizer ------------------------------------------------------


def dynamics_mark(patch: Patch, profile: SpeakerProfile | None,
                  config: MarkConfig) -> str:
    """How loud, against this speaker's own usual level."""
    db = patch.velocity_db
    thresholds = config.dynamics
    if db <= thresholds.hushed_db:
        return "hushed"
    if db <= thresholds.soft_db:
        return "soft"
    if db >= thresholds.shouted_db:
        return "shouted"
    if db >= thresholds.strong_db:
        return "strong"
    return "level"


def articulation_mark(patch: Patch, profile: SpeakerProfile | None,
                      config: MarkConfig) -> str:
    """How the delivery was attacked and held.

    The loudness envelope against the speaker's own envelope: a delivery that
    falls away quickly is "clipped", one that holds is "drawn", and one that
    sits above the speaker's usual level without falling away is "pressed".
    """
    thresholds = config.articulation
    decay_ratio = 1.0
    if patch.profile_decay_ms > 0:
        decay_ratio = patch.decay_ms / patch.profile_decay_ms
    if decay_ratio >= thresholds.drawn_decay_ratio:
        return "drawn"
    if decay_ratio <= thresholds.clipped_decay_ratio:
        return "clipped"
    sustain_offset = patch.sustain_db - patch.profile_sustain_db
    if sustain_offset >= thresholds.pressed_sustain_db:
        return "pressed"
    return "even"


def contour_mark(patch: Patch, profile: SpeakerProfile | None,
                 config: MarkConfig) -> str:
    """What the pitch did.

    The boundary tone comes first: a pitch that lifts at the very end of the
    delivery is "questioning", which is the shape a question takes. What is
    left is read from the pitch movement over the whole delivery: a late
    maximum is a "rise-fall", and the rest is "falling", "lifting" or "flat".
    """
    if patch.eop_tone == "rise":
        return "questioning"
    if patch.contour_class == "rise-fall":
        return "rise-fall"
    movement = patch.contour_median_norm
    band = config.contour.flat_band
    if movement <= -band:
        return "falling"
    if movement >= band:
        return "lifting"
    return "flat"


def pace_mark(patch: Patch, profile: SpeakerProfile | None,
              config: MarkConfig) -> str:
    """How fast, against this speaker's own usual tempo."""
    ratio = 1.0
    if profile is not None and profile.tempo_syl_per_s > 0:
        ratio = patch.tempo_syl_per_s / profile.tempo_syl_per_s
    thresholds = config.pace
    if ratio >= thresholds.rushed_ratio:
        return "rushed"
    if ratio >= thresholds.brisk_ratio:
        return "brisk"
    if ratio > thresholds.regular_ratio:
        return "regular"
    return "measured"


def colour_mark(patch: Patch, profile: SpeakerProfile | None,
                config: MarkConfig) -> str:
    """The timbre, against this speaker's own timbre."""
    ratio = 1.0
    if profile is not None and profile.centroid_hz > 0:
        ratio = ((profile.centroid_hz + patch.brightness_offset_hz)
                 / profile.centroid_hz)
    thresholds = config.colour
    if ratio >= thresholds.bright_ratio:
        return "bright"
    if ratio <= thresholds.dark_ratio:
        return "dark"
    return "plain"


#: Which function writes each family.
MARKERS = {
    "dynamics": dynamics_mark,
    "articulation": articulation_mark,
    "contour": contour_mark,
    "pace": pace_mark,
    "colour": colour_mark,
}


def quantize_families(patch: Patch, profile: SpeakerProfile | None = None,
                      config: MarkConfig | None = None) -> dict[str, str]:
    """One mark per family, as a family → mark mapping."""
    config = config or MarkConfig.default()
    return {name: MARKERS[name](patch, profile, config) for name in FAMILIES}


def quantize_marks(patch: Patch, profile: SpeakerProfile | None = None,
                   config: MarkConfig | None = None) -> list[str]:
    """The mark set, in family order."""
    return [quantize_families(patch, profile, config)[name]
            for name in FAMILIES]


def marks_line(marks: list[str]) -> str:
    """Render a mark set the way a score is written."""
    return "marks: [" + ", ".join(marks) + "]"


#: The legend. It describes the *format* — what each word names — and not
#: which end of the scale means which intent. That mapping is the reader's
#: own knowledge of intonation, the way a person hearing the line would have
#: it. A legend that gives the answer away measures the hint, not the format.
MARKS_LEGEND = (
    "The marks describe how the voice was delivered, measured against this "
    "speaker's own usual delivery. "
    "Dynamics: 'hushed' is much quieter than usual, 'soft' is quieter, "
    "'level' is the usual loudness, 'strong' is louder and 'shouted' is much "
    "louder than usual. "
    "Articulation: 'clipped' is cut short, 'pressed' is pushed and held above "
    "the usual level, 'even' is uniform, 'drawn' is stretched out and held. "
    "Contour: 'falling' is the pitch going down over the delivery, 'flat' is "
    "the pitch staying where it was, 'lifting' is the pitch going up, "
    "'rise-fall' is the pitch going up and then back down, 'questioning' is "
    "the pitch lifting at the very end of the delivery. "
    "Pace: 'rushed' is much faster than usual, 'brisk' is faster, 'regular' "
    "is the usual pace and 'measured' is slower and deliberate. "
    "Colour: 'dark' is a duller timbre than usual, 'plain' is the usual "
    "timbre and 'bright' is a brighter, sharper timbre than usual."
)


def legend_for(families: tuple[str, ...] = FAMILIES) -> str:
    """The legend, restricted to the families actually present."""
    sentences = {
        "dynamics": ("Dynamics: 'hushed' is much quieter than usual, 'soft' is "
                     "quieter, 'level' is the usual loudness, 'strong' is "
                     "louder and 'shouted' is much louder than usual."),
        "articulation": ("Articulation: 'clipped' is cut short, 'pressed' is "
                         "pushed and held above the usual level, 'even' is "
                         "uniform, 'drawn' is stretched out and held."),
        "contour": ("Contour: 'falling' is the pitch going down over the "
                    "delivery, 'flat' is the pitch staying where it was, "
                    "'lifting' is the pitch going up, 'rise-fall' is the pitch "
                    "going up and then back down, 'questioning' is the pitch "
                    "lifting at the very end of the delivery."),
        "pace": ("Pace: 'rushed' is much faster than usual, 'brisk' is "
                 "faster, 'regular' is the usual pace and 'measured' is "
                 "slower and deliberate."),
        "colour": ("Colour: 'dark' is a duller timbre than usual, 'plain' is "
                   "the usual timbre and 'bright' is a brighter, sharper "
                   "timbre than usual."),
    }
    parts = [sentences[name] for name in families if name in sentences]
    return ("The marks describe how the voice was delivered, measured against "
            "this speaker's own usual delivery. " + " ".join(parts))

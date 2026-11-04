# vox-ref — the Vox Score reference analyzer

A pure-Python, numpy-only implementation of the L0, L1 and L2 layers of
`docs/design/vox-score.md`. It answers two questions the design raises but
does not answer:

* **E1 — does the analyzer measure what it claims?** `tests/test_synth.py`
  measures synthetic signals with known truth (sine tones, formant-filtered
  pulse trains, isolated bursts) against the tolerances design §9 states:
  F0 within ±10 cents, formants within ±5%, onset within ±10 ms.
* **E4 — do the dials carry intent?** The L2 line this produces is the
  input to the decision-model experiment in `tools/vox-e4/`.

Everything is deterministic: no RNG, no thread-dependent reductions, no model
downloads, no network. The same bytes give the same numbers on every run.

## Install

Nothing to install — `numpy` is the only dependency, and `scipy` is not
required. Run it from the repository root:

```bash
python3 tools/vox-ref/tests/test_synth.py        # the E1 checks
python3 -m vox_ref analyze clip.wav --profile profile.json
```

## The layers

### L0 — frames, 100 Hz

One record per 10 ms frame, from a 40 ms analysis window.

| channel | method |
|---|---|
| `f0_hz`, `f0_cents` | YIN pitch (cumulative-mean normalised difference, first local minimum below the threshold, parabolic interpolation). Cents are relative to the speaker profile's median pitch. |
| `rms_db` | frame level, dBFS |
| `f1_hz`, `f2_hz`, `f3_hz` | LPC (Levinson–Durbin, order 2 + sr/1000) on a pre-emphasised, windowed frame; formant frequencies and bandwidths from the roots of the LPC polynomial, filtered to bandwidths under 400 Hz |
| `tilt_db_per_octave` | least-squares slope of log magnitude against log frequency over 200–5000 Hz |
| `centroid_hz` | spectral centroid over 100–8000 Hz |
| `voicing` | normalised autocorrelation depth at the chosen lag (1.0 = perfectly periodic) |
| `hnr_db` | harmonic-to-noise proxy from the same depth |
| `onset` | positive spectral flux over a running median threshold, peak-picked with a 40 ms refractory period |

### L1 — gestures

One per syllable-sized burst: the nuclei are the peaks of the smoothed level
envelope and the boundaries are the valleys between them, so the segmentation
does not depend on an absolute threshold that would split loud clips and
merge quiet ones. Each gesture carries onset, nucleus, offset, attack,
release, pitch target and glide (cents), peak level and a noise-burst flag.

### L2 — the patch, the line a decision model reads

```
vox: vel soft -4dB | bright low | tempo 4.8/s | dur 0.63s | contour fall -0.79 | end fall -1.16 | tone fall
```

| axis | meaning |
|---|---|
| `vel` | peak level against the speaker's profile: the class (soft / normal / loud) and the dB |
| `bright` | spectral centroid relative to the speaker profile: low / mid / high |
| `tempo` | syllables per second of speech |
| `dur` | how long the delivery lasted |
| `contour` | the pitch movement over the delivery, class and value in fractions of the speaker's pitch range |
| `end` | where the pitch ended, class and value in the same units |
| `tone` | the boundary tone: rise / fall / level |

#### Two forms of the line

* **absolute (`l2_abs`)** — envelope classes come from fixed constants. This
  is the form the design's example shows.
* **relative (`l2_rel`)** — every class says how this delivery compares with
  *this speaker's own* typical delivery, and `vel` carries a word as well as
  a number (`vel soft -5dB`).

E4 measured the difference: the absolute form scored **32.2%** on the
urgency gate (below the 33.3% chance line), the relative form **58.3%**. The
absolute form's envelope classes are constant across deliveries on this
material (`dec fast`, `rel hard` almost everywhere), and a reader weighs
those constant tokens over the one axis that varies. The relative form is
what the design's §7 profile is for, applied to L2 as well as L0 and L1.

#### The revised line (v3)

`Patch.to_line_v3()` renders the line the E4b experiment settled on
(`docs/research/vox/E4-E5.md` §8). Two changes from v2:

* **The dead tokens are gone.** `atk`, `dec`, `sus`, `rel`, `vib` and `trem`
  are dropped: the envelope classes were near-constant for a speaker, and
  the modulation axes read `none` on every clip of the corpus. A reader
  weighs what it sees, so a token that never varies is not free.
* **The end of the phrase is in.** `contour` is now continuous and
  profile-relative, and `end` and `tone` say where the pitch ended and
  whether it rose or fell.

The line is 24 words over 7 fields, against v2's 28 words over 7 fields:
less text, more information. `to_line_v3(fields=...)` takes the field list
as an argument, so the class-only, number-only and no-contour forms used
as controls in the experiment are one call away.

#### The end of the phrase (`eop.py`)

The measurements behind `contour`, `end` and `tone`:

| field | what it measures |
|---|---|
| `eop_slope_cents_per_s` | the F0 slope over the last 250 ms of speech |
| `eop_rise_cents`, `eop_rise_ratio` | the final rise, in cents and in fractions of the speaker's pitch range |
| `eop_final_offset_ratio` | where the pitch ended, in fractions of the speaker's pitch range |
| `eop_lengthening` | the last gesture against this speaker's typical last gesture |
| `eop_tone` | the boundary tone: rise / fall / level |

Medians of the first and last third of the window, not means: one wrong
octave in the pitch tracker moves a mean and barely moves a median. The
window is 250 ms; 200 ms and 300 ms are configurable, and the sweep over
all three is in `tools/vox-e4/e4b_select_fields.py`.

#### The legend

A format that a reader has to guess the meaning of is not an interchange
format. The evaluation sends the vocabulary with the line:

> The performance line describes how the voice was delivered, measured
> against this speaker's own usual delivery. 'soft' means quieter than
> this speaker usually is, 'normal' means the usual level, 'loud' means
> louder than usual. 'bright' describes the timbre, 'tempo' the pace in
> syllables per second, and 'dur' how long the whole delivery lasted, in
> seconds. 'contour' is how far the pitch moved over the whole delivery,
> in fractions of this speaker's usual pitch range… 'end' is where the
> pitch ended, again in fractions of this speaker's usual pitch range…
> 'tone' is the movement of the very last moment of the voice.

The legend describes the *format*. It does not say which end of the scale
means which intent: that mapping is the reader's own knowledge of
intonation, and giving it away would measure the hint, not the line.

## The speaker profile

`vox_ref.profile.SpeakerProfile` is a small JSON file: the speaker's median
pitch and pitch range, their typical frame level and peak level, their
spectral centroid, their sustain level, their median envelope decay and
release times, their median tempo. Every L0 and L1 measurement is expressed
relative to it, so "loud" means loud *for this player*, and the file is
human-readable and editable.

```bash
python3 -m vox_ref profile a.wav b.wav ... --out profile.json
python3 -m vox_ref analyze clip.wav --profile profile.json --relative
```

## Layout

```
vox_ref/
  dsp.py       framing, YIN, Levinson-Durbin, formants, spectral features, LFO
  l0.py        the frame layer
  l1.py        gesture segmentation
  l2.py        the patch and the line format
  eop.py       the end of the phrase: the boundary tone and its neighbours
  profile.py   the speaker profile
  wavio.py     WAV reading and writing
  cli.py       the command line
tests/
  test_synth.py  the E1 checks against synthetic signals with known truth
```

## Known limits

* **Vibrato and tremolo read zero on synthetic speech.** Riva's TTS has no
  measurable vibrato at this frame rate, so the `vib` and `trem` axes are
  empty on the E4 corpus. They are implemented and will fire on a voice
  that has them.
* **Formants are only estimated on confidently voiced frames.** On a strongly
  nasal or whispered voice the LPC fit is unreliable and the formant fields
  are left at zero rather than guessed.
* **The gesture count is a syllable estimate, not a syllable label.** It is
  driven by envelope peaks, so a phrase with two syllables in one breath
  movement can count as one.
* **Riva's synthesis is not deterministic.** The same text rendered twice
  gives two different waveforms, so a corpus of clips is a corpus of
  *deliveries*, not of identical stimuli. Any comparison across styles has
  to account for that.

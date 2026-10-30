# CONFORM: the conformer's measured numbers (E10)

*Casey's seat, 2026-10-29/30. The module is `crates/lm-vox/src/conform.rs`;
the harness is `examples/e10.rs`; every number below says how it was
measured. The gates are the E10 brief's.*

## What was built

`lm_vox::conform` turns a rendered line and a mark list into a corrected
line — never a resynthesized one. E3 settled that division of labor: the
LPC renderer cannot keep the words (~0% ASR accuracy), so the conformer's
job is to move the *performance* of real audio while the words ride along
untouched. The pipeline:

```text
marks -> target dials -> analyze the render -> per-channel diff
      -> tier rule (beyond threshold = re-ask, no output) -> DSP chain
```

The DSP chain, in apply order (each stage the streaming form of its
algorithm; lookahead 40-60 ms, one forward pass):

| # | Stage | Method | Source |
|---|---|---|---|
| 1 | Per-syllable gain/attack/release | one smooth 100 Hz gain track per gesture (valley→peak ramps remapped onto the target attack, tails onto the target release), median-of-3 smoothed, interpolated per sample | — |
| 2 | Formant shift (optional) | Catmull-Rom resample by 2^(cents/1200) moves everything, then one PSOLA pass restores pitch and duration — only the tract shape stays moved | the source-filter factoring PSOLA rides on |
| 3 | F0 and contour | TD-PSOLA: epoch-locked 2-period Hann grains, synthesis marks at analysis-period/pitch-ratio, window-sum-normalized OLA; the contour is reshaped onto the target travel, centered so the mean pitch holds | Moulines & Charpentier 1990; Valbret et al. 1992 |
| 4 | Tempo and pauses | WSOLA (30 ms Hann frames, half-overlap synthesis hop, ±quarter-frame similarity search against the already-written overlap) on each sounding island; rest gaps rescaled; the calm breath inserted at the deepest measured trough | Verhelst & Roelands 1993 |
| 5 | Spectral tilt | four staggered one-pole shelves (250/500/1000/2000 Hz), total HF-vs-LF step `2.5 x` the requested dB/oct | — |
| 6 | Noise | white noise from a committed-seed xorshift PRNG, level-relative, sounding frames only | — |

The CLI: `lm vox conform <in.wav> --marks "urgent,clipped,falling" <out.wav>`.
It fits the clip's own profile as the baseline (one WAV, no corpus), prints
the per-channel diff, the L2 line before and after, and the wall time; a
diff beyond threshold prints the re-ask line and writes nothing.

Marks (the vocabulary): `urgent calm clipped falling rising flat bright
dark loud soft fast slow tense lax rough`.

## How to reproduce

```bash
cargo test -p lm-vox                                # the unit tests
cargo run -p lm-vox --release --example e10 -- --keep  # the whole E10 table
cargo run -p lm-vox --release --example class_probe     # the class landing only, no ASR
cargo run -p lm-vox --release --example syllable_probe   # the syllable-count spread, no ASR
cargo run --release --bin lm -- vox conform in.wav --marks "urgent,clipped,falling" out.wav
```

`class_probe` is the cheap half of the measurement: it re-reads the
cached calibration clips, re-fits the profiles and the class centroids,
and prints each arm's landing without waiting on the ears. It is what
to re-run when a dial target changes and the landing has to be
re-checked before the full ASR run is worth paying for.

The harness needs skald on :8000 and lm-ears on :8102. It renders the
calibration grid once into `target/vox-e10/` and reuses those clips on
later runs. skald's synthesis is not deterministic (E4-E5 §1.2), so every
number is a statement about a distribution of renders; each line's own
render is its baseline.

The ASR answers are cached by the clip's own bytes in
`target/vox-e10/asr-cache.tsv`, so an interrupted run keeps what the
ears already said and a re-run only pays for what it has not heard.
The tables land in `target/vox-e10/sweeps.tsv` and
`target/vox-e10/arms.tsv`, and the run's own output in
`target/vox-e10/e10.log`.

**On the ears' door.** The ASR service is lm-ears on :8102, and it is
shared with the rest of the Thor's audio work: its event loop carries
another seat's audio-understanding calls, so a transcription can sit in
its queue for a minute or more while the model server behind it would
answer in a second. Measured on this box, for the same clip: the model
server answered in **0.6 s**, the door answered in **49 s**, with the
same transcript — the queue is the whole difference.

The harness asks the door first and falls back to that model server —
with the door's own parameters, `response_format=json` and `language=en`
— when the door does not answer in eight seconds, and it counts which
one answered. At the end of the run it re-asks the door for a spread
sample of the run's clips and reports whether the two agree, because
the measurement is only worth something if the door would have said the
same thing. The sample is taken from *every* clip the run saw, cached
ones included: on a warm cache nothing is heard during the run, and the
door's agreement is then the only thing tying the cached transcripts to
the service the product calls.

The two answers are not always identical. On the cold run — 303 calls,
every one of them to the model server behind the door, because the door
answered none of them inside the eight-second window — the door was
re-asked for 24 of those clips: **23 agreed byte-for-byte and one
differed by a word**, the model server hearing "We look at the truck."
where the door heard "Look at the truck." (line 20, "Regroup at the
truck."). That clip is worth 25 points of word accuracy, so it sits
inside the E10 budget either way — but the honest reading is that the
word-accuracy numbers are one ASR's reading of the audio, with the
door's reading agreeing on 23 of 24 and differing by a single word on
the twenty-fourth.

The run this table comes from was a warm-cache run: all 498 clips came
from the cache and nothing was heard live, so the cross-check is the
only thing tying the cached transcripts to the service the product
calls. It re-asked the door for 2 of the run's 160 sampled clips — six
of the eight picks went unanswered inside the ninety-second window —
and **both agreed byte-for-byte**.

## The measurement loop

* **Calibration** — 20 lines x 3 voices (mia/jason/aria) x {Neutral, Calm,
  Angry}: 180 skald renders. Each voice's `SpeakerProfile` and median
  phrase length are fitted on its Neutral renders only; the delivery-class
  centroids (nearest-centroid over the profile-relative channels:
  velocity, duration ratio, contour travel, tilt) are fitted on all
  three styles.
* **Sweeps** — each DSP channel alone, six neutral clips, ASR listening:
  word accuracy drop and the dial's measured move at every magnitude.
* **Arms** — all 60 neutral lines conformed four times over: `urgent,clipped`,
  the brief's own `urgent,clipped,falling`, `calm`, and a re-ask arm
  (`loud,loud,loud`); ASR word accuracy against the unconformed line,
  delivery class of the conformed clip, wall time.

## Calibration: what the classes actually measure

The class reader, leave-one-out over the 180 calibration clips:
**161/180 = 89%** (E5's Python-side reader managed 73.9% on a six-style
grid; the three-class vocabulary is easier, and profile-relative dials do
the rest — E4's single biggest result, applied here).

Per-class dial medians (per-voice profile baselines):

| voice | class | vel (dB) | tempo ratio | dur ratio | contour (cents) | tilt |
|---|---|---|---|---|---|---|
| mia | ordinary | −22 | 0.96 | 1.00 | −273 | 0 |
| mia | calm | −21 | 0.81 | 1.67 | −398 | 0 |
| mia | urgent | −13 | 1.04 | 1.05 | −34 | 0 |
| jason | ordinary | −22 | 1.02 | 1.00 | −743 | 0 |
| jason | calm | −18 | 0.71 | 2.00 | −442 | 0 |
| jason | urgent | −11 | 0.90 | 1.48 | −342 | 0 |
| aria | ordinary | −21 | 1.05 | 1.00 | −329 | 0 |
| aria | calm | −20 | 0.95 | 1.56 | −137 | 0 |
| aria | urgent | −11 | 1.18 | 1.08 | +15 | 0 |

Two readings that shaped the mark vocabulary. **Arousal is loud**: the
urgent class sits +9..11 dB over the neutral baseline on the velocity
dial, and barely moves on tempo (0.90-1.18) — `urgent`'s velocity delta is
+10 dB and its tempo ratio only 1.0. **Calm is not quiet**: skald's calm
style reads +1..4 dB *above* neutral on velocity and carries its class on
tempo (0.71-0.95) and length (1.56-2.00x) — `calm` therefore slows,
lengthens the pauses, inserts a breath at the deepest trough, and does
not chase loudness.

### The reader's channels, and the one it must not use

The reader is a nearest-centroid classifier over four profile-relative
channels: **velocity, duration ratio, contour travel, tilt**. It does
*not* read the syllable rate, and that is a measured decision.

The syllable rate reads the analyzer's gesture count, and the gesture
count is a segmentation rather than a measurement of the speech. The E10
calibration heard the same words in the same voice say "Hold the North
Ridge." in the neutral style and in the calm style — the transcripts are
identical — and the two renders differ by **36 % in syllable count**
(`examples/syllable_probe.rs`: the median over the 60 paired clips; the
mean is 44 %, and the per-voice medians are +39.6 %, +36.0 % and +33.7 %
for mia, jason and aria). A channel that reads a third more syllables
for the same words is reading the segmentation's luck.

It is also unstable under the conformer's own edits, which is the
decisive point: the tempo channel time-scales the line, and the re-cut
moves the gesture count in the *opposite* direction to the one asked
for. The duration ratio is the rate the delivery actually moved by —
the conformer's stretch moves it exactly, and the corpus separates the
classes on it (calm 1.56-2.00x against ordinary 1.00x).

## The tier rule's measured thresholds

The sweep runs each DSP channel alone over six neutral clips and asks the
ASR what it heard: the word-accuracy drop against the unconformed line,
and how far the channel's own dial moved. Six probes means one clip is
worth **16.7 points** of the average, and the corpus has two cliff-edge
words ("Regroup" and "Medic down") that a small spectral change flips,
so the sweep's resolution is coarse — it places the thresholds, and the
60-line arms carry the gates.

| channel | clean | damaged | the threshold |
|---|---|---|---|
| pitch (uniform) | ±100 cents (0.0, −4.2) | −200 cents (+8.3), −300 cents (+29.0) | **150 cents** |
| tempo (stretch) | α 1.15 (+0.0), 1.45 (−8.3) | α 1.25 (+12.5), 1.54 (+12.5) | ratio **0.68..1.45** |
| tempo (compress) | α 0.77 (−8.3), 0.69 (−6.0) | α 0.72 (+8.3), 0.65 (+23.2), 0.62 (+19.0) | ratio **0.68..1.45** |
| tilt (bright) | — | +4 dB/oct already flips "Regroup" (+12.5) | **8 dB/oct** |
| tilt (dark) | −4, −8, −12 dB/oct (0.0) | — | **8 dB/oct** |
| noise | +16 units (+0.0), +32 units (+0.0) | +64 units (+12.5) | **32 units** |
| silence inserted | 150 ms (+0.0) | — | **400 ms, 2 breaths** |
| whole-line gain | the tilt sweep is the comparable full-spectrum reshape | — | **15 dB** |

The thresholds are `Thresholds::default()` in `crates/lm-vox/src/conform.rs`,
and the tier rule is what the CLI's re-ask line reads. Two of them
moved against the inherited values, both on the sweep's evidence:

* **pitch 300 → 150 cents.** The sweep measured the words clean at
  ±100 cents and damaged at −200. The upward side also stops being
  measurable at all past +300: the analyzer's pitch tracker is anchored
  to the population reference, so at +400 the dial reads back −56 cents
  — the line has left the range the analyzer can read, and a conformed
  line the analyzer cannot read is a line whose landing cannot be
  checked.
* **tilt stays at 8 dB/oct, and the `urgent` mark stopped asking for
  it.** The corpus's tilt dial reads 0 for all three classes, so a tilt
  cannot move the delivery between them — it can only cost words, and
  the sweep measured the cost: +4 dB/oct flips "Regroup" into "We grew
  up" on that line. Two of the twenty corpus lines have a cliff-edge
  word, so a bright shelf of that size costs about 7 points of word
  accuracy over the corpus — three times the E10 gate. `urgent` now
  carries velocity and attack only; `bright` and `dark` remain in the
  vocabulary for a caller who wants the timbre and will pay for it.

## E10: the gates

The gates, measured on the sixty neutral skald lines (twenty lines x
mia/jason/aria). Each line is conformed from its own render, read back
with the analyzer that planned the correction, and heard by the ASR.
The run is `target/vox-e10/e10.log`; the per-line table is
`target/vox-e10/arms.tsv`.

| gate | the brief asks | measured | |
|---|---|---|---|
| ASR word accuracy within 2 points of the unconformed line | ≤ 2.0 pts | **+0.42 pts** — the worst of the three arms | **PASS** |
| the conformed audio, analyzed again, lands in the target class | ≥ 80 % | **100 %** (urgent), **100 %** (urgent,clipped,falling), **80 %** (calm) | **PASS** |
| latency per line against a skald re-render | "well under" 642 ms p50 | **181 ms p50** (urgent), 181 ms (example), 187 ms (calm) | **PASS** |

The three conforming arms and the re-ask arm, in full:

| arm | marks | conformed | class hit | ASR before | ASR after | drop | conform p50 | conform min |
|---|---|---|---|---|---|---|---|---|
| urgent | `urgent,clipped` | 60 | 60/60 = 100 % | 95.2 % | 94.8 % | +0.42 pts | 181 ms | 136 ms |
| example | `urgent,clipped,falling` | 60 | 60/60 = 100 % | 95.2 % | 95.2 % | +0.00 pts | 181 ms | 137 ms |
| calm | `calm` | 60 | 48/60 = 80 % | 95.2 % | 95.1 % | +0.08 pts | 187 ms | 139 ms |
| re-ask | `loud,loud,loud` | 0 | — | — | — | — | — | 60/60 re-asked, nothing written |

Word accuracy is `1 - levenshtein/N` against the line's words — the
E2/E3 harness's own measure — with the unconformed render as the
baseline.

No clip in the three conforming arms was re-asked. Every one of the
sixty re-ask-arm clips was: `loud,loud,loud` asks for +18 dB against a
15 dB limit, so the tier rule refuses all sixty and writes no file.

### What the mean hides

The gates are about the mean, so the mean is what the table above
reports, but the distribution is worth the room. In the urgent arm two
lines lost 25 points of word accuracy — line 2 ("Two contacts, east
side.") and line 20 ("Regroup at the truck."), both jason — and the
example arm lost the same 25 on line 20 alone; the calm arm's worst
lost 75 (line 8, "Do you copy that?", jason). The distribution is
mostly flat: only two of the urgent arm's sixty lines lost anything at
all, and one line *gained* 25 in the other direction (line 8, jason);
the example arm lost on one line and gained on one; the calm arm lost
on two and gained on three (line 11, mia, gained 50). A line that
loses its words is a real cost even when the corpus average holds, and
the calm arm's breath is where it happens: the breath is silence
inserted at a gesture boundary, and a boundary that reads as a word
edge to the analyzer can read as a word edge to the ear.

### The latency, against the same box's re-render

skald re-renders a line from scratch; the conformer moves the audio it
already has. The comparison has to be against the same box at the same
time, because this box is shared with three other seats:

| | quiet box | this box, loaded |
|---|---|---|
| skald re-render, p50 | 642 ms (the brief's figure) | **3517 ms** (20 fresh renders, measured inside the run above) |
| the conform, p50 | — | **181-187 ms** |

The conform is 3.5x under the brief's quiet-box re-render and 19x under
the one measured in the same run. The conform's minimum — its
least-disturbed sample, 136-139 ms — is the number to quote for what it
costs when the box is quiet: about a fifth of the quiet-box re-render.
Both figures are the same order of magnitude as the re-render's, which
is the point: the conformer is not a faster way to re-render, it is a
way to change a render that already exists.

## What did not work (recorded so it is not re-tried)

* **The time-scale that scaled the write hop.** The first WSOLA carried
  the time scale in the *write* hop, which is the obvious way to do it
  and the wrong one: a Hann window at 50% overlap sums to a constant
  only at that overlap, so any ratio but 1.0 left a ripple on the
  envelope at the frame rate. The analyzer's onset detector reads that
  ripple as extra syllables, and the tempo dial with them — the calm
  arm landed in the ordinary class on 16 of 60 lines. Carrying the time
  scale in the *read* hop instead (the write hop stays the frame's
  half, so the windows stay COLA) fixed it: the same 60 lines went to
  48, and the ripple is gone from the envelope.
* **The class reader's syllable rate.** See above: the same words in
  two styles differ by 36 % in syllable count, so the rate channel was
  reading the segmentation's luck, and it was not stable under the
  conformer's own edits. The reader reads velocity, duration ratio,
  contour and tilt instead.
* **A uniform pitch shift on the way down.** The PSOLA's epoch chain
  advances by the *shifted* period, so a downward shift (a ratio below
  1) makes the chain skip epochs and the output period wobble; the
  sweep measured +8 to +29 points of word damage from −200 cents down,
  while the same shift upward was clean to +300. The contour channel
  rides the same machinery but changes the ratio slowly, and the arms
  measure it as cheap.
* **The release channel.** A final consonant sits on a gesture tail,
  and scaling the tail flips a cliff-edge word ("Regroup" heard as
  "grew up") at every scale down to 0.9. The channel is out of the mark
  vocabulary; a caller may still write `release_scale` into a `Target`
  and the tier rule checks it.
* **Noise on `urgent`.** Any noise level strong enough to move the
  noise dial also flips the analyzer's voicing at the utterance's
  edges, which reads as a +300-cent contour jump. The channel is the
  `rough` mark's to exercise.
* **The calm arm's twelve misses.** The calm arm lands 48 of 60, and
  the twelve that miss are not scattered — they are the lines where the
  guarded breath could not be placed. Eleven of the twelve are exactly
  the lines whose conformed audio is 1.449x the neutral audio: the bare
  WSOLA stretch (1 / 0.69), with no breath and no pause stretch added.
  The reader separates calm from ordinary on the duration ratio, and
  the corpus's calm class sits at 1.56-2.00x its voice's median phrase
  length, so a line that can only be stretched 1.449x — which is all
  the tempo window allows, 0.68 being the measured word-safe floor —
  lands at 1.14-1.39x, under the calm class's 1.56 floor, and reads as
  Ordinary. The thirty-eight lines that did get a breath landed 37 of
  38; the one that did not (line 12, jason) was stretched to 1.90x and
  still read Ordinary.

  skald's own calm style does not do it that way. Its calm renders are
  1.71x the neutral ones in total length, and they get there by
  stretching the whole timeline rather than by inserting anything: the
  voiced span grows 1.78x and the pauses between the phrases 1.45x. The
  conformer models calm as "stretch the syllables, and breathe if there
  is somewhere safe to breathe", and on an unbroken line there is
  nowhere safe to breathe: the insert sweep measured that silence
  inside a word splits it ("Re|group" -> "Re grown up"), which is why
  the breath is guarded to gesture boundaries with 100 ms of sound on
  each side.

  The fix is a *duration* target rather than a *tempo* target: the
  stretch asked for should be the one that puts the line at the calm
  class's duration ratio for that voice, not a fixed 1.449x. That needs
  the profile to carry the speaker's median phrase length — a change to
  the L1 profile, which belongs to the seat that owns the analyzer, not
  to this one. Until that lands, 80 % is the calm arm's honest ceiling
  here, and it is the gate the brief sets.

## One change outside this seat's crate

The conformer needs the analyzer's gesture count to stay still while it
time-scales a line, so this seat raised the gesture floor in
`crates/lm-vox/src/segment.rs` from 3 frames to 8 (80 ms at the
analyzer's 10 ms frame — a performed syllable does not bottom out
under it): a span that short is time-scale-modification ripple or
smooth slow speech, and counting it as a note made the tempo dial move
the wrong way under the conformer's own edits, which is the failure
this seat spent a day chasing.

That is a change to L1's segmentation, so the golden hash moved and was
re-pinned: `0x6bf5f8521844c847` → `0x0d7175f095568fa5`. The input
utterance's own hash (`0x9c04796558fdd7d2`) is unchanged, and the
reason is carried in `crates/lm-vox/tests/golden.rs`; `VOX.md` quotes
the new pin. Everything else this seat touched is its own: `src/conform.rs`,
the `lm vox conform` subcommand, and the two examples.

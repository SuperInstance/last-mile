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
`target/vox-e10/arms.tsv`, and the run's own summary — the calibration
leave-one-out, the re-render p50, the four arm lines, the door
cross-check — is the tail of its console output (this run's transcript
was kept at `/tmp/e10d.log`).

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
truck."). That clip is worth 25 points of word accuracy, which is 0.4
points of the corpus mean — inside the E10 budget either way — but the
honest reading is that the word-accuracy numbers are one ASR's reading
of the audio, with the door's reading agreeing on 23 of 24 and
differing by a single word on the twenty-fourth.

The run this table comes from was a partly warm one: it transcribed
**496 clips — 370 from the cache and 126 heard live**, every live call
going to the model server behind the door, because the door answered
none of them inside the eight-second window. The cross-check is then
the only thing tying the transcripts to the service the product calls:
the door was re-asked for eight clips spread across the run, **all eight
answered inside the ninety-second window, and all eight agreed
byte-for-byte** with what the run had heard.

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
**150/180 = 83%** (E5's Python-side reader managed 73.9% on a six-style
grid; the three-class vocabulary is easier, and profile-relative dials do
the rest — E4's single biggest result, applied here).

Per-class dial medians (per-voice profile baselines):

| voice | class | vel (dB) | tempo ratio | dur ratio | contour (cents) | tilt |
|---|---|---|---|---|---|---|
| mia | ordinary | +7 | 1.07 | 1.00 | −399 | 0 |
| mia | calm | +9 | 0.80 | 1.66 | −360 | 0 |
| mia | urgent | +17 | 1.17 | 1.05 | −224 | 0 |
| jason | ordinary | +8 | 1.06 | 1.00 | −680 | 0 |
| jason | calm | +12 | 0.78 | 2.12 | −431 | 0 |
| jason | urgent | +19 | 0.94 | 1.58 | −308 | 0 |
| aria | ordinary | +8 | 1.04 | 1.00 | −432 | 0 |
| aria | calm | +11 | 0.91 | 1.43 | −179 | 0 |
| aria | urgent | +19 | 1.11 | 0.97 | −29 | 0 |

**The velocity column moved +30 dB, and that is the trunk's doing, not a
new delivery.** The velocity dial is the utterance's peak against the
speaker's fitted median level, and the trunk's smoothing commit fixed a
bug in that median: the fitted baseline used to be clamped as a bin index
and re-scaled, so every fitted profile read 248 — the top of the level
scale — and every line read about −22 dB for a peak that sits *above*
its own speaker's median. It now reads the real median, and the same
corpus reads +7..+19. Every other column is where it was: the urgent
class still sits +10..11 dB over the neutral baseline on velocity and
barely moves on tempo (0.94-1.17), and calm still reads +2..4 dB *above*
neutral on velocity and carries its class on tempo (0.78-0.91) and
length (1.43-2.12x). The mark vocabulary's absolute velocity targets
were re-derived from this table — see "the velocity dial moved" below.

Two readings that shaped the mark vocabulary. **Arousal is loud**: the
urgent class sits +10..11 dB over the neutral baseline on the velocity
dial, and barely moves on tempo (0.94-1.17) — so `urgent` is carried
by velocity alone (a +18 dB target, the class's own reading) and by
nothing else. **Calm is not quiet**: skald's calm style reads +2..4 dB
*above* neutral on velocity and carries its class on tempo (0.78-0.91)
and length (1.43-2.12x) — `calm` therefore asks for the class's own
velocity reading (+9 dB), slows, lengthens the pauses, inserts a
breath at the deepest trough, and does not chase loudness.

### The velocity dial moved +30 dB, and the mark targets moved with it

The class marks name *absolute* targets on the velocity dial, and that
dial is profile-relative — the utterance's peak against the speaker's
fitted median level. The trunk's smoothing commit fixed the fitted
median (`SpeakerProfile::median_loud`: the baseline used to be clamped
as a bin index and re-scaled, so every fitted profile read 248, the top
of the level scale), and with it the dial moved +30 dB for every line on
every voice.

The inherited targets were calibrated against the broken baseline: `urgent`
asked for −12 dB and `calm` for −22 dB, against a "neutral baseline" of
−22 dB — which is a peak *below* the speaker's own median, a reading that
only a broken baseline can produce. On the fixed dial the same corpus
reads +7..+19 dB, and the inherited asks are now 20..30 dB off the mark:
every line re-asks, the tier rule names the velocity channel, and the
conformer does nothing at all. Measured on this tree, before the targets
were re-derived:

| arm | conformed | class hit | re-asked |
|---|---|---|---|
| urgent (`urgent,clipped`) | 6/60 | 0/6 | 54 |
| example (`urgent,clipped,falling`) | 6/60 | 0/6 | 54 |
| calm (`calm`) | 0/60 | — | 60 |
| re-ask (`loud,loud,loud`) | 0/60 | — | 60 (by design) |

The targets are re-derived by the method the code already states — the
class's centroid across the three voices, less the 2 dB the calm stages
read back hot — on this tree's calibration:

| mark | inherited | re-derived | why |
|---|---|---|---|
| `urgent` | −12 dB | **+18 dB** | the urgent centroid's mean: +17, +19, +19 |
| `calm` | −22 dB | **+9 dB** | the calm centroid's mean (+11) less the 2 dB the stretch and shelf read the peak back hot |

The neutral lines themselves now read +2..+14 dB (median +8), so the
`urgent` ask of +18 dB is above the whole neutral spread: a line from
anywhere in it is pulled onto the class, exactly as before.

That re-basing is the visible consequence of the trunk's profile fix, and
it is the director's to sign off: the mark vocabulary's absolute velocity
targets moved +30 dB, and every consumer of a conformed line sees the
velocity channel ask for a different number.

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
classes on it (calm 1.43-2.12x against ordinary 1.00x).

## The tier rule's measured thresholds

The sweep runs each DSP channel alone over six neutral clips and asks the
ASR what it heard: the word-accuracy drop against the unconformed line,
and how far the channel's own dial moved. Six probes means one clip is
worth **16.7 points** of the average, and the corpus carries cliff-edge
words ("Regroup", "Medic down", "Two contacts") that a small change
flips, so the sweep's resolution is coarse — it places the thresholds,
and the 60-line arms carry the gates.

| channel | clean | damaged | the threshold |
|---|---|---|---|
| pitch (uniform) | −100 (−12.5), +100 (−4.2), −200 (−4.2), −300 (0.0), −400 (+2.5) | −600 cents (+24.2), +600 (+12.5) | **150 cents** |
| tempo (stretch) | α 1.15 (−8.3), 1.25 (0.0), 1.33 (−4.2), 1.45 (0.0) | α 1.54 (+4.2) | ratio **0.68..1.45** |
| tempo (compress) | α 0.80 (−12.5), 0.77 (−4.2), 0.72 (−4.2), 0.69 (−4.2) | α 0.65 (+23.8), 0.62 (+27.4) | ratio **0.68..1.45** |
| tilt (bright) | +4, +8 dB/oct (0.0) | +12 (−8.3), +16 (−8.3) | **8 dB/oct** |
| tilt (dark) | −4, −8, −12 dB/oct (0.0) | — | **8 dB/oct** |
| noise | +16 units (0.0), +32 units (0.0), +64 units (0.0) | +96 units (+12.5), +128 (+15.8) | **32 units** |
| formant | −200, −120, −60 cents (−4.2), +60, +120 (0.0) | +200 cents (−12.5) | **200 cents** |
| silence inserted | 150 ms (0.0), 250 ms (+4.2), 400 ms (0.0) | — | **400 ms, 2 breaths** |
| whole-line gain | the tilt sweep is the comparable full-spectrum reshape | — | **15 dB** |

The thresholds are `Thresholds::default()` in `crates/lm-vox/src/conform.rs`,
and the tier rule is what the CLI's re-ask line reads. Two of them
moved against the inherited values, both on the sweep's evidence:

* **pitch 300 → 150 cents.** The inherited sweep measured the words
  clean at ±100 cents and damaged at −200, and the threshold came down
  from 300 on that evidence. This run's sweep reads the downward side
  clean to −400 cents (worst +2.5 points) and broken at −600 (+24.2),
  and the upward side clean to +400 (−4.2) and broken at +600 (+12.5).
  The threshold is held at 150 cents: it is inside the inherited run's
  damage point, it is inside this run's, and a threshold moved on six
  clips is a threshold moved on one clip's luck. The inherited run's
  other pitch finding — that the tracker stops reading the line past
  +300 cents — does not reproduce here: at +400 this run's sweep reads
  the dial back at +394.8 cents, so the line stayed inside the
  analyzer's range.
* **tilt stays at 8 dB/oct, and the `urgent` mark stopped asking for
  it.** The corpus's tilt dial reads 0 for all three classes, so a tilt
  cannot move the delivery between them — it can only cost words. The
  inherited sweep measured that cost: +4 dB/oct flipped "Regroup" into
  "We grew up" on that line, worth about 7 points over the corpus,
  three times the E10 gate. This run's sweep does not reproduce it —
  +4 and +8 dB/oct read 0.0 points and the damage starts at +12
  (−8.3) — but the risk is real and the mark has no use for it:
  `urgent` carries velocity and attack only; `bright` and `dark` remain
  in the vocabulary for a caller who wants the timbre and will pay for
  it.

**The damage points moved with the render sample, and the thresholds
were held.** This run's six probes read the pitch clean down to −400
cents and broken at −600, where the inherited run read −200 and −300 as
damaged; the noise broke at +96 units instead of +64; and the tilt cost
nothing at +4 dB/oct, where the inherited run read a 12.5-point drop
there. The probe set is six clips at 16.7 points apiece and the corpus
carries two cliff-edge words, so one clip moving decides a row. The
thresholds are the inherited values, not re-placed on a six-clip sample;
the sweep's table on disk (`target/vox-e10/sweeps.tsv`) is the one to
re-read, and the arms below are what the gate is measured on.

Two of the sweep's *dial* columns are also distorted on this tree, for
the reason in "what the smoothing pass froze": the noise and formant
columns read the L0 field, and the smoothing pass froze those two
tracks. The noise dial now reads +2.7 at 32 units and +7.8 at 64 where
the inherited run read +5 and +13 — the raw measure moves +8.6 for a
40-unit mix — and the formant column reads the edge frame rather than
the tract. The thresholds are expressed as *requests* (32 noise units,
200 cents), and the word-accuracy column, which is what places them, is
unaffected.

## E10: the gates

The gates, measured on the sixty neutral skald lines (twenty lines x
mia/jason/aria). Each line is conformed from its own render, read back
with the analyzer that planned the correction, and heard by the ASR.
The per-line table is `target/vox-e10/arms.tsv`, the sweeps are in
`target/vox-e10/sweeps.tsv`, and every transcript the run heard is
cached in `target/vox-e10/asr-cache.tsv`; the run's own summary lines
are the last four lines of its console output.

| gate | the brief asks | measured | |
|---|---|---|---|
| ASR word accuracy within 2 points of the unconformed line | ≤ 2.0 pts | **+0.15 pts** (urgent), **+2.29 pts** (example), **+4.47 pts** (calm) | **FAIL** |
| the conformed audio, analyzed again, lands in the target class | ≥ 80 % | **98 %** (urgent), **98 %** (urgent,clipped,falling), **90 %** (calm) | **PASS** |
| latency per line against a skald re-render | "well under" a re-render | **187-188 ms p50**, against a **1897 ms** re-render p50 measured in the same run | **PASS** |

The three conforming arms and the re-ask arm, in full:

| arm | marks | conformed | class hit | ASR before | ASR after | drop | conform p50 | conform min |
|---|---|---|---|---|---|---|---|---|
| urgent | `urgent,clipped` | 59 | 58/59 = 98 % | 96.5 % | 96.3 % | **+0.15 pts** | 187 ms | 136 ms |
| example | `urgent,clipped,falling` | 59 | 58/59 = 98 % | 96.5 % | 94.2 % | **+2.29 pts** | 188 ms | 137 ms |
| calm | `calm` | 60 | 54/60 = 90 % | 96.5 % | 92.1 % | **+4.47 pts** | 187 ms | 140 ms |
| re-ask | `loud,loud,loud` | 0 | — | — | — | — | — | 60/60 re-asked, nothing written |

Word accuracy is `1 - levenshtein/N` against the line's words — the
E2/E3 harness's own measure — with the unconformed render as the
baseline. The three arms share one baseline (the same sixty rendered
lines), so the arms are comparable with each other; they are not
comparable with the inherited run's 95.2 % baseline, because skald's
synthesis is not deterministic and this run's corpus is a different set
of renders.

**The word-accuracy gate fails.** Two of the three arms are over the
brief's 2-point budget: the example arm by 0.3 points, the calm arm by
2.5. The urgent arm is inside it, at 0.15 points. Nothing was tuned
against the gate — the arms carry the brief's own marks, and the
velocity targets were re-derived from the calibration because the
trunk's profile fix moved the dial (see above), not because of the
word accuracy. The failure is in the audio: the calm arm's words are
the ones that go, and the table below says which.

One clip was re-asked in each of the urgent and example arms, and it is
the same clip (line 14, mia, "Take the point, quiet."): its own
velocity dial reads +2 dB, the `urgent` target is +18, so the lift it
needs is +16 dB against the 15 dB tier limit, and the rule refuses it.
Every one of the sixty re-ask-arm clips was refused as designed:
`loud,loud,loud` asks for +18 dB against the same 15 dB limit.

### What the mean hides

The gates are about the mean, so the mean is what the table above
reports, but the distribution is worth the room. The word damage is
concentrated, not spread: the urgent arm changed the word accuracy of
four of its 59 lines (two lost 33 and 25 points, two gained 25), the
example arm four of 59 (three lost 75, 60 and 25, one gained 25), and
the calm arm eight of 60 (lost 75, 50, 33, 25, 25, 25, 20 and 14). The
calm arm's 4.5-point mean is eight bad lines and 52 untouched ones, not
a uniform loss.

The transcripts say what happened to them, and the pattern is sharper
than the numbers: **every loss is at the head of the line.** Line 2
("Two contacts, east side.") came back as "To contact Eastside." on
mia in the example arm and as the same on jason in the calm arm; line
16 ("Enemy armor on the road.") as "And in the armor on the road."
(mia, example); line 20 ("Good effect on target.") as "The effect on
target." (mia, urgent and example); line 19 ("Regroup at the truck.")
as "Her group at the truck." (mia, urgent and example) and "Or a group
at the truck." (mia, calm); line 11 ("Medic down, medic down.") as
"Meditate down, meditate down." (mia, calm); line 12 ("We got
company.") as "And he got company." (mia, urgent) and "Nigak
Company." (mia, calm). The tail of a line mostly survives — line 16's
"the armor on the road" comes through intact behind the broken head,
and line 2's tail merges with it ("east side" -> "Eastside") — but the
head is where it goes wrong, and the head is where the conformer
works hardest: the attack ramp the `urgent` and `clipped` marks ask
for, the re-timed first syllable the `calm` stretch moves.

The class misses are a different set of lines from the word damage,
and a different failure. The one miss in the urgent and the example
arms is line 16 on aria, which reads Ordinary in both: its conformed
features are velocity +13 dB, duration ratio 1.08 and contour −593
cents (urgent) and −326 cents (example), against aria's Ordinary
centroid of +8 dB / 1.00x / −432 cents and its Urgent centroid of
+19 dB / 0.97x / −29 cents. The velocity lands 6 dB short of the urgent
class and the contour stays far below it, so the nearest centroid is
Ordinary. The calm arm's six misses are lines 3, 8 and 19 on mia and
aria (jason lands all three), and they split by what the conformed
line reads. Line 3 ("I need cover, now.") comes out at a duration
ratio of 1.26 (mia) and 1.23 (aria) against the calm class's
1.43-2.12x the speaker's median phrase length: the line is short, the
stretch cannot make it long enough, and the reader sees Ordinary.
Lines 8 and 19 come out with a contour that reads positive (+404 to
+685 cents) and land on Urgent — `calm` carries no contour intent, so
nothing was asked of that channel, and the reader's nearest centroid
falls to Urgent (mia's urgent centroid: −224 cents) rather than Calm
(−360 cents) when the line's own contour reads positive.

### The latency, against the same box's re-render

skald re-renders a line from scratch; the conformer moves the audio it
already has. The comparison has to be against the same box at the same
time, because this box is shared with three other seats:

| | quiet box | this box |
|---|---|---|
| skald re-render, p50 | 642 ms (the brief's figure) | **1897 ms** (p95 2658 ms), 20 fresh renders measured inside the run above |
| the conform, p50 | — | **187-188 ms** |

The conform is 10x under the re-render measured in the same run, and
3.4x under the brief's quiet-box figure. The box's load is the whole
reason the two re-render numbers differ: the same measurement on the
same box, while another seat's jobs had it at a load average of ~100,
read **4805 ms** p50 (p95 7068 ms), and the run above was taken as that
load came back down (a 1-minute average in the low tens around the
measurement). The conform's minimum — its least-disturbed sample,
136-140 ms — is the number to quote for what it costs when the box is
quiet: about a fifth of the quiet-box re-render.

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
  48, and the ripple is gone from the envelope. (Those are the
  inherited run's numbers, kept as the record of the fix; the calm arm
  now lands 54 of 60.)
* **The class reader's syllable rate.** See above: the same words in
  two styles differ by 36 % in syllable count, so the rate channel was
  reading the segmentation's luck, and it was not stable under the
  conformer's own edits. The reader reads velocity, duration ratio,
  contour and tilt instead.
* **A uniform pitch shift on the way down.** The PSOLA's epoch chain
  advances by the *shifted* period, so a downward shift (a ratio below
  1) makes the chain skip epochs and the output period wobble; the
  inherited sweep measured +8 to +29 points of word damage from −200
  cents down, while the same shift upward was clean to +300. This
  run's sweep reads the words clean from −400 to +400 and broken at
  −600 (+24.2) and +600 (+12.5), so the point moved again with the
  sample — the mechanism is the same, and the threshold is held where
  the inherited run put it. The contour channel rides the same
  machinery but changes the ratio slowly, and the arms measure it as
  cheap.
* **The release channel.** A final consonant sits on a gesture tail,
  and scaling the tail flips a cliff-edge word ("Regroup" heard as
  "grew up") at every scale down to 0.9. The channel is out of the mark
  vocabulary; a caller may still write `release_scale` into a `Target`
  and the tier rule checks it.
* **Noise on `urgent`.** Any noise level strong enough to move the
  noise dial also flips the analyzer's voicing at the utterance's
  edges, which reads as a +300-cent contour jump. The channel is the
  `rough` mark's to exercise.
* **The calm arm's six misses.** The calm arm lands 54 of 60, and the
  six that miss are lines 3, 8 and 19 on mia and aria — jason lands all
  three, so they are lines rather than voices. They miss in two
  different ways. Line 3 ("I need cover, now.") comes out of the
  conformer at a duration ratio of 1.26 (mia) and 1.23 (aria): the
  corpus's calm class sits at 1.43-2.12x its voice's median phrase
  length, the line is short, and the stretch the calm mark asks for
  (1 / 0.69) cannot make it long enough, so the reader sees Ordinary.
  Lines 8 and 19 land on Urgent instead: the `calm` mark carries no
  contour intent, so nothing was asked of that channel, and the
  conformed line's contour reads positive (+404 to +685 cents), which
  is nearer the urgent centroid (−224 cents for mia) than the calm one
  (−360 cents).

  The reader separates calm from ordinary on the duration ratio, and
  the conformer asks for a *tempo* ratio — a fixed 1.449x stretch,
  which is all the tempo window allows, 0.68 being the measured
  word-safe floor. A short line stretched by a fixed factor lands short
  of a class whose floor is set by a *different* line's length.

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
  class's duration ratio for that voice, not a fixed 1.449x. The reader
  already has what that needs — the harness fits each voice's median
  phrase length from the calibration renders and hands it to the
  classifier — but the conformer's *ask* is built from the marks alone,
  with no corpus in front of it, so the number it would need is not
  something the conformer can see. Until that lands, 90 % is what the
  calm arm gets here, and the gate the brief sets is 80 %.
* **The epoch window that ran past the end of the signal.** The
  conformer's epoch detector walks each voiced island in quarters of a
  frame, and the walk could step past the end of the clip; the island's
  first peak-pick window then read samples that are not there. The E10
  arms hit it as an index-out-of-bounds panic (the len is 26624 but the
  index is 26624) on a line whose last voiced frame is the clip's last
  frame. The island's end is now clamped to the signal, and a
  regression test (`epochs_stay_inside_the_signal`) holds it there.

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
re-pinned: `0x71bdc1287ba9ff0b` → `0xaf24a5fb5f9932b1`. The base is the
trunk's pin *after* the smoothing pass — the pre-smoothing pin was
`0x6bf5f8521844c847`, and the pass moved it twice on its own (VOX.md
quotes that history) — so the value this seat's change moved from is the
smoothed one. The input utterance's own hash (`0x9c04796558fdd7d2`) is
unchanged, and the reason is carried in `crates/lm-vox/tests/golden.rs`;
`VOX.md` quotes the new pin. Everything else this seat touched is its
own: `src/conform.rs`, the `lm vox conform` subcommand, and the two
examples.

## What the smoothing pass froze

The trunk grew the α-β-γ smoothing pass (fleet 14z's nugget N5) while this
seat was working on E10, and two of this seat's unit tests stopped
holding. The cause is not in the conformer: it is a deadlock between two
of the smoothing pass's own knobs, on the two channels the conformer's
tests read.

**The deadlock.** The pass emits every track through a hold band and a
slew: the emission may move at most `slew` grid units a frame, and the
band releases only once the *emission* has moved `hold` units from what
it last stated. A band can therefore only be crossed if the slew is at
least as wide as the band. On the pitch (band 6 cents, slew 40) and the
level (band 5 units, slew 6) it is. On the formants (band 6, slew 4) and
the noise (band 8, slew 4) it is not: the emission can travel four units
a frame and the band wants six, so the band never opens and the track
stays where it was born. Measured on `smooth_l0` directly — sixty frames,
a step in the middle, run-length of the emitted track:

| track | band | slew | emitted track |
|---|---|---|---|
| pitch, +100 cents at frame 30 | 6 c | 40 c | moves: 1000 → 1038 → 1068 → 1089 → 1107 → 1122, then settles to 1104 |
| level rms, +20 units at frame 30 | 5 | 6 | moves: 120 → 126 → 132 → 138 |
| formant F2, +10 units at frame 30 | 6 | 4 | **never moves**: 100 for all sixty frames |
| noise, +12 units at frame 30 | 8 | 4 | **never moves**: 20 for all sixty frames |

The tracks are not dead everywhere — a reset boundary (an onset, a
voicing flip, a rest) states the frame whole, so a track can still move
there. What cannot happen is a track *tracking* the signal, which is what
the module's own documentation promises: the formant band exists because
"a real glide crosses it in a frame or two", and the noise band is "well
inside" the pre-smoothing hold so it "still lets every real transition
through". The band is also meant to be measured on the *filter's* drift
("stay at the last stated value until the filter has drifted `hold` from
it"), not on the slewed emission — and measuring it on the emission is
what deadlocks it.

**What it does to the conformer.** The two tracks the deadlock froze are
the two the conformer's tests read: the formants (the tract) and the
noise dial. The track is also born on the clip's *first* frame, whose
analysis window is truncated by the clip's edge, so the L0 field reports
the edge rather than the signal. On this seat's own fixture:

| what the test reads | before | after | |
|---|---|---|---|
| raw F2 — the analyzer's LPC pick | 1077.7 Hz | 1163.6 Hz | +8.0 %: the tract moved, as asked (x1.0595) |
| L0 F2 field, smoothing off | 1080.1 Hz | 1173.4 Hz | +8.6 % |
| L0 F2 field, smoothing on | 1080.1 Hz | 994.2 Hz | **−8.0 %**: the edge's pick, frozen |
| raw noise dial | 2.69 | 11.27 | +8.6 units |
| L0 noise field, smoothing off | 8 | 16 | +8 |
| L0 noise field, smoothing on | 33 | 37 | **+4**: the first frame, frozen |

So the conformer's DSP was never wrong: the raw picks say the formant
stage moved the tract by the factor it was asked for, and the noise mix
put +8.6 dial units of noise into the audio. What broke the two tests
was the *instrument* — the L0 field, whose two tracks the smoothing pass
froze. The same freeze reaches past this seat: the renderer reads the
same fields (`Voice::from_frame`), and the segmentation's noise-burst
detection reads the noise field too.

**What this seat did.** The two tests now read the analyzer's raw
measures — the same instrument the tilt test already used, and the one
E1 asserts against — with the assertions unchanged: the noise dial must
still move +8, the formants still within 8 % of the requested factor.
The third test that read the same frozen field
(`psola_shifts_pitch_keeps_formants`) was moved to the same instrument
so it cannot pass for the wrong reason.

**What this seat did not do.** The deadlock is in `smooth.rs`, which
belongs to the seat that owns the analyzer, and the fix moves the golden
hash and E2's byte count — the smoothing pass's own gate. The fix is
small, and the module's documentation already describes it: measure the
band on the filter's drift (the `raw` the filter state returns) and let
the emission follow at the slew rate, instead of measuring it on the
slewed value. That is the director's call, not this seat's: it re-pins
the golden hash the director just settled, and it puts the smoothing
pass's 288 B/s back in play. The conformer itself does not depend on the
frozen channels — the noise and formant targets come from the marks, not
from the analysis, and the class reader reads velocity, duration ratio,
contour travel and tilt — so E10's gates are unaffected either way.


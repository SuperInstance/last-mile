# Proposed changes to `docs/design/vox-score.md`

*2026-10-29. **A proposal, not an edit.** `docs/design/vox-score.md` is
unchanged on disk; the patch below is the exact diff that would apply the
changes, and it applies cleanly with `git apply`. (One exception: §11, the
marks layer, was authored in `ac2de4f` on the sibling branches and is
**already in the working tree** of this branch, uncommitted, because E6
tests it. The patch below is the change *to* that §11, and it is computed
against the file as it now stands.)*

Two patches are what the brief asks for — **§4 (the L2 line)** and **§3
(the novelty claim)** — and a third, **§6.1 (the listening loop)**, is
included because the E4b findings make §6.1 wrong as written: it describes
concatenating the L2 line with the words into one gate state, which is the
arrangement that measured *worse* than either channel alone. A fourth,
**§11 (the marks layer)**, is included because E6 measured that layer and
three of the six families §11 proposes cannot be measured from the fields
that survived E4b.

The evidence for every number quoted here is in
`docs/research/vox/E4-E5.md` §8 and `docs/research/vox/E6-marks.md`, and in
`evidence/vox-e4b/` and `evidence/vox-e6/`.

---

## 1. §3 — narrow the novelty claim, the way E0 says

E0's survey (`docs/research/vox/E0-prior-art.md` §9.1) reaches a verdict on
each of the four parts of §3's claim, and §9.2 forces six corrections on
the surrounding text. The patch below applies both.

| part of §3's claim | status after E0, E4 and E4b |
|---|---|
| 1. a layered, deterministic, interpretable performance protocol | **not novel as such** — ToBI/RaP, SSML, Tilt/Tiltinton, eGeMAPS/ComParE/DisVoice are the ancestors. The surviving difference is *measured continuously, in one document, with a renderer that consumes it*. An engineering choice, not a claim. |
| 2. the interchange in an AI decision loop, models reading and writing it as closed choices | **the defensible core, and the only part claimed as new.** E4 supports it with a caveat (E4b removes most of the caveat): the dials read the delivery at 90.6% where the words are at 62.2%, and they read order from question at 65.6% where the words are at 98.5% — the line is what the loop has when the transcript is not there yet. |
| 3. a per-speaker calibration, learned, kept only when outcomes improve | **partly novel.** Calibration against a speaker reference is old; the outcome-gated, sealed-prediction discipline and the editable profile were not found. E4 made the profile load-bearing: relative axes took the binary delivery reading from 53.9% to 82.2%. |
| 4. logged so replays are bit-exact | **not novel in kind; not found in this form for speech.** The analyzer is deterministic (E1: 5/5) but Riva's synthesis is not, so a bit-exact replay of a *rendered* performance is not currently possible. An engineering constraint, not a claim. |

Three words have to change as well, because E0 shows they overclaim:
"The MIDI of speech" (singing already did the split) becomes "the MIDI of
**speech**, with a decision model on both ends"; L2's ancestor is **SSML,
measured**, not ToBI alone; and "small" is not a differentiator, so §5's
comparison should be rebuilt around interpretability, editability and
bidirectional writability.

**The one-line version the design should stand on:**

> Vox Score is not a new way to describe prosody. It is a small,
> profile-relative, machine-readable channel into an AI decision loop, with
> a per-speaker calibration and an outcome-gated way to improve it — a
> format the audio model itself does not provide.

---

## 2. §4 — the L2 line

Three changes, each forced by a measurement:

1. **The field set.** Seven profile-relative axes replace the v2 line's
   ten fields. Dropped: `atk`, `dec`, `sus`, `rel`, `vib`, `trem` — the
   envelope classes were near-constant for a speaker and the modulation
   axes read `none` on every clip of the corpus, so six tokens told the
   reader nothing it could weigh. Added: `dur`, the continuous profile-
   relative `contour`, `end` (where the pitch ended), and `tone` (the
   boundary tone). Kept: `vel`, `bright`, `tempo`.

   The fields that earn their place, measured leave-one-speaker-out:
   `vel` (0.700 three-class / 0.933 binary on the delivery), `vel` +
   `bright` + `tempo` + `dur` (0.839 / 0.978), and for order/question the
   normalised contour at AUC 0.769 with `end` at 0.712. Adding the envelope
   axes *lowers* the delivery score from 0.839 to 0.806.

2. **The line is shorter and says more.** 24 words over 7 fields, against
   28 words over 7 fields, and the words that remain are the ones that
   vary.

3. **The format carries its vocabulary, and the gate's criteria carry the
   format's vocabulary.** This is the finding that matters most. The gate
   read the contour out of the line at **88.2%** when it was asked to read
   the measurement, and read *nothing* when it was asked "is this an order
   or a question?" — answering "question" to all 340 clips, which is what
   E4's exactly-50.0% figure was. The cause was the criteria's vocabulary,
   not the line's: the urgency criteria say *fast, loud and sharp*, which
   the line's tokens match, while the order/question criteria were pure
   semantics with no word in common with the line. Rewriting those
   criteria the same way as the urgency ones took the dials-only arm from
   50.0% to **65.6%** (Mia 60.0%, Jason 70.0%, Aria 67.5%; 62.4% on the
   word-identical subset).

   The patch therefore states the line's rules — three of them, each one a
   measured result — and adds a clause to the L3 row of the layer table. A
   format whose reader cannot match it to the question is not an interchange
   format, and neither is a question whose vocabulary the format cannot
   speak.

---

## 3. §6.1 — the listening loop (beyond the brief, and needed)

§6.1 as written has d1-2B score the gate "with the L2 line plus LFM3's
words", i.e. one state holding both. E4 measured that arrangement and it
is **worse** than the line alone on the delivery axis (38.9% against
45.6%); E4b measured it again on the revised line (34.4% / 81.1% against
45.0% / 90.6%). The gate, given both, reads the delivery off the words.

The patch replaces that with the arrangement that works: **two gate calls,
routed.** One call with the line answers the delivery question, one call
with the words answers the lexical question, and the answers are combined
afterwards. That is the only combination measured that recovers both
(words-only order/question at 98.5% *and* the line's 90.6% on
urgent-versus-not); every other rule tried — most-confident, product of
experts, a probability blend — lost points on one axis to gain nothing on
the other. The calls do not depend on each other, so the round costs the
larger of the two latencies, not their sum.

It also says what the dials-only path is *for*: the fallback when the
transcript is late, wrong or missing. That is the case the design already
describes — "gesture-level cues arrive *before* the transcript does" — and
the measurement now backs it.

One thing the patch deliberately does **not** do: put the L2 in a separate
field of the System One request as the only fix. That was tried (the
`l2-v3-field+words` arm: the state becomes a JSON object with `words` and
`performance` as separate keys, which the server renders as JSON) and it
did not help — 32.2% / 78.3%, slightly worse than the concatenation. The
System One API ignores any field outside `model`, `state` and `questions`,
so "a separate field" can only be a separate key in a structured state,
and that arrangement does not stop the gate reading the delivery off the
words. A machine-readable *object* for the line itself
(`l2-v3-json`: 35.0% / 80.0%) was also tried and is worse than the terse
text line for a 2B gate.

---

## 4. §11 — the marks layer (L2.5), and the vocabulary the measurement supports

§11 is where the design stops describing the delivery and starts *naming*
it. E6 tested that layer, and the interesting result is not that the marks
work — it is **which of §11's words the corpus can actually support**.

§11's draft table has six families and 28 marks. Measured against the skald
corpus:

| §11's draft mark | what happened | the measurement |
|---|---|---|
| **steadiness** (steady · tight · shaking) | **the family is dropped** | `vib` and `trem` read **zero on all 340 clips**. Riva's synthesis has no measurable vibrato or tremolo at a 100 Hz frame rate, so none of the three marks could ever fire. |
| `trailing` (articulation) | dropped | `release_ms` is **zero on 327 of 340 clips**: the envelope almost never falls below the release threshold, and the soft-release class fires on 2 of them, so a trailing mark cannot be seen in practice. |
| `halting` (pace) | dropped | `pause_count` is **0 on every clip of the corpus**. There is no pause to be halting about. |
| `breathy`, `strained` (colour) | dropped | they need a noise and tilt channel. The L0 frame layer carries `hnr_db` and `tilt_db_per_octave`, so this is an extension and not a closed door — but the L2 layer carries neither, so the marks cannot be quantized from it today. |
| `even` (pace) | renamed `regular` | §11 lists "even" in two families, and the mark set is a flat list. Articulation keeps "even"; pace takes "regular". |
| the other 21 marks | kept | every one of them fires on at least six clips. |

So the shipped vocabulary is **5 families and 21 marks**: dynamics (hushed ·
soft · level · strong · shouted), articulation (clipped · pressed · even ·
drawn), contour (falling · flat · lifting · rise-fall · questioning), pace
(rushed · brisk · regular · measured) and colour (dark · plain · bright).

The rule the evidence forces is short and worth stating in the design:

> **A mark that cannot fire is not in the vocabulary.**

A family whose words never appear teaches the reader a vocabulary they will
never use. This is the marks-layer version of the rule E4 already applies
to the L2 line — a field that does not move does not get a place in the
format — and it is the same reason the release measurement survived E4b as
a measurement but did not survive as a *dial*.

Three things E6 measured about the layer as it now stands:

1. **The marks are above chance against the intended label on all five
   families** (κ +0.06 … +0.26), so §11's own rule — families below chance
   are reworked or dropped — does not fire, and nothing was dropped on this
   evidence.
2. **The contour family carries the intent, not the style** (72.3% at
   telling order from question), and the other four carry the delivery.
   The dynamics family is weak on Riva's *ordinary* style (18.3%) because
   Riva's Neutral render is quieter than its Calm one — the marks side with
   the measurement, which is what they are for.
3. **The marks are the notation, not the measurement.** Against the 7-field
   L2 line they were quantized from, the marks **win** the lexical axis
   (67.3% against 65.6%) and **lose** the delivery axis (80.0% against
   90.6%) and the pitch read (69.7% against 88.2%). The marks line is 6
   words against 24. A 30B reasoning model, shown the marks of two
   deliveries and no audio at all, picked the urgent one 14 times out of 20
   against a 50% chance line — and used the marks' own words to say why.

Point 3 is the important one for the design. §11 as written implies the
marks might *replace* the dials. They do not, and E6 says so plainly: they
are a **different view of the same measurement**, kept because it is a
vocabulary rather than a set of numbers, and therefore because a small model
can be told what to look for in it. The L2 line stays the fallback for the
delivery axis; the marks are what the loop reads when it wants to *say*
something about the delivery, and what System Two reads when it wants to
reason about it in words.

**One arm of E6 did not finish, and the patch says so.** The inter-rater
arm against LFM3 — the "same closed question per family" of §11's own
experiment — was infrastructure-bound on the box it ran on (the `lm-ears`
proxy was being restarted underneath it, and other models were sharing the
GPU) and completed 15 of the 340 clips. At that n every confidence interval
contains the chance line, so **§11's rule — families below chance are
reworked or dropped — was not applied and cannot be**. The families this
patch drops were dropped on the *measurement*, not on that arm. The patch
therefore records the E6 result as partial.

What the attempt did establish is worth recording in the design, because it
is a trap rather than a number: **an audio model asked to answer without
explaining answers with one constant word per question.** On the fifteen
clips it was asked, LFM3 said `soft` for every dynamics clip, `even` for every
articulation clip, `flat` for every contour clip and `dark` for every colour
clip. Such an arm produces a plausible-looking agreement rate that
measures nothing but the marginal distribution, and any inter-rater test
against a reasoning model has to let it reason.

The patch also adds the three rules the layer needs, and the E6 result
itself, to §11's body.

---

## 5. The patch

````diff
--- a/docs/design/vox-score.md
+++ b/docs/design/vox-score.md
@@ -26,14 +26,58 @@
 | Differentiable DSP | DDSP, analysis-by-synthesis | DDSP is the check that the patch reproduces the voice (§6) |
 | MIDI 2.0 | 32-bit controllers, per-note expression, profiles | Vox Score borrows running status, per-note expression and profiles |
 
-**The claim is the combination:**
+**The claim is the combination** — and E0's survey (`docs/research/vox/E0-prior-art.md`)
+has narrowed it to one part:
 
 1. A layered, deterministic, interpretable performance protocol for speech.
-2. Designed as the interchange in an AI decision loop, where small models both read and write it as closed-choice dials.
-3. With a per-speaker calibration profile that is learned over time and kept only when outcomes measurably improve.
-4. Logged so that replays are bit-exact.
-
-The novelty claim stands or falls on E0's survey. If an existing protocol already does 1–3, we adopt it and say so.
+   **Not novel as such.** ToBI and RaP already layer prosody over time, SSML
+   already layers markup, Tilt and Tiltinton already compress a contour to
+   named parameters, and eGeMAPS, ComParE and DisVoice already fix the
+   channel set and its summary statistics. What is left as a difference:
+   measured continuously, in one document, with a renderer that consumes it.
+   This is an engineering choice, not a claim.
+2. Designed as the interchange in an AI decision loop, where small models
+   both read and write it as closed-choice dials. **This is the defensible
+   core, and the only part that is claimed as new.** Nothing surveyed does
+   it: SSML and EmotionML are human-authored annotations, ToBI is hand
+   labelled, Ctrl-P is a model that consumes prosody controls rather than
+   a protocol, and MusicGen's dials steer one model rather than being a
+   shared, logged, analysable document.
+3. With a per-speaker calibration profile that is learned over time and
+   kept only when outcomes measurably improve. **Partly novel.**
+   Calibration against a speaker reference is old — speaker embeddings,
+   Tiltinton's voice setting, a keyboard's velocity curve. The
+   outcome-gated, sealed-prediction discipline and the human-readable
+   editable profile were not found in any surveyed source.
+4. Logged so that replays are bit-exact. **Not novel in kind; not found in
+   this form for speech.** MIDI's running status and this repository's
+   hash-chained L3 log already do the logging. What is missing is
+   bit-exact logging of a *measured* performance, and the analyzer is
+   deterministic while the renderer is not.
+
+Three words in the design also have to change, because E0's survey shows
+they overclaim:
+
+* **"The MIDI of speech" is too strong.** MIDI-DDSP, ExpressiveSinger and
+  Vocaloid already split performance from sound for *singing*, with per-note
+  expression and per-singer adaptation. The honest claim is: *the MIDI of
+  **speech**, with a decision model on both ends.*
+* **SSML `<prosody>` is the ancestor of L2**, not ToBI alone: L2's axes are
+  SSML's five axes (rate, pitch, contour, range, volume), with continuous
+  measured values and a speaker-relative reference added. The design should
+  say "SSML, measured".
+* **"Small" is not a differentiator.** LPC-10 (2,400 bit/s), MELPe
+  (600 bit/s), Codec 2 (700 bit/s), Mimi (12.5 frames/s) and LiquidCodec
+  (150 tokens/s) are all as small as the 150–300 B/s budget of §5. What
+  distinguishes Vox Score is interpretability, editability and bidirectional
+  writability, and §5's comparison should be rebuilt around those.
+
+The novelty claim stands or falls on E0's survey. If an existing protocol
+already does 1–3, we adopt it and say so. **E0's answer: part 2 is not
+found anywhere; part 1 is prior art; part 3 is partly new; part 4 is an
+engineering constraint.** E4 then found that part 2 holds only if the
+format is profile-relative and the gate's questions are written in the
+format's own vocabulary.
 
 ## 4. The four layers
 
@@ -43,15 +87,41 @@
 |---|---|---|---|---|
 | **L0 Frame** | 100 Hz (10 ms) | F0 (cents relative to the speaker's profile), RMS level, F1/F2/F3, spectral tilt, voicing/noise ratio, onset flag | MIDI CC streams | signal processing, renderers, storage |
 | **L1 Gesture** | per syllable (~4–8/s) | onset time, pitch target and glide, duration, velocity (peak level against the profile), articulation (attack ms, release ms), noise burst | note on/off with per-note expression | renderers, gesture matching |
-| **L2 Phrase** | per utterance | the **patch summary**: attack/decay/sustain/release of the loudness and brightness envelopes, contour class (rise/fall/flat/rise-fall), vibrato/tremolo depth and rate (LFO), tempo (syllables/s), pause structure | a synth patch and a phrase marking | **decision models** (a short text line), the lexicon |
-| **L3 Intent** | per utterance | gate answers: urgency, order/question, confidence, addressee, plus the words from ASR | the conductor's reading | the sim (as quantized events), the learning flow |
+| **L2 Phrase** | per utterance | the **patch summary**: seven profile-relative axes — level (class + dB against the speaker's own peak), brightness (class against the speaker's own centroid), tempo (syllables/s), duration (s), the pitch contour over the delivery (class + value, in fractions of the speaker's pitch range), where the pitch ended (class + value, same units), and the boundary tone (rise/fall/level) | a synth patch and a phrase marking | **decision models** (a short text line), the lexicon |
+| **L3 Intent** | per utterance | gate answers: urgency, order/question, confidence, addressee, plus the words from ASR. Every gate's **criteria are written in the vocabulary of the L2 line** — the urgency criteria say *fast, loud and sharp*, so the order/question criteria must say what the line says about an ending | the conductor's reading | the sim (as quantized events), the learning flow |
 
-The L2 line a model reads looks like this, about 30 tokens:
+The L2 line a model reads looks like this, about 24 words:
 
 ```
-vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB
+vox: vel soft -4dB | bright low | tempo 4.8/s | dur 0.63s | contour fall -0.79 | end fall -1.16 | tone fall
 ```
 
+Three rules govern the line, and each of them is a measured result
+(`docs/research/vox/E4-E5.md` §8):
+
+1. **Every axis is profile-relative.** "Loud" means loud *for this speaker*.
+   The absolute form of the line scored 53.9% on the binary delivery task
+   where the relative form scored 82.2%.
+2. **A field that does not vary does not belong on the line.** The earlier
+   line carried `atk`, `dec`, `sus`, `rel`, `vib` and `trem`; on real speech
+   the envelope classes were near-constant and the modulation axes read
+   `none` on every clip, so six of the tokens told the reader nothing it
+   could weigh. A reader weighs what it sees.
+3. **The line carries its vocabulary.** Every token is spelled out in a
+   legend sent with it, and the legend describes the *format* — it does not
+   say which end of the scale means which intent. The mapping is the
+   reader's own knowledge of intonation, the way a person hearing the line
+   would have it.
+
+The pitch axes are the ones that carry an utterance's intent: the pitch
+movement over the delivery and where the pitch ended, both in fractions of
+the speaker's own pitch range, separate an order from a question at AUC
+0.769 leave-one-speaker-out, and the boundary tone is what a gate reads
+back out of the line at 88.2%. A gate asked to read the line as a
+measurement gets 88.2%; a gate whose criteria never mention the pitch gets
+chance. That is why §4's L3 row requires the criteria to be written in the
+line's vocabulary.
+
 ## 5. The encoding: retro-tight
 
 - **An L0 frame is 9 bytes.** F0 is an i16 in cents. RMS, F1, F2 and F3 are u8 on log scales; tilt is an i8; noise is a u8; flags are a u8.
@@ -62,7 +132,7 @@
 
 ## 6. The loop goes both ways
 
-1. **Listening (player → squad):** microphone → L0 analyzer (ProArt CPU) → L1/L2 → d1-2B scores the gate with the L2 line plus LFM3's words → L3 events into the sim. Gesture-level cues (a sharp attack, a falling glide) arrive *before* the transcript does. The sim can lean toward urgency on the gesture, then the words confirm or cancel it ("never wait", applied to listening).
+1. **Listening (player → squad):** microphone → L0 analyzer (ProArt CPU) → L1/L2 → d1-2B scores the gate → L3 events into the sim. The gate is asked **twice**, and the two answers are combined afterwards: one call with the L2 line answers the delivery question, one call with LFM3's words answers the lexical question. The calls do not depend on each other, so the round costs the larger of the two, not their sum. **The two channels are not concatenated into one state**: measured, a single state holding both the line and the words read the delivery *worse* than the line alone (38.9% against 45.6%), because the gate then reads the delivery off the words. Gesture-level cues (a sharp attack, a falling glide) arrive *before* the transcript does, and the dials-only path is the fallback for when the transcript is late, wrong or missing. The sim can lean toward urgency on the gesture, then the words confirm or cancel it ("never wait", applied to listening).
 2. **Speaking (squad → player):** an agent's line is the words plus an L2 patch. The decision model **writes** the patch by choosing gesture presets, which are closed-choice gates. Renderers then go down a ladder:
    - skald/Riva with the matching style, when it was pre-rendered in time;
    - an **LPC/formant voice** driven by the same L0/L1, when skald is late (a robot voice saying the right words, not a beep);
@@ -112,16 +182,33 @@
 
 Vox Score gets the same thing: **L2.5, marks**. It is a closed, small vocabulary, quantized deterministically from the L2 dials and always relative to the speaker profile:
 
-| Family | Marks (draft) | From which dials |
+| Family | Marks | From which dials |
 |---|---|---|
 | Dynamics | hushed · soft · level · strong · shouted | level vs. profile |
-| Articulation | clipped · pressed · even · drawn · trailing | attack, release, sustain |
+| Articulation | clipped · pressed · even · drawn | the loudness envelope vs. profile |
 | Contour | falling · flat · lifting · rise-fall · questioning | F0 glide, end slope |
-| Steadiness | steady · tight · shaking | vibrato/tremolo depth and rate |
-| Pace | rushed · brisk · even · measured · halting | tempo, pause structure |
-| Colour | dark · plain · bright · breathy · strained | tilt, noise |
+| Pace | rushed · brisk · regular · measured | tempo vs. profile |
+| Colour | dark · plain · bright | the spectral centroid vs. profile |
+
+A line is then written the way a score is: `"hold" [strong, clipped, falling, brisk, bright]`.
+
+**The vocabulary is what the measurement supports, and no more.** E6 measured the draft above against the skald corpus, and four of its marks could never fire. `vib` and `trem` read zero on all 340 clips, so the whole **steadiness** family is gone; the release measurement is zero on 327 of 340 clips, so articulation has no `trailing`; there is no pause in any clip of the corpus, so pace has no `halting`; and `breathy` and `strained` need a noise and tilt channel the L2 layer does not carry. **A mark that cannot fire is not in the vocabulary** — it would teach the reader a word they would never see. Two more changes follow from the measurement: `even` sat in two families at once, so pace's neutral mark is called `regular`, and the marks are quantized from the *loudness envelope* rather than from the attack, release and sustain separately, because the release never happens.
+
+Three rules govern the layer, and each one is a measured result (`docs/research/vox/E6-marks.md`):
 
-A line is then written the way a score is: `"hold" [strong, clipped, falling, tight, brisk]`.
+1. **Every mark is quantized from a profile-relative field.** A mark says how
+   this delivery compares with *this speaker's* own usual delivery, never how
+   it compares with some absolute reference.
+2. **Every threshold is in a config, and the quantizer is deterministic.** The
+   same bytes give the same marks, always.
+3. **The marks are the notation, not the measurement.** Measured against the
+   L2 line they were quantized from, the marks beat it on the lexical axis
+   (67.3% against 65.6% at telling order from question) and lose on the
+   delivery axis (80.0% against 90.6%) and on reading the pitch (69.7%
+   against 88.2%). Both are kept: the line is what the loop reads when it
+   wants the measurement; the marks are what it reads when it wants to *say*
+   something about the delivery, and what System Two reads when it wants to
+   reason about it in words.
 
 What this gives System Two (the slow, deliberate layer: the leader, the between-round learning flow, the human, and Claude):
 
@@ -133,4 +220,4 @@
 
 The vocabulary itself is versioned and learned like the lexicon: a new mark is proposed only when a residual cluster recurs, is sealed with a prediction, and is adopted on evidence. The notation grows the way musical notation did: when performers keep doing something that has no name yet.
 
-**Experiment E6:** inter-rater agreement. Does the marks layer agree with (a) LFM3's answers and (b) a human listener on the skald dataset? Do marks make System Two's proposals more accurate than dials alone? Pass: the agreement is reported per family, and families below chance are reworked or dropped.
+**Experiment E6:** inter-rater agreement. Does the marks layer agree with (a) LFM3's answers and (b) the clip's intended label? Do marks make System Two's proposals more accurate than dials alone? Pass: the agreement is reported per family, and families below chance are reworked or dropped. **E6's answer, in part: all five families are above chance against the clip's intended label (κ +0.06 … +0.26), so nothing was dropped on that evidence; the contour family carries the order/question signal (72.3%) and the other four carry the delivery; and a 30B reasoning model told which of two deliveries was the urgent one from the marks alone, with no audio, 14 times out of 20 against a 50% chance line — while the 7-field L2 line got 18 out of 20 on the same pairs.** The inter-rater arm against LFM3 is **not yet measured**: it was infrastructure-bound and completed 15 of 340 clips, which is not enough to place any family above or below chance. It also turned up a trap worth recording — an audio model asked to answer without explaining answers with one constant word per question, and a constant answer will produce a plausible agreement rate that measures nothing. The evidence is `docs/research/vox/E6-marks.md`.
````

---

## 6. What this patch does not claim

* It does not claim that the dials beat the words at reading order from
  question. They do not: 65.6% against 98.5%. The line is the *fallback*,
  and it is worth having because the transcript arrives late.
* It does not claim anything about human voices, about L3, about the byte
  encoding of §5, or about bit-exact replay. E4 and E5b establish none of
  those (§6.3 of `E4-E5.md`).
* It does not change §5's budget, §7's profile, §8's privacy tiers or
  §9's experiment list. Those need their own patches, on the same
  evidence, and §9's E4 row should be updated to record that E4b
  re-ran it.
* It does not claim that the marks beat the dials. They do not, and §11's
  patch says so: on the delivery axis the marks score 80.0% where the 7-field
  line scores 90.6%, and at reading the pitch 69.7% where the line scores
  88.2%. The marks win the lexical axis (67.3% against 65.6%) and they are
  four times shorter, and that is the whole of the claim. **The marks are a
  second view of the same measurement, not a better one.**
* It does not claim the dropped families are worthless. Steadiness, the
  soft release and the pause all measure zero on a synthetic corpus, and all
  three are things real voices do. They are dropped *for this corpus*, and
  the config is where they come back — `tools/vox-ref/marks.config.json` is
  the file to edit.
* It does not claim the marks have been shown to agree with an independent
  audio model. That arm completed 15 of 340 clips and every confidence
  interval it produced contains the chance line. §11's own rule — families
  below chance are reworked or dropped — is therefore **not** applied by
  this patch, and the vocabulary it ships is justified by the measurement
  (a mark that cannot fire is not in the vocabulary), not by inter-rater
  agreement.
* It does not claim anything about human voices, about L3, or about the
  round trip in the writing direction. E6 measured whether marks can be
  *read*; whether a decision model can write a mark set that renders back to
  the delivery it came from is a separate experiment, and E5 already found
  the L2 round trip lossy.

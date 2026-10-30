# Proposed changes to `docs/design/vox-score.md`

*2026-10-29. **A proposal, not an edit.** `docs/design/vox-score.md` is
unchanged on disk; the patch below is the exact diff that would apply the
changes, and it applies cleanly with `git apply`.*

Two patches are what the brief asks for — **§4 (the L2 line)** and **§3
(the novelty claim)** — and a third, **§6.1 (the listening loop)**, is
included because the E4b findings make §6.1 wrong as written: it describes
concatenating the L2 line with the words into one gate state, which is the
arrangement that measured *worse* than either channel alone.

The evidence for every number quoted here is in
`docs/research/vox/E4-E5.md` §8 and in `evidence/vox-e4b/`.

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

## 4. The patch

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
````

---

## 5. What this patch does not claim

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

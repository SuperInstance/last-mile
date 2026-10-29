# Vox Score: speech as a performance, not a recording

*Status: design proposal, 2026-10-29. Not yet a decision. It becomes D73 if the experiments in §9 hold up.*

## 1. The idea in one paragraph

MIDI split music into two things that had always been fused: the **performance** (which note, when, how hard, how it bends) and the **sound** (the instrument that plays it). That split is why a 1983 cable at 3,125 bytes/s could drive a whole studio. It is also why a performance can be stored, edited, replayed and played on any instrument. Speech technology never made this split. Today speech moves between machines in one of two forms. Text keeps the words and throws away *how* they were said. Audio, or neural codec tokens, keeps everything, but it is heavy, opaque, model-specific and impossible to edit. **Vox Score is the MIDI of speech.** It is a small, layered, physically meaningful event stream that carries how a line is *performed*, separate from the words and from the voice that renders it. Models, humans and machines can all read it, write it, store it and learn from it.

## 2. Why it matters (and why now)

- **Decision models are good at dials.** last-mile's System One API asks small models closed questions with 2–10 options, and the answers come back calibrated in tens of milliseconds (D62, D70). A synth patch is exactly that: a few dozen dials, each with a meaning. A decision model can **read** a performance as dials and **write** one as dials, using the same API.
- **The analysis is mechanical and cheap.** Pitch (YIN), vocal-tract shape (LPC), loudness (RMS), noise (zero-crossing rate, spectral flatness) and brightness (spectral tilt) are 1970s–1990s signal processing. They run on a CPU at 100 frames/s, with no GPU and no model download, and they can be made bit-exact across machines.
- **Neural speech is now local.** skald/Riva (399 voices, emotion styles) and LFM3-Audio (ASR, audio understanding, 50 ms time to first token) run on the Thor. The neural parts supply what dials can't: the words, the meaning, and who is speaking. The dials carry the rest.

## 3. Prior art, and the honest novelty claim

Each piece exists. The sources below are to be surveyed in the first experiment (§9, E0); this list is the starting point, not a finished review.

| Piece | Prior art | How Vox Score differs |
|---|---|---|
| Acoustic feature sets | eGeMAPS (the minimal standard paralinguistic set), ComParE | Vox Score is a *streaming event protocol* with layers and an encoding, not a feature vector per clip |
| Parametric vocoders | WORLD (F0, spectral envelope, aperiodicity), LPC-10, Speak & Spell | Vox Score uses them as one renderer among many, not as the format |
| Prosody annotation | ToBI, SSML `<prosody>` | Vox Score is measured continuously from the signal and is machine-writable at 100 Hz, not hand-annotated or coarse |
| Singing synthesis | Vocaloid, UTAU (notes + expression parameters) | Vox Score targets speech, and goes both ways: analysis *and* synthesis |
| Neural codecs | EnCodec, Mimi, LiquidCodec | Codecs are the "sampler" (audio as opaque tokens); Vox Score is the "MIDI" (an interpretable performance) |
| Differentiable DSP | DDSP, analysis-by-synthesis | DDSP is the check that the patch reproduces the voice (§6) |
| MIDI 2.0 | 32-bit controllers, per-note expression, profiles | Vox Score borrows running status, per-note expression and profiles |

**The claim is the combination:**

1. A layered, deterministic, interpretable performance protocol for speech.
2. Designed as the interchange in an AI decision loop, where small models both read and write it as closed-choice dials.
3. With a per-speaker calibration profile that is learned over time and kept only when outcomes measurably improve.
4. Logged so that replays are bit-exact.

The novelty claim stands or falls on E0's survey. If an existing protocol already does 1–3, we adopt it and say so.

## 4. The four layers

Each layer is derived from the one below it. Every derivation except the intent layer is mechanical and deterministic.

| Layer | Rate | Contents | Comparison | Who reads it |
|---|---|---|---|---|
| **L0 Frame** | 100 Hz (10 ms) | F0 (cents relative to the speaker's profile), RMS level, F1/F2/F3, spectral tilt, voicing/noise ratio, onset flag | MIDI CC streams | signal processing, renderers, storage |
| **L1 Gesture** | per syllable (~4–8/s) | onset time, pitch target and glide, duration, velocity (peak level against the profile), articulation (attack ms, release ms), noise burst | note on/off with per-note expression | renderers, gesture matching |
| **L2 Phrase** | per utterance | the **patch summary**: attack/decay/sustain/release of the loudness and brightness envelopes, contour class (rise/fall/flat/rise-fall), vibrato/tremolo depth and rate (LFO), tempo (syllables/s), pause structure | a synth patch and a phrase marking | **decision models** (a short text line), the lexicon |
| **L3 Intent** | per utterance | gate answers: urgency, order/question, confidence, addressee, plus the words from ASR | the conductor's reading | the sim (as quantized events), the learning flow |

The L2 line a model reads looks like this, about 30 tokens:

```
vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB
```

## 5. The encoding: retro-tight

- **An L0 frame is 9 bytes.** F0 is an i16 in cents. RMS, F1, F2 and F3 are u8 on log scales; tilt is an i8; noise is a u8; flags are a u8.
- **Raw, that is 900 B/s per speaker.** With **running status** (send only the fields that changed, as 4-bit deltas where they fit, and a rest marker for silence), the target is **150–300 B/s**. The same idea that let MIDI fit 16 channels down one cable.
- **L1 and L2 events cost tens of bytes per syllable or utterance.**
- **Budget for a full match:** 9 speakers × 20 minutes × 20% talk time ≈ 2 MB raw L0, well under 1 MB encoded. Every frame of every match is kept for later analysis.
- **Comparison:** LPC-10 speaks intelligibly at 2,400 bit/s (300 B/s), and the Speak & Spell at about 1,200 bit/s.

## 6. The loop goes both ways

1. **Listening (player → squad):** microphone → L0 analyzer (ProArt CPU) → L1/L2 → d1-2B scores the gate with the L2 line plus LFM3's words → L3 events into the sim. Gesture-level cues (a sharp attack, a falling glide) arrive *before* the transcript does. The sim can lean toward urgency on the gesture, then the words confirm or cancel it ("never wait", applied to listening).
2. **Speaking (squad → player):** an agent's line is the words plus an L2 patch. The decision model **writes** the patch by choosing gesture presets, which are closed-choice gates. Renderers then go down a ladder:
   - skald/Riva with the matching style, when it was pre-rendered in time;
   - an **LPC/formant voice** driven by the same L0/L1, when skald is late (a robot voice saying the right words, not a beep);
   - the existing tone track, as a last resort.

   Lateness degrades the quality, never the rhythm.
3. **Analysis by synthesis (the honesty check):** render a Vox Score with the LPC voice, analyze the result again, and measure the round-trip error on each channel. The ASR checks the words survived. Where the JEPA/LFM3 latent sees a difference that Vox Score does not carry, that residual names the channel the protocol is missing. **The protocol grows by evidence**, not by guesswork.
4. **Agent to agent:** agents exchange words plus Vox Score, not audio. One agent "hears" another's urgency at the cost of reading 30 tokens.

## 7. Growing to understand the player

A good keyboard has a **velocity curve** fitted to a player's touch. Vox Score has a **speaker profile**: a small file holding this player's median pitch and range, loudness range, typical attack, glide and tempo. L0 and L1 are always *relative* to the profile, so "loud" means loud *for this player*. Each between-round learning flow (D64–D66) does two things:

1. **Updates the profile**, a deterministic running estimate.
2. **Proposes prosody lexicon entries**: "this player's 30 ms-attack falling *hold* meant *hold now*." Each proposal is sealed with a prediction, tested on held-out seeds, and adopted only if outcomes move as promised.

The profile and lexicon are human-readable. The player can open them and see how the squad has learned to hear them, and correct it.

## 8. Determinism and privacy

- **Determinism:** the sim never sees audio or floating-point numbers. L3 events are quantized and logged in the hash chain like key presses. L0/L1 are stored in a **sidecar** next to the log (D67), so the learning flow can rerun on old matches without any audio. The analyzer is written in integer/fixed-point arithmetic where it affects L3, and a golden test checks it is bit-exact on x86-64 and aarch64 (D69).
- **Privacy tiers:** L0 with formants (F1–F3) can be resynthesized into intelligible speech. That is how LPC voices work, so it is **not anonymous**. A `prosody-only` storage tier drops the formants and keeps F0, level, tilt, noise and timing. That is enough for intent and not enough for words. The tier is a setting per store. All storage stays on the player's machines, and the profile can be reset.

## 9. Experiments, with exit criteria

| # | Question | Pass |
|---|---|---|
| **E0** | Prior art: does an existing protocol already do layered, interpretable, bidirectional speech performance for AI loops? | a written survey with sources; the novelty claim in §3 is confirmed, narrowed or dropped |
| **E1** | Does the analyzer measure what it claims? | on synthetic signals with known truth (sine sweeps, formant-filtered pulse trains, noise bursts): F0 within ±10 cents, formants within ±5%, onset within ±10 ms; bit-exact on x86-64 and aarch64 |
| **E2** | How small is it? | encoded L0 ≤ 300 B/s on skald-rendered speech; full-match budget measured |
| **E3** | Does the LPC voice keep the words? | ASR word accuracy on the LPC resynthesis ≥ 80% (skald itself is the ceiling); per-channel round-trip error reported |
| **E4** | Do the dials carry intent? | on skald deliveries (same words; calm/ordinary/urgent; order/question): urgency and order/question accuracy for L2-dials-only, LFM3-only, words-only, and L2+words through the d1-2B gate. Pass if L2+words beats words-only by a clear margin |
| **E5** | Can the decision model write a performance? | d1-2B picks L2 presets for a line and context; the rendered result, analyzed again, lands in the intended class ≥ 80% |

The datasets are rendered by skald, so no human voice is recorded. A human-voice test comes later, with Casey's consent.

## 10. Where it lives in the code

- `crates/lm-vox`: the protocol types (L0–L3), the encoder/decoder, the analyzer, gesture segmentation, the phrase summary, the speaker profile and the LPC/formant renderer. It depends on nothing in lm-core, and lm-core does not depend on it.
- `crates/lm-voice` (the voice seat's crate): the render ladder calls lm-vox for the LPC rung.
- `crates/lm-agents`: the L2 line goes into gate prompts, and the gesture presets become gate options.
- `crates/lm-learn`: the profile updates and the prosody lexicon.

## 11. The notation layer: words for nuance, so System Two can think in it

Dials are what System One reads and writes. But nuance that can't be *named* can't be discussed, compared or deliberately reproduced. Music solved this once already. Over the dials (velocity, timing, envelope) sits a small, shared **notation**: dynamics (pp … ff), articulation (staccato, tenuto, accent, marcato), shape (crescendo, a fall, a lift), time (rubato, fermata). Two musicians who have never met can say "play bar 12 marcato, falling" and both hear the same thing. Notation is a vocabulary for performance that sits above sound and beside words.

Vox Score gets the same thing: **L2.5, marks**. It is a closed, small vocabulary, quantized deterministically from the L2 dials and always relative to the speaker profile:

| Family | Marks (draft) | From which dials |
|---|---|---|
| Dynamics | hushed · soft · level · strong · shouted | level vs. profile |
| Articulation | clipped · pressed · even · drawn · trailing | attack, release, sustain |
| Contour | falling · flat · lifting · rise-fall · questioning | F0 glide, end slope |
| Steadiness | steady · tight · shaking | vibrato/tremolo depth and rate |
| Pace | rushed · brisk · even · measured · halting | tempo, pause structure |
| Colour | dark · plain · bright · breathy · strained | tilt, noise |

A line is then written the way a score is: `"hold" [strong, clipped, falling, tight, brisk]`.

What this gives System Two (the slow, deliberate layer: the leader, the between-round learning flow, the human, and Claude):

1. **It can be seen.** The between-round screen shows how the player said it in marks, beside what they said, and the player can correct the reading.
2. **It can be patterned.** Mark sequences are discrete tokens, so recurring **motifs** can be mined, counted and compared across rounds and players, like n-grams over a score ("this player's urgent orders are always strong-clipped-falling; their questions are soft-drawn-lifting").
3. **It can be reproduced.** A mark set renders back down to dials and then to voice (§6). An agent can be told "say it the way Casey says *hold* when it's real".
4. **It can be discussed at a higher level.** The learning flow's proposals, sealed predictions and verdicts can be written in marks: "adopt: [clipped, falling] on *hold* means *now*; predicted: hold_ticks rises ≥ 1". A human can read that sentence and disagree with it.
5. **It is vectorized, with names on the axes.** Each family is an ordered axis, so a delivery is a point in a small, interpretable space. System Two can measure distances ("this was halfway between calm and urgent"), interpolate, cluster and ask "what changed?" without dropping to raw vectors. Learned latents (JEPA, LFM3) are the dense space; marks are its named coordinates. The residual between them (§6.3) is where new marks get proposed.

The vocabulary itself is versioned and learned like the lexicon: a new mark is proposed only when a residual cluster recurs, is sealed with a prediction, and is adopted on evidence. The notation grows the way musical notation did: when performers keep doing something that has no name yet.

**Experiment E6:** inter-rater agreement. Does the marks layer agree with (a) LFM3's answers and (b) a human listener on the skald dataset? Do marks make System Two's proposals more accurate than dials alone? Pass: the agreement is reported per family, and families below chance are reworked or dropped.

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

## 12. The full score: face, gaze and body on the same staff

Speech is one instrument of a person's expression. The face, gaze, head and hands play at the same time, and meaning often lives in how they line up: a falling *hold* with a nod and lowered brows is a different act from the same word with raised brows and a glance away. The same MIDI split applies to them, and it is older there than in speech. MPEG-4's Facial Animation Parameters (1999) were designed to animate a face at a few kbit/s. FACS action units name the muscles. ARKit's 52 blendshapes and the VRM/VMC (OSC) protocols stream them live today.

**One score, many staves.** A person is one **system** in an orchestral score. Every staff shares one clock (the sim tick, plus a sub-tick frame index):

| Staff | L0 (frame) | L1 (gesture) | L2.5 (marks, draft) |
|---|---|---|---|
| Voice | Vox Score L0 (§4) | syllables | strong, clipped, falling… |
| Face | ~52 blendshape / action-unit levels at 30–60 Hz | an AU's onset and release, with attack and decay (the same ADSR as a note) | brows-down, tight-lipped, smile-onset, jaw-set |
| Gaze | direction and fixation | saccade, hold, aversion | at-you, away, scanning |
| Head | pose (3 angles) | nod, shake, tilt, lean | nodding, still, turning-away |
| Hands/body | key joints | point, beckon, halt-palm, shrug | pointing-left, open, closed |

The same encoding (§5: running status, deltas, rests) keeps a face to a few hundred B/s. The same layers apply: L0 → gestures → a phrase patch → marks → intent. Marks are shared across staves where they mean the same thing (emphasis, hesitation, address).

**The synoptic view is the conductor's score.** last-mile already draws a piano roll, one lane per agent. That roll grows into a full score: per person, a voice staff (the words above it, marks below), a face staff, a gaze staff and a gesture staff, all aligned on the same time axis. You see the *complete* expression at once: what was said, how it was said, where they looked and what their face did, and how those lined up or contradicted each other. Cross-staff patterns (motifs that span voice + face + head) become minable and nameable, exactly as single-staff motifs are (§11).

**Both directions, again.**
- **Perception (reality):** camera → face/pose landmarks → blendshape/AU levels → the face, gaze, head and body staves. Microphone → the voice staff. The decision model reads cross-staff marks, which are more reliable than any single channel.
- **Generation (virtual):** an agent's decision writes *one* score, and every renderer plays from it: the voice (skald / LPC), the avatar's face and the avatar's body. Voice and face can't disagree, because they are the same performance played on different instruments. This is the fix for the "uncanny" mismatch of separately generated speech and animation.
- **Coaching and review:** the score can be replayed, edited ("make the brows softer here") and re-rendered, like editing MIDI and re-playing it.

**Constraints and honesty:**
- Faces are far more identifying than prosody. The face staff stores blendshape levels, never images, and is a separate privacy tier (§8), on by explicit choice only.
- NVIDIA Audio2Face and Maxine are x86-only (verified by the Thor's agent), so on the Thor the face path is landmark-based.
- The Thor already hosts 3D vision services (:8098 capture, :8099 MoGe). Whether a face/pose landmark model runs there is to be measured, not assumed.

**Experiments:**
- **E7, the synoptic view:** extend the piano roll into a multi-staff score for agents, using their own decisions first (they already have intents and talk), then the voice staff from Vox Score. Pass: a reviewer can tell from the score alone which of two deliveries was the urgent one.
- **E8, one score drives voice and face:** an agent's line renders voice and avatar face from the same L2 marks in the 3D view. Pass: in a blind A/B, viewers judge the expression as coherent more often than with independent voice and animation.
- **E0 is extended** to FACS, MPEG-4 FAP/FBA, ARKit blendshapes, VRM/VMC, OSC, BVH and Live Link.

## 13. Who plays the score: engine, gamepad, and director

A game player doesn't move every muscle in both legs. The stick says *that way, this fast*. The buttons say *jump, crouch*. The engine's locomotion system turns that into footfalls, balance, foot placement and arm swing. The same three-tier split governs every staff in this design. It is also last-mile's existing split: reflexes, System One and System Two (D26, D55-D56).

| Tier | Rate | Who | What it controls | Synth comparison |
|---|---|---|---|---|
| **0 Engine** | 60–120 Hz | scripted, procedural, deterministic | the low-order motion: the gait cycle, foot IK, balance, breathing, blinks, saccades, lip shapes from the voice's L0 | oscillators and envelopes |
| **1 Gamepad** | 10–15 Hz | **System One** (d1 closed-choice gates) | a small controller state: heading, speed class, stance, a look-at target, gesture triggers, and *choices at the moment* (step over the puddle / around left / around right) | playing notes, the mod wheel |
| **2 Director** | per event to per round | **System Two** (the leader, the learning flow, a human) | **the patch and the intent**: "hurry, you're late" → marks [rushed, pressed]; tuning tier-0 and tier-1 parameters from what was observed | choosing and editing the patch |

**The marks are the language between the tiers.** A System Two order is written in marks, and every staff's renderer reads the same marks. "Hurry, you're late" → [rushed, pressed] sets the gait patch (run, a longer stride, a forward lean), the voice patch (a brisker tempo, sharper attacks), the face (strained) and System One's priors (the gate options re-weight toward *step over* rather than *around*). One instruction makes the whole body coherent.

**Example: the puddle.**
1. Tier 0 is walking.
2. Perception flags an obstacle two steps ahead.
3. System One is asked one 3-option gate (about 80 ms on d1-2B, D70): over / around-left / around-right, plus a stride adjustment.
4. Tier 0 executes it by re-planning the next two footfalls with IK.
5. If System One is late, tier 0's reflex default applies (shorten the stride, go around). The body never waits ("late AI falls back to reflexes", the doctrine we already have).
6. If System Two has set [rushed], the prior favours *over*, and the stride stays long.

**Fine-tuning by observation.** Between rounds (D64–D66), System Two reads the outcomes and proposes parameter changes as sealed predictions. For example: "step-over succeeded at widths ≤ 0.45 m, so raise the over-threshold from 0.35; predicted: fewer detours, no more stumbles". Tier 0's scripts stay simple and deterministic; their dials are learned.

**Perception runs the same tiers in reverse.** Observed footfalls and posture are tier-0 frames. The observed gait choices are tier-1 gestures. The marks inferred from them ([rushed, pressed], or [halting, guarded] for a limp) are what System Two reasons over: *they're hurrying, they must be late*.

**Prior art to check in E0:** subsumption architecture (Brooks, 1986); game locomotion (motion matching, procedural IK, NaturalMotion Euphoria); learned controllers with style (DeepMimic, AMP); and, closest of all, the dual-system robot foundation models (NVIDIA GR00T N1's "System 1 / System 2", Figure's Helix). The Thor is built to run those models. The claim here is not the hierarchy, which is established. The claim is **a shared, named mark vocabulary that crosses tiers and modalities, with System One as a closed-choice gamepad and System Two learning the dials by sealed prediction**.

**E9:** in last-mile's 3D view, agents get tier-0 procedural locomotion, a System One gamepad pack (speed class, stance, step choices) and System Two patches (hurried / cautious / weary). Pass: (a) no frame ever waits on a decision, with the fallback counts reported; (b) a viewer can name the patch from motion alone, which is the perception inverse.

## 14. The last mile: conform any voice to the score

MIDI never made a sound. It carried instructions all the way to the end, and **the device at the end** (a synth, a sampler, a drum machine) turned them into sound in time. That end device is the last mile. Vox Score works the same way. Everything above (dials, marks, tiers, the full score) exists to carry *intended performance* right up to the last mile. At the last mile, any voice generator can be the skin: skald/Riva, the LPC voice, or a commercial service.

The generator does not have to understand the score. **We conform its output to the score:**

```
intended score (L2 marks + L1 gestures)
        │
generator (any: Riva, a cloud TTS, ...) ──► audio ──► analyzer (L0/L1, §4)
                                                │
                              measured score ◄──┘
        │
   diff (intended − measured, per channel, per gesture)
        │
   conformer (classic DSP, streaming) ──► audio that performs the intended score
```

The conformer is the speech counterpart of a pitch-correction tool, but for *every* channel, not just pitch, and driven by meaning rather than by a musical key:

| Channel off-target | Correction (established DSP) |
|---|---|
| pitch contour / end glide | F0 modification (PSOLA or a source-filter vocoder), applied per gesture |
| tempo, syllable durations, pauses | time-scale modification (WSOLA), with pauses inserted or shortened |
| dynamics and attack | gain envelope per syllable (sharpen or soften the onset) |
| brightness / pressed vs. breathy | spectral tilt filter, noise mix |
| tension | small formant shift or bandwidth change |

**It's tiered, like everything else, and it never waits:**
- **Small diffs** are corrected in the stream at the last mile.
- **Large diffs**, beyond what DSP can do without artifacts, go back up one tier: the generator is asked again with better hints (style, SSML, a different voice), in time for the *next* line.
- **If the re-ask is late**, the corrected original plays.

**What this buys:**
1. **Words stay fixed while the delivery changes.** The ears experiment found that Riva's emotion styles change the words themselves ("hold" → "Old!"). Rendering neutral and conforming to [urgent] keeps the words and adds the urgency.
2. **Generators become interchangeable instruments.** Every vendor's output is measured against the same score and conformed to it, so a character sounds like *itself* across engines, and a generator can be swapped without re-tuning the game.
3. **Fine-grained control that generators don't expose.** Most TTS gives a style name or a slider. The conformer gives syllable-level control over every channel, driven by the decision model's marks, as the line plays.
4. **The same pipe works for a human voice,** with consent: a player's line can be cleaned up or matched to the moment, which is the perception side of §12 running the other way.

**Honesty:**
- Heavy DSP correction sounds processed. The tier boundary (what to correct vs. what to re-ask) must be set by measurement, with E3's ASR check and a listening test.
- Commercial TTS is an outward-facing service. Sending lines to one is a per-deployment choice, never a default.

**E10:** render neutral skald lines; conform them to targets (calm/ordinary/urgent, order/question) using marks only. Pass:
- **Words kept:** ASR word accuracy stays within 2 points of the unconformed line.
- **Delivery lands:** the conformed audio, analyzed again, falls in the target class ≥ 80%.
- **Cheaper than re-rendering:** latency per line is well under a re-render.
- **A listening comparison:** conformed vs. Riva's own style for the same target.

## 15. The polisher: editing meaning in audio someone else rendered

§14 conforms a line to a score. The bigger use is **polishing**, the way pitch correction polishes a raw vocal: take a finished render (a paragraph from any generator) and change *what it means* without re-recording it.

- **Move the emphasis.** "I didn't say *he* took it" → "I didn't say he *took* it": raise the F0 peak, lengthen and lift the level of the target word, and flatten the old focus word.
- **Bring out an attitude the render missed.** For example, the sarcasm a line was written with: a widened pitch range, lengthened stressed vowels, an exaggerated or inverted contour, slightly slowed timing.
- **Shape emotion along a line, cued by the scene.** A character sounds a little more worried through the first half of a line, then, at the tick where the gaze staff (§12) shows them seeing the listener's face, eases into relief. The next phrase continues from that state, so it fits the scene organically. This is **automation**, as in a DAW: mark curves over time, cued by events on other staves.

**What makes it possible is learning what the knobs do together.** The knobs interact. Raising pitch also reads as brighter. A pitch fall plus lengthening reads as emphasis, while a pitch fall alone reads as finality. Sarcasm is a *combination* that no single knob holds. So the polisher has two learned maps over the same knobs:

1. **A forward map:** knob changes → perceived marks. The judges are LFM3-Audio's understanding mode (§6.3), the JEPA latent distance, and sparse human ratings. It captures the interactions.
2. **An inverse search:** target marks (and their automation curves) → the smallest set of knob changes that the forward map says reaches the target. The constraints are a measured artifact budget and **voice identity** (stay close to the original speaker).

**Training data is cheap and needs no human voice.** Take rendered lines and apply random knob perturbations (thousands per hour, by DSP). The judges score them, and the forward map learns from that. Human ratings are only for calibrating the judges. The model is small and runs on the dials, not on audio, so the search is fast enough for the last mile.

**One instruction language end to end.** The director (§13: System Two, a human writer, or Claude) writes in marks and automation: "line 3: worried → relieved at the look; emphasis on *took*; dry". The same sentence drives the generator hint (§14's re-ask), the conformer and the polisher, and the same marks drive the face and body (§12), so the polish lands across the whole performance.

**E11:** on neutral skald paragraphs, three edits:
1. **Emphasis move:** a listener or LFM3 picks the new focus word ≥ 80%.
2. **Attitude overlay**, for sarcasm and worry: paired against the unpolished line, judged better-matched ≥ 70%.
3. **Cued automation**, worried → relieved at a given tick: the measured marks cross over within ±150 ms of the cue.

For all three, ASR word accuracy stays within 2 points and the voice-identity distance stays under a threshold set from E3.

## 16. Learning from recorded conversation: intention, reception and speechcraft

The forward map (§15) and the mark vocabulary (§11) can learn from almost any recorded speech, because Vox Score turns audio into something many different judges can label cheaply and consistently. The richest sources are **conversations**, because they hold two things a single voice doesn't: **intention** (how a line was performed) and **reception** (how it landed).

**Sources, from cleanest to richest:**
- **Audiobooks and read speech:** performance with a known text.
- **Interviews and podcasts with guests:** turn-taking, reactions, sentiment carried through a narrative.
- **Meetings and debates:** disagreement, persuasion, hedging.
- **Streams with live chat, or videos with timestamped comments:** an *audience* reaction curve aligned to the score ("12:34, the way she said that 😂").

**Three kinds of label, all aligned on the score's clock:**

| Kind | Labeller | Gives |
|---|---|---|
| **Measured** | the analyzer (§4) | the dials and marks (mechanical, free) |
| **Judged** | JEPA surprise and latent distance; LFM3 audio understanding; d1 gates over transcript + marks; text sentiment of the transcript | what the line probably meant |
| **Received** | **in the audio:** the other speaker's backchannels ("mm-hm"), laughter, interruptions, response latency, and whether they mirror the marks (entrainment). **Outside it:** listener comments and chat, timestamped where possible | how it actually landed |

**The gap between intention and reception is the most valuable data.** Lines whose delivery the judges read as sarcastic but listeners took literally, or emphasis that drew laughter where none was meant, teach **speechcraft**: what makes a line land. That is the knowledge the polisher (§15) applies, and that generation and the director (§13) write with. Consistent judges are weak labels. Where judges disagree, the item goes to the sparse human ratings.

**The next turn is a free label.** In a conversation, how the other person replies (how fast, with which marks, whether they laugh or push back) is a label on the previous line's reception. last-mile already has this structure in the game. An order (intention) is followed by the squad's response and the round's outcome (reception). So the game is itself a corpus generator, and the between-round learning flow (D64–D66) is this section's loop at small scale.

**Scale is cheap because the score is small.** An hour of conversation is a few MB of L0 with formants, and much less prosody-only (§5). A library covering a very large number of hours of performances and their receptions fits on one disk and can be mined for motifs: how thousands of speakers actually do sarcasm, reassurance or urgency.

**Rights and privacy (non-negotiable, and before any collection):**
- Recorded conversations, podcasts and comments belong to people. Use **licensed or permissive sources first**: corpora released for research under clear licences, public-domain audiobooks, and content whose owners agree. Respect platform terms; no scraping against them.
- Store **derived scores, not audio**, at the **prosody-only tier** by default (§8): no formants, so the score can't be turned back into the speaker's voice or words. Speaker identity is dropped; comments are reduced to timestamped reception labels, not kept as text with names.
- A human voice of our own (Casey's) is used only with explicit consent, as in §12.

**E12:** on one permissively licensed multi-speaker conversational corpus (chosen and licence-checked in E0):
1. Extract scores and judged labels.
2. Derive reception labels from the next turn (backchannel, laughter, latency).
3. Test whether **marks + words predict reception** better than **words alone**.

Pass: a clear margin on held-out speakers. A second test: whether the forward map trained on it improves E11's attitude overlays.

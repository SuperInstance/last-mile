# 09: The utterance score: words time-stamped to "notes"

The idea, in your words:
> MIDI with the words time-stamped to their notes creates a reproducible set of nuances that an agent could translate as no-loss values, making their communication have far more flavor than text alone. More of the vector of understanding gets across. As the system plays, it learns to recognize different tones, and with the human in the loop giving feedback it becomes more than text-based.

This holds up, and the precedent is old. Standard MIDI Files have **lyric meta events** time-aligned to notes (karaoke `.kar` files did exactly this). Linguists call the non-word channel **prosody**: pitch contour, timing, loudness, pauses, emphasis. Prosody is how "Move." differs from "Move?" and from "*Move!*", and how sarcasm differs from sincerity. Text throws all of that away.

## The utterance as a score
```
UtteranceScore {
  speaker, channel (shout/voice/whisper/radio), addressees, tick_start,
  words: [ { text, t_on, t_off } ],            // lyric events, timed
  lanes: {                                     // controllers, per word or per frame
    pitch:    [...],   // F0 contour (relative to speaker baseline)
    energy:   [...],   // loudness
    rate:     [...],   // local speaking rate
    emphasis: [...],   // per-word stress 0..1
    pauses:   [...],   // gaps before/after words
  },
  tags: { intent: order|ack|warn|query|..., affect: calm|urgent|uncertain|... }
}
```
It's a few hundred bytes per sentence. Like MIDI, it's **renderer-independent**:
- the formant synth renders it fully (every lane is a synth parameter),
- Riva TTS renders it approximately (text plus SSML prosody spans: rate, pitch, volume, emphasis, breaks, where the model supports them),
- an agent "hears" it **without loss**, by reading the lanes directly. No speech recognition, no guessing.

## Where scores come from

| Speaker | How the score is made |
|---|---|
| **Agent** | System 1 emits text **plus** prosody intent (emphasis words, pace, contour shape, pauses). Expanding that into lanes is cheap and deterministic. |
| **Human** | Riva ASR gives the words with **word-level timestamps**. Pitch (F0) and energy come from cheap DSP on the mic audio (YIN/pYIN-class pitch tracking), aligned per word. Optionally, an emotion/affect model on the Thor adds `affect` tags. **The human's tone reaches agents in the same format agents use with each other.** |

## "No-loss" is true where it matters
- **In the log:** lossless. The full score is recorded and replays exactly.
- **Between agents:** lossless at the source. The **hearing simulation** then degrades it physically: distance and walls drop or blur words and flatten lanes, and radio squeezes the pitch range and adds dropouts. Mishearing becomes a *simulated, reproducible* event, not ASR noise.
- **Into an LLM prompt:** LLMs read tokens, so the score is **projected** into a compact annotated form:
  ```
  [radio, urgent] "contact *EAST*↑ — two… (0.4s) moving↓"
  ```
  The projection is lossy by design. The full lanes stay available for learned models later (below).

## How the system learns tone

1. **Outcome learning (automatic).** The log links every score to what happened next: did the addressee comply, how fast, correctly? Over rounds, the after-action review (05) can find patterns like *"clipped + high energy on 'NOW' → compliance in 0.8 s vs 2.1 s"*. Those become **tonal lexicon** entries in the squad's doctrine.
2. **Human feedback (in the loop).** The human labels moments: 👍 or 👎 on an utterance, or a quick tag such as "sounded sarcastic", "too slow", "perfect callout". Labels attach to the exact score, so they're reproducible training data, not vibes.
3. **Emergent paralanguage.** Squads under airtime pressure (03) can develop conventions *beyond* words: a rising contour on a callsign means "status?", a flat fast delivery means "routine". These are measurable, because they're lanes in a log.
4. **Learned prosody models (later).** With enough labelled scores, train a small encoder on the Thor that maps lanes to a vector. Agents then receive `text + prosody embedding`, and the LLM projection becomes richer than hand-written tags.

## Retro and real precedents
- **MIDI/karaoke lyric events:** words locked to musical time.
- **Speak & Spell / SAM / DECtalk:** speech as a parameter stream, with pitch and stress as explicit controls.
- **Vocoders and LPC speech** (military radio, 1940s onward): speech compressed to a handful of parameters per frame, rebuilt by the receiver. *That's the MIDI principle applied to voice.*
- **Emergent-communication research** in multi-agent RL: agents under bandwidth pressure invent compact codes. Our airtime cost is the same pressure, in a game.

## Milestone placement
- **M1:** the score format, agent-generated scores (text plus emphasis/pace/affect tags), Riva rendering with SSML spans, the LLM projection, logging, and the 👍/👎 hook.
- **M2:** human mic to score (ASR word times plus F0/energy), hearing degradation of scores, the formant renderer.
- **M3+:** the tonal-lexicon mining in after-action review, then a learned prosody encoder.

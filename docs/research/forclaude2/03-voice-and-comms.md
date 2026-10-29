# 03: Voice and comms

Squad communication is the core mechanic of our game, so this file matters most for gameplay.

## What the source proposed
- Model speech as "intent channels": pitch, volume, formants F1–F4, nasality, noise and plosive bursts. In other words, a parametric (formant) synthesizer driven by data.
- Override Riva Magpie TTS prosody by writing tokens into its latent space and attention weights.
- Drive Audio2Face facial animation from the same intent stream instead of from audio.
- Attenuate each voice by speaker–listener distance and occlusion using simple arithmetic.
- Broadcast only on change, so silent listeners cost nothing.
- Verify generated audio with Riva ASR against a signed ledger ("anti-deepfake").

## What's real, what isn't

| Claim | Reality |
|---|---|
| Inject prosody into Riva's latent space through NIM | **No.** NIMs expose gRPC/HTTP APIs, not model internals. The real levers are **SSML prosody tags** (rate, pitch, volume, where the chosen model supports them), voice selection or zero-shot cloning, and **what text you send**. |
| Audio2Face driven from intent instead of audio | Possible in principle, but **irrelevant at our art style.** Low-poly retro faces need a jaw flap, or a few visemes driven by amplitude/phoneme timing. That's what 1990s games did, and it's free. |
| Formant synthesis | **Real and old.** Klatt synthesizers, the Speak & Spell (1978), SAM on the C64 (1982), DECtalk. Cheap enough to run dozens of voices on the ProArt CPU. Fully parametric, so urgency, stress, radio crackle and fatigue can each be a parameter. |
| Distance and occlusion attenuation | **Real and essential.** It's how the sim decides *who hears what*, not just how loud it sounds. |
| ASR plus signed-ledger deepfake shield | Not our problem. Drop it. |

## How it matters for us: the comms model

**An utterance is a simulation object, not just audio:**
```
Utterance { speaker, channel: Shout|Voice|Whisper|Radio(squad), addressees,
            text, intent_tags (order/ack/warn/query), urgency, tick_start, duration }
```
- **Duration costs time.** Speaking takes sim time, based on word count and speech rate. That cost pushes squads toward brevity, which is the pressure the "learn which language works" goal needs.
- **Hearing is simulated.** Voice and shout reach listeners within range and line of sight, attenuated by distance and walls. Radio reaches the squad's channel but can be jammed or congested. Enemies **can overhear** voice-range speech.
- **Agents receive text plus metadata**, plus *whether* and *how clearly* they heard it. Speech recognition is only needed for the human's microphone.
- **The audio is for the human.** It's rendered only for what the player or spectator camera can hear.

**Voice level-of-detail (the retro trick).** Treat voice like mesh LOD:
| Situation | Voice source | Runs on |
|---|---|---|
| Distant chatter, background radio traffic | Formant/parametric synth (SAM-style, with a radio filter) | ProArt CPU, free |
| Squad radio and nearby agents | Riva TTS, fast voices, pre-chosen per agent | Thor |
| Cached "barks" (ack, contact!, moving) | Pre-generated Riva clips per agent voice | ProArt cache, instant |

The radio-filtered formant voice also *suits* the retro-modern look, the same way low-poly hides generative-model artifacts.

**Human voice in:** Riva ASR on the Thor. Push-to-talk keeps it simple and cuts false triggers. Optionally, voice-activity detection on the ProArt NPU later.

## Milestone 1 scope
- Utterance object with the four channels; range and line-of-sight hearing; speech duration cost
- Riva TTS for squad voices, plus cached barks
- A jaw flap on the speaker's model
- Formant voices and enemy overhearing: milestone 2

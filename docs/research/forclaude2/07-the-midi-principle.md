# 07: The MIDI principle (the gold nugget)

> MIDI carries almost no information on its own. It's tiny because the *receiving device does the rendering*: a sample or a synth turns "note 60, velocity 90" into sound. The same logic can record every action of every sprite in a game.

This is the part of the transcripts that matters most, and it's bigger than the transport question in 02.

## Why MIDI worked (1983 → still everywhere)
1. **Performance is separate from rendering.** The stream says *what happened* ("this key, this hard, now"). The instrument decides *what it looks or sounds like*. A $50 keyboard and a $5,000 sampler play the same file at different fidelity.
2. **A shared vocabulary.** General MIDI (1991) fixed program 1 = piano and channel 10 = drums, so any device rendered any file plausibly.
3. **Start/stop, not frames.** Note On … Note Off. The *instrument* fills in the in-between: envelope, sustain, release. The stream never sends samples.
4. **Multi-channel by design.** 16 independent channels per port, each with its own instrument (program), controllers and notes.
5. **Two kinds of change:** discrete events (notes) and continuous controllers (mod wheel, pitch bend), sent only when they change.
6. **Shared time.** A clock, Start/Stop/Continue, and Song Position Pointer (seek). Everything locks to one tempo.
7. **Recordable for free.** A Standard MIDI File is just the stream with timestamps: tracks, plus meta events for markers and **lyrics**.

Games used this directly. Tracker MODs on the Amiga were patterns plus samples. LucasArts' **iMUSE** (1991) let game state steer MIDI music through markers and hooks. Console sprite engines worked the same way: game logic said "play walk-cycle 3 facing left" and the hardware or animation system produced the frames.

## The mapping to our world

| MIDI concept | last-mile equivalent |
|---|---|
| Instrument / sound module | **The renderer**: ProArt full client, spectator view, headless sim, a future phone viewer. Each renders the *same performance* at its own fidelity. |
| Channel (16 per port) | **One agent per channel.** 16 channels = 4 squads × 4 agents: one "port" is one match. |
| Program Change | Agent **role/loadout/voice**: which animation set, which voice, which model "patch" |
| Note On (note, velocity) … Note Off | **Action start/stop**: `Sprint`, `Crouch`, `Fire`, `Speak`, with intensity. The renderer supplies the animation envelope (wind-up, loop, recovery). |
| Control Change | **Continuous parameters**: aim yaw/pitch, gait speed, stance height, stress level. Sent on change. |
| Pitch bend | Fine steering on top of a gait |
| Lyric meta event / SysEx | **Utterance text** and other rich payloads (squad intent, perception summary) |
| Marker meta event | Round events: objective taken, squad wiped, phase change |
| Clock, Start/Stop/Continue, Song Position | **Sim tick clock, round start/pause/resume, replay seek** |
| Standard MIDI File | **The round log** (see 01), one track per agent |
| General MIDI | A **General Action Set**: a fixed, versioned action vocabulary every renderer understands |

## What this gives us
- **Level of detail for free.** A distant agent's `Sprint` note renders as a 4-frame impostor loop; a close one gets the full procedural animation. Same stream either way.
- **Thor and cloud send performances, not poses.** The Thor's System-1 output is *notes and controllers*, not transforms. That's tiny, and the ProArt's reflex/motor layer is the instrument that plays it.
- **Distributed AI becomes a sequencer.** Reflex, System 1 and System 2 are "players" at different tempos writing into the same channels, with priority rules (reflex can cut off a note System 1 started, just as a later note steals a voice).
- **Replay, spectating and Studio mode share one format.** A round is a score, and you can watch it on any "instrument".
- **DAW tooling as a bonus.** Export a round as a real `.mid` file and open it in Reaper/Ableton: every agent is a track, actions are notes, utterances are lyrics. A piano-roll debugger and Studio prototype that costs nothing to build.

## Where the analogy needs care (the bones)
- **Gameplay-relevant results must be identical across renderers.** If one synth plays a note slightly differently, nobody cares. If one renderer resolves a hit differently, the game breaks. So we split:
  - **Authoritative sim** (positions, hits, hearing, scores): deterministic, runs the same everywhere, and is part of the "score".
  - **Cosmetic rendering** (animation style, voice timbre, effects): free per renderer, like a synth.
- **Rich payloads don't fit 7-bit or 32-bit fields.** Text and intent go in "lyric/SysEx-style" events in our native format.
- **Real MIDI encoding as the wire format?** Recommended: our **native protocol mirrors MIDI's semantic model** (channels, programs, note on/off, controllers, clock/transport, meta text) in our own compact binary. Then a **lossless-enough exporter** writes Standard MIDI Files (or MIDI 2.0 clips) for DAW viewing. That way we get MIDI's design and its tooling without being held to its field widths.

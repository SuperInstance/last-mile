# 01: The "MIDI digital tape", reframed as the round event log

## What the source proposed
Record every input and state change as a tiny timestamped packet (MIDI 1.0: 3 bytes plus a 4-byte tick; later 16-byte MIDI 2.0 UMP). It claimed this gives:
- tiny replays (about 8 KB/minute)
- trivial rewind ("reverse the tick counter")
- cache-friendly linear storage
- data that ML models can read directly

## What's right
This is **event sourcing**, and it's exactly the old-console trick behind Doom's `.lmp` demos, StarCraft replays and fighting-game rollback. Our rounds should work this way:

```
round = { seed, ruleset_hash, script_hashes, events[] }
event = { tick, source, kind, payload }
```

Every non-deterministic input is an event: the player's inputs, every System-1 and System-2 model output, every utterance, every random draw that comes from outside the sim. Replaying the events against the same seed and scripts reproduces the round exactly, **even though the LLMs themselves are non-deterministic**, because we record their outputs rather than re-running them.

## What's wrong
1. **"Rewind by reversing playback direction."** An event like "agent 3 position = X" overwrites the old value. You can't invert it without knowing what was there before. The real technique is **periodic keyframe snapshots** (for example, every 5 s of sim time) plus **forward replay** from the nearest keyframe. That's how every replay system with scrubbing works.
2. **"Hardware JR timestamps replace our tick counter."** MIDI 2.0 Jitter Reduction timestamps are 16-bit values at roughly 32 µs resolution, so they wrap about every 2 seconds. They exist to de-jitter a live link, not to timestamp history. Our authoritative clock is the **simulation tick** (fixed rate), which is also what makes replay deterministic.
3. **"32-bit precision removes the need for interpolation."** Value resolution and sampling rate are separate problems. You interpolate because samples arrive at 10 Hz and you render at 60+ Hz, not because values are quantized.
4. **"Formula for state at any time = Σ tokens × step function."** That only works for additive deltas. Most of our events are absolute values or discrete actions.

## How it matters for us

| Use | Milestone |
|---|---|
| Round log as the single source of truth: replay, debugging, after-action review | **1** (log plus straight replay) |
| Hash-chained log segments (each segment's hash includes the previous one) so learning data and spectator replays are tamper-evident | 1 (cheap, do it early) |
| Keyframe snapshots plus scrub bar for spectators | 2 |
| Curve-fit compression (keyframe reduction) of motion tracks when archiving | Later. Raw logs for 16 agents are already small. |
| **Overdub / counterfactual replay**: replay a round with one agent's or squad's decision stream swapped (new script, new doctrine) and compare outcomes | Studio mode (see 06). A powerful A/B tool. |
| **Headless fast-forward**: the same sim crate built with no renderer replays or re-simulates rounds at many times real time on the Thor/Orin CPU | 2. Feeds the learning loop. |

## Retro precedent
Doom (1993) demos stored only player inputs per tic and relied on a fully deterministic engine, so a demo file was a few KB. When the engine changed, old demos desynced. **Lesson:** version the ruleset and scripts *in* the log header, and refuse to replay a log under a different version unless explicitly asked (that would be a counterfactual run).

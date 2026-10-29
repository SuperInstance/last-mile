# 06: Studio mode (separate from the game)

You defined it: *"the ultimate game studio would be to in-game vibe code and A/B test anything, but that's not a game — it's a separate mode."* Most of the source's UI and "director" material lands here.

## What the source proposed (the "Master Conductor Interface")
- A multi-lane **piano-roll timeline**: one lane per channel (pitch, volume, joint angles…)
- **AI vs human overlay**: green predicted line, red corrected line, glow where they diverge
- **Draw to correct**: sketch a new curve, and the AI re-synthesizes that segment
- **Time-scrub dial**: rewind and fast-forward the whole state; freeze, tweak, resume
- **Overdubbing**: re-record one lane on top of an old run
- A "one-click cryptographic seal"

## What carries over (adapted to squads and comms)

| Source idea | Studio mode feature |
|---|---|
| Piano-roll lanes | **Round timeline** with lanes per squad and agent: utterances (as text blocks), intents, actions, System-1 and System-2 calls (with latency bars), budget usage |
| AI vs human overlay | **Run A vs run B overlay**: same seed, different script or doctrine; divergence highlighted |
| Draw to correct | **Edit an event and re-simulate from there**: change one utterance or order and replay forward headlessly (branching timelines) |
| Time-scrub dial | Keyframe-plus-replay scrubbing (see 01) |
| Overdubbing | **Counterfactual swap**: replay a round with squad B's decisions coming from a new script or model while everything else is fixed |
| Physical dial | A **MIDI control surface** (via `midir`): faders for sim parameters (budget sizes, speech rate, hearing range), jog wheel for scrubbing, pads for bookmarks. This is the one place real MIDI belongs. |
| Cryptographic seal | Drop. Log hash-chaining (see 01) covers integrity. |

## Why it's a separate mode
- Studio needs **mutability everywhere** (edit rules, scripts and models mid-session). The game needs **fixed rules within a round** so learning and scoring mean something.
- Studio wants headless re-simulation running in the background, which is Orin/Thor CPU work the game shouldn't compete with.
- Both share the same building blocks: event log, headless sim, scripting layer, and timeline data. Building the game well *is* most of the work toward Studio.

## Sequencing
1. Game milestone 1: event log plus straight replay (no UI beyond a spectator replay)
2. Game milestone 2: keyframes, scrub bar, between-round metrics
3. Studio v1: timeline lanes, run A/B overlay, edit-event-and-re-simulate
4. Studio v2: MIDI control surface, counterfactual swaps, live script hot-swap mid-round

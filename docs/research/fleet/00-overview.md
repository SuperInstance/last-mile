# Fleet scouting: what Casey's other repos give last-mile

*2026-10-28. A scouting pass over the SuperInstance account, done for last-mile. The goal was to take what fits, cite where it came from, and leave the rest.*

## Method

- **Account size:** 5,335 repos. 4,508 are not forks; about 2,100 are Rust and 1,300 are Python. Most were written by agents between April and October 2026.
- **Read in depth:** the 11 repos Casey named, plus about 25 more found two ways:
  - a keyword scan over every repo description, covering MIDI, determinism, replay, voice, reflex, budget, squad, room, fixed-point and Jetson;
  - a MiniMax Code triage of half the description list, run read-only in tmux. Its picks were spot-checked against the real repos.
- **ZCode:** the headless `-p` run returned no output for the other half. The fleet record already notes that ZCode's headless mode is limited, and the keyword scan covered that half instead.
- **Handling rule (from whole-house-quilt `prior-art.md`, and Casey's correction in §25):** much of the machinery last-mile needs already exists in the fleet. Before designing anything "new", read what's here. Repos written as ideation (terrain, chrono-tensor) are **nugget mines**: read them for mechanisms, verify before building on them, and don't fix them in passing.

## The short version

last-mile is the newest member of a family Casey has been growing all year. The family already has:

- **A working squad game with budgeted thinking:** COLOR SQUAD (`color-squad`, `voice-games`). It has 4-unit squads, a hard budget of thinking calls with reflexes underneath, deterministic seeds, and a running season with innovation breaks. Its voice version turns a command into a local reflex after 3 clean runs.
- **The "LLM compiles, executor executes" rule, learned the hard way:** `thor-firstline`. Speech goes in, a 14-number policy comes out, and a Lua script runs it. Refusal counts as an outcome, and policies expire on a TTL into HOLD.
- **Reflexes learned from repetition:** `voice-reflex-gate` (3 consistent model answers compile a reflex), and `pincher` + `saddle` (a reflex shell, plus a harness with frozen alignment states and night cycles).
- **Conversation as a sequencer:** `quilt-conversation` (per-agent cadence, drafting the next line while a teammate talks, an energy budget, silence as signal) and `cmidi-core` (speech acts → pitch classes, prosody → CCs, agent → channel, written as a valid SMF).
- **Time as a first-class, recordable dimension:**
  - `chrono-tensor`: 16-track event format, scrubbable ring buffer, and downmixing that never drops notable events;
  - `erised-sequencer`: dice derived from the ledger tip, so the past replays byte-identical and a re-spun future re-rolls honestly;
  - `stitch`: hash-chained per-tenant ledgers that agree without a sequencer.
- **One truth, many shadows:** `terrain`, `mud2scummvm`, `lau-terrain`, `lau-a2ui`. Renderers never write back. This is D8 under another name.
- **The umbrella design:** `whole-house-quilt`. Rooms of text state, decision models as dice, generators up and instances down, and a staging ladder for rendering. It also has **measured facts about these exact machines**: LAN latency, the Thor's live services, and disk space. See [01](01-measured-facts.md) and [03](03-whole-house-quilt-fit.md).

The concrete changes to last-mile are in [02-lessons.md](02-lessons.md) and are recorded as D23–D34.

## Repo verdicts

**Keep** means the idea goes into last-mile now. **Adapt** means keep the idea but change its shape. **Later** means it's useful after M1. **Drop** means it doesn't fit ("if it doesn't fit, it doesn't fit").

### Named by Casey

| Repo | What it is | Verdict for last-mile |
|---|---|---|
| [whole-house-quilt](https://github.com/SuperInstance/whole-house-quilt) | The umbrella design. Includes an adjudicated first slice, Stage 0 measurements and a prior-art inventory | **Keep:** measured facts, dice, generators up / instances down, job descent, staging ladder. **Adapt:** engine placement (see [03](03-whole-house-quilt-fit.md)) |
| [mud2scummvm](https://github.com/SuperInstance/mud2scummvm) | MUD text ↔ point-and-click scene; clicks map back to commands; policy sliders become `set policy …` | **Keep** the idea that UI gestures compile to the same decisions agents emit (spec §6, possess). **Later:** policy sliders for spectator vibe-coding |
| [terrain](https://github.com/SuperInstance/terrain) | `rooms.mud` text compiled into Three.js scenes; material inferred from words; "true state vs shadows" | **Keep:** shadows never feed back (D8). **Later:** text room descriptions as the procgen input for L1+ chunks. Nugget mine; verify before lifting |
| [lau-terrain](https://github.com/SuperInstance/lau-terrain) | Rust grid map with several render modes ("same state, different lenses") | **Drop** the code; the idea is already D8 |
| [chrono-tensor](https://github.com/SuperInstance/chrono-tensor) | 4D event format (tick, track 0–15, intent vector, pressure) + provenance + cost; scrubbable ring buffer; downmixing | **Adapt:** downmixing rules for agent memory and the archive (D30); cost on every event (already in provenance) |
| [continuum-runtime](https://github.com/SuperInstance/continuum-runtime) | Rust runtime where patches wake on stream events; kill-test parity | **Keep:** the kill test (SIGKILL mid-round, resume, identical stream) in our soak tests. **Drop:** the runtime itself |
| [plato-portal](https://github.com/SuperInstance/plato-portal) | Python SDK: markdown agent memory, in-memory fleet; plus `CATALOG.md` of 2,000 repos | **Later:** markdown memory is already our harness. Its CATALOG is a useful index for future scouting |
| [moldmaker](https://github.com/SuperInstance/moldmaker) | Voice turned into annotated "translation file" deposits (said vs inferred) with an agent-to-agent (A2A) twin; questions are the asset | **Adapt** for between-round vibe-coding: each spectator change request becomes a deposit (D33) |
| [stitch](https://github.com/SuperInstance/stitch) | 11-op hash-chained ledger; per-tenant surprise budget; agreement without a sequencer | **Later:** the budget-ledger file format (M2 economy across machines). The surprise budget is an interesting future budget kind |
| [lane-sheets](https://github.com/SuperInstance/lane-sheets) | Agent workflows as one JSON sheet; byte-exact trace conformance | **Later:** a candidate format for an agent's cognition pipeline once harnesses self-edit (M2). Lesson kept now: *implementations agree by identical traces* |
| [erised-cli](https://github.com/SuperInstance/erised-cli) | Cooperative-fiction runner: a DM plus characters per round; run lineage; resonance/citation diffs | **Adapt:** per-round "who cited whom" squad metric (D32); run lineage resembles our round history |

### Found by scouting

| Repo | What it is | Verdict |
|---|---|---|
| [color-squad](https://github.com/SuperInstance/color-squad) | Headless deterministic 4v4 capture-the-flag; JSON strategies; `budget.calls` hard limit; cheap/smart pulse tiers; event reflexes; season with breed, fleet-authored and hand-authored innovation; decision logs → decision graphs | **Keep:** the closest ancestor. Season plus innovation breaks feed the M2 re-harness design; pulse tiers confirm the cognition tiers; budget stop vs reflex fallback becomes a parameter (D24) |
| [voice-games](https://github.com/SuperInstance/voice-games) | COLOR SQUAD by voice: tutor-unlocked verbs, a Jev negation guard, "3 clean runs → local reflex", visible learning counters | **Keep:** tutor ladder for the human support role (D29); reflex promotion (D25); negation guard |
| [thor-firstline](https://github.com/SuperInstance/thor-firstline) | Stormworks co-pilot: the planner compiles speech into a numeric policy, a Lua executor runs it; refusal is an outcome; TTL → HOLD; Lua↔Python parity vectors in CI; half-duplex voice gate | **Keep:** refusal as outcome and intent TTL (D26), parity vectors (D10 testing), half-duplex gate (D28) |
| [voice-reflex-gate](https://github.com/SuperInstance/voice-reflex-gate) | STT text plus context hashed to a cached response; fuzzy tier; after 3 similar answers a reflex compiles; reflexes decay, get superseded or evicted | **Keep:** bark cache and reflex promotion with decay (D25) |
| [pincher](https://github.com/SuperInstance/pincher) / [saddle](https://github.com/SuperInstance/saddle) | Reflex shell (embedding match ≥0.80 fires directly); harness toolkit with double-entry ledger per cell, **frozen states**, night cycles, directives sent in pieces | **Adapt:** frozen harness snapshots as the rollback unit (D27). **Later:** night cycles |
| [quilt-conversation](https://github.com/SuperInstance/quilt-conversation) | Conversation as a live sequencer: cadence per agent; T-minus pre-drafting; nudges; energy budget; silence as signal | **Adapt:** pre-drafting replies and airtime budget for squad talk (D31) |
| [cmidi-core](https://github.com/SuperInstance/cmidi-core) | Speech acts → pitch classes, paralinguistics → CC, agent → channel; output is a valid SMF; tension curve analysis | **Keep** as the edge `.mid` export mapping (D9). Verify the crate before depending on it |
| [erised-sequencer](https://github.com/SuperInstance/erised-sequencer) | Rewindable scene engine; dice seeded from `sha256(prev_tip|seq|solid|n)`; scars persist through rewind | **Adapt:** model "dice" sampled from the sim's seeded RNG (D23). Scars-through-rewind fits Studio-mode branching (M3) |
| [midi-temporal](https://github.com/SuperInstance/midi-temporal) | "16 tracks = 16 characters"; words as timed events; "drive last-mile TTS" from captured cadence | **Keep** (already D12); confirms the utterance-score direction |
| [opensmile-bridge](https://github.com/SuperInstance/opensmile-bridge) | eGeMAPS voice features (F0, jitter, shimmer, HNR…) mapped to MIDI CC | **Later (M2):** human prosody capture into utterance lanes. Watch the licence: openSMILE is not permissive for commercial use |
| [lau-fixedpoint](https://github.com/SuperInstance/lau-fixedpoint) | Q16.16 in i64, trig, sqrt, lerp, no_std, zero deps | **Adapt:** review it as the `lm-core` fixed-point base, or keep our own. Decide in phase 1 by property tests (D10) |
| [proof-game-sync](https://github.com/SuperInstance/proof-game-sync) | Float drift vs snapping floats to a lattice after each step | **Drop** as a method. Snapping still diverges when values straddle a rounding boundary. Integer fixed point (D10) stands |
| [dungeon-ml-zoo](https://github.com/SuperInstance/dungeon-ml-zoo) | Five small learners on a deterministic dungeon; **sealed pre-registered predictions**; train seeds and eval seeds never mixed | **Keep:** honest re-harness gates on held-out seeds (D32) |
| [fleet-rooms](https://github.com/SuperInstance/fleet-rooms) | Glue pipeline: room field → JSONL MIDI bars → audio; "one truth-holder per quantity" | **Keep** the doctrine. Drop the code |
| [holodeck-core](https://github.com/SuperInstance/holodeck-core) | no_std Rust MUD engine: room graph, gauges, scoped comms | **Drop** for M1; our sim is spatial, not a room graph |
| [old-school-machine-wisdom](https://github.com/SuperInstance/old-school-machine-wisdom) | FORTH threaded code, DGD trampolines, MUD1 shared memory, MUDDL "the function lives in the data" | **Later:** MUDDL's data-as-program is how Rhai rulesets already work |
| [quilt-engine](https://github.com/SuperInstance/quilt-engine) / [jev-quilt](https://github.com/SuperInstance/jev-quilt) | Send/receive agreement, Jev-pair seam arbitration with a deadband | **Later:** relevant only if last-mile federates with the quilt |
| `foundry` (this account) | A fork of foundry-rs (Ethereum tooling) | **Not** the Thor's asset Foundry. That Foundry is a Thor-local service on :8200, see [01](01-measured-facts.md) |
| `fleet-midi-*` (~60 repos) | One small MIDI service each | **Drop.** The ideas are already covered by D8, D9 and D12 |

**Not yet read, worth a look when their phase comes:**
- `lau-agent-runtime`: agents building mini-interpreters with energy budgets (M2 harness).
- `spectral-prosody`: prosody timing analysis (M2 tone learning).
- `lau-instinct`: a reflex engine.
- `band-protocol-rs`: musical-ensemble wire protocol.
- `lau-replay`: no description.
- `quilt-tournament`: no README.
- `Mycelium`: "a prompt plus a seed reproduces the action".

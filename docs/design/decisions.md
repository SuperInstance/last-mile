# Design decisions

Append-only. Each entry: date, decision, why, and where it's discussed. If an entry is superseded, add a new one that references it.

| # | Date | Decision | Why | Ref |
|---|---|---|---|---|
| D1 | 2026-10-28 | Engine: **Bevy (Rust)** | Code-first ECS, precise memory control, and the user prefers it | brainstorm |
| D2 | 2026-10-28 | Visual target: **retro-modern**: low-poly, vertex colors, shared palette, ~640×360–960×540 internal resolution upscaled | Fits 6 GB VRAM; hides generative-asset artifacts; spends budget on world scale and agent count | brainstorm |
| D3 | 2026-10-28 | Milestone 1 is a **thin slice through every tier** | The hard problems are at the seams between machines | brainstorm |
| D4 | 2026-10-28 | **2–4 squads × 4 agents**, each squad with a visible leader | Manageable count; squads give the AI a structure and give the network a batching unit | brainstorm |
| D5 | 2026-10-28 | Human is an **embodied squad member** (any role, not necessarily the leader) or a **spectator** (free camera / drone view) | The human can learn the game in a support role | brainstorm |
| D6 | 2026-10-28 | Talk within squads is the **core mechanic**; rounds form a **learning loop** | Squads learning to communicate is the game | research/05, 09 |
| D7 | 2026-10-28 | Normal mode: between-round changes go through a **sandboxed, hot-reloaded scripting layer**. **Studio mode** (A/B test and vibe-code anything) is a separate mode. | Rules stay fixed within a round so learning and scoring mean something | research/06 |
| D8 | 2026-10-28 | **The MIDI principle:** a stream of performances (decisions) separate from rendering; agent = channel, action = note, round log = score, General Action Set | Tiny stream, renderer-independent, LOD for free, DAW tooling as a bonus | research/07 |
| D9 | 2026-10-28 | Native binary protocol that **mirrors MIDI semantics**; not MIDI UMP on the wire. Real MIDI only at the edges (controllers, `.mid` export). | Payloads are semantic; MIDI's field widths don't fit | research/02, 07 |
| D10 | 2026-10-28 | **Only decisions travel; deterministic lockstep sim on every node; fixed-point math in the core** | Verified in the MIDI Maze source: 1 joystick byte per player per tick. Fixed point avoids x86/ARM float divergence. | research/08 |
| D11 | 2026-10-28 | **AI decisions are scheduled for future ticks; the sim never waits** (late → reflex fallback) | Lockstep's fatal flaw was the slowest node stalling everyone | research/08 |
| D12 | 2026-10-28 | **Utterance = score**: words timed against tone tracks (pitch, energy, rate, emphasis, pauses), plus intent and affect tags | Lossless nuance between agents; the human's tone is captured too; tone can be learned | research/09 |
| D13 | 2026-10-28 | **Agents perceive the score (a physics-filtered view), never pixels**; screens and speakers are for humans | The DAW piano-roll analogy; Thief-style numeric perception | research/10 |
| D14 | 2026-10-28 | **The Thor pre-renders ahead and the 4050 composites**: GI baked into vertex colors, neural vistas, impostor sprites, generated textures; progressive chunk fidelity L0–L3; liked-asset bank on the Orin | EdenLive-class per-frame neural rendering needs a 5090-class GPU | research/10, 11 |
| D15 | 2026-10-28 | **Agents, not bots**: pi-style self-edited harnesses, re-harnessed between rounds; bootstrap harness = situation summaries from the score | The game plays from round 1 and grows from there | research/12 |
| D16 | 2026-10-28 | **Budgets are gameplay**: per-provider token allotments with an exchange table; local models served by a **least-use-first fair-share queue** per round | Subscriptions, not dollars; rewards saving fast thinking for the late round | research/12 |
| D17 | 2026-10-28 | Build approach **A: simulation core first** (shared `lm-core` + `lm-proto`, 2D overhead/piano-roll view, then 3D) | Determinism and replay are correct from day one | brainstorm |
| D18 | 2026-10-28 | **The ProArt is the sequencer and authority**; the Thor and Orin run mirror sims from the decision stream | Human input never waits on the network | brainstorm |

## Pending (under discussion)
- Section 2 of the design: 30 Hz tick, 100 ms minimum scheduling lead, decision kinds, stale-drop policy
- Thor model inventory (spec sheet coming from the user)

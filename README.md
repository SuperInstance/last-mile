# last-mile

A procedurally generated world for game, simulation and AI-agent play, spread across a small home cluster:

| Machine | Role |
|---|---|
| **ProArt laptop** (RTX 4050 6 GB, Ryzen AI 9 HX) | Sequencer and authoritative simulation; last-mile rendering (the 4050 composites what the Thor pre-renders); human input and audio |
| **Jetson AGX Thor** | Agent host: mirror simulation, per-agent score views, fast models, Riva speech, vector DB canon, pre-rendering ahead of the player |
| **Jetson Orin Nano Super** (2 TB NVMe) | Archive: round logs, content-addressed asset bank, headless replay verification |
| **Cloud subscriptions** (Claude, MiniMax, Kimi, z.ai) | Slow "System 2" thinking, spent from per-provider token allotments |

**Status:** design phase. No code yet. A written spec and implementation plan come next.

## Core ideas (one line each)
- **The game is the score.** As with MIDI, a tiny stream of decisions is the whole game. Renderers are instruments: a better one sounds richer but never changes the rhythm.
- **Only decisions travel** (MIDI Maze, 1987). Every machine runs the same deterministic fixed-point simulation.
- **Agents play the piano roll, not pixels.** Screens and speakers are for humans only.
- **Agents, not bots.** Each agent owns a pi-style harness that it rewrites between rounds, and spends compute, token, storage and context budgets as part of the game.
- **Utterances are scores:** words timed against tone tracks, so squads can learn which phrasing and tone actually work.
- **Never wait.** Late AI falls back to reflexes, and late pre-rendering falls back to lower chunk quality.

## Repository map
| Path | What |
|---|---|
| [`docs/design/decisions.md`](docs/design/decisions.md) | Running log of design decisions, with reasons. Start here. |
| [`docs/research/forclaude2/`](docs/research/forclaude2/00-overview.md) | Research digests: the MIDI principle, the MIDI Maze source study, utterance scores, EdenLive, pre-render-ahead, self-harnessing agents |

## For agents picking this up
Read `docs/design/decisions.md` first, then `docs/research/forclaude2/00-overview.md`. Commit messages explain *why* each piece was added. Decisions are append-only: supersede an entry, don't rewrite history.

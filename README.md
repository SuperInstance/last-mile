# last-mile

A procedurally generated world for game, simulation and AI-agent play, spread across a small home cluster:

| Machine | Role |
|---|---|
| **ProArt laptop** (RTX 4050 6 GB, Ryzen AI 9 HX) | Sequencer and authoritative simulation; last-mile rendering (the 4050 composites what the Thor pre-renders); human input and audio |
| **Jetson AGX Thor** | Agent host: mirror simulation, decision models (d1, Clef) for fast thinking, Riva speech, vector-DB canon, pre-rendering ahead of the player |
| **Jetson Orin Nano Super** (2 TB NVMe) | Archive: round logs, content-addressed asset bank, headless replay verification. It yields to Casey's desktop |
| **Cloud subscriptions** (Claude, MiniMax, Kimi, z.ai, plus Jev/Clef APIs) | Slow "System 2" thinking and teacher-side judging, spent from per-provider allotments |

**Status (2026-10-28):** building **Milestone 1, Phase 1 (Core)** on branch [`phase1-core`](https://github.com/SuperInstance/last-mile/tree/phase1-core). It's a headless, fixed-point, deterministic simulation of a 2-squad "Beacons" round that replays bit-exact.
- Twelve of fifteen plan tasks are merged, with 153 tests green in debug and release.
- Remaining: the sim step (Task 11), the CLI and replay proof (Task 14), and `lm branch` (Task 15).
- Plan: [`docs/superpowers/plans/2026-10-28-m1-phase1-core.md`](docs/superpowers/plans/2026-10-28-m1-phase1-core.md).

## Core ideas (one line each)
- **The game is the score.** As with MIDI, a tiny stream of decisions is the whole game. Renderers are instruments: a better one sounds richer but never changes the rhythm.
- **Only decisions travel** (MIDI Maze, 1987). Every machine runs the same deterministic fixed-point simulation, with no floats in the core.
- **Agents play the piano roll, not pixels.** They see a fog-of-war `ScoreView`. Screens and speakers are for humans only.
- **Fast thinking is a decision model answering typed questions** (gates) over **facts that code computes** (D37, D49): code is the calculator, the model is the judge.
- **Agents, not bots.** Each agent owns a harness (prompts, Rhai reflexes, building blocks, gates) that it rewrites between rounds, and spends compute, token, storage and airtime budgets as part of the game.
- **Never wait.** Late thinking falls back to each plan's own **safe branch** (FASTER), then to reflexes.
- **The permanent test:** watch a stretch of play, change one rule, re-run from that point, feel the difference (`lm branch`, Task 15).

## Build and test (WSL / Linux)

```bash
bash scripts/check.sh          # fmt, clippy -D warnings, no-float guard, tests (debug + release)
cargo test --workspace         # tests only
cargo run -p lm-tools -- gen-trig > /tmp/t.rs   # regenerate the pinned trig table (it must stay byte-identical)
```

Toolchain: Rust 1.99, pinned by `rust-toolchain.toml`. The ProArt-native Bevy host (phase 2) builds with Windows `cargo` from a Windows path.

## Crates

| Crate | What it is | Rule |
|---|---|---|
| [`lm-core`](crates/lm-core/src/lib.rs) | The deterministic simulation: fixed-point math, worldgen, LOS, JPS, decisions and sequencer, lane conditioner, agents with safe branches, fog-of-war views and menus, Beacons rules, sandboxed Rhai reflexes, the hash-chained round log | No floats, no unordered containers, no clock, no network. Overflow checks on in release |
| [`lm-proto`](crates/lm-proto/src/lib.rs) | Wire messages, a strict postcard codec, the version handshake | No transport yet (QUIC comes in phase 3) |
| [`lm-tools`](crates/lm-tools/src/main.rs) | The `lm` CLI: `gen-trig` today; `run` / `replay` / `diff` / `golden` / `branch` coming in Tasks 14–15 | Floats allowed here only (the table generator) |

## Repository map

| Path | What |
|---|---|
| [`docs/design/decisions.md`](docs/design/decisions.md) | Append-only log of design decisions (D1–D50) with reasons. **Start here.** |
| [`docs/superpowers/specs/`](docs/superpowers/specs/2026-10-28-last-mile-m1-design.md) | The Milestone 1 design spec |
| [`docs/superpowers/plans/`](docs/superpowers/plans/2026-10-28-m1-phase1-core.md) | The Phase 1 implementation plan (TDD tasks) |
| [`docs/research/forclaude2/`](docs/research/forclaude2/00-overview.md) | Research digests: the MIDI principle, the MIDI Maze source study, utterance scores, EdenLive, pre-render-ahead, self-harnessing agents |
| [`docs/research/fleet/`](docs/research/fleet/00-overview.md) | Casey's other repos (COLOR SQUAD, thor-firstline, the whole-house quilt, Syzygy…), decision models, where it all leads |
| [`docs/research/poc/`](docs/research/poc/) | Proof-of-concept experiments with code and results: presence lanes, adaptive lead, tiles, trainer/student, generation as decisions, Jev/Clef/d1 on our task, Wi-Fi cost, the determinism audit |

## How this is built

Claude (the design lead) writes the plan and a brief per task. The **ZCode** and **MiniMax Code** seats implement tasks in parallel git worktrees. Claude reviews every diff, runs the full checks, and commits with the seat named in the message. Commit messages explain *why*.

## For agents picking this up

Read `docs/design/decisions.md` first, then the plan. Decisions are append-only: supersede an entry, don't rewrite history. Determinism is the product: a change that moves a golden hash must update `tests/golden.toml` in the same commit, with the reason.

Spin-off ideas that generalize beyond this game live in [SuperInstance/decision-daw](https://github.com/SuperInstance/decision-daw).

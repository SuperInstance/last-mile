# last-mile

A procedurally generated world for game, simulation and AI-agent play, spread across a small home cluster:

| Machine | Role |
|---|---|
| **ProArt laptop** (RTX 4050 6 GB, Ryzen AI 9 HX) | Sequencer and authoritative simulation; last-mile rendering (the 4050 composites what the Thor pre-renders); human input and audio |
| **Jetson AGX Thor** | Agent host: mirror simulation, decision models (d1, Clef) for fast thinking, Riva speech, vector-DB canon, pre-rendering ahead of the player |
| **Jetson Orin Nano Super** (2 TB NVMe) | Archive: round logs, content-addressed asset bank, headless replay verification. It yields to Casey's desktop |
| **Cloud subscriptions** (Claude, MiniMax, Kimi, z.ai, plus Jev/Clef APIs) | Slow "System 2" thinking and teacher-side judging, spent from per-provider allotments |

**Status (2026-10-28):** **Phase 1 core: headless rounds replay bit-exact.** A headless, fixed-point, deterministic simulation of a 2-squad "Beacons" round, played by reflex-only agents and replayed tick by tick to the same hashes. Building on branch [`phase1-core`](https://github.com/SuperInstance/last-mile/tree/phase1-core).
- Thirteen of fifteen plan tasks are merged, with 176 tests green in debug and release.
- Remaining: `lm branch` (Task 15), the permanent "watch a stretch, change one thing, re-run" test.
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
bash scripts/check.sh          # fmt, clippy -D warnings, no-float guard, tests (debug + release), lm golden, acceptance session
cargo test --workspace         # tests only
cargo run --bin lm -- gen-trig > /tmp/t.rs   # regenerate the pinned trig table (it must stay byte-identical)
```

The headless tools. The crate is `lm-tools`; the binary it builds is `lm`:

```bash
cargo build --release -p lm-tools                                  # builds ./target/release/lm
./target/release/lm run --seed 1 --ticks 9000 --out /tmp/r.lml   # play a round, write its log
./target/release/lm replay /tmp/r.lml                            # OK <n> hashes
./target/release/lm diff /tmp/a.lml /tmp/b.lml                   # the first tick that moves
./target/release/lm golden                                       # OK 3/3, against tests/golden.toml
./target/release/lm golden --write                               # re-pin tests/golden.toml
```

`cargo run --release --bin lm -- <subcommand>` does the same thing without the
explicit build. `bash scripts/acceptance.sh` drives the built binary through a
whole session (run, replay, diff, golden) and says PASS or FAIL per step.

`lm run` can also inject decisions from a JSON Lines file
(`--inject decisions.jsonl`), one decision and its arrival tick per line:

```json
{"arrived": 120, "decision": {"apply_tick": 120, "source": {"Agent": 0}, "seq": 1, "kind": {"ActionOn": "Hold"}, "provenance": "Test"}}
```

Toolchain: Rust 1.99, pinned by `rust-toolchain.toml`. The ProArt-native Bevy host (phase 2) builds with Windows `cargo` from a Windows path.

## Crates

| Crate | What it is | Rule |
|---|---|---|
| [`lm-core`](crates/lm-core/src/lib.rs) | The deterministic simulation: fixed-point math, worldgen, LOS, JPS, decisions and sequencer, lane conditioner, agents with safe branches, fog-of-war views and menus, Beacons rules (native and scripted), sandboxed Rhai reflexes, the hash-chained round log | No floats, no unordered containers, no clock, no network. Overflow checks on in release |
| [`lm-proto`](crates/lm-proto/src/lib.rs) | Wire messages, a strict postcard codec, the version handshake | No transport yet (QUIC comes in phase 3) |
| [`lm-tools`](crates/lm-tools/src/main.rs) | The `lm` CLI (the crate is `lm-tools`, the binary it builds is `lm`): `run`, `replay`, `diff`, `golden` and `gen-trig`, plus the headless runner ([`lm_tools::runner`](crates/lm-tools/src/runner.rs)) the phase-2 host will drive | Floats allowed here only (the table generator) |
| [`lm-archive`](crates/lm-archive/src/lib.rs) | The Orin archive: the `lm-archive` command and the crate behind it — the flat store (rounds, reports, state, every file written atomically), the framed `lm_proto::Msg` ingest, the headless verifier that replays a round and checks every state hash, the service (accept loop, a thread per connection, a verification worker with a bounded queue), the yielding to the desktop (`nice`/`ionice`, applied by re-executing the process) and the cross-architecture golden check | No floats, no clock in the round, std-only networking. A round is never rewritten |

## Repository map

| Path | What |
|---|---|
| [`docs/design/decisions.md`](docs/design/decisions.md) | Append-only log of design decisions (D1–D51) with reasons. **Start here.** |
| [`tests/golden.toml`](tests/golden.toml) | The pinned golden hashes of the headless rounds. Re-pin with `lm golden --write`, and say in the commit why the world moved |
| [`docs/superpowers/specs/`](docs/superpowers/specs/2026-10-28-last-mile-m1-design.md) | The Milestone 1 design spec |
| [`docs/superpowers/plans/`](docs/superpowers/plans/2026-10-28-m1-phase1-core.md) | The Phase 1 implementation plan (TDD tasks) |
| [`docs/research/forclaude2/`](docs/research/forclaude2/00-overview.md) | Research digests: the MIDI principle, the MIDI Maze source study, utterance scores, EdenLive, pre-render-ahead, self-harnessing agents |
| [`docs/handoffs/`](docs/handoffs/orin-archive-and-golden.md) | Handoffs to the other boxes: building the archive on the Orin, running the golden check there and on the Thor, and running the service under `systemd --user` |
| [`docs/research/fleet/`](docs/research/fleet/00-overview.md) | Casey's other repos (COLOR SQUAD, thor-firstline, the whole-house quilt, Syzygy…), decision models, where it all leads |
| [`docs/research/poc/`](docs/research/poc/) | Proof-of-concept experiments with code and results: presence lanes, adaptive lead, tiles, trainer/student, generation as decisions, Jev/Clef/d1 on our task, Wi-Fi cost, the determinism audit |

## How this is built

Claude (the design lead) writes the plan and a brief per task. The **ZCode** and **MiniMax Code** seats implement tasks in parallel git worktrees. Claude reviews every diff, runs the full checks, and commits with the seat named in the message. Commit messages explain *why*.

## For agents picking this up

Read `docs/design/decisions.md` first, then the plan. Decisions are append-only: supersede an entry, don't rewrite history. Determinism is the product: a change that moves a golden hash must update `tests/golden.toml` in the same commit, with the reason.

Spin-off ideas that generalize beyond this game live in [SuperInstance/decision-daw](https://github.com/SuperInstance/decision-daw).

# last-mile

A procedurally generated world for game, simulation and AI-agent play, spread across a small home cluster:

| Machine | Role |
|---|---|
| **ProArt laptop** (RTX 4050 6 GB, Ryzen AI 9 HX) | Sequencer and authoritative simulation; last-mile rendering (the 4050 composites what the Thor pre-renders); human input and audio |
| **Jetson AGX Thor** | Agent host: mirror simulation, decision models (d1, Clef) for fast thinking, Riva speech, vector-DB canon, pre-rendering ahead of the player |
| **Jetson Orin Nano Super** (2 TB NVMe) | Archive: round logs, content-addressed asset bank, headless replay verification. It yields to Casey's desktop |
| **Cloud subscriptions** (Claude, MiniMax, Kimi, z.ai, plus Jev/Clef APIs) | Slow "System 2" thinking and teacher-side judging, spent from per-provider allotments |

**Status (2026-10-29):** the trunk is [`m1-integration`](https://github.com/SuperInstance/last-mile/tree/m1-integration). Every phase branch is merged into it, and `bash scripts/check.sh` passes.

| Phase | State |
|---|---|
| 1. Core | Done: headless, fixed-point, deterministic rounds replay bit-exact, on x86-64 and on the Thor's aarch64 (D69). `lm branch` rewinds, changes one thing and re-runs |
| 2. Host | Done (Casey's hands-on acceptance pending): the Bevy map, piano roll, HUD and replay on the ProArt |
| 3. Agents | Done: agents think on the Thor's d1 models in the host. d1-2B is the default fast tier (D62), and members think at 1 Hz, sized to the Thor's measured supply (D68) |
| 4. Talk | Done: utterances, notation, hearing physics, agents speaking, the voice crate, push-to-talk, the half-duplex gate, talk metrics, and talk in the host (`--talk off\|stub\|riva`). In progress: agents barely speak on the real model, and that's being tuned |
| 5. Learning | Done: metrics, router, sealed ledger, review, held-out eval, deposits, Rhai rulesets, `lm learn`, and the between-round screen (human overrides are carried out). Open: the D63 tie-break call |
| 6. 3D + archive | Done: the 3D view (`--view 2d\|3d\|both`, `V` switches), integer upscale, shared fog, and the Orin archive |

Measured facts that shape the design live in [`docs/design/decisions.md`](docs/design/decisions.md) (D1–D69), for example: draws are mirror ties, not timeouts (D63), and the Thor serves about 9 decisions/s on d1-2B, roughly one thought per agent per second (decision-daw x5).

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
bash scripts/live.sh           # the live lane: the ignored tests that need the real Thor (run it on the LAN)
cargo test --workspace         # tests only
cargo run --bin lm -- gen-trig > /tmp/t.rs   # regenerate the pinned trig table (it must stay byte-identical)
```

**The live lane.** `bash scripts/live.sh` runs the ignored tests that need the real Thor: `live_thor`, `live_round_on_thor`, the gates tuning and the talk soak. Run it before merging anything that touches `lm-agents`' wire format, the gates or the harness packs — those are the changes that can break the System One request without breaking a single offline test, and the gate cannot see them. It preflights the Thor's health on the ports in `AgentsConfig`'s defaults and prints `SKIP: Thor unreachable` (exit 0) when the Thor is not up, because a missing Thor is not a failure of the code; otherwise it runs the tests and checks the live facts: the Thor's answers become decisions in the game, and the soak prints its table.

The headless tools. The crate is `lm-tools`; the binary it builds is `lm`:

```bash
cargo build --release -p lm-tools                                  # builds ./target/release/lm
./target/release/lm run --seed 1 --ticks 9000 --out /tmp/r.lml   # play a round, write its log
./target/release/lm replay /tmp/r.lml                            # OK <n> hashes
./target/release/lm diff /tmp/a.lml /tmp/b.lml                   # the first tick that moves
./target/release/lm golden                                       # OK 3/3, against tests/golden.toml
./target/release/lm golden --write                               # re-pin tests/golden.toml
./target/release/lm learn --seed 1 --harness ./harness           # one learning round
```

`lm learn` runs one learning round end to end: it plays the round, measures
it, asks a reviewer for a change, **seals what the change is expected to do
before testing it**, evaluates the change on held-out seeds, and adopts it
only if the evidence backs it. The harness is a directory of harnesses, one
per agent (`a0`, `a1`, …); a change lands in that agent's harness repo, in a
commit tagged `round-<id>`, and the next round's header pins that commit.
Everything the round did is on disk: the log, the ledger (`runs/ledger.jsonl`),
the deposit (`runs/deposits/`), the flow record (`runs/flow/<round-id>.json`)
and the harness's git history.

```bash
./target/release/lm learn --seed 1 --harness ./harness --runs ./runs
./target/release/lm learn --seed 1 --harness ./harness --reviewer router   # a cloud pool reviews
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
| [`lm-proto`](crates/lm-proto/src/lib.rs) | Wire messages, a strict postcard codec, the version handshake | |
| [`lm-tools`](crates/lm-tools/src/main.rs) | The `lm` CLI (the crate is `lm-tools`, the binary it builds is `lm`): `run`, `replay`, `diff`, `golden`, `branch`, `learn` and `gen-trig`, plus the headless runner ([`lm_tools::runner`](crates/lm-tools/src/runner.rs)) the phase-2 host will drive | Floats allowed here only (the table generator) |
| [`lm-learn`](crates/lm-learn/src/lib.rs) | The between-round learning loop: the metrics of a round, the sealed-prediction ledger, the reviewer's proposal and its strict parsing, the held-out evaluation, the deposits, and the [`flow`](crates/lm-learn/src/flow.rs) that chains them into one auditable round | No floats, no clock, no network of its own: the keys stay in `lm-router` |
| [`lm-archive`](crates/lm-archive/src/lib.rs) | The Orin archive: the `lm-archive` command and the crate behind it — the flat store (rounds, reports, state, every file written atomically), the framed `lm_proto::Msg` ingest, the headless verifier that replays a round and checks every state hash, the service (accept loop, a thread per connection, a verification worker with a bounded queue), the yielding to the desktop (`nice`/`ionice`, applied by re-executing the process) and the cross-architecture golden check | No floats, no clock in the round, std-only networking. A round is never rewritten |
| [`lm-agents`](crates/lm-agents/src/lib.rs) | How agents think: gates, plain-language facts, System One calls to the Thor's d1 models or the Jev cloud tier, the least-use-first queue, the Mind and its tier table, utterances | Network is optional: the stub and the reflex keep a round playable offline |
| [`lm-host-model`](crates/lm-host-model/src/lib.rs) | The host's logic without a screen: the round, the map and replay models, the agents' wiring, turn detection | Pure logic, tested in the gate |
| [`lm-host`](crates/lm-host/src/main.rs) | The Windows-only Bevy app around `lm-host-model` | Doesn't build in WSL: `bash scripts/win-build.sh build -p lm-host --release` |
| [`lm-router`](crates/lm-router/src/lib.rs) | The cloud purse: provider adapters with metered allotments, cold pools and config-order fallback | Keys come from `~/.config/last-mile/secrets.env` and are never logged |
| [`lm-render`](crates/lm-render/src/lib.rs) | Bevy-free rendering logic: the palette, the chunk mesh builder, the greedy mesher, chunk hashes, the agent figure | No GPU needed; tested in the gate |
| [`lm-voice`](crates/lm-voice/src/lib.rs) | Speech: TTS/ASR traits, stubs, Riva placeholders, the bark cache, and an audibility gate so only lines the human can hear are spoken | |

## Repository map

| Path | What |
|---|---|
| [`docs/design/decisions.md`](docs/design/decisions.md) | Append-only log of design decisions (D1–D69) with reasons. **Start here.** |
| [`tests/golden.toml`](tests/golden.toml) | The pinned golden hashes of the headless rounds. Re-pin with `lm golden --write`, and say in the commit why the world moved |
| [`docs/superpowers/specs/`](docs/superpowers/specs/2026-10-28-last-mile-m1-design.md) | The Milestone 1 design spec |
| [`docs/superpowers/plans/`](docs/superpowers/plans/) | The implementation plans: [phase 1](docs/superpowers/plans/2026-10-28-m1-phase1-core.md), [2](docs/superpowers/plans/2026-10-28-m1-phase2-host.md), [3](docs/superpowers/plans/2026-10-28-m1-phase3-agents.md), [4](docs/superpowers/plans/2026-10-29-m1-phase4-talk.md), [5](docs/superpowers/plans/2026-10-28-m1-phase5-learning.md), [6](docs/superpowers/plans/2026-10-29-m1-phase6-3d-archive.md) |
| [`docs/research/forclaude2/`](docs/research/forclaude2/00-overview.md) | Research digests: the MIDI principle, the MIDI Maze source study, utterance scores, EdenLive, pre-render-ahead, self-harnessing agents |
| [`docs/handoffs/`](docs/handoffs/orin-archive-and-golden.md) | Handoffs to the other boxes: building the archive on the Orin, running the golden check there and on the Thor, and running the service under `systemd --user` |
| [`docs/research/fleet/`](docs/research/fleet/00-overview.md) | Casey's other repos (COLOR SQUAD, thor-firstline, the whole-house quilt, Syzygy…), decision models, where it all leads |
| [`docs/research/poc/`](docs/research/poc/) | Proof-of-concept experiments with code and results: presence lanes, adaptive lead, tiles, trainer/student, generation as decisions, Jev/Clef/d1 on our task, Wi-Fi cost, the determinism audit |

## How this is built

Claude (the design lead) writes the plan and a brief per task. The **ZCode** and **MiniMax Code** seats implement tasks in parallel git worktrees. Claude reviews every diff, runs the full checks, and commits with the seat named in the message. Commit messages explain *why*.

## For agents picking this up

Read `docs/design/decisions.md` first, then the plan. Decisions are append-only: supersede an entry, don't rewrite history. Determinism is the product: a change that moves a golden hash must update `tests/golden.toml` in the same commit, with the reason.

Spin-off ideas that generalize beyond this game live in [SuperInstance/decision-daw](https://github.com/SuperInstance/decision-daw).

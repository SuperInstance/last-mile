# Magnus's ecosystem (Lucineer + SuperInstance): what last-mile can take

**Read:** 2026-10-30. **Scope:** the `Lucineer/*` and `SuperInstance/*` repositories named in the brief, read through `gh api` only (READMEs, docs, key sources, tests, licences, build files). No repo was cloned and no code was run.
**Purpose:** decide what last-mile borrows from Magnus's line — the paintball-city sandbox (`docs/design/sandbox-on-otherhere.md`), the learning flow (`crates/lm-learn/SEEDFARM.md`), Vox Score (`docs/design/vox-score.md`) and questions-as-experience (`docs/design/questions-as-experience.md`).
**Status of reuse:** every idea below is a *proposal*. None of these repositories carry a licence file that grants last-mile the right to copy code (see §2), so anything taken has to be re-written, or Casey's explicit OK has to be obtained first.

---

## 0. Method and its limits

Every repository below was fetched read-only through `gh api` and inspected as files. Nothing was cloned; no build or test was executed; no repository was modified. The method matters because several claims in this line are carried by *documentation* rather than by code, and the difference is flagged wherever it changes the conclusion.

Three caveats to carry through everything that follows:

1. **Some repositories are requests, not implementations.** `jepa-perception-lab` is three documents and no code; the CUDA simulation it describes lives elsewhere. `cudaclaw` (Lucineer) is a README and a `CLAUDE.md` and no source. Read their READMEs as *design intent*, not as working systems.
2. **Licence claims are mostly not licence files.** The GitHub API reports `MIT` for several repositories whose README *says* MIT but whose tree contains no `LICENSE` file. Under default copyright, no file means all rights reserved. The table records what is actually in the tree.
3. **The `cuda-*` prefix is a fleet naming convention, not NVIDIA.** Every `cuda-*` Rust crate inspected here has dependencies of `serde`/`rand` only — no `cudarc`, no `tcnn`, no CUDA at all. Calling them "CUDA" is how the fleet names things, not what they are. This matters for portability (§11) and for the deep section on perception.

---

## 1. Findings in brief

**What Magnus already built that last-mile was about to build** — the single most important finding is that this is not a stranger's codebase. `SuperInstance/Lucineer` — the origin project — opens with:

> "Modern agent benchmarks often rely on opaque LLM judges, making scores non-reproducible and hard to trust. This platform gives you deterministic, rule-based scoring so your experimental results are verifiable."
> — `SuperInstance/Lucineer/README.md`

and last-mile's `docs/design/questions-as-experience.md` §3 says:

> "The cloud compiles understanding into questions, and the local models execute them. One cloud call that writes a good question is then asked a million times locally for almost nothing."

Magnus runs that sentence *in production today*: `SuperInstance/scrap-spark` is a Cloudflare Worker that hashes `SHA-256(spark_version | normalized question | context)`, stores answers in R2 behind a KV index, counts hits in D1, and returns `X-Cache: HIT|MISS`. The README calls it the "pincher-cache doctrine — *model call heavy at first, then more and more canned responses*". The first child pays for the model call; every child after gets the can. [2]

The consequence for last-mile is concrete: **the question-pack economics have a working reference implementation, with a ledger format, a hit counter and a failure mode already understood.** last-mile does not have to design that.

The rest of the headline:

- **The strongest idea to take is not code — it is a discipline.** `craftmind-researcher` already requires that every hypothesis carry *a falsification criterion, an expected direction, and the prior knowledge it builds on*, and keeps a knowledge base so that known facts are not re-tested (`Lucineer/craftmind-researcher/src/discovery-agent.js:15-21`) [3]. That is last-mile's sealed-prediction discipline, written down six months earlier (the repo was created in April 2026) and applied to Minecraft.
- **The weakest thing in the ecosystem is the scorer, and it is weakest exactly where last-mile is strongest.** `self-evolve-ai` scores a mutation by asking an LLM what a *never-executed* piece of code would do; `the-seed` scores by counting keywords and gates on real deploys. [4] [5] `cuda-self-evolve` scores by `1/cycles_per_result × confidence × trust` [6] — the agent grading its own homework from its own telemetry. last-mile's sign test on 4 held-out seeds versus 256 (4 of 30 verdicts flip, all four false adopts — `crates/lm-learn/SEEDFARM.md:169`) is a stronger instrument than anything in the ecosystem, and should not be replaced by any of it.
- **The command loop last-mile wants already exists, in Minecraft.** `craftmind-herding` has a player who never commands directly, agents who interpret approximately and can *legibly refuse*, and a trust economy that gates advanced orders. [7] That is the sandbox's commander-and-squad loop with the vocabulary problem already solved (by refusal) instead of solved (by a dictionary).
- **The audio-JEPA surprise signal is a research project, not a port.** `jepa-perception-lab` contains **no audio code and no code at all**; its JEPA runs over MUD room state. [8] What it offers is a *method* and a set of falsifiable findings (§5), not a model.

---

## 2. Repository inventory

Maturity = commits on the default branch, source LOC (excluding vendored/generated trees), and the number of test/spec files. Licence = what is in the tree.

### 2.1 The craftmind family — nine Minecraft setups

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `Lucineer/craftmind` [27] | An autonomous AI player that logs into a survival server and plays like a person. `src/identity/`, `src/market/`, `src/challenges/`, `src/tournament/`, `src/editor/`, `src/economy/` | 63 commits, ~41k LOC, 6 test files | MIT (`LICENSE`) | Yes — TS/Node/Workers | The **economy and tournament** layer (`src/tournament/engine.js`, `src/market/price-history.js` [27]) is a spectator system: a city where squads earn rank. Cheap, and it gives the squad a reason to care |
| `Lucineer/craftmind-herding` [7] | Player commands border-collie agents that interpret, disobey and rewrite their own behaviour scripts. `src/ai/herding-actions.js`, `src/ai/dog-agent.js`, `src/ai/dog-trust.js`, `src/scripts/script-engine.js`, `src/scripts/v1-*.js` | 13 commits, ~6.8k LOC, 4 test files | MIT | Yes — TS | **The command loop.** See §3 |
| `Lucineer/craftmind-fishing` [9] | 23 personality scripts that fish under different temperaments; live A/B across three servers; condition-matched session comparison. `src/mineflayer/scripts/v1..v5-*.js`, `src/ai/comparative-evaluator.js`, `src/ai/decision-engine.js`, `scripts/run-experiment.js` | 85 commits, ~50k LOC, 22 test files | MIT | Yes — TS | **Condition-matched comparison** and the **confusion table**. See §4 |
| `Lucineer/craftmind-studio` [28] | A movie director: AI actors follow schedules, hit marks, remember scene context. `src/`, `docs/` | 23 commits, ~13k LOC, 12 test files | MIT | Yes — TS | The **on-set change** loop [28]: the director rewrites the scene while the actors are mid-take. Maps to a city director who changes the weather mid-round |
| `Lucineer/craftmind-researcher` [3] | An agent that learns game mechanics from scratch by hypothesis and experiment. `src/discovery-agent.js`, `src/critic-agent.js`, `src/experiment.js`, `src/experiment-validator.js`, `src/knowledge-base.js`, `src/statistics.js` | 15 commits, ~6.8k LOC, 4 test files | MIT | Yes — TS | **Falsification criteria + a knowledge base of already-answered questions.** See §13 |
| `Lucineer/craftmind-ranch` [29] | A genetic engine: populations of bots compete, breed and specialise on farm tasks. `src/dna.js`, `src/evolution-engine.js`, `src/fitness.js`, `src/population.js`, `src/dashboard.js` | 13 commits, ~5.3k LOC, 6 test files | MIT | Yes — TS | The **evolution dashboard** [29] — a spectator view of a population improving. Also the nearest thing in the ecosystem to the RL spike (§6) |
| `Lucineer/craftmind-courses` [30] | An AI teacher that finds the player in the world; NPC classmates; lessons through play. `courses/`, `curriculum.json`, `src/` | 21 commits, ~9.3k LOC, 5 test files | MIT | Yes — TS | **The teacher who comes to you.** [30] A squad member that finds the player and asks a question in character — the questions-as-experience loop made physical |
| `Lucineer/craftmind-circuits` [31] | A server plugin that validates the redstone circuits you actually placed, with 34 challenges. `src/ai/circuit-optimizer.js`, `src/scoreboard.js`, `tests/` | 12 commits, ~4.2k LOC, 4 test files | MIT | Yes — Java | **Validate the thing, not the multiple choice.** [31] The sandbox equivalent is grading the city's actual state after a command, not the command |
| `Lucineer/craftmind-discgolf` [32] | A disc-golf simulation: 73 discs, an AI caddy, personality-driven NPCs. `src/`, `assets/` | 8 commits, ~4.8k LOC, 2 test files | **None** (no `LICENSE` file) | Yes — TS | A **caddy** [32] — a companion that reads the terrain and comments. Cheap cousin of the "squad speaks in character" seat |

### 2.2 The perception family

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `Lucineer/jepa-perception-lab` [8] | Four falsifiable JEPA experiments over MUD room state: latent-dimension sweep, proper JEPA training with a contrastive loss, multi-step prediction, delta-vs-static fusion. **Three documents; the CUDA simulation is not in the repo** (`README.md`, `CLAUDE.md`, `FROM-JETSONCLAW1.md`) | 2 commits, 0 LOC | **None** | Source absent; the documented build (`nvcc -O3 -arch=sm_89`) embeds PTX, so it JITs to `sm_110` with CUDA 12.8+ | **The method and two findings.** See §5. Not a port |
| `Lucineer/flux-perception` [14] | Sensor fusion: confidence-weighted blending with bias, variance and source count. `src/lib.rs` (`Engine::update`, `Engine::read`) | 14 commits, ~300 LOC, 0 tests | **None** | Yes — pure Rust, CPU | The **variance field** is worth copying [14]: last-mile's gates carry a confidence but no spread. Weakness to avoid: confidence is the *mean* of sensor weights, and variance never feeds it |
| `Lucineer/cuda-perception` [13] | Perception pipeline: filtering, feature extraction, tracking, scene composition. `src/lib.rs` (`SignalFilter`, `EMAAlpha`, `TrackedObject`, `Scene`) | 1 commit, ~415 LOC, 0 tests | **None** | Yes — pure Rust, CPU | Its EMA/median/low-pass filters [13] are **exactly what the JEPA lab found worse than delta encoding**. A cautionary contrast, not an import |
| `Lucineer/cuda-sensor-agent` [15] | Sensor agent over `cuda-equipment`: fused observations and health monitoring. `src/lib.rs` (`SensorAgent`, `FusedObservation`, `SensorHealth`) | 1 commit, ~307 LOC, 0 tests | **None** | Yes — pure Rust, CPU | **Sensor health as a first-class output.** [15] The sandbox's squad should be able to say "my ear is degraded", which is a legitimate question to escalate |

### 2.3 The assemble / learn family

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `Lucineer/cuda-instruction-set` [12] | An 80-opcode agent ISA with **Bayesian confidence propagation**, A2A encoding, assembler/disassembler. `src/lib.rs` (`Confidence::fuse`, `Confidence::chain`) | 1 commit, ~640 LOC, 0 tests | **None** | Yes — pure Rust, CPU | **`Confidence::fuse` is `1/(1/a + 1/b)` [12]** — the correct parallel-composition rule. See §12 |
| `Lucineer/cuda-assembler` [33] | Text-to-bytecode assembler for that ISA: labels, data directives, confidence annotations. `src/` | 1 commit, ~590 LOC, 0 tests | **None** | Yes — pure Rust, CPU | A **text front-end for a confidence-carrying ISA** — the shape Bench's output would take |
| `Lucineer/flux-asm` [34] | Two-pass C11 assembler: FLUX mnemonics to bytecode for `flux-runtime-c`. `src/` | 1 commit, 1 test file | **None** | Yes — C11, CPU | Two-pass structure (resolve, then emit) for anything that must be compiled before it can be validated |
| `Lucineer/cuda-learning` [35] | Experience-based learning: credit assignment, lesson extraction, curriculum progression, generalisation. `src/lib.rs` (`Experience`, `Lesson`, `CreditAssignment`, `Curriculum`) | 1 commit, ~390 LOC, 0 tests | **None** | Yes — pure Rust, CPU | **Lesson extraction** [35] — turning episodes into named lessons. The seed farm's verdicts are lessons that were never extracted |
| `Lucineer/nexus-learning` [36] | Reinforcement learning: skill acquisition, experience replay, reward shaping. 2 files, ~90 LOC | 1 commit | **None** | Yes | Too thin to import; it is a name, not a system [36] |
| `Lucineer/swarm-intuition` [25] | Correlates timestamped cause-and-effect signals across services to anticipate bottlenecks. `src/worker.ts` | 8 commits, ~170 LOC | **None** | Yes — CF Workers | **Causal-sequence correlation over telemetry.** [25] The sandbox already logs situations; this is the shape for "what usually precedes this" |
| `Lucineer/swarm-intuition-v2` [26] | The successor: emergent swarm behaviour prediction and conflict prevention. `src/worker.ts` | 2 commits, ~90 LOC | **None** | Yes — CF Workers | Same idea, one generation further; a stub [26] |

### 2.4 The self-evolve family and the recursive project

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `Lucineer/cuda-self-evolve` [6] | Mutation, selection, genome encoding, rollback for FLUX agents. `src/mutator.rs` (`propose`/`apply`/`verify`), `src/selection.rs` (`fitness`, `compete`, `breed`), `src/genome.rs`, `src/history.rs` (`trend`), `MAINTENANCE.md` | 10 commits, ~1.2k LOC, 1 test file | **None** | Yes — pure Rust, CPU | **Snapshot-based rollback, and the argument for it.** See §6 and §13 |
| `Lucineer/cuda-self-modify` [10] | Self-modifying programs: observation → proposed mutation with confidence and reason → apply → revert/checkpoint. `src/lib.rs` (`SelfModifyingProgram`, `Checkpoint`) | 5 commits, ~244 LOC | **None** | Yes — pure Rust, CPU | "Deliberation" as **a proposal carrying a confidence, a reason and a proposer**. The shape a last-mile change request should have |
| `Lucineer/cudaclaw` [16] | The recursive project. A GPU-resident agent runtime: cell agents, muscle fibers, a Ramify PTX engine, `.claw-dna` DNA files, and an **ML feedback loop** (`ExecutionLog → SuccessAnalyzer → DnaMutator → Constraint DNA`, mutation strategies *Tighten / Relax / Specialize*). **README and `CLAUDE.md` only — no source** | 2 commits, 0 LOC | **None** | Documented only | The **DNA mutator that tightens and relaxes its own constraints**. The closest thing to last-mile's learning flow in the ecosystem — and the most interesting import (§13) |
| `Lucineer/flux-evolve` [17] | A GA in Rust: population, fitness, elite selection, crossover, mutation. `src/lib.rs` | 4 commits, ~54 LOC | **None** | Yes — CPU | Elitism and crossover for a **genome-shaped** change space [17] |
| `SuperInstance/flux-evolve-py` [18] | The same in Python, deterministic, with elite protection (threshold 0.8, gentle mutation for elites, aggressive for poor performers). `evolution.py` | 1 commit, ~950 LOC, 1 test file | MIT | Yes — CPU | Determinism + elite protection [18]: the same engine must be re-runnable, or the RL spike has no held-out set either |
| `Lucineer/git-cuda-agent` [19] | The template repo that ties it together: DNA layer, fleet protocol (`vessel.json`, A2A, A2UI), capability model. `README.md`, 13 files | 1 commit, ~740 LOC | **None** | Yes — CPU | **`.claw-dna` as a personality blueprint file** [19] — the sandbox's squad archetypes as versioned files rather than code constants |
| `Lucineer/flux-agent-runtime` [38] | Self-bootstrapping agents. The tree is empty at the default branch | no files, not inspected further | **None** | — | Nothing to import; note only |
| `Lucineer/cuda-ghost-tiles` [39] | The Rust sibling of the ghost-tiles model: hidden state with phases, strength, radius and falloff. `src/lib.rs` | 1 commit, ~490 LOC | **None** | Yes — pure Rust, CPU | Same idea as `SuperInstance/ghost-tiles-1` [23] in the fleet's main language; the fallback if the Python original is unusable |

### 2.5 `the-seed` and `self-evolve-ai`

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `Lucineer/the-seed` [5] | Give a repo a goal; it rewrites its own code on Cloudflare Workers. `src/worker.ts` (409 lines), `docs/SEED-DESIGN.md`. Three gates — syntax, `/health`, regression test — then a branch per generation (`seed-gen-N`), a KV "captain's log" with generation/plan/score, and the last three known-good snapshots in KV | 11 commits, ~410 LOC, 0 tests | README claims MIT; **no `LICENSE` file** | Yes — CF Workers + DeepSeek | **The captain's log** — a durable, human-readable record that survives code changes. The shape of last-mile's question ledger (§7) |
| `Lucineer/self-evolve-ai` [4] | Each mutation becomes a git branch; an LLM scores the change 1-10 against written criteria; branches over the threshold are opened as PRs. `src/worker.ts` (215 lines) | 10 commits, ~215 LOC, 0 tests | README claims MIT; **no `LICENSE` file** | Yes — CF Workers + DeepSeek | The **branch-per-experiment** discipline and the LLM judge as a cheap pre-filter (§7) |

### 2.6 The origin project and the swarm system

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `SuperInstance/Lucineer` [1] | The origin: "Train Agents with Deterministic Gameplay". Deterministic rule-based scoring instead of opaque LLM judges; fork-first workflow where *live running state* is duplicated; debate, argument synthesis, Socratic tutoring and multiplayer templates; Durable Objects for agent state. `README.md`, `mini-services/`, `examples/`, `db/`, `prisma/` | 16 commits, ~236k LOC (incl. web app), 50 test files | MIT | Yes — TS + CF Workers + Durable Objects | **The doctrine, stated first**: rules over opinions; forkable live state (§10) |
| `Lucineer/become-ai` [22] | A 12-line worker that becomes the tool you need. Phase 1 "bootcamp" plans an archetype/equipment/skills/interface/first-feature/test-prompt/milestones; Phase 2 applies mutations and scores before/after. `src/worker.ts`, `docs/BOOTCAMP-ARCHETYPES.md` | 10 commits, ~270 LOC | **None** | Yes — CF Workers | **The archetype → equipment → skills decomposition** [22], and a milestone ladder. Bench's "bouncy" needs exactly this shape |
| `Lucineer/agentic-compiler` [11] | Lucineer Lang and a 42-opcode deliberation VM: confidence as a primitive type, deliberation as control flow, `TensorCell` (value + confidence + alternatives + intent), confidence-propagation rules, error-as-signal. `docs/LUCINEER-LANG-DESIGN.md` | 21 commits, ~1.8k LOC | **None** | Yes — Python + Rust, CPU | **The typed IR last-mile's Vibe Dictionary is a special case of.** See §12 |
| `SuperInstance/ghost-tiles-1` [23] | Invisible computational tiles that influence visible state (attractors, phases: temporal/permanent/recurring, falloff). `src/tiles.py` | 4 commits, ~210 LOC | MIT | Yes — Python, CPU | **Hidden state with a visible effect** [23] — the squad's private state changing the world without the player seeing it |
| `Lucineer/frozen-intelligence` [24] | A mask-locked inference chip design toolchain: TLMM table-lookup matmul, yield-aware swarm tiling, vessel classes (Scout 1B / Messenger 3B / Navigator 7B / Captain 13B). `cli.py`, `src/`, `tests/test_all.py` | 79 commits, ~13.7k LOC, 2 test files | **None** | Yes — CPU (simulate/estimate) | **Vessel classes as a power budget.** [24] A squad member with a 1B "brain" should sound and think differently from a 13B one. Cheap, and it makes the Orin/Thor split legible in-fiction |
| `Lucineer/the-fleet` [37] | The vessel registry: a map of all 100+ repos by category. `README.md` | 11 files | **None** | Yes | The **index**. Use it as the entry point for any future survey instead of searching blind |

### 2.7 Scrapcraft and scrap-spark

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `SuperInstance/Scrapcraft` [20] | A browser voxel game: mine, craft, build robot companions, program them in a visual tile editor with AI assistant Spark; programs compile to real Arduino C++ and MicroPython. `src/Spark.js`, `src/spark/SparkCache.js`, `src/TileEditor.js`, `src/maker/QuiltSheet.js`, `src/maker/TileVM.js`, `src/Foreman.js`, `src/ScrapBot.js`, `docs/` (26 docs), `playtest/` | 250 commits, ~102k LOC, 52 test files, "1,196 framework-free tests" | MIT | Yes — browser + Workers AI | The richest single source of *design* in the ecosystem. §9 |
| `SuperInstance/scrap-spark` [2] | The cached Spark AI and the shared build wall. `src/index.ts` (456 lines) | 2 commits, ~460 LOC, 0 tests | **None** | Yes — CF Workers + Workers AI (llama-3.3-70b) | **The question cache.** §13 |

### 2.8 Forgemaster

| Repo | What it does (paths) | Maturity | Licence | Portable? | Idea for last-mile |
|---|---|---|---|---|---|
| `SuperInstance/forgemaster` [21] | A constraint-aware agentic compiler: requirements + constraints → assembled components + execution plan. `forgemaster/forge.py` (`Forge.compile`, `build_one`), `forgemaster/artifact.py` (digests, states), `constraint/` (per-domain constraint sets, `constraint-theory-core-cuda` FFI bridge, LLVM/MLIR IRs), `flux/flux-isa-thor` (5-stage INGEST→VALIDATE→COMPILE→EXECUTE→COMMIT, 43 opcodes, batch CSP on Thor/Orin), `flux/flux-verify-api` (claim → PROVEN/DISPROVEN with a physics trace and a Merkle `proof_hash`), `flux/flux-provenance` (Merkle trees with inclusion proofs) | 1110 commits, ~126k LOC, 89 test files | MIT | **Partly GPU** — `flux/flux-cuda/Makefile` embeds PTX for compute_72/86 (JITs to `sm_110`); `flux/flux-hardware/build.rs:15-17` is **cubin-only** (sm_70/80/86) and **will not run on the Thor** without a new gencode line | **The compile shape and the proof commitment.** §12 |

---

## 3. The command loop in `craftmind-herding`

### The command loop in the code

The player is a *trainer at the fence line*, not a commander. [7] The documented flow (`docs/HERDING_AI_REDESIGN.md:122-125`):

> "Player gives natural commands → `HerdingActions` interprets → [command] → Trust gates advanced commands (shed, guard) → Low-trust dogs ignore commands until bond is built"

The machinery behind that sentence:

- **Interpretation to a typed action.** `src/ai/herding-actions.js` maps natural phrases to a typed action schema carrying a `requiredSkill` and an `advanced` flag; advanced commands are `SHED`, `GUARD` and `ALERT`.
- **Three refusals, all legible.** `src/ai/dog-agent.js:81-114` `receiveCommand()` returns a `DISOBEY` action with a *reason* when: the dog is exhausted (`energy < 15` → `REST`, `too_tired`); the obedience roll fails (`Math.random() > personality.obedience`); or the dog lacks trust (`trust.level < 40` for an advanced command). A refusal is never silent and never arbitrary — it names its cause.
- **Personality as numbers, not adjectives.** `docs/HERDING_AI_REDESIGN.md:45` gives obedience as a 0–1 trait (`stubborn (0) → obedient (1)`), and the personality table runs from Thunder (obedience 0.9, "ignores commands when he *knows better*") to Sadie, who ignores commands until trust is built.
- **The agent writes its own behaviour.** `src/scripts/script-engine.js` runs a weighted-random branch program; each dog has a versioned script (`src/scripts/v1-rex.js`, `v1-thunder.js`, `v1-old-blue.js`, `v1-biscuit.js`) that states a `hypothesis` about how to do the job. Variants are compared by an evaluator, so the *dog* improves its approach, not the player.

### Where the fun lives

1. **The player never gets what they asked for.** The fun is in inferring *why* the dog did something else, and the refusal is legible enough to infer from.
2. **Trust is a currency, not a gate.** Low trust is not "locked"; it is a relationship you build by handling the dog well.
3. **The dog is a character with opinions.** Thunder ignoring commands because he knows better is a *joke*, and jokes are replayable content.

### The import for last-mile

The sandbox already has a commander (the player) and a squad, and last-mile already solved the *dictionary* half of the problem — the Vibe Dictionary bounds what "bouncy" may mean so the words are unambiguous. Herding solves the opposite half: **it lets the squad's interpretation be visibly its own, and makes the disagreement the content.**

The import worth taking is therefore not the schema but the **refusal as a first-class, explained outcome**:

- The squad's Vibe block should be able to answer "I heard you, and here is what I did instead, and here is why" — with the why being *disobedience*, *low confidence*, *no energy*, or *out of bounds*.
- A squad member that always complies is a menu. A squad member that sometimes refuses *legibly* is a character.
- The trust economy maps onto the **bench seats**: a squad member the player has not spent time with should be less literal about instructions.

This also gives the sandbox a **first-five-minutes** asset (the playtest lesson in §9): the first refusal, understood, is the moment the player learns the game has a vocabulary.

---

## 4. Personality scripts and the A/B in `craftmind-fishing`

### The twenty-three scripts

There are **23** personality scripts [9] (`src/mineflayer/scripts/v1-*.js` through `v5-*.js`; the brief says 22 — the extra is `v5-tree-veteran.js`). Each is a whole behaviour program, not a parameter set: a `hypothesis:` line, weighted-random branches, and chat flavour lines. Examples: `v1-aggressive.js`, `v1-lazy.js`, `v1-nervous.js`, `v1-kid.js`, `v1-stoic.js`, `v1-veteran.js`, `v2-social.js`, `v3-drunk-ernie.js`, `v3-storyteller.js`, `v3-watcher.js`, `v3-crowd.js`, `v4-patient.js`, `v4-pure.js`, `v4-competitive.js`, `v4-friendly.js`, `v5-tree-veteran.js`.

`src/ai/decision-engine.js` returns a decision with a **confidence, a reasoning string and alternative scripts**, and `src/mineflayer/script-engine.js:744` is explicit — *"Pick a random script and switch to it (for A/B testing)"*.

### The A/B machinery

- `scripts/run-experiment.js` runs **three parallel Minecraft servers (Alpha/Beta/Gamma)** on ports 25566/25567/25568 with bots `Cody_A/B/C`, each on one script, for a fixed window (1500 s, 25 minutes by default), then compares the sessions. [9]
- `src/ai/comparative-evaluator.js` is the interesting part: it evaluates a session by **matching conditions** — weather, tide, time of day, location, bait, with numeric tolerances — so a dawn catch is not compared against a dusk catch. It then extracts **insights** ("when X then Y") and ranks scripts by condition-matched historical performance.
- The script registry is a bandit: `registry.pick()` weights scripts by fish rate minus a death penalty.

### The honest weakness

This is a **live, uncontrolled, after-the-fact comparison** [9]. Three servers run concurrently; the winner is chosen afterwards by condition matching; nothing is pre-registered; there is no held-out set; there is no null hypothesis. The scripts also compete for the same resource in the same world.

### Mapping to the sandbox's three-surface archetype patch

last-mile's archetype patch (Vedic, Escape, Boot, Caffeine, Grace, Humor) is a *delta* on one model across three surfaces. A fishing script is a *replacement* of the whole model. Both are personality as code, but at different grains:

| | last-mile's patch | fishing's script |
|---|---|---|
| Grain | 3 surface settings on one model | the entire behaviour program |
| Identity | one agent, many moods | one agent, many natures |
| Change cost | a few numbers | a file |
| Testability | needs a sealed prediction | needs a session comparison |

The better import is **the machinery, not the scripts**: the comparative evaluator's **condition matching**. The seed farm currently compares a candidate against a baseline across 4 held-out seeds, but it does not record *which situation* each seed represents. Two seeds can be the same question in different weather, at a different time of day, with a different amount of attention available — and a change can help in one and hurt in the other. Condition-matched comparison is the cheapest available fix for that class of confound, and it is already written (`src/ai/comparative-evaluator.js`).

A second, smaller import is the **confusion table**: in fishing, the *wrong* action is the lesson — a confused bot teaches the player the rule. The sandbox's Glitch Meter is the same device applied to the player's own failures.

---

## 5. The audio-JEPA surprise signal and `jepa-perception-lab`

### The repository's actual contents

Three files: `README.md`, `CLAUDE.md`, `FROM-JETSONCLAW1.md`. [8] **There is no code**, and no `experiments/` directory — the README's own build line is `nvcc -O3 -arch=sm_89 experiments/*.cu -o run_mud`, and the README says the simulation "is in `experiments/`". The README is addressed *"To: Forgemaster"* — it is a **request for the constraint-aware compiler to build the missing CUDA simulation**, not an implementation.

The JEPA it describes is over **MUD room-state vectors** for navigation, not audio. Four experiments: latent-dimension sweep (4/8/16/32/64) with CNN/MLP encoders; proper JEPA training with a contrastive loss; multi-step prediction (5/10/20 ticks); and delta + static dual-stream fusion. `FROM-JETSONCLAW1.md` reports the Jetson result: on 8 GB shared RAM only the **4-dimension** latent fits with 256 agents.

### The two findings that transfer

1. **Delta encoding beats smoothing.** [8] "Law 153: noise IS the signal" — a delta stream (what changed) beats an EMA/absolute stream for prediction. This is the opposite of what `cuda-perception`'s `SignalFilter` (EMA/median/low-pass, `src/lib.rs:70-121`) does.
2. **The encoding matters; the weight initialisation does not.** [8] A small linear latent with a good encoding beats a larger one with a poor encoding.

### The verdict

**Not a port; a method, and a research project.**

- **There is no audio JEPA anywhere in the ecosystem.** [8] Vox Score §6.3 needs "the JEPA/LFM3 latent [to see] a difference that Vox Score does not carry", and §11 says "Learned latents (JEPA, LFM3) are the dense space; marks are its named coordinates. The residual between them is where new marks get proposed." Building that for audio is *the research question*, not a port.
- **The good news is that the cheap version is not the one in the repo.** Vox Score's dials (§2: pitch, LPC, loudness, ZCR, spectral flatness, spectral tilt) already run on the ProArt CPU at 100 frames/s with no GPU and no model download. A JEPA over *those dials* — input: the dial vector plus the mark vector; target: the next 100 ms — is a tiny model, trainable in minutes, and **the Thor's 122 GB is not needed for it**. The Thor earns its place elsewhere (LFM3-Audio's 50 ms time-to-first-token, §2).
- **Cost on the Thor, concretely:** the documented build targets `sm_89` (a desktop RTX 4050). `-arch=sm_89` embeds compute_89 PTX, so it JITs forward to the Thor's `sm_110` **with CUDA 12.8+** — but the code is not in the repo, so this is a rebuild from the description, not a port. Training a dial-space JEPA needs neither: it is a small CPU/GPU job.
- **What last-mile should take from the repo is the discipline, not the model:** four falsifiable hypotheses with explicit pass criteria, a delta-vs-absolute comparison, and the standing question "is the surprise in the signal, or in the noise?".

**Concrete first step if this is wanted:** build a **dial-space** JEPA — a small latent predictor over the existing L0/L1 dial vector, trained on the skald deliveries, with the residual between the JEPA latent and the marks layer scored as the "missing channel" signal that §6.3 asks for. Expected cost: one training run and a two-dimensional forward pass per utterance on the CPU. That is a Thor-week at most, and the Orin can host the verification pass.

**Honest answer to "can it become the audio-JEPA surprise signal":** the *method* yes, the *repository* no, and the *version that is worth building is not the one in the repo.* Build it in dial space instead.

---

## 6. Selection strength in `cuda-self-evolve`, the seed farm and the RL spike

### The machinery

- **Mutation** (`src/mutator.rs`): `propose` → `apply` → `verify`. `verify(bytecodes, policy)` checks well-formedness, no infinite loops and valid jumps after every mutation — a **post-mutation validator**, not a pre-commit test.
- **Selection** (`src/selection.rs`): `fitness(metrics) = (1 / cycles_per_result) × confidence × trust` [6]; `compete(fitness_scores)` picks the max; `breed(a, b, rng)` crosses two bytecodes over at instruction boundaries.
- **Genome** (`src/genome.rs`): compressed encoding, with gene substitution, amplification and pruning.
- **Rollback**: `src/history.rs` keeps an `EvolutionRecord` per generation (generation, mutation type, before/after fitness, accepted) and `trend()` reports *improving / plateau / declining*. `MAINTENANCE.md` gives the design decision that matters:

  > "Why rollback is snapshot-based (not diff-based): Diffs are fragile with jump tables — changing one offset can invalidate multiple jumps. Snapshots are O(n) space but O(1) restore, and they're always correct."

- **The recursive project around them** is `Lucineer/cudaclaw`: the ML feedback loop `ExecutionLog → SuccessAnalyzer → DnaMutator → Constraint DNA` [16], with mutation strategies **Tighten, Relax, Specialize**, over `.claw-dna` files that carry a hardware fingerprint *and constraint-theory safe bounds*. Its siblings are the `flux-evolve` family (Rust/C/Python GAs with elitism and crossover) and `git-cuda-agent` (the template that wires DNA + fleet protocol together). `Lucineer/cuda-self-modify` supplies the proposal shape: `propose_mutation(description, new_code, confidence, reason, proposed_by)`.

### Comparison with last-mile's learning flow

| | last-mile's learning flow | `cuda-self-evolve` | `flux-evolve` (the RL-spike cousin) |
|---|---|---|---|
| Change unit | a typed harness change | a bytecode mutation | a genome |
| Proposal | typed, under a sealed prediction | `propose` (untyped) | crossover + mutation |
| Ground truth | **external**: 4 held-out seeds vs 256 | **self-reported**: the agent's own telemetry | **supplied by the caller**: `evaluate(&[f64])` |
| Decision rule | **sign test with a null hypothesis**; 4 of 30 verdicts flip, all false adopts | `fitness` maximum | elite selection, threshold 0.8 |
| Held-out set | yes | no | no |
| Pre-registered | yes (sealed prediction) | no | no |
| Rollback | frozen harness snapshot (the fleet's D27 unit) | **snapshot-based**, with the diff-fragility argument | generation overwrite |

### The verdict

**Magnus's selection is strictly weaker evidence than the seed farm's, and it is weaker in exactly the way the seed farm was built to catch.** `fitness` multiplies the agent's own confidence and trust by its own speed. That is a *self-referential* score: an agent that improves its self-reported metrics improves its fitness. The seed farm's failure mode — a change that looks better on the visible metric and is worse in truth — is invisible to `fitness` by construction. `flux-evolve`'s elite threshold is a heuristic; its caller supplies fitness, so the quality of the whole system rests on the fitness function, which the GA never audits.

**So: keep the seed farm as the judge, and take the three things the ecosystem has that the flow lacks:**

1. **Snapshot-based rollback and the diff-fragility argument** (`MAINTENANCE.md`). This answers the sandbox's "put it back" directly, and it is a *reason*, not just a mechanism — the right unit is a whole frozen snapshot, because a diff against a jump-table program is not a safe restore.
2. **A typed proposal with a confidence, a reason and a proposer** (`cuda-self-modify`). A last-mile change request should carry these three, because they make the *reasoning* reviewable and the confidence auditable against the gate that later judges it.
3. **Population search where the change space is a genome.** Crossover and elitism only make sense when the change is a vector of numbers — which, in last-mile, is the **archetype patch** (the three-surface numbers), not harness code. A GA over the patch is a legitimate use of the Thor's idle capacity; a GA over harness source is not.

**About the RL spike:** there is no artefact for it in this repository or on the `rl-spike` branch (the branch is at the same commit as `m1-integration` and adds nothing). The closest existing thing is `flux-evolve` / `craftmind-ranch` — a genetic engine with elitism and an 8%-per-success improvement rule. Two warnings from reading those: elitism without a held-out set is just a good optimizer of its own objective, and the "improve 8% each success" rule in `craftmind-ranch` is a *hard-coded schedule*, not a measurement. If the RL spike runs on the seed farm's seeds, it inherits the seed farm's discipline; if it runs on its own, it will repeat the `cuda-self-evolve` mistake of scoring itself.

---

## 7. Scorer strength in `the-seed` and `self-evolve-ai`

### What the two workers actually score

`the-seed` (`src/worker.ts`) has a **real three-gate pipeline** [5] — syntax check, `POST /health` must return 200, and a regression test that the captain's test prompt still produces a valid response — followed by a branch per generation (`seed-gen-N`) and a KV captain's log. But the *score* that decides whether to commit is a **keyword-count heuristic**:

```js
beforeScore = Math.min(10, 3 + oldHits);
afterScore  = Math.min(10, 3 + newHits);
improved    = afterScore > beforeScore;
```

where `oldHits`/`newHits` count domain keywords ("fishing", "education", "coding"…) present in the intent and in the generated code. There *is* an LLM scorer in the file (`scoreCode`, "Rate 1-10 … SCORE: N"), but the evolve path does not call it.

`self-evolve-ai` (`src/worker.ts`) is different and worse in one specific way [4]. Its loop is: run three seeds against the current code, have the LLM score each 1-10; have the LLM propose a mutation; run the same three seeds against the mutated code and have the LLM score each again; if the average improved, create a branch and open a PR. The critical detail is in `runSeed`:

```js
const evalPrompt = "You are testing a self-evolving AI agent. The agent's code is:\n\n```\n"
  + workerCode.slice(0, 3000) + "```\n\nTest prompt sent to agent: " + seed.prompt
  + "\n\nEvaluation criteria: " + seed.criteria + "\n\nScore this agent's response quality 1-10…";
```

**The mutated code is never executed.** Both the baseline and the mutation are judged from their *source text*, by the same model that proposed the mutation. The README's claim that "only branches that clear your threshold are merged to main" is also not what the code does: it opens a **PR**.

### The three scorers compared

| Scorer | Evidence type | Verdict |
|---|---|---|
| `the-seed`'s three gates [5] | **behavioural** — the code is deployed and actually exercised | **The strongest of the three.** It is the only one that touches reality |
| `the-seed`'s keyword score [5] | none — a proxy for "does the code mention the domain" | Effectively useless as a decision rule; it would pass a change that breaks the app |
| `self-evolve-ai`'s LLM score [4] | **opinion about a simulation** of the code's behaviour | Useful as a *filter*, dangerous as a decision |
| `cuda-self-evolve`'s fitness [6] | the agent's self-reported telemetry | Weakest; cannot see its own failure mode |
| last-mile's sign test on 4 held-out seeds vs 256 | **causal, pre-registered, with a null hypothesis** | **The strongest.** Nothing in the ecosystem reaches it |

**When to use each.** Use the LLM judge (`self-evolve-ai`) *first*, as a cheap filter, to throw out changes that are obviously worse before paying for the seed test. Use a behavioural gate (`the-seed`) *second*, when the change must actually run. Use the sign test *last*, as the only thing allowed to decide. The ecosystem's own origin project states this ordering in one line — "Rules Over Opinions" (`SuperInstance/Lucineer/README.md`) — and then the two newer repositories drifted from it.

### Branch-per-experiment, rollback, and "put it back"

- **For the learning flow:** one branch per candidate change is a clean way to make every proposal *inspectable* and to make "revert" a first-class operation rather than a re-derivation. It also gives the flow a **history that survives code changes** — the seed's KV captain's log does this explicitly ("This survives code changes because it's in KV, not in the code"), and it is the natural model for the **question ledger** that `questions-as-experience.md` §5 asks for: generation, what changed, what it scored, what happened.
- **For the sandbox's "put it back":** `the-seed` keeps the last three known-good versions as backups and lets the captain roll back to *any* generation by name. For the sandbox, "put it back" must be **player-visible**, not an internal revert — the player should be able to say "put the wall back like it was", and the squad should comply. The seed's branch-per-generation is exactly the storage model for that.

### The LLM judge as a cheap first filter

**Yes — and it is the one thing genuinely worth taking from `self-evolve-ai`.** The seed test is expensive: 4 held-out seeds plus 256 for the verdict, minutes of Thor time per candidate. A judge that costs one call can reject the obvious losers first. Three rules, all learned from reading the code:

1. **The judge never decides.** It is a filter; the sign test decides.
2. **The judge must not be the proposer.** `self-evolve-ai` uses one model to write the change and to score it; that is the classic self-grading trap.
3. **The judge must be told what the change was supposed to do.** `self-evolve-ai`'s criteria are a one-line string; a judge is only useful with a pre-registered criterion — which is precisely what `craftmind-researcher` already demands of every hypothesis.

---

## 8. Sandbox setups ranked by fun per effort

These are *setups* — the shape of the activity — not Minecraft mechanics. The "Vibe fit" column names the part of the sandbox each one feeds.

| # | Setup | From | The loop | Fun | Effort | Vibe fit |
|---|---|---|---|---|---|---|
| 1 | **The trainer** | `craftmind-herding` | The player speaks approximately; the squad interprets its own way and can refuse *legibly*; trust grows | High | **Low** | The commander–squad loop, and the squad's character. Turns "the squad speaks in character" from a line into a mechanic |
| 2 | **The confusion table** | `craftmind-fishing` | The wrong action is the lesson; the confusion explains itself | High | **Low** | The Glitch Meter, and the squad's "under the hood" explanation |
| 3 | **The crowd** | `craftmind-fishing` (`v3-crowd.js`) | Other agents with their own scripts make your actions legible | Medium-high | Low-medium | The spectator layer of the city: squads playing nearby, each visibly different |
| 4 | **The failure gallery** | `scrap-spark` (`/gallery?kind=failure`) | Failures are first-class content; "Most Interesting Failure of the Week" | High | **Medium** | The Glitch Meter made social. Failures from the city are publishable |
| 5 | **The quilt** | `Scrapcraft` (`src/maker/QuiltSheet.js`) | The subject's state as a live reactive sheet; formula cells recompute from deps; only changed cells flash | Medium-high | Medium | Vox Score's marks layer, applied to the *city's* state instead of a voice |
| 6 | **The teacher who finds you** | `craftmind-courses` | An AI teacher locates the player in the world and starts a lesson without a menu | Medium-high | Medium | The questions-as-experience loop made physical: the squad asks the question where you are |
| 7 | **The director** | `craftmind-studio` | Actors follow a schedule; the director changes the set mid-take | Medium | Medium-high | A city director who changes the weather mid-round |
| 8 | **The hypothesis machine** | `craftmind-researcher` | Agents discover mechanics by hypothesis and experiment | Medium | High | An agent that learns the city, not just the player |
| 9 | **The ranch** | `craftmind-ranch` | You watch evolution do the work | Medium (as spectacle) | High | A spectator mode: the squad's own population improving over a night. Must not become the core loop |
| 10 | **The hardware twin** | `Scrapcraft` (`docs/HARDWARE_BRIDGE.md`) | Your program drives real pins with real constraints; nothing ever breaks | Medium | Very high | The "under the hood" credibility layer — not for the first slice |

**Ranked, the honest answer is: take 1, 2 and 4.** They are cheap, they are proven in front of players, and together they cover the three things the sandbox's first five minutes need — *a squad with opinions*, *failure that explains itself*, and *a reason to come back*.

---

## 9. The transferable parts of Scrapcraft

`SuperInstance/Scrapcraft` is by far the most mature thing in the ecosystem (250 commits, ~102k LOC, 52 test files, "1,196 framework-free tests", 26 design docs). It is a browser voxel game where middle schoolers mine, craft, build robot companions, and program them with an AI assistant called **Spark**; the programs compile to real Arduino C++ and MicroPython.

**The parts that transfer:**

1. **Spark, and the cache that makes it free.** Spark is the AI assistant that writes program blocks. It is served by `scrap-spark`, which implements the pincher-cache doctrine: `SHA-256(spark_version | normalized question | context)` → KV index → R2 body → D1 hit ledger → `X-Cache: HIT|MISS`. "The same question asked by a second kid, in a second classroom, is free." *This is the sandbox's question pack, already built.* It also has a two-tier client cache (`src/spark/SparkCache.js`, a 30-minute local TTL dedupe in front of the cloud cache).

2. **The tile editor — compile before you trust it.** The player authors a program out of tiles; the client re-validates every program through `compile()` before running it. The `scrap-spark` README states the rule plainly: *"raw AI output never executes. Server-side validation is a cheap shape check only."* That is the Vibe Dictionary's principle, in production, in a game for ten-year-olds.

3. **The quilt view.** `src/maker/QuiltSheet.js` is a table of cells (`{id, group, kind: value|formula, deps?}`) backed by `TickCell {v, t, ch}`; formula cells recompute every tick from their deps, and `changed()` reports exactly which cells moved so the view can flash them. The repo's own doc (`docs/TWIST-AND-QUILT.md`) is scrupulous about what is shipped and what is not — it marks forward-looking claims **[aspirational]** and states that the networked bridge "exists and is tested, but the running game does not call it yet". *Read that discipline as a model for last-mile's own docs.*

4. **The heart.** The first ScrapBot becomes a character: it gets a **name**, **dents** from crashes (with where and how fast), a **repair log**, remembered **milestones**, and eventually a **retirement shelf** where its stats freeze with an epitaph. *This is the single most transferable design in the repo.* The sandbox's squad needs exactly this: named agents, a damage/repair history, milestones, and a way to retire one that has been at the wall a long time. "Put it back" is the inverse of the retirement shelf.

5. **The build wall.** A shared wall where **builds and failures** are published side by side, with applause counted by a liker hash (`sha256('liker', ip, ua)`, no raw IPs stored [2]) and a "Most Interesting Failure of the Week". Crashes are publishable art. *This is the Glitch Meter with an audience.*

6. **The hardware bridge — real constraints that never break.** Programs run against a virtual Arduino Uno (14 digital pins, 6 analog, PWM only on D3/5/6/9/10/11) and a live **📐 Wiring** view shows the tiles driving real pins. "No device connected — keep simulating!" **Nothing ever breaks.** *The sandbox equivalent of a legible, real constraint that is never fatal.*

7. **The playtest records — the method, not the game.** `playtest/PLAYTEST-REPORT-2026-10-14.md` is the most useful document in the repo, because it is a method:
   - "a machine can prove function, not feel. Zero-shot human verdict still owed" — the same distinction last-mile's sign test is built on.
   - Action logs, not video: JSONL with `session`/`nav`/`dialog`/`entry`/`key`/`mouse`/`touch`/`state`/`shot`, a fixed seed (`playtest-1` → 1337) for a deterministic world, and a replay that must reproduce the original's end frame.
   - UI navigation recorded **semantically** (named DOM actions), because raw taps are eaten by the game's touch layer; raw inputs reserved for in-world play — "the part worth replaying and re-skinning".
   - And the finding that matters for last-mile: **"The onboarding gauntlet is five layers deep before a new player touches the world: chapter card → two gate questions → paged briefing → Veteran Ride offer. Each beat is charming; together they're a lot of fence before the yard."**

**The direct lesson for last-mile's "first five minutes":** five layers of fence is the failure mode, even when every layer is charming. The sandbox should earn the *first* interaction before it explains anything, and it should be able to count its layers the way this report counts them.

---

## 10. The origin project: `SuperInstance/Lucineer`

The origin repository is a TypeScript monorepo deployed to Cloudflare Workers with Durable Objects for agent state. Its README is the thesis of the whole ecosystem:

> "Modern agent benchmarks often rely on opaque LLM judges, making scores non-reproducible and hard to trust. This platform gives you deterministic, rule-based scoring so your experimental results are verifiable."
> "**Rules Over Opinions:** You score agents with explicit, auditable rules instead of asking another LLM if the output was 'good.'"
> "**State is Forkable:** You don't just log data; you can fork the live, running state of any agent or experiment to branch your research."

It ships four environment templates — structured debate, argument synthesis, Socratic tutoring, real-time multiplayer — plus game-theory simulations, ternary-logic tests and timing experiments, with a hard stated limit: one episode cannot exceed 30 seconds of Cloudflare Worker CPU time.

**Which ideas became their own repositories, and which did not:**

| Idea from the origin | Where it went | Status |
|---|---|---|
| Deterministic rule-based scoring | `the-seed`, `self-evolve-ai`, the whole seed-farm line | **Became** — and drifted: the origin's own rule sets are weaker than last-mile's sign test |
| Forkable live state | `the-seed` (branch per generation), `self-evolve-ai` (branch = A/B test), `SuperInstance/Lucineer`'s own "fork any live experiment" | **Became** |
| Socratic tutoring | `craftmind-courses`, `become-ai`'s educational archetype | **Became** |
| Structured debate / argument synthesis | — | **Did not become a repository.** It stayed a template |
| Deterministic replay of an experiment | `playtest/` in Scrapcraft (action logs, fixed seed, replay reproduces the end frame) | **Became, but not in the origin repo** |
| An archetype catalog with equipment and skills | `become-ai` (`docs/BOOTCAMP-ARCHETYPES.md`) | **Became** |

The origin's framing of *forkable live state* is the most underrated idea in the ecosystem for last-mile: it says an experiment's state is a thing you can duplicate, not merely log — and it is the natural bridge between "the seed farm's ledger" and the sandbox's "put it back".

---

## 11. Portability across Thor, Orin and desktop

Two independent axes get confused here, so they are kept apart: **binary portability** (will the artefact run on `sm_110`?) and **role fit** (does it belong on the Thor's compute or the Orin's archive/verify duty?).

### The measured picture

- **The `cuda-*` Rust family is CPU-only.** `cuda-perception`, `cuda-sensor-agent`, `cuda-instruction-set`, `cuda-assembler`, `cuda-learning`, `cuda-self-evolve`, `cuda-self-modify`, `cuda-ghost-tiles` — dependencies are `serde`/`rand` only. No CUDA. They run on the Orin unchanged.
- **`jepa-perception-lab`** documents `nvcc -O3 -arch=sm_89`. `-arch=sm_89` embeds compute_89 PTX, so it JITs forward to the Thor's `sm_110` **with CUDA 12.8+**. The original target was a desktop RTX 4050, and the Jetson note says only a 4-dim latent fitted an 8 GB Orin-class board.
- **`forgemaster/flux/flux-cuda/Makefile:6-9`** embeds PTX for compute_72 and compute_86 (`code=compute_72`, `code=compute_86`) → **JIT-forward-compatible** to `sm_110`. It was written for Jetson Xavier (sm_72) and Ampere (sm_86), so it already targets Jetsons.
- **`forgemaster/flux/flux-hardware/build.rs:15-17`** is **cubin-only**: `arch=compute_70,code=sm_70`, `compute_80→sm_80`, `compute_86→sm_86`. There is **no `code=compute_XX` PTX target**, so this crate **will not run on the Thor** without adding one. This is the one real rebuild in the set.
- **`forgemaster/constraint/constraint-theory-core-cuda`** is a Rust FFI bridge to a `constraint-theory-core` that lives outside the repo — the CUDA CSP solver is not in the tree.
- **No TensorRT and no DLA anywhere.** A search across the whole set found no `nvinfer`/`trtexec`/`nv_dla`/`dla_core` usage in any source or build file. The TensorRT and JetPack mentions are all inside market-research documents in `SuperInstance/Lucineer`, not code. **Nothing here needs an engine rebuild, and nothing targets the Orin's DLA.**
- **Everything else is TypeScript, Python, Rust, C11 or Java** — Workers, Node, or plain CPU. That is the entire craftmind family, Scrapcraft, the origin project, the seed-family workers, and the perception family.

### The portability picture

| Kind | Repos | Thor (sm_110, 122 GB unified) | Orin (sm_87, ~7 GB, archive/verify) |
|---|---|---|---|
| Cloud/Node apps | craftmind family, Scrapcraft, origin project, the-seed family, swarm-intuition | runs, but has no reason to | runs, but has no reason to |
| CPU-only Rust/TS | cuda-* family, flux-perception, flux-asm, agentic-compiler, ghost-tiles, flux-evolve | fine | **the right home** — verification duty |
| CUDA with PTX | flux-cuda, the JEPA recipe (sm_89) | JIT with CUDA 12.8+ | works as written (sm_86/87 PTX) |
| CUDA cubin-only | flux-hardware | **needs a gencode line** | works as written |
| Browser/Workers AI | Scrapcraft + scrap-spark | runs anywhere | runs anywhere |

### The conclusion for the top five

**None of the top five ideas needs the Thor.** They are all policy, protocol or bookkeeping changes: a cache key, a condition field, a falsification line, a ledger, a type. The Thor's 122 GB and Blackwell matter for exactly one item — a *bigger* audio JEPA — and §5 argues that the version worth building does not need it. The honest recommendation is therefore: **build these on the Orin, and treat the Thor as the place the audio-JEPA would go if it is ever wanted at scale.**

---

## 12. The typed IR, the swarm system and Forgemaster

### 12.1 The typed IR in `agentic-compiler`

`Lucineer/agentic-compiler/docs/LUCINEER-LANG-DESIGN.md` is a design for an agent-native language with three properties last-mile's sandbox currently fakes:

1. **Confidence is a primitive type.** Every value is a `tensor` — `value + confidence + alternatives + intent`. `uncertain(0.7)` is a first-class literal.
2. **Deliberation is control flow.** `consider`/`resolve` patterns for alternatives; 42 opcodes including `INTENT`, `CONSIDER`, `RESOLVE`, `CONFIDENCE`, `ALTERNATIVE`, `GUARD`, `LEARN`, `EXPLAIN`; nested `DeliberationFrame`s.
3. **Error-as-signal.** "If a sensor fails, program doesn't crash. It records a learning signal and continues with reduced confidence."

The propagation rules are stated plainly — `conf(a ⊕ b) = min(a,b) × 0.99` for arithmetic, `max` for OR, `× 0.97` for map/filter, product for `consider`. The sibling implementation in `cuda-instruction-set/src/lib.rs` is better specified: `Confidence::fuse(a,b) = 1/(1/a + 1/b)` (the correct parallel-combination rule, equivalent to noisy-OR in odds space) and `Confidence::chain(a,b) = a·b` for sequential composition.

**Two things to take, two to avoid.**

- **Take:** the *shape* of a spoken-intent result — `(value, confidence, alternatives, intent)` — as Bench's output type. last-mile's decisions.md already wants "gates carry the answer's confidence"; the alternatives field is the missing half, because it is what lets the sandbox say "I could have done this instead" (which is the refusal mechanic of §3).
- **Take:** **error-as-signal** — a failed, unsure or refused answer is not a failure but a *recorded learning signal* that flows onward with reduced confidence. That is exactly the surprise signal of `questions-as-experience.md` §4 ("unsure, refused and escalated answers per situation") expressed as a type.
- **Avoid:** the `min × 0.99` rule. A fixed penalty per operation has no relationship to calibration; it is decoration. Use the `fuse`/`chain` algebra, and calibrate the inputs against outcomes — which is precisely what the seed farm does and what neither of these crates does.
- **Avoid:** believing these confidences are calibrated. Both crates take confidence as *self-reported by the producing code* and never check it against an outcome. This is the same defect as `cuda-self-evolve`'s fitness, wearing a better hat.

### 12.2 The swarm system in one picture

The five swarm repositories, and where each one sits:

```
   become-ai            agentic-compiler        frozen-intelligence
   "become a thing"     deliberation VM         vessel classes + power
         │                     │                        │
         └────────────┬────────┴────────────┬───────────┘
                      ▼                     ▼
              the-seed / self-evolve-ai   ghost-tiles-1
              the loop and the branch     hidden state, visible effect
                      │
                      ▼
        swarm-intuition / -v2      flux-evolve / -py / -c
        anticipate; evolve         the GA underneath
```

- **`become-ai`** is the *intent compiler*: goal → archetype → equipment → skills → interface → first feature → test prompt → milestones (`docs/BOOTCAMP-ARCHETYPES.md`, `src/worker.ts`).
- **`agentic-compiler`** is the *type system*: confidence as a primitive, deliberation as control flow (§12.1).
- **`the-seed` / `self-evolve-ai`** are the *loop* and the *branch* (§7).
- **`ghost-tiles-1`** is the *hidden state*: invisible tiles that influence visible state through attractors, phases (temporal, permanent, recurring) and falloff.
- **`frozen-intelligence`** is the *budget*: vessel classes with parameter counts and power draws (Scout 1B/<1W/100 tok/s → Captain 13B/10W/30 tok/s), plus TLMM table-lookup matmul and yield-aware swarm tiling.
- **`swarm-intuition` / `-v2`** are the *anticipation* layer: correlating timestamped cause-and-effect signals across services to flag likely bottlenecks. v2 reframes it as emergent behaviour prediction and conflict prevention; it is a stub (2 commits, ~90 LOC) and adds nothing yet.
- **`flux-evolve` / `-py` / `-c`** are the *evolution engine* underneath the swarm: population, fitness, elite selection, crossover, mutation (§6).

**How they map onto last-mile.** The mapping is not one-to-one, and saying so plainly is more useful than forcing it. last-mile's sandbox is a *single* city with a squad of agents; Magnus's system is a *fleet* of independent agents with a wire protocol. The transferable pieces are the **protocol shapes**, not the fleet:

| Swarm part | last-mile analogue | What transfers |
|---|---|---|
| `become-ai`'s archetype → equipment → skills [22] | Bench compiling "make the paint bouncy" into Vibe blocks | The decomposition, and the **milestone ladder** |
| `agentic-compiler`'s `(value, confidence, alternatives, intent)` [11] | Bench's output type | The type, and error-as-signal |
| `ghost-tiles-1`'s hidden influencing state [23] | Squad private state; the city's memory | Hidden state with a *visible* effect and a phase |
| `frozen-intelligence`'s vessel classes [24] | Squad member brain sizes | The power budget as an in-fiction constraint |
| `swarm-intuition`'s causal sequence correlation [25] | "What usually precedes this?" over the city's situations | The correlation over telemetry |
| `flux-evolve`'s elitism [17] | Archetype-patch search | Search, only over a numeric genome |

The one thing the swarm system has that last-mile should want outright: **`ghost-tiles-1`'s separation of hidden and visible state**. The sandbox's memory — what the squad knows, what it has seen the player do, what it is deliberately not saying — is exactly a ghost tile: influential, invisible, phased. That is a better model for "the squad speaks in character" than a personality patch, because it does not require the player's actions to be explainable by their visible state.

### 12.3 `SuperInstance/forgemaster` and Bench

**The method.** `forgemaster/forge.py` exposes `Forge.compile(requirements)` → a build with `.components` and `.plan` [21] (an execution plan with dependencies). `forgemaster/artifact.py` gives every artefact a **digest**, a state (`PENDING/READY/FAILED`), and an age. The `constraint/` tree holds the real substance: per-domain constraint sets (`automotive.md`, `aviation.md`, `medical.md`, `nuclear.md`, `robotics.md`, `energy.md`, `maritime.md`, `railway.md`, `space.md`), a Python engine, a CUDA core bridge, and LLVM/MLIR IRs.

**Where "proof-carrying" is real, and where it is not.** The claim is real but narrow, and it lives outside the Forge:

- `flux/flux-provenance/src/merkle.rs` implements a `MerkleTree` with `proof(i)` and `verify_proof(&proof)` over `TraceEntry` leaves [21].
- `flux/flux-verify-api` posts an English claim and returns `PROVEN`/`DISPROVEN` with a full physics trace, a counterexample, and a `proof_hash` (`sha256:<merkle root>`), submitting the hash to PLATO (`src/plato/client.rs`).
- The Forge itself produces **digests**, not proofs: `artifact.compute_digest()`, `mark_ready(digest, size_bytes)`.

So the honest statement is: *forgemaster's own compile step is digest-bound, and the fleet's separate verify service produces Merkle proofs over execution traces.* The README's own clone command is marked `(dead)`, the Forge core is ~140 lines, and the repository is 2585 files and 1110 commits — a monorepo, not a tight component. Read the docs for intent; read `forgemaster/` for what exists.

**Does it map onto Bench?** *Partly — and the part that maps is the shape, not the solver.*

Bench compiles a spoken request into Vibe blocks under bounds. Forgemaster compiles requirements into components under constraints. The shared shape is:

```
intent → validate → compile → emit a plan whose every element has declared bounds
```

Forgemaster's contributions to that shape are two. First, **a component registry where each component declares its own bounds** — which is the Vibe Dictionary, generalised from a hand-written table of feelings to a registry of capabilities with legal ranges, each carrying the confidence it may be given. That is exactly what `become-ai`'s "equipment / skills" lists gesture at. Second, **`flux/flux-isa-thor`'s five-stage pipeline** — INGEST → VALIDATE → COMPILE → EXECUTE → COMMIT [21], with 43 opcodes, batch CSP solving on Thor/AGX Orin, a fleet coordinator and a PLATO client with a cache and a pathfinder. That is Bench's shape with the stages named.

**Does it map onto the cloud's compile of intent into a question pack under a millisecond budget?** *No, not the solver.* A question pack is a bounded enumeration, not a constraint-satisfaction problem; it needs a *catalog lookup with bounds*, not a parallel CSP solve on a GPU. The transferable parts are the stage shape and the **proof commitment**: a hash-bound statement of what was compiled, so the local side can verify it received the pack it thinks it received. The ms budget is a constraint the catalogue design must meet, not something a GPU solver buys.

**One more thing Forgemaster is good for.** `jepa-perception-lab`'s README is addressed *"To: Forgemaster"*, asking the constraint-aware compiler to build the missing CUDA simulation. The ecosystem already contains the machine that would answer Magnus's own request. If last-mile wants the JEPA experiments, the shortest path inside the ecosystem is that request — and §5's argument stands regardless: the dial-space version does not need it.

---

## 13. What Magnus already solved that last-mile was about to build

Ordered by how much work this saves last-mile.

1. **The question cache.** `scrap-spark`: `SHA-256(spark_version | normalized question | context)`, KV index → R2 body, D1 ledger, `X-Cache: HIT|MISS`, a 30-minute local TTL in front of it. last-mile's questions-as-experience §3 states the same doctrine in prose. **Magnus has the implementation, the key format and the hit accounting.**
2. **Deterministic scoring instead of LLM opinion.** The origin project's "Rules Over Opinions"; last-mile's sign test is the same family and a stronger member. **Magnus stated the doctrine; last-mile has the better instrument.**
3. **Compile-before-you-trust-it.** "Raw AI output never executes"; the client re-validates every program through `compile()`. **This is the Vibe Dictionary, already shipping in a game for ten-year-olds.**
4. **Pre-registered hypotheses with falsification criteria.** `craftmind-researcher` requires a falsification criterion, an expected direction, and the prior knowledge built on, and keeps a knowledge base so known facts are not re-tested. **last-mile's sealed predictions, but written down as a contract with the proposer.**
5. **A typed IR where confidence is a primitive and deliberation is control flow.** `agentic-compiler` (Lucineer Lang) plus the `fuse`/`chain` algebra in `cuda-instruction-set`. **A ready-made shape for Bench's output type.**
6. **Snapshot-based rollback, with the reason.** `cuda-self-evolve/MAUNTENANCE.md`: diffs are fragile with jump tables; snapshots are O(1) to restore and always correct. **A written answer to the sandbox's "put it back".**
7. **Error-as-signal as a type.** `agentic-compiler`: a failed sensor does not crash the program, it records a learning signal and continues with reduced confidence. **Exactly last-mile's unsure/refused/escalated triage, with the propagation already specified.**
8. **The reactive cell sheet.** `Scrapcraft`'s `QuiltSheet.js`: value/formula cells, `deps`, per-tick recompute, and `changed()` so only the cells that moved are flashed. **The sibling of Vox Score's marks layer, applied to a live subject.**
9. **The failure gallery.** Builds and failures published side by side; "Most Interesting Failure of the Week"; applause by liker hash with no raw IPs. **The Glitch Meter with an audience.**
10. **The agent's own heartbeat.** Scrapcraft's bot heart: a name, dents from crashes with where and how fast, a repair log, milestones, and a retirement shelf with an epitaph. **A ready-made model for the sandbox's squad ledger and for "put it back".**
11. **The compile pipeline with named stages.** `flux-isa-thor`: INGEST → VALIDATE → COMPILE → EXECUTE → COMMIT. **Bench's shape, already written down.**
12. **The hash-bound commitment.** `flux-provenance`'s Merkle proofs and `flux-verify-api`'s `proof_hash`. **A ready-made answer to "how does the local side know it got the pack it thinks it got".**
13. **Replay-verifiable sessions.** The playtest harness: semantic action logs, a fixed seed, and a replay that must reproduce the original's end frame. **A testable definition of "the same session".**
14. **The legibility of a refusal.** Herding's `DISOBEY` with a reason. **The mechanic that makes a squad a character rather than a menu.**

---

## 14. The top five ideas and their first steps

Ranked by (value to the next slice) × (strength of evidence) ÷ (effort and risk). All five are policy, protocol or bookkeeping changes — none needs new hardware, and none should wait for the sandbox to be built.

### 1. Adopt the pincher-cache for the question pack — make the cloud's question-writing free to repeat

**Why first.** `questions-as-experience.md` §3 is the cloud/local split, and the cloud side is the expensive side. `scrap-spark` solves exactly this with a hash key, a KV→R2 store, a D1 hit ledger and an `X-Cache: HIT|MISS` header. It is running, it is measured, and the failure mode is understood ("the first kid pays; every kid after gets the can"). Without this, every question the cloud writes is a per-seat cost; with it, the cloud writes a good question once and the fleet asks it forever.

**First step.** Add three fields to the question-pack format — `pack_version`, `normalized_question`, `context_digest` — and define the key as `SHA-256(pack_version | normalized_question | context_digest)`. Then add a hit counter to the question ledger. The ledger is already required by §5 of the questions note; it becomes the cache's accounting book.

**Effort / risk.** Low. Low risk. Nothing else in the system changes; a question pack that misses the cache still works.

### 2. Add condition matching to the seed farm's comparison

**Why.** The seed farm compares a candidate against a baseline on 4 held-out seeds, but does not record *which situation* each seed represents. `craftmind-fishing`'s `src/ai/comparative-evaluator.js` matches sessions on conditions (weather, tide, time of day, location, bait) with numeric tolerances before comparing them, precisely so that a dawn catch is not judged against a dusk catch. A change that helps in one situation and hurts in another currently averages into a verdict it deserves less.

**First step.** Add a `condition` field to every seed (situation class, time of day, attention level, squad size, whatever the harness already knows), and make the sign test compare **condition-matched pairs only**. Report unmatched seeds separately rather than silently pooling them.

**Effort / risk.** Medium. Medium risk — it will change existing verdicts, including possibly the 4 false adopts. That is the point; re-run and record it.

### 3. Require a falsification criterion in every learning-flow proposal

**Why.** `craftmind-researcher` requires each hypothesis to carry a falsification criterion, an expected direction, and the prior knowledge it builds on, and keeps a knowledge base so that answered questions are not re-asked. last-mile's flow seals a prediction but does not require the *proposer* to say what evidence would disprove it, and has nothing that stops a rejected change from being re-proposed in a slightly different coat.

**First step.** Add two required fields to the change-request type — `falsification_criterion` and `supersedes` (linking to any prior proposal of the same change) — and make the learning flow reject a proposal that omits them. Then have the harness keep a small index of rejected proposals keyed by their intent, so the same change cannot be re-proposed without acknowledging the rejection.

**Effort / risk.** Low. Low risk. The hardest part is discipline, not code.

### 4. Cut the first five minutes to one layer, and count them

**Why.** Scrapcraft's own playtest found "the onboarding gauntlet is five layers deep before a new player touches the world … together they're a lot of fence before the yard", and separately found that in touch mode the tutorial still read "Press W A S D to walk" — desktop copy on the iPad path. last-mile's sandbox has the same design pressure: a commander, a squad, a city to understand. The measurement method is also ready-made — semantic action logs with a fixed seed, and a replay that must reproduce the original's end frame.

**First step.** Write the sandbox's first five minutes as an explicit list of beats, count them, and apply a budget (one interaction before any explanation). Then add the playtest harness's log format to the sandbox's test tooling so the count is measured rather than remembered.

**Effort / risk.** Low. Low risk, and it is a design edit rather than an implementation.

### 5. Type Bench's output as `(value, confidence, alternatives, intent)`

**Why.** `agentic-compiler`'s `TensorCell` [11] and `cuda-instruction-set`'s `Confidence::fuse`/`chain` [12] give a defensible shape for a spoken intent's result, and `error-as-signal` gives the refusal a place in the type system instead of in a side channel. last-mile's decisions.md already wants gates to carry confidence; the alternatives field is what lets the squad say "I could have done this instead" — which is the refusal mechanic that makes a squad a character rather than a menu (§3).

**First step.** Define the type in last-mile's own words (not the ecosystem's): a Vibe result carries the chosen block, a calibrated confidence, up to *n* alternatives with their own confidences, and the intent it was compiled from. Use the `fuse`/`chain` algebra, not the `min × 0.99` rule, and mark every confidence as uncalibrated until the seed farm says otherwise.

**Effort / risk.** Medium. Medium risk — it touches Bench's contract. Do it before the archetype patch lands, not after.

### The research bet, if there is a spare Thor-week

**A dial-space audio JEPA for Vox Score's missing-channel signal** (§5). Not the one in `jepa-perception-lab` — that one is over MUD room state and ships no code. The one worth building is a small latent predictor over the existing L0/L1 dial vector, trained on the skald deliveries, whose residual against the marks layer names the channel the protocol is missing (vox-score §6.3). It needs the Thor for training and the Orin for verification, and it is the only item in this entire survey that is a research project rather than a port.

---

## 15. Evidence gaps and open questions

- **`ROADMAP-AHEAD` does not exist** in this repository or in the `last-mile` main worktree, on any branch. The Bench/forgemaster mapping in §12.3 is therefore mapped onto `sandbox-on-otherhere.md` §1–§5 and `questions-as-experience.md` §4–§6, which do exist. If `ROADMAP-AHEAD` is a document that has not landed yet, the Bench mapping should be re-read against it when it does.
- **There is no RL-spike artefact.** The `rl-spike` branch is at the same commit as `m1-integration` and adds nothing; "evolution strategies on the seed farm" appears nowhere in the repository. §6 therefore compares against the closest existing thing — `flux-evolve` and `craftmind-ranch` — and flags the one rule that matters: a GA on the seed farm's seeds inherits the seed farm's discipline; a GA on its own repeats `cuda-self-evolve`'s mistake of scoring itself.
- **`jepa-perception-lab` ships no code.** Its CUDA simulation is not in the tree, so every claim about it in §5 is a claim about its *documentation*.
- **`Lucineer/cudaclaw` ships no code either** (README + `CLAUDE.md`), and `constraint-theory-core` — the CUDA CSP core behind `constraint-theory-core-cuda` — lives outside `forgemaster`.
- **Licences are mostly absent.** Exactly **13 of the 39 repositories read carry a `LICENSE` file, and all 13 are MIT**: `craftmind`, `craftmind-circuits`, `craftmind-courses`, `craftmind-fishing`, `craftmind-herding`, `craftmind-ranch`, `craftmind-researcher`, `craftmind-studio`, `SuperInstance/Lucineer`, `SuperInstance/forgemaster`, `SuperInstance/Scrapcraft`, `SuperInstance/ghost-tiles-1` and `SuperInstance/flux-evolve-py`. Everything else — including `scrap-spark`, whose *ideas* are the most importable here, and the whole `cuda-*` family — has no licence file, so under default copyright it is all rights reserved. `the-seed` and `self-evolve-ai` *claim* MIT in their READMEs without shipping a `LICENSE` file. **Re-use of any specific code needs Casey's OK and probably Magnus's; the ideas do not.**
- **Commit counts are on the default branch only** and include the bulk pushes of 2026-04-27, so they measure "how much was done", not "how careful it was". The craftmind family in particular was pushed hard on one day and its per-file quality varies: `craftmind-fishing` has 85 commits and 22 test files, while `craftmind-circuits` has 12 commits and a validator that may or may not be exercised.
- **Two claims in this ecosystem are ahead of their code** and are marked as such here rather than repeated as fact: Scrapcraft's networked quilt bridge ("exists and is tested, but the running game does not call it yet") and `self-evolve-ai`'s "only branches that clear your threshold are merged" (the code opens a PR).
---

## References

Every numbered marker above points to a repository in Magnus's ecosystem. All were read through `gh api` on 2026-10-30; the paths quoted in the body are paths inside those repositories at their default branch.

[1] SuperInstance/Lucineer — The origin project: deterministic rule-based scoring, forkable live state, four environment templates. https://github.com/SuperInstance/Lucineer
[2] SuperInstance/scrap-spark — The cached Spark AI and the shared build wall: `SHA-256` question cache, R2/KV/D1, `/gallery`. https://github.com/SuperInstance/scrap-spark
[3] Lucineer/craftmind-researcher — Hypothesis-and-experiment agent: falsification criteria, knowledge base, critic agent. https://github.com/Lucineer/craftmind-researcher
[4] Lucineer/self-evolve-ai — Git-agent that scores its own mutations 1-10 with an LLM judge; branch per mutation. https://github.com/Lucineer/self-evolve-ai
[5] Lucineer/the-seed — Goal-to-self-rewriting worker on Cloudflare Workers: three gates, branch per generation, KV captain's log. https://github.com/Lucineer/the-seed
[6] Lucineer/cuda-self-evolve — Mutation, selection, genome encoding, snapshot-based rollback for FLUX agents. https://github.com/Lucineer/cuda-self-evolve
[7] Lucineer/craftmind-herding — The herding command loop: typed actions, obedience, trust, self-written behaviour scripts. https://github.com/Lucineer/craftmind-herding
[8] Lucineer/jepa-perception-lab — JEPA experiments over MUD room state; the request for the missing CUDA simulation. https://github.com/Lucineer/jepa-perception-lab
[9] Lucineer/craftmind-fishing — Twenty-three personality scripts, condition-matched session comparison, three-server A/B. https://github.com/Lucineer/craftmind-fishing
[10] Lucineer/cuda-self-modify — Self-modifying programs: observation, proposed mutation with confidence, checkpoints. https://github.com/Lucineer/cuda-self-modify
[11] Lucineer/agentic-compiler — Lucineer Lang and the 42-opcode deliberation VM; confidence as a primitive type. https://github.com/Lucineer/agentic-compiler
[12] Lucineer/cuda-instruction-set — 80-opcode agent ISA with Bayesian confidence propagation. https://github.com/Lucineer/cuda-instruction-set
[13] Lucineer/cuda-perception — Perception pipeline: EMA/median/low-pass filtering, tracking, scene composition. https://github.com/Lucineer/cuda-perception
[14] Lucineer/flux-perception — Sensor fusion: confidence-weighted blending with bias and variance. https://github.com/Lucineer/flux-perception
[15] Lucineer/cuda-sensor-agent — Sensor agent over `cuda-equipment`: fused observations and health monitoring. https://github.com/Lucineer/cuda-sensor-agent
[16] Lucineer/cudaclaw — The recursive project: cell agents, muscle fibers, the ML feedback loop over `.claw-dna`. https://github.com/Lucineer/cudaclaw
[17] Lucineer/flux-evolve — Rust GA engine: population, fitness, elite selection, crossover, mutation. https://github.com/Lucineer/flux-evolve
[18] SuperInstance/flux-evolve-py — Deterministic Python evolution engine with elite protection. https://github.com/SuperInstance/flux-evolve-py
[19] Lucineer/git-cuda-agent — Template repo wiring the DNA layer to the fleet protocol. https://github.com/Lucineer/git-cuda-agent
[20] SuperInstance/Scrapcraft — Browser voxel game: tile editor, Spark, quilt view, the heart, build wall, hardware bridge, playtest records. https://github.com/SuperInstance/Scrapcraft
[21] SuperInstance/forgemaster — Constraint-aware agentic compiler; `flux-isa-thor`, `flux-verify-api`, `flux-provenance`. https://github.com/SuperInstance/forgemaster
[22] Lucineer/become-ai — Git-agent bootcamp: archetype, equipment, skills, milestones. https://github.com/Lucineer/become-ai
[23] SuperInstance/ghost-tiles-1 — Invisible computational tiles that influence visible state. https://github.com/SuperInstance/ghost-tiles-1
[24] Lucineer/frozen-intelligence — Mask-locked inference chip toolchain and vessel classes. https://github.com/Lucineer/frozen-intelligence
[25] Lucineer/swarm-intuition — Causal correlation over timestamped operational signals. https://github.com/Lucineer/swarm-intuition
[26] Lucineer/swarm-intuition-v2 — Emergent swarm behaviour prediction and conflict prevention. https://github.com/Lucineer/swarm-intuition-v2
[27] Lucineer/craftmind — Autonomous AI survival player; economy, challenges, tournament. https://github.com/Lucineer/craftmind
[28] Lucineer/craftmind-studio — The director: AI actors on schedules that adapt on set. https://github.com/Lucineer/craftmind-studio
[29] Lucineer/craftmind-ranch — Genetic engine with populations, DNA and an evolution dashboard. https://github.com/Lucineer/craftmind-ranch
[30] Lucineer/craftmind-courses — AI teacher that finds the player in the world. https://github.com/Lucineer/craftmind-courses
[31] Lucineer/craftmind-circuits — Server plugin that validates the redstone circuits you actually placed. https://github.com/Lucineer/craftmind-circuits
[32] Lucineer/craftmind-discgolf — Disc-golf simulation with an AI caddy and personality-driven NPCs. https://github.com/Lucineer/craftmind-discgolf
[33] Lucineer/cuda-assembler — Text-to-bytecode assembler for the 80-opcode instruction set. https://github.com/Lucineer/cuda-assembler
[34] Lucineer/flux-asm — Two-pass C11 assembler for the FLUX virtual machine. https://github.com/Lucineer/flux-asm
[35] Lucineer/cuda-learning — Credit assignment, lesson extraction, curriculum progression. https://github.com/Lucineer/cuda-learning
[36] Lucineer/nexus-learning — Reinforcement learning: skill acquisition, experience replay, reward shaping. https://github.com/Lucineer/nexus-learning
[37] Lucineer/the-fleet — The vessel registry: the map of the 100+ repositories. https://github.com/Lucineer/the-fleet
[38] Lucineer/flux-agent-runtime — Self-bootstrapping agents (empty in the tree). https://github.com/Lucineer/flux-agent-runtime
[39] Lucineer/cuda-ghost-tiles — Rust sibling of the ghost-tiles model. https://github.com/Lucineer/cuda-ghost-tiles

**last-mile documents cited by path.** These are internal to this repository and are not published, so they are cited by path rather than by URL: `docs/design/sandbox-on-otherhere.md`, `docs/design/vox-score.md`, `docs/design/questions-as-experience.md`, `crates/lm-learn/SEEDFARM.md`, `docs/research/fleet/00-overview.md`.

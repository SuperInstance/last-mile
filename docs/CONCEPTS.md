# last-mile concepts: the vocabulary

*Short definitions with where each lives. When a term appears in a brief, a commit or a design doc, it means this.*

**Agent / squad**: one of eight units (two squads of four: a leader and three members). Each has a **mind**.

**Answer contract**: what a model answer may become: a decision applied at a tick through lm-core's applied-decision path, never more.

**Applied decision**: a model's choice that landed in the sim at a tick and was logged. The only way intelligence touches the game.

**Beacon**: a control point. Holding one scores (`hold_ticks`). Three per map today.

**Bench / Bolt** (sandbox): Bench is the AI co-developer who turns a kid's spoken request into a named, bounded change. Bolt is the rival team's leader, who negotiates scoped offers. The same agent under different personality patches (design: sandbox-on-otherhere.md).

**Between-round screen**: the host's view of the learning flow: metrics, proposal, sealed prediction, verdict, Adopt / Reject / Override, and where the minds struggled.

**Confidence floor**: the per-backend minimum confidence for a gate's answer to count (d1-2B 0.30, d1-3B 0.15). Per gate, not per pack (D72).

**Conform** (Vox E10): reshape an already-rendered line toward a mark list instead of re-rendering it with skald: `lm vox conform in.wav --marks "urgent,clipped,falling" out.wav`; an impossible mark set prints the re-ask line and writes nothing. Class and latency gates pass (class landing 90–98% across the three arms; 187–188 ms p50 against a 1897 ms skald re-render); the ASR word-accuracy gate fails and was not tuned against (calm +4.47 points against a 2.0 budget). Landed on the `conform2` branch (2c55e84), pending merge; every number is `crates/lm-vox/CONFORM.md`.

**Decompose / escalate / prune**: three answers to a hard question. Ask finer questions of the same fast model; ask a second model (clef-flash, D71/D75); or ask only the options that tend to win (learned sparsity). See questions-as-experience.md.

**Deposit**: an adopted change applied to the harness, pinned for the next round.

**Determinism**: same inputs, same outputs, bit-exact on every machine (D69). Achieved by fixed-point maths, no clocks in the sim, ordered maps, and logged decisions.

**Directive**: a set of weights over the learning flow's metrics — the reward a coach changes between rounds. Inert in the shipped game (the metrics are affinely dependent: tags 0, margin = hold − const), alive wherever they disagree (`crates/lm-learn/RL-SPIKE.md`).

**Doctrine**: the move gate's strategy branches. The default is `advance` (D77); `hold` keeps squads home.

**Escalation**: asking a second model when the first is unsure (D71); only the move gate by default (D75); fenced by the apply tick (N3).

**Facts**: the text an agent's mind sees, built by facts.rs from what the agent may know (`view`).

**Gate**: one closed question in a pack: **choice** (pick an option, criteria map), **noul** (is X present), **score** (an ordered level). Wording is behaviour.

**Ghost**: (1) an invisible field that influences the visible world (attractor, repel, hot zone; with phases and falloff). The field model is built: `crates/lm-agents/src/shared.rs`, driven per harness by `facts.toml`, off by default; (2) something that exists logically but is computed only when it has earned it (learned sparsity). See ROADMAP-AHEAD §2.8.

**Glitch Meter** (sandbox): the stability measurement of a proposed change, from the held-out seeds' guard metrics. Never decorative.

**Golden**: pinned final-state hashes of reference rounds (`lm golden`, tests/golden.toml). Re-pin only with a written reason.

**Harness**: the data that shapes agents: `assets/harness/default/` (gates.toml, policy.toml, voice.toml, phrasebook; an optional `facts.toml` adds the shared target) plus variants. Changed by the learning flow and by people, never by generated code.

**Hodge triage**: splitting a question's pairwise option preferences into gradient (keep the answer), curl (decompose the question) and harmonic (refuse). After hodge-consensus; ROADMAP-AHEAD §2.6. The curl detector is refuted for a single model's probabilities (docs/MORNING-2026-10-30.md).

**Hot zone**: the shared target's `ghost` rule with a walking attractor — it sits on the lowest-numbered unheld beacon and moves every 600 ticks (`hot_beacon`, `crates/lm-agents/src/shared.rs`). Judged by D78 at two horizons: nothing meets by 1500 ticks (first contact lands at tick 1740–2237); at 3000 ticks it qualifies on contact (14 of 16 seeds, one-sided p = 0.0020) but not on the pre-registered composite (6 of 16 — eight of its rounds leave a squad on zero points), and its beacon walk order fails the fairness guard (a win gap of 28 over a bound of 20). **Not adopted as shipped** (`crates/lm-agents/tests/engagement.md`, "The hot zone, judged"; the raw records are `evidence/hotzone-1500.json` and `evidence/hotzone-3000.json`).

**Learning flow**: measure → propose → seal → test on held-out seeds → adopt or reject (lm-learn; D64-D66, D78).

**Marks** (Vox Score L2.5): named delivery words (strong, clipped, falling, brisk, bright...), quantized from measured dials relative to the speaker. Words for nuance (vox-score.md §11; E6).

**Never wait**: nothing in the game waits on a model, a voice, the network or a renderer. A late thing falls back: reflex, phrasebook, tone, plain render.

**Pack**: the set of gates asked in one request. The move pack every think; the talk pack on triggers (D70).

**Paint** (M2a): hashed world state since protocol v4 — a tag splats paint on the tagged cell plus a Chebyshev square of `splat_radius_cells`, clipped to the map, and the paint dries in the sim before the rules run (`crates/lm-core/src/world/paint.rs`). An empty layer adds no bytes to the state hash, so pre-paint logs still verify and replay. The movement knobs `slow_q8` and `bounce_q8` default to 0 and have no effect yet (defaults in `crates/lm-core/src/params.rs`; the decisiveness sweep re-measured unchanged, cell for cell: `crates/lm-core/tests/decisiveness.md`). The rule that uses them (slow down, push back) is measured on the `painteff` branch (8ab6cb8), pending merge, defaults still 0/0.

**Personality patch**: a System Two patch that biases gate choices, sets voice marks and seeds barks (ROADMAP-AHEAD §2.4).

**Phrasebook**: the fixed wording used when the realizer is late or off.

**Protocol v4**: the round-log protocol since the paint (M2a): the paint's four knobs were appended to Params, and version-3 logs decode through `ParamsV3`/`HeaderV3` and migrate with the paint defaults (`crates/lm-core/src/params.rs`, `crates/lm-core/src/header.rs`).

**Question library / ledger**: an agent's experience as its versioned, evidenced set of questions (questions-as-experience.md; ROADMAP-AHEAD §2.5).

**Realizer**: words a planned line (LFM2-1.2B on :8092; plain words from the plan, not the facts).

**Reflex**: the deterministic fallback behaviour when no decision lands in time.

**Refusal**: a named reason an ask produced no decision (13 kinds, N2), logged and counted.

**RL spike**: the measurement, not a change (`crates/lm-learn/RL-SPIKE.md`, bench `crates/lm-learn/examples/rlspike.rs`): a (1+λ) evolution strategy over a 36-entry policy table trains a squad policy that beats the shipped reflex on held-out seeds (29 of 32 up, sign test p = 1.278e-6) and rediscovers the hold tactic on its own; the hand-written hold rule still wins (32 of 32, 197 beacon-ticks against 186), and policy against policy keeps the mirror (30 of 32 level).

**Sealed prediction**: a proposal's claim (metric, direction, min delta) fixed before testing, judged on held-out seeds.

**Seat**: a coding agent (zcode, mcode, pi) working a BRIEF.md in its own worktree. Seats never commit; the director merges.

**Seed / seed farm**: a round's RNG seed; the seed farm evaluates many seeds in parallel across cores (SEEDFARM.md).

**Shared target**: one objective both squads are steered toward, stated by the facts builder from a harness parameter (`facts.toml` beside gates.toml and policy.toml; rules in `crates/lm-agents/src/shared.rs`). **Off by default** — the shipped harness has no facts file. Rules: `free` (the lowest-numbered beacon no squad holds), `centre`, and `ghost` (the hot zone's walking attractor). The 100-seed headless sweep: draws fall from 63% to 0% and tags rise from 0.62 to 4.19–5.45 a round, at the cost of collapsed rounds (45–76 of 100) (`crates/lm-agents/tests/engagement.md`).

**Situation key**: a coarse, deterministic bucket of an agent's state (beacon, enemy ladder, stunned, margin, time), used by the surprise signal and future question retrieval.

**Soak**: the 3-hourly run on the Thor of the golden, check.sh and live.sh against the trunk tip, with regressions flagged (`~/lm-soak/latest.md`).

**Surprise signal**: per situation, the share of asks that were unsure, refused or escalated. It points the learning flow at missing questions.

**System One / System Two**: fast, local, closed questions answered in the loop / slow, deliberate writing of questions, patches and proposals between rounds (or on events).

**Talk**: utterances on the squad radio. Planned (utter), worded (realizer or phrasebook), voiced (bank or tone); airtime and hearing are simulated.

**Vibe Dictionary / block** (sandbox): the closed vocabulary of typed, bounded changes a kid can ask for (ROADMAP-AHEAD §2.1).

**Vox Score**: a MIDI for speech performance: L0 frames (100 Hz), L1 gestures, L2 phrase line, L2.5 marks, L3 intent; analyzer, encoder, conformer and polisher (vox-score.md; lm-vox). The conformer ships as `lm vox conform` (see Conform; pending merge).

**Worktree**: a git worktree per seat job (ProArt `/home/proart/lm-wt/<job>`, Thor `~/lm-wt/<job>`), with a Windows build mirror per worktree name.

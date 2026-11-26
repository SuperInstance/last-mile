# Roadmap ahead: phases, ready schemas, open questions, and the next job waves

*The director's working plan (2026-10-30). Purpose: stay three moves ahead of the seats. Every phase has its schemas drafted before a seat builds it, every open question has a research track running while others build, and every device has its next wave queued. Pairs with docs/HANDOFF.md (the state now) and docs/design/sandbox-on-otherhere.md, questions-as-experience.md and vox-score.md (the designs).*

## 1. The phases

| Phase | Goal | Done means | Gated by |
|---|---|---|---|
| **M1 close** | M1 accepted by Casey | ACCEPTANCE.md refreshed, Casey's 20-minute pass, fixes landed, D75-D78 recorded | merges in flight (HANDOFF §4) |
| **M2a: a match worth watching** | the starting game is fun to watch with plain graphics | `advance` + the shared-target fact gives contact in most rounds; auto-director camera; paint splats as readable state; personalities audible | Q1, Q2, Q3 |
| **M2b: the sandbox slice** | a kid changes the match by talking | sandbox §8: one place, 10 Vibe blocks, Bench on the radio, sealed prediction + 16-seed test, thumbs up/down, put it back, one Bolt offer | Q4, Q5, Q6 |
| **M2c: Otherhere stage** | the match plays on Lincoln Street | sim grid ↔ place frame, legible tiers, Three.js client or Bevy host decided, EdenLive/restyle as optional last mile | Q7, Q8, Q9 |
| **M3: experience that grows** | the question library is the product | question ledger (evidence DAG), decompose-before-escalate everywhere it wins, per-question model routing, cloud-written questions adopted by test | Q10, Q11, Q12 |
| **M4: people** | Magnus playtests; two players; kids' safety reviewed | parent/teacher view, privacy review, two-squad multiplayer, Magnus's feedback loop | Q13, Q14, Q15 |

## 2. Schemas, drafted ahead

These are design drafts for seats to implement or adjust. Rule: data, not code; deterministic where the sim sees it (integers / fixed-point, logged, hashed); everything else is presentation or telemetry.

### 2.1 Vibe block (M2b): `assets/harness/default/vibe.toml`
```toml
[[block]]
id = "unit.vision"              # stable id; what Bench names out loud
says = ["let them see farther", "they should see farther"]  # the closed vocabulary the vibe gate's options are built from
field = "params.view_range_cells"  # the one field it writes: a params.* branchable field (BRANCH_FIELDS), or a rules.* field
kind = "level"                  # toggle | level | choice
min = 4                         # level: bounds, inclusive; refused outside, never clamped
max = 64
levels = []                     # choice only: the allowed values; anything else is refused
default = 20                    # must equal the field's shipped value, or load refuses
metric = "sight_ticks"          # the measure the default prediction watches
expect = "up"                   # up | down | either (either: the proposal must bring its own prediction)
tier = "A"                      # A locked | B learnable (the anti-drift guard, sandbox §3)
scope = "match"                 # match | round | team (Bolt's offers are team + round)
explain = "view_range_cells is how far a Dab sees; seeing farther means more eyes meeting"
age_ok = true                   # kid-safe vocabulary review passed
```
Built (M2b slice, `unit.vision`): a `level` entry carries `min`/`max` bounds rather than an enumerated `levels` list (a 61-value list is data noise; bounds are the honest shape, and `Params::set_branchable` + `validate()` enforce them in code); `field` may name a `params.*` **branchable** field because that is what the sim hashes per round — a Vibe change retunes a round, never reshapes it; `tier` is part of the file; `metric`/`expect` are the *default* prediction only, and a proposal may word its own (`--predict "sight_ticks:up,tags:up"`). The measures the pipeline knows: `sight_ticks`, `tags`, `hold_ticks`, `draws` (whole-round, both squads) beside lm-learn's per-squad metrics. Load refuses a dictionary whose `default` disagrees with the shipped `Params` — "against the current values" must be true. The `vibe` gate is a `choice` question whose options are block ids, plus `none` (D70: asked only when the kid speaks). Unsure answer → Bench asks a follow-up question (decompose before escalate), not a guess.

Built (M2b slice 2, the landing): four entries (`unit.vision`, `unit.speed`, `unit.tag_range`, `unit.stun_time`), each carrying its bounds' *provenance* in a comment — `Params::validate` where the code states them (speed: 0..=2048, one cell per tick), the file itself where it does not (tag_range 1024..=8192, stun 0..=300; validate bounds only the field's width). An adopted value lands through the one path `lm learn` commits through (`lm-learn::flow`'s apply → commit-with-derived-date → next-header pin): the in-force projection is written as data (`vibe-state.toml` in the harness root), committed with the same discipline, and the next round's header (`<runs>/vibe/next-header.json`) carries the values in its params and the records in `Header::vibe` (`VibeCarried { seal, block, value }`, protocol v5; hashed only when non-empty, so a round with no records in force hashes as before). `lm vibe state` reads what is in force from the ledger; `lm vibe undo <seal>` appends a `VibeUndo` line (the ledger stays append-only) and re-lands. Only the record in force for its entry can be undone; the field goes back to the previous record's value, else the shipped default.

### 2.2 A proposal, sealed (M2b; extends lm-learn's ledger, built)
```rust
struct VibeProposal { block: String, field: String, from: i64, value: i64,   // the entry, the field, the baseline and the change
                      label: String /* the kid's words as a bounded (48-char) label; the utterance itself is
                                     session memory only, never in the log */,
                      author: String /* bench | kid | bolt | flow */, claims: Vec<VibeClaim>, seeds: Vec<u64>, ticks: u64 }
struct VibeClaim { metric: String, direction: Direction /* Up | Down */, min_delta: i64 }
// sealed: VibeProposal is a ledger line; the seal is its line number and the chain hash of its bytes
struct MeasureOutcome { claim, deltas: Vec<(u64, i64)>, qualified: u16, moved: u16, claimed: u16,
                        p: String /* sign test, truncated */, met: bool }
struct VibeOutcome { seal: u64, seal_hash: String /* must name the sealed bytes */, measures: Vec<MeasureOutcome>,
                     adopted: bool /* every measure met, by D78's ceil(3N/4) + sign test */ }
```
**Stability** (the Glitch Meter, §2.2 of the sandbox design) is not in the slice yet: the verdict carries the per-seed deltas it will be computed from. The **prediction-trust tally** is: how many measures held, of how many were claimed and judged (`lm vibe ledger` prints it). It's a measurement, never a vibe.

### 2.3 Paint as state (M2b): lm-core
```rust
struct PaintCell { owner: TeamId, wet_q8: u8 /* decays per tick */, splats: u8 }   // per grid cell, in the hashed world state
// rule params (bounded in validate): splat_radius_cells, dry_ticks, tag_stun_ticks, bounce_q8, slow_q8
```
Paint goes into the state hash (or replays won't catch it). Splats are the readable consequence and the mirror-breaker (D63).

### 2.4 Personality patch (M2a): `assets/harness/personas/<id>.toml`
```toml
id = "anxious-grunt"
gate_bias = { move = { retreat = 8, around = 4 }, attention = { nearest_threat = 6 } }  # added to option log-odds, Q4
marks = ["halting", "soft", "lifting"]       # Vox Score L2.5 delivery target (§11)
voice = "jason"; style = "Sad"               # skald voice id + style suffix (D73)
barks = ["is it hot in here?", "I am NOT getting paint on this"]  # phrasebook seeds the realizer may word around
```
A personality is a System Two patch (vox-score §13): it biases closed choices and sets delivery. It never adds a question to the pack (D70).

### 2.5 Question ledger entry (M3): the evidence DAG, after rea
```toml
[question.attention_v3]
asks = "Which teammate or enemy should you watch now?"   # wording is part of the identity
kind = "choice"; options_from = "facts.visible"
decomposes = ["attention_near_far", "attention_pick"]   # if adopted as a 2-step tree
authority = "controlled_replay"   # shipped_artifact | controlled_replay | external_model | analyst_inference
confidence = "derived"            # observed | derived | inferred
evidence = ["seal:1841", "seeds:16", "sign_p:0.003", "judge:d1-3b agree 0.71"]
calibration = { "thor-d1-2b" = { floor = 0.30, mean_conf = 0.44 }, "clef-flash" = { mean_conf = 0.21 } }
cost_ms = { "thor-d1-2b" = 66 }
limitations = ["measured on the advance doctrine only", "quiet Thor"]
residual_unknowns = ["behaviour in 2v2 split squads"]
adopted = "2026-11-02"; retired = ""
```

### 2.6 Hodge triage of an answer (M3; offline first, then online)
For one question's answers (one model or several), build pairwise option preferences from the probabilities. Decompose into gradient, curl and harmonic. Then: **gradient-dominant → keep the answer; curl-dominant → decompose the question; harmonic-dominant → refuse (reflex plays).** Offline on the pack corpus (`~/lm-batch/packs`) before any online use.

### 2.7 Session telemetry for the parent/teacher view (M4)
Per session: the blocks used, proposals and verdicts, time played, and transcripts **only if a parent opted in** (default: none stored; voice never stored, sandbox §7). It's a readable list, not a dashboard of the child.

### 2.8 Ghost field (M2a/M2b): invisible intent over the visible world (after SuperInstance/ghost-tiles)
```rust
struct Ghost { at: Cell, kind: GhostKind /* Attract | Repel | Slow | Hot */, strength_q8: u8, radius_cells: u8,
               falloff: Falloff /* Linear | InvSquare | Step */, phase: Phase /* Temporal{ticks} | Permanent | Recurring{period, duty} */,
               author: Author /* Director | Bench | Bolt | Kid | Flow */ }
```
Fields are fixed-point and part of the hashed state when they touch the sim. facts.rs reports them as facts ("pulled toward beacon 2, strong"), so System One reads intent instead of taking orders. Uses: the shared target (a recurring hot zone), directors and Bench steering by placing fields, Vibe blocks and Bolt's scoped offers as temporal ghosts, "where" questions answered by reading a field, the learning flow proposing ghost placements, and belief fields (a rumoured enemy, urgency spreading from a shout). Viewers may see them drawn faintly.

**The second face of ghosts: earned presence** (after SuperInstance/cuda-ghost-tiles, plato-ghostable). Things exist logically but are *computed* only when they have earned it: weights strengthen with use, decay without it, prune to a budget, revive on demand. Applied: **questions** decay to ghosts when they rarely change a decision (D70's talk triggers, learned); **options** of a big question are pruned to the ones that win (a third alternative to decomposition and escalation); the **world** gets detail where the action is. Lifetimes follow plato-ghostable's persistence classes: **Eternal** (core rules), **Persistent** (adopted questions, the player profile), **Ephemeral** (Bolt's one-round offer, a kid's experiment).

## 3. Open questions, each with its research track

| # | Question | Track (seat or job) | The answer unblocks |
|---|---|---|---|
| Q1 | Does a shared-target fact make most rounds fight without breaking decisiveness? | `sharedtarget` | M2a default |
| Q2 | Can an auto-director camera find the action without jitter? | `autocam` | M2a watchability |
| Q3 | Do personality patches stay audible and fun, and do they hurt win rates? | next wave (A/B archetypes à la Magnus's craftmind-fishing) | M2a cast |
| Q4 | Does decompose-before-escalate beat escalation on accuracy per ms? | `decomp` (+ Hodge curl test) | the Vibe gate design, M3 |
| Q5 | What did kids' creation tools and NL-to-game systems learn? COPPA/UK Children's Code limits? | `res-prior` | M2b safety and UX |
| Q6 | 16 seeds × 1500 ticks costs ~2-10 s: is that an acceptable "Bench is testing…" wait, or do we need a faster pre-screen (an LLM judge, or fewer ticks with a guard)? | next wave: measure + Magnus's self-evolve-ai comparison (`reslucineer`) | M2b loop latency |
| Q7 | Three.js client (Otherhere, browser/iPad) vs the Bevy host? | next wave: spike rendering the sim log in the Otherhere client | M2c |
| Q8 | Can EdenLive (or FLUX/Qwen-Image) restyle frames fast enough, and is it shippable? | `res-eden` | M2c last mile |
| Q9 | Sim grid ↔ Otherhere place frame: square or axial hex? | next wave (hex nugget #4; DE study `resde` for RTS readability) | M2c |
| Q10 | Can the cloud write questions that pass sealed tests (a cot-quilt-style pipeline)? | `resdec`, then a spike | M3 |
| Q11 | Per-question model routing: which model per gate? | next wave on the pack corpus (offline, no tokens) | M3 |
| Q12 | Does RL (ES on the seed farm) add skill the question library can't? | `rlspike` | M3 scope |
| Q13 | Magnus's ecosystem: what does he already have that we'd build? | `reslucineer` | M4 and reuse (with Casey's OK) |
| Q14 | MIG: worth the risk for decision latency under studio load? | done: `16-mig-on-thor.md`; window pending Casey | platform |
| Q15 | Two players on two squads: what does the lockstep need? | next wave: DE multiplayer notes (`resde`) + a design note | M4 |

## 4. The next three waves, per device

**Wave 1 (queued now; see HANDOFF §4).** Thor: decomp, resdec, reslucineer, res-prior, sharedtarget, resde, rlspike, res-eden, conform. ProArt: apply, esc3, timingmerge, accept2, sisurvey, autocam, localwords. Free GPU: the E6 sweep, the pack corpus.

**Wave 2 (write briefs as wave 1 lands; each depends on a wave-1 answer):**
- Thor: **persona A/B** (Q3: 6 archetypes as patches, measured like craftmind-fishing: contact, win rate, lines, marks); **Hodge triage offline** on the pack corpus (Q4/§2.6); **per-question routing** offline (Q11); **cloud-writes-questions spike** (Q10, from resdec's pipeline).
- ProArt: **paint as state** in lm-core (§2.3, golden re-pin); **the Vibe gate + vibe.toml** for 10 blocks (§2.1); **auto-camera polish** from autocam's notes.
- pi (local, free): mechanical pieces with exact tests: the vibe.toml validator, the sign-test table generator, the docs-link checker (rerun), the ledger file linter.

**Wave 3 (M2b assembly):** Bench on the radio (the realizer + skald + radio framing; TALKSITKA's thumbs); the proposal → seal → 16-seed test → verdict UI on the between-round screen; "put it back" as an inverse deposit; Bolt's one scoped offer per round; the Otherhere stage spike (Q7/Q9 answers); a privacy review against `res-prior`'s findings; then Casey's playtest, and Magnus's if Casey agrees.

## 5. How to stay three moves ahead
- When a job lands, read its "what's next" and **write the next wave's brief the same hour**, before the seat goes idle.
- Keep the **schemas here current**. A seat builds against §2, and a change found while building comes back as an edit here.
- Each open question has exactly one track. When it's answered, record the answer and its evidence (a D-number if it's a decision), move the question out of §3, and add any new questions it raised.
- Put work where it fits: Windows/UI → ProArt (≤2 seats); GPU measurement and research → Thor (keep it quiet when measuring latency); mechanical jobs with tests → pi; archive/verify → the Orin when the corpus outgrows the NVMe.

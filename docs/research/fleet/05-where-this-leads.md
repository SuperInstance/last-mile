# Where this leads: seeing as a stream, generating as decisions, learning by questions

*2026-10-28. Sources:*
- *Casey's notes on Jev and Clef and "video instead of a flip book";*
- *`forclaude5.md` (a continuation of forclaude4);*
- *[Syzygy](https://github.com/SuperInstance/Syzygy), [syzygy-lattice](https://github.com/SuperInstance/syzygy-lattice), [syzygy-curriculum](https://github.com/SuperInstance/syzygy-curriculum), [syzygy-demo](https://github.com/SuperInstance/syzygy-demo);*
- *PoCs run by the ZCode and MiniMax Code seats (§6).*

## 1. The one idea under all of it

**Encode the world as a lattice of small typed cells. Recognize and generate only where cells change. Let every choice be a typed question with seeded dice. Learn by growing the set of questions.**

| Instance | "Cells" | "Only what changed" |
|---|---|---|
| MIDI | notes on 16 channels | events, not samples |
| MIDI Maze (D10) | 1 joystick byte per player per tick | only inputs travel |
| MPEG | macroblocks | P-frames and motion vectors |
| Syzygy / syzygy-lattice | 2×4-pixel cells → one glyph (band × orientation) + Braille dots | one fused pass; grids merge by join |
| last-mile's score (D8) | agents × lanes × ticks; chunks | decisions only |
| quilt rooms | rooms of text state | change-only ticks plus a heartbeat |

Casey's "video, not a flip book" is what happens when the cells are cheap enough to read **every** frame. Recognition stops being a sampled guess, "just fast enough not to trip", and becomes a continuous presence: D38's presence mode. Two things make that affordable:

1. **A cheap, exact encoder in front.** Syzygy turns a 160×96 frame into text in about 0.1 ms with integer math. syzygy-lattice packs brightness and edge orientation into one glyph. The fast model never sees pixels.
2. **Few questions per frame, asked only about changed cells.** Decision models answer up to dozens of typed questions in one pass: Clef allows 64, and d1 takes about 1.3× as long for 3 questions as for 1. A model that answers 8 questions over the changed region is cheap enough to run like a video.

## 2. For last-mile's agents, the lattice already exists

Agents never see pixels (D13). Their lattice is the **score**: world-anchored chunks, cells and lanes, changing only through decisions. That's the strongest version of "only what changed", because nothing changes except through a logged event. Presence mode (D38) is how agents get video instead of a flip book.

**Pixels and Syzygy matter on the other side:**
- the human-facing renderer;
- the vision cross-check (D41);
- neural skinning;
- any **real camera**: the quilt's sim/real seam, the boat, the desk.

There, the lattice text is the bridge: rasters of the score and real camera frames become the same kind of glyph field, read by the same questions. *A gate written against the glyph field doesn't care whether the far side is the sim or a camera.* That's the quilt's §23 seam, applied to vision.

## 3. Generation is recognition run as decisions

A decision model returns probabilities over a closed set. It can't paint pixels. But **generation can be posed as a stream of closed-set choices over a grammar**:
- which tile here (wave-function collapse);
- which archetype and parameters (Foundry: seed + params *are* the asset);
- which motion primitive next;
- which vector stroke.

Each choice is a typed question; each answer is rolled with seeded dice (D23). The same model, on the same lattice vocabulary, **recognizes** ("is this a doorway?") and **generates** ("put a doorway here, p=0.7").

What follows for last-mile's art pipeline (D14, research/11):

- **Structure first, skin last.** Procgen and animation happen in structure space: the score drives skeletal poses procedurally (notes → motion), rooms come from a grammar, and in-between frames are interpolated in structure (poses, vector shapes, glyph fields), cheaply and exactly. Neural models only **skin** structure.
- **Skin only what changed, cached by structure hash.** A skin tile is keyed by `hash(structure codes of the tile + style)`. A frame needs neural work only for keys not in the cache. Moving cameras make this **world-anchored**: tiles keyed in world space, as our chunks already are, not screen space. E1 tests this (§6).
- **Delta regeneration.** A 👎 on part of a room regenerates *that region* under the constraints of its neighbours, not the whole room. E3 tests this.
- **3D is the same, one dimension up:** chunk and voxel cells, archetype choices, Foundry parameters. The Thor's prerender-ahead (D14) becomes "roll the grammar for the next rooms with the taste model, skin overnight, keep what's liked".

## 4. Learning is bounded by the questions, so the trainer's job is the questions

Three pieces of fleet evidence point the same way:

- **syzygy-lattice `clicklearn`:** the learner recovers a hidden labeller's rule only if it lies **inside the declared hypothesis space**. A rule outside it is never accepted. That's honest, and it is *the* boundary.
- **syzygy-curriculum:** text models located things in text frames but didn't count or check. Teaching a **procedure** (count cell by cell; check the goal's four neighbours) added **+13 points** for both readers.
- **COLOR SQUAD / voice-games:** verbs unlock as earlier ones are mastered, and repeated wins become reflexes.

So "the student gets as good as its trainer at its own speed" is true **within the space the trainer has declared**. System-2's most valuable output isn't labels, it's:

1. **new gates** (questions the student should answer);
2. **new building blocks** (code the student can choose: levels, D40);
3. **procedures** (how to read: the curriculum lesson).

Each adds a dimension the fast student can learn in. Each block that fully matches a gate's behaviour converts model work to code: the model-vs-code dial moves, and the excavator gets better while the operator gets smaller.

**Receipts make it auditable.** syzygy-lattice seals every detection and accepted hypothesis as a hash-chained receipt. last-mile's log already carries every decision with provenance. Gate answers, accepted hypotheses and promoted reflexes should be receipted the same way, so a claimed improvement can be replayed and checked (D32).

## 5. What this changes or adds

| # | Change | Phase |
|---|---|---|
| a | **Skin cache keyed by world-anchored structure hash** for the 3D view and neural layers; only uncached keys go to the Thor | M2 (prerender), design now |
| b | **Procgen as a decision grammar** (WFC/archetypes) rolled with seeded dice, steered by a taste model learned from 👍/👎; delta regeneration of disliked regions | M2 |
| c | **A lattice-text codec as the vision bridge** (syzygy-lattice style: band × orientation glyph + Braille), used by D41's image path and any real-camera far side; gates can be posed over glyph fields | M2 spike |
| d | **Trainer output = gates, blocks and procedures**, with hypothesis acceptance by patience (N consecutive agreements), sealed as receipts | M2 (D39/D40 detail) |
| e | **Jev's place:** API-only, so it pays the WAN tail (~33 ms p50 from the ProArt). It's the right **teacher-side judge** and a cloud pool; local d1 is the right **presence-mode student**. Keys are held like syzygy-demo's rack: in a secret store, never in the page or repo | M1 router adapter; spike E4 |

## 5b. Borrowed from FASTER (MIT ACL)

[FASTER](https://github.com/mit-acl/faster) plans a fast, **optimistic** trajectory through unknown space, and at every replan it also commits a **safe** trajectory. The safe trajectory branches off the optimistic one, lies entirely in known-free space, and ends at rest. If the next replan fails or is late, the robot takes the safe branch. It's fast *and* safe because it always has a committed way out.

For last-mile (D45):
- **Fog of war is FASTER's known/unknown split.** An agent's optimistic plan may cross cells its squad hasn't seen. Its safe branch uses only known-free cells and ends in cover or a hold.
- **"Never wait" becomes "always hold an exit".** An expired, refused or late intent falls to *that intent's own* safe branch, which makes sense in context, instead of a generic default. Agents can afford to be bold and are never stupid when the Thor hiccups.
- **The commit horizon is the scheduling lead.** FASTER freezes the part of the plan that runs before the next replan can land. That's D34's adaptive lead.
- **Jump Point Search** on the coarse grid gives fast, deterministic integer pathfinding in `lm-core`.
- **Corridors as gate options.** FASTER's solver picks which convex free-space region each segment uses. For us, a small set of candidate corridors becomes a `choice` gate, so the decision model picks *the route class* and code does the geometry. This is the model-vs-code split again.

## 6. Experiments (run by the seats; results below as they land)

| Exp | Question | Seat | Result |
|---|---|---|---|
| A | Presence vs point mode: accuracy, lag, jitter, cost | MiniMax Code | **Conditioning beats call rate.** Presence 5 Hz + lane (α=½, deadband 0.2): 88.2% acc, 3.0 spurious flips/agent-min, 300 calls. Point argmax: 83.2%, 33.8 flips, needs 5× more server batches (per-agent requests don't batch). Raw presence: ~84%, up to 221 flips. **Rolling raw p[]: 56%, a dead end** → D43. Ladder: 10 Hz/600 calls ≈ 90–92%, 15 Hz ≈ 93%, 30 Hz ≈ 95% (but 30 Hz skips 8.6% of periods). Caveat: stub errors were independent per call; real models' errors correlate, which weakens EMA, so S1 re-measures |
| B | Fixed vs adaptive scheduling lead on our measured Wi-Fi | MiniMax Code | **Adaptive (host-level estimator, N=32, +10 ms): 99.77% on time at 102 ms mean lead vs fixed-3 at 98.28% / 100 ms; storms bump the lead to 4 ticks only while live. Verified by re-run** |
| E1 | Tile change rate and cache hit, static vs follow camera vs world-anchored | ZCode | **Only what changed holds, if tiles are world-anchored.** Static camera: 2.6% of tiles change per frame. Follow camera with screen-anchored tiles: **68%** (p95 100%), cache ≤61%. Same view, **world-anchored**: **4.8%**, 95.6% hits with a 256-entry LRU, changed-only bytes 4.8% of full. Storms barely move it. → D46 |
| E2 | Student vs teacher: raw input vs authored gates vs code; outside-space boundary | MiniMax Code | **The questions are the learning.** 5 authored gates: 91% at 50 samples, 100% at 1000, 15 mul-adds. Raw board: 32% → 46% (≈ majority baseline), 2,318 mul-adds. Forgetting the one key gate (LOS) caps it at 96.7% forever. A rule outside the gate space: the gated student is **structurally blind** (50% on matched pairs whose gate vectors are identical); raw input learns it given data (88%). Gated student = code on 100% → replace with code. → D47 |
| E3 | Generation as typed decisions; taste learning from thumbs; delta regen | ZCode | **One mechanism recognizes, generates and repairs.** WFC with decision dice: thumbs-up rate 60% → 99% over 30 batches from one bit per map; it recovered the hidden likes (cover near beacons +, water near spawns −); seeded replay byte-identical. **Delta regen of a disliked 6×6: 100% success, touches 9.4% of the map, taste −5.4 → +0.4.** Caveat: crude credit assignment (one thumb per ~380 decisions). → D48 |
| E4 | Jev on text-rendered grids, with blind/shuffled controls (the curriculum's method) | Claude | **Jev reads the frame:** action 0.72 vs blind 0.23 (majority 0.40), bearing 0.75 vs 0.15; shuffled = blind. Code-given gate facts lift action to 0.88 but not 100%, so code computes gates and the model judges over them. ~235 ms p50 over the WAN: Jev is point-mode/teacher-side; local d1 is the presence student. See poc/jev |

## 7. What the experiments settle

1. **World anchoring is the precondition for "only what changed"** (E1). last-mile already has it, because chunks and cells live in world space. Every cache (skins, recognition answers, prerendered tiles) is keyed by **world tile + structure hash**, never by screen tiles. The 3D view's neural layer only ever works on the ~5% of tiles whose key is new.
2. **Questions beat data, and blindness can be detected** (E2). Authored gates beat raw input by 59 points at 50 samples, at a sliver of the cost. A missing gate is a permanent ceiling, not a data problem. The matched-pair probe is a **tool for the trainer**: build pairs that differ only in a candidate factor. If the gate vectors are identical but the outcomes differ, the student is blind to that factor, and a new gate is needed. System-2 proposes factors, and the probe tells it which ones are missing. That's "setting gates across an array of the right questions" made mechanical.
3. **The dial moves when model and code agree** (E2). When a gated student matches a code rule on 100% of held-out cases, the rule takes over. The expensive part was always the *gates* (LOS, BFS: code). The model on top is tiny. A calculator wrapped in a thin judgement layer, with a storyteller only where the questions run out.
4. **Generation, taste and repair are one mechanism** (E3). Procgen posed as typed decisions with seeded dice learns taste from 👍/👎 alone, replays exactly, and repairs a disliked region without touching the rest. That's the Thor's prerender-ahead design (D14) in miniature, and the same shape scales to archetype parameters and 3D chunks.

**Where this leads.** One substrate, the world-anchored lattice of typed cells, carries everything:
- agents perceive it (the score);
- fast decision models live on it (presence lanes);
- slow models improve it by adding **questions and blocks**, not by re-deciding;
- generation rolls on it (grammars);
- rendering skins it only where it changed;
- every step is a logged, replayable, receipted decision.

The model-vs-code dial settles per job, and the operators get smaller as the machinery gets better.

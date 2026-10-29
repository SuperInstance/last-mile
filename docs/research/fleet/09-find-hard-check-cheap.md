# Find hard, check cheap: last-mile and the ETH result

*2026-10-29. Prompted by Casey ("our origin-first, MIDI-like record bypasses a lot of the hard parts of his math… we're doing it intuitively") and [this video](https://www.youtube.com/watch?v=P6idZrAVL3E) (digest: [sources/2026-10-29-eth-video-digest.md](../sources/2026-10-29-eth-video-digest.md)). The video reports AI-produced, Lean-checked claims that the Exponential Time Hypothesis is false: a deterministic 2^o(n) algorithm for #3-SAT. These are claims awaiting independent scrutiny.*

## Where the intuition is right

**last-mile lives on the cheap side of NP.** Finding a good joint plan for 16 agents is a search over a combinatorial space; *checking* a recorded one is linear. The architecture already splits the two:

| Role | Search (hard) | Verification (cheap) |
|---|---|---|
| Who does it | a few choosers: agents' models, reflexes, the human | every node, every replay, every CI run |
| What it produces | decisions | identical hashes |
| last-mile mechanism | gates and menus, lanes, intents | the decision log as certificate (D8, D10); replay as checker; golden hashes; `lm branch` |
| Analogue in the video | the AI's proof search | Lean 4 checking the proof |

The tools practical SAT solvers use to beat worst-case behaviour (while ETH constrains only the worst case) have appeared in last-mile by intuition:

| SAT-solver technique | last-mile |
|---|---|
| Unit propagation (prune before searching) | menus of **legal** options only (D23/D37); refusals (D26) |
| Preprocessing / simplification | **code computes the facts** (D49): models never re-derive LOS, paths or bearings |
| Branching heuristics (which variable to ask about) | **the questions are the learning** (E2/D47): the right gates make a tiny model sufficient |
| Conflict-driven clause learning | lessons from losses become reflexes, blocks and lexicon (D25, D40, phase 5) |
| Restarts and time-outs (anytime search) | never wait (D11); safe branches (D45); **thinking time is the game** (D55/D56) |

## Where it stops

We don't bypass the mathematics; we **don't pose the worst-case problem**. ETH concerns exact answers on adversarial instances. A game needs good-enough decisions inside a time, money and size budget (D55–D58), on worlds full of exploitable structure: locality, symmetry (D3 and the fairness work), world-anchored deltas (D46), small menus. That's bounded rationality by design. The hardness still exists. It's just never what we ask for.

## The refinement: certificate-carrying decisions

Make the cheap side explicit. A decision from an expensive mind can carry a **certificate**: a short, code-checkable claim of why it's legal and safe. For example "path to beacon 2 exists (JPS length 14), no enemy with LOS within 6", or for an intent, its safe branch's known-free cells. `lm-core` (or an agent-side checker) verifies the certificate in constant or linear time before applying the decision; a failed certificate is a refusal. This keeps verification cheap no matter how expensive the thought was. It's also the shape that lets slow, smart tiers (D55) and human edits be trusted without re-running them.

**Proposed decision (D59):** certificates are optional fields on Intent/Lane decisions in phase 3 Task 7 / phase 5. The first certificates are the safe-branch cells (already checkable) and the facts key the decision was made on (D25), so a decision made on stale facts can be detected and refused.

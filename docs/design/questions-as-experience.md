# Questions as experience: how decision models change the game, locally and in the cloud

*Design note, 2026-10-30, from Casey's prompt: "the questions you know to ask become your experience, and the level of decomposition can be as granular as needed." Not a decision yet; §6 lists what it would take to make it one.*

## 1. The claim

In last-mile, a decision model never decides in the open. It answers **closed questions** (gates) that we wrote: a choice among 2-10 options, a yes/no presence, an ordered score, each with criteria and a calibrated probability (D62, D70, D72). The model is an **evaluator**. What the agent *knows how to notice and weigh* lives in the **questions**: which ones exist, how they are worded, when they are asked, and how finely a situation is broken down before the model is asked anything.

So **an agent's experience is its question library.** It is not the model's weights. Swap d1-2B for clef-flash or a cloud model and the agent keeps its experience, because the questions travel. Every finding tonight that changed behaviour was a change to a question, not to a model:
- the intention gate's levels named facts the game never states, and rewording them made fights sound urgent (urgent: 0/179 → 19/19);
- "Is this moment worth an urgent tone?" read as "is this even here?", and asking for the moment's *news* made agents talk (talkthor, D72);
- the order/question axis scored at chance until the question was asked in the vocabulary of the signal (E4b: 50% → 65.6%);
- the doctrine's *hold* branch was followed 100% of the time, because the question had only one sensible answer (engagement);
- the missing question is "who to attend to when nobody is there", which the surprise signal found by counting where answers were unsure.

## 2. Decomposition is a dial, and it is the cheapest dial we have

On the Thor's d1 server a question costs about **11 ms per option** (D70). A ten-option question costs about 110 ms, and on the attention gate it is unsure by nature (0.29-0.64). The same choice decomposed as two questions ("near or far?", then one of 3-4 candidates) costs about 6 options, 66 ms, and each step is easier to answer confidently, because each has fewer, more distinct options.

So **granularity is a budget knob, chosen per moment**:
- **Coarse when calm, fine when it matters.** In a quiet stretch, one coarse question a second; in a fight, a finer tree several times a second.
- **Anytime answers ("never wait", applied to thinking).** Ask the coarsest question first; act on it if time runs out; refine if the deadline allows. Every level of the tree is a usable answer.
- **Decompose before you escalate.** Today an unsure answer goes to a second, slower model (D71: about 570 ms, and on ten-option gates usually *worse*, D75). The alternative is a **finer question to the same fast model**. This is the sharpest testable claim in this note (§5).

Taken to its limit, a decision is a short path through a tree of small calibrated questions, like twenty questions, and the tree's shape *is* the agent's skill. The Vox Score marks (§11 of vox-score.md) are the same idea in perception: each mark is the answer to a tiny closed question about the sound.

## 3. Local and cloud have different jobs

| | Local (Thor: d1-2B, clef-flash, LFM2, LFM3-Audio) | Cloud (large models; and the seats that write this code) |
|---|---|---|
| Speed | 10s-100s of ms | seconds |
| Cost | GPU time, no per-call fee | per call |
| Privacy | stays on the machine (kids, voice) | leaves it |
| Best at | **answering** many small questions, all the time | **asking**: writing, rewording and decomposing questions; reviewing evidence |
| In the loop? | yes, System One | no, System Two, between rounds or on events |

The division of labour that follows: **the cloud compiles understanding into questions, and the local models execute them.** One cloud call that writes a good question is then asked a million times locally for almost nothing. That is exactly how the seats have worked tonight: cloud agents measured, diagnosed and reworded gates; the Thor answered them in the game.

## 4. How experience grows

1. **Find where the questions are missing:** the surprise signal (unsure, refused and escalated answers per situation), plus refusals by kind (N2) and outcomes (engagement, seed farm).
2. **Propose a question:** a new gate, a rewording, or a decomposition. Written by the cloud director, by a person, or by Bench in the sandbox ("make the paint bouncy" is a question about the world: *how bouncy?*).
3. **Seal a prediction and test it** on held-out seeds with the sign test (16 seeds, D-pending), so a question earns its place by moving outcomes.
4. **Keep the question, with its record:** who wrote it, from what evidence, its per-model calibration and its cost.

Two consequences:
- **Experience is readable.** A question library can be shown, discussed and edited, including by a kid in the sandbox ("Bench, what do the squads ask themselves before they shoot?").
- **Experience is personal.** What *this* player means by a clipped "hold" is a learned question about the player (vox-score.md §7), kept in their profile.

## 5. The experiments this implies (ranked)

1. **Decompose vs. escalate**, on the attention gate (ten options, the most unsure gate, the most escalated):
   - arm A: today (d1-2B, escalate to clef-flash when unsure);
   - arm B: a two-step decomposition on d1-2B alone (coarse partition, then a small choice within it);
   - arm C: A and B combined.

   Measure confidence, agreement with a neutral judge (d1-3B, as esctune did), latency and ms per decision. Prediction: B is faster and more confident than A, and at least as accurate.
2. **Anytime packs:** act on the coarse answer when the deadline would otherwise be missed (the late answers in N4's ledger: 198 of 252 under load) and refine when time allows. Measure late answers and decision quality.
3. **Per-question model routing:** route each question to the model measured best for it (clef-flash on the move gate, d1-2B on attention), not one tier for all.
4. **A question ledger** in the harness: provenance, evidence, calibration per model, cost, and adopt/retire dates.

## 6. What it would take to make this a decision
Experiment 1 showing decomposition beats escalation on accuracy per millisecond; a question ledger format in the harness; and the learning flow able to propose a *decomposition* (not only parameter changes). Then: "a mind's competence is its versioned question library; models are interchangeable evaluators; the cloud writes questions, the local models answer them."

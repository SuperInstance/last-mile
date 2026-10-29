# Decision models: fast models that answer typed questions

*2026-10-28. Sources:*
- *Casey's note on JEV, Clef and signal-chain;*
- *`C:\Users\casey\forclaude4.md`, an ideation transcript ("seeds, not specs");*
- *[signal-chain](https://github.com/SuperInstance/signal-chain);*
- *vendor pages for the three model families, linked below. All vendor numbers are vendor-reported until we measure them on our machines.*

## 1. Casey's thesis, restated

1. **Fast decision models can be a presence, not a sequence of points.** A model that answers typed questions in a single forward pass (JEV, Clef, Liquid d1) can run as a stream instead of a sequence of separate calls. Its answers become a **waveform**: a continuous lane of probabilities that's always there, in the present tense.
2. **A slow trainer and a fast student.** Slower, conventional recognition plus cloud System-2 watches, labels and judges. It trains System-1 by **choosing the right questions** (the gates) and by grading answers. The student can get as good as its trainer *at its own speed*, as far as the trainer's questions reach.
3. **Building blocks as levels.** Higher models write better algorithmic building blocks. The decision model then hands more of the work to code, until the right **ratio of model to code** for the application is found. "Sometimes you want a calculator and sometimes you want a storyteller."
4. **The deadband of negative space.** Whatever the ratio, the output must stay inside the bounds of what's allowed, including what's deliberately left unsaid, on its way to meeting the prompt's spec.

This fits last-mile closely. D23 (dice over a menu), D25 (the descent into reflexes) and D13 (agents read numbers) were already halfway there. Decision models are the missing part: **a model class built for the System-1 slot.**

## 2. The model class (October 2026)

All three take **state plus a schema of typed questions** and return **a probability for every allowed answer in one pass**. There's no text to parse, and they can't produce an answer outside the schema.

| Model | Size / base | Weights | Latency (vendor) | Inputs | Fit for us |
|---|---|---|---|---|---|
| **Liquid d1-2B** (LFM3; replaces d1-3B) | ~2B | Open (LFM Open License: free below $10M revenue) | d1-3B: **~16 ms on Jetson AGX Thor**, ~50 ms on Orin Nano; 3 questions ≈ 1.3× one | text + images (vision-language base) | **Default member System-1 candidate**: local on the Thor, small enough for ProArt fallback |
| **Liquid d1-Omni-2B / d1-omni-600M** | 2B / 0.6B | Open (LFM) | n/a | text, image, audio | Audio/tone experiments (utterance score, M2) |
| **Cloudflare Clef-flash** | 9B (Qwen3.5-9B) | **Apache-2.0**, HF `Cloudflare/clef-flash` (load with the bundled `joint_schema_model.py`; GGUF ports are not equivalent) | ~39 ms median on Workers AI | text, JSON, up to 4 images; ≤64 questions; 64K context | **Leader / harder-decision candidate**, local on the Thor or via Workers AI as a cloud pool |
| **Cloudflare Clef** | 27B (Qwen3.8-27B) | Apache-2.0 | ~209 ms median | same | Between-round judge / teacher-side labeller |
| **TypeSafe Jev** | undisclosed | **Proprietary**, hosted API only (early access; also DigitalOcean) | vendor claims ~100–200× faster than LLMs on classification | text + schema | Optional cloud pool, budgeted by `lm-router`. Clef is Jev-API compatible, so one adapter serves both |

**Question types** (Clef): **boolean**, **choice** (named options) and **score** (ordered options). These line up exactly with what our agents need:

- `boolean`: *engage?*, *speak now?*, *trust this callout?*
- `choice`: *which building block*, *which target*, *which speech act*, *whom to address*
- `score`: *aggression 0–4*, *urgency*, *confidence in my squad's plan*

**Naming note:** the fleet already uses "JEV/Jev" for other things: `jev-quilt`, the Thor's JEV rack (I-JEPA, V-JEPA 2, a judge on :8077) and voice-games' "Jev negation guard". In last-mile docs, **"Jev" means TypeSafe's model** and **"the JEV rack" means the Thor service**.

## 3. How it lands in last-mile

### 3a. System-1 becomes "state + gates → probabilities" (D37)

- The agent's harness gains **`gates.toml`**: its question set. Each question is a name, a type (bool, choice or score) and its options, which can be **generated from the sim** (D23's menu: the legal targets, visible teammates, available building blocks).
- The state sent with the questions is the deterministic summary of the `ScoreView` as typed JSON. Later it may include images (§3e).
- Answers come back as `p[]` per question, quantized to u8 or u16.
- `lm-core` turns answers into actions **deterministically**:
  - a choice is rolled with the seeded RNG (D23);
  - a score is sampled or rounded per question policy;
  - a boolean is thresholded with hysteresis.
- **Utterance text** still needs a generator, but less of one:
  - the decision model chooses the **speech act, addressee, intent and affect tags**, which are score lanes (D12);
  - the *wording* comes from either a **phrasebook template** (code: "the calculator") or a small generative model ("the storyteller").
  - ⚙ `utter.realizer = template | llm | auto` is exactly Casey's dial, applied to speech.

### 3b. Point mode and presence mode (D38)

- **Point mode** (today's design): an agent thinks when the policy says it's worth it (novelty, threat, addressed by name) and waits in the least-use-first queue.
- **Presence mode:** while the measured latency for the whole squad batch fits the budget, every agent's gates are evaluated at a steady rate (⚙ `s1.presence_hz`, e.g. 10 Hz). Each answer becomes a **control lane** in the score: a per-agent CC-like stream (D8).
  - `lm-core` conditions the lanes **in fixed point**: an EMA low-pass (⚙ α), then a **deadband with hysteresis** (⚙ width). An action changes only when the conditioned lane leaves its deadband.
  - This is signal-chain's `raw events → dials → field → nudge`, with the Clipper as the hard bounds.
  - "A presence in the present tense" means agents feel continuous and steady, rather than jerking at each think.
- ⚙ `s1.mode = point | presence | auto`. With `auto`, presence mode runs while `p99_batch_latency + transit ≤ presence period`, and the agent drops back to point mode when it doesn't fit, such as when the Thor is busy or its Wi-Fi is spiking.
- **Determinism is unchanged.** Lane samples are decisions. They're logged, scheduled with lead (D11/D34) and applied at a tick, so mirrors and replays never need the model.
- **Bandwidth stays tiny:** 16 agents × 8 questions × ~4 options × 1 byte × 10 Hz ≈ 5 KB/s.
- **Budget:** presence mode spends System-1 continuously. Under least-use-first (D16) it costs queue priority, which makes it a strategic choice in its own right: "go present" for the late-round push.

### 3c. The trainer and the student (D39)

Slow judges teach the fast student. This is **teacher-student distillation with adversarial curricula**, not literally a GAN: no discriminator gradient flows. It runs between rounds or overnight ("the night works twice", from the quilt).

1. **Record.** Every gate answer is logged with its state hash, `p[]`, and what happened next (D25's situation key and outcome).
2. **Judge (slow paths).**
   - Cloud System-2, and Clef-27B on the Thor as a cheaper judge, label situations:
     - *what should the answer have been*, given the outcome;
     - *was the question even the right one*.
   - A second, independent path judges the same moment differently: a vision model on a **rendered top-down map of the same score**, see §3e.
   - Where the two disagree, that's a **dissonance flag** (chrono-tensor track 7) and a priority training example.
3. **Gate authoring.** The trainer's main lever is **choosing and editing questions**: adding a question the student lacks, splitting an overloaded one, or retiring one nobody uses. This is "setting gates across an array of the right questions." Gate edits are harness commits (D27) with sealed predictions (D32).
4. **Fine-tune.**
   - d1 and Clef weights are open, so per-role LoRA adapters can be trained on the Thor overnight from labelled episodes.
   - Cloudflare's RL fine-tuning platform is partner-only today, so it's not on our path.
   - Adapters are versioned and pinned in the round header like harness commits.
5. **Challenge.** The trainer generates **hard situations**: seeds and scripted scenarios where the student's answers were confidently wrong. The next night's training uses them (dungeon-ml-zoo honesty rules: held-out eval seeds, D32).

The student "gets as good as its trainer at its own speed" only where the trainer's questions and labels reach. The measured gap is the **student–teacher agreement rate** on held-out seeds, reported per gate.

### 3d. Building blocks as levels, and the model-vs-code dial (D40)

- The harness gains **`blocks/*.rhai`**: parameterized behaviors written by higher models or by hand, such as `hold_zone(z, spacing)`, `bound_overwatch(a, b)`, `flank(side, depth)` and `relay_callout()`.
- Gates choose **which block** to run and **its parameters**; the block runs as deterministic code inside `lm-core` (D21).
- Each better block moves work from the model to code, which is the excavator rubric: better excavators, smaller operators.
- This is the **levels** idea:
  - level 0 is primitive actions (the General Action Set);
  - each level is a library of blocks built from the one below;
  - an agent's gates can only name blocks from levels it has unlocked or earned. This reuses the tutor ladder (D29) for agents.
- **The model-vs-code dial (signal-chain), made a metric:** per agent and per gate, the share of decision weight decided by code (reflexes, blocks, templates) versus by a model.
  - It replaces "reflex share" (D25) as the general measure. Reflex share is the extreme end of it.
  - The goal is **not** to maximize code. The best ratio depends on the role and the situation: a scout's callouts may want the storyteller, and a hold-the-beacon loop wants the calculator.
  - ⚙ `dial.target` per role is a setting, and learning reports distance to target.
  - signal-chain's own lexicon dial (text that reads like code vs prose) can classify utterance *style* on the same axis, as a squad-talk metric.

### 3e. Vision as the other path (D41; spike, off by default)

Agents still never see the 3D render (D13). But the models accept images, so the **slow, second path** can look at **a raster of the score itself**:

- a top-down tactical map;
- the piano roll of the last 10 s;
- the agent's fog-of-war view as a small image.

These are cheap to render on the ProArt or Thor, fully determined by the score, and they show the same truth "from a different angle". Two uses:

1. **Training cross-check:** numeric gates vs image gates on the same moment. Disagreement becomes a training signal (§3c).
2. **An alternative thinking path:** a leader may spend budget to ask an image-capable gate ("is this formation sound?") when the numeric path is uncertain, i.e. high entropy in `p[]`.

Image inputs cost more and are slower, so this is budgeted and **off by default** (⚙ `s1.image_path = off | judge | leader`). It's a spike before it's a feature.

### 3f. The deadband of negative space

Three layers keep output inside the allowed space, from cheapest to most expensive:

1. **The schema:** a decision model can't answer outside the options it was given. Our menus are generated from what's legal *right now*.
2. **`lm-core` legality and the deadband:** illegal or out-of-bounds results are refused, never coerced (D26). Conditioned lanes only act outside their deadband (§3b).
3. **Silence is an option:** every speak and act gate includes *no / hold / silence* (D31). Negative space is part of the menu, not the absence of an answer.

## 4. forclaude4: what fits

| Idea | Verdict |
|---|---|
| Four temporal tiers: code at frame rate, fast local decision 10–100 Hz, mid 1–10 Hz, LLM ~0.1 Hz | **Keep.** It's our reflex → System-1 (presence/point) → leader System-1 → cloud System-2 stack, now with real model names |
| "The decider never renders; the renderer never decides" | **Keep.** It's D8/D13 in other words |
| Every decision and its signal transformation is logged; offline replay analysis when the human is away; consolidating good patterns into blocks | **Keep** (D25, D39, D40) |
| Levers & labels: models modulate labelled continuous levers inside deterministic constraint labels | **Adapt:** levers = control lanes (§3b); labels = leader-placed zones and keep-outs that `lm-core` enforces (a block parameter) |
| Signal conditioning (EMA, normalization, scaling) between model and engine | **Keep**, in fixed point inside `lm-core` |
| Snapping floats to Pythagorean lattice points (constraint-theory-core) for determinism; quantization noise as uncertainty | **Drop.** Snapping can still diverge across a rounding boundary. We never let floats into the sim (D10). The entropy of `p[]` is the uncertainty measure |
| cudaclaw GPU-resident pipeline | **Drop.** Our sim is tiny, and the quilt adjudication already rejected immortal kernels on multi-tenant GPUs |
| OSC/TouchDesigner/Unreal bridges; liquid networks for excavator hydraulics | **Drop.** Different targets; we're Bevy and a game |

## 5. Open questions → spikes

- **Spike S1 (Thor):** d1-2B and Clef-flash latency for **16 agents × 8 questions, batched**, measured at p50/p99 with the Thor's other tenants loaded. This decides the `auto` thresholds for presence mode and whether Clef-flash fits as the leader model.
- **Spike S2 (ProArt):** d1-2B quantized on the 4050 alongside rendering, as a candidate for `s1_fallback`.
- Check the d1 schema and API shape from the HF model card. Clef's question types are confirmed, d1's output format isn't.
- Licensing: LFM Open License (revenue threshold) vs Apache-2.0 (Clef). Fine for a personal project; record it.

## Sources

- [Cloudflare: Introducing Clef](https://blog.cloudflare.com/clef-decision-models/) · [Workers AI changelog](https://developers.cloudflare.com/changelog/post/2026-10-01-clef-workers-ai/) · [MarkTechPost on Clef](https://www.marktechpost.com/2026/10/01/cloudflare-releases-clef-and-clef-flash/) · [Flavio Copes: Clef deep dive](https://flaviocopes.com/clef/) · [Clef-flash summary](https://ai-tldr.dev/models/clef-flash/) · [Clef-flash GGUF card](https://huggingface.co/prithivMLmods/clef-flash-GGUF)
- [TypeSafe: Introducing System One models & Jev](https://typesafe.ai/blog/introducing-system-one-models-and-jev) · [TrueFoundry on Jev](https://www.truefoundry.com/blog/typesafe-ai-jev) · [DigitalOcean: What is Jev](https://www.digitalocean.com/resources/articles/what-is-jev)
- [MarkTechPost: Liquid d1-3B / d1-omni-600M](https://www.marktechpost.com/2026/10/07/liquid-ai-releases-open-weight-d1-3b-and-d1-omni-600m-multimodal-decision-models-with-zero-output-tokens/) · [Liquid: LFM3](https://www.liquid.ai/blog/lfm3) · [MarkTechPost: LFM3](https://www.marktechpost.com/2026/10/27/liquid-ai-lfm3-open-weight-2b-multimodal-models/) · [Decision models on Jetson](https://llmconfigurator.com/en/guides/decision-models/edge-hardware)

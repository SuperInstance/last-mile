# 05: The learning loop: logs, then lessons, then better squads

## What the source proposed
- Telemetry tokens `[Δt, group, channel, index, value]` are "pre-tokenized reality" that transformers or LSTMs can eat directly.
- Train autoregressive models to predict the next packet.
- Mine many match logs for behavioral habits (for example, "a 45 ms hesitation before firing around corners").
- Generate "targeted assist scripts" that the Thor injects into the *player's* input live.
- Train "1000× faster" by scaling Δt in the dataset.
- Splice tracks from different rounds into "Frankenstein" macro scripts.

## Assessment
- **Compact event logs as training data:** sound. Discrete, ordered, small, with no rendering involved.
- **"No normalization or cleaning needed":** false. Every dataset needs feature design. Raw lane values are not meaningful tokens.
- **Scaling Δt to train 1000× faster:** wrong mechanism. The real version is **headless faster-than-real-time simulation** (see 01), which is legitimate and powerful.
- **Assist scripts injected into player input:** that's an aimbot. **Reframed for us:** the same loop improves *agent squads*, not the human's hands.
- **Splicing tracks across rounds:** an interesting counterfactual tool. It belongs in Studio mode (see 06).

## How it matters for us: the between-round loop

This is the System-2 half of your design ("squads learn which language worked").

```
Round N log ──► after-action review (cloud, System 2, budgeted)
                 │  inputs: the squad's utterances, who obeyed/ignored/misread,
                 │          outcomes, timing; plus the squad's current lexicon
                 ▼
            lexicon/doctrine diff  ──► stored in canon (Thor vector DB)
                 ▼
Round N+1: System 1 and reflex scripts read the updated lexicon/doctrine
```

**Measuring "language that worked" (concrete metrics from the log):**
- *Order uptake:* fraction of orders acted on correctly within T seconds
- *Words per successful order:* does the squad get terser over time?
- *Misread rate:* the receiver's action didn't match the speaker's intent tags
- *Airtime share:* who talks, who just listens

These go into the review prompt and get tracked across rounds. That turns "the squad learned to communicate" into a curve you can watch.

**Later (beyond milestone 1):**
- Fine-tune or distill the Thor's System-1 model on accumulated good rounds (behavior cloning from your own logs). This is where "logs as training data" actually pays off.
- Run headless fast-forward A/B on the Orin: re-run round N's opening under doctrine A vs B, many times, before the next live round.

## Milestone 1 scope
- The log records utterances, intent tags, receivers and outcomes
- One cloud System-2 review per squad per round, producing a lexicon/doctrine diff written to the vector DB
- System 1 reads the lexicon at round start
- The four metrics above computed and shown on the between-round screen

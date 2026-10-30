# A review of the Gemini "Thor as a game engine" conversation (2026-10-29)

*Source: Casey's conversation with Google Gemini about last-mile (C:\Users\casey\fromgoogle1.md, about 208 KB, not committed). It is mostly long architecture proposals with C++/CUDA "implementation blueprints". This note says what holds up against what we have measured, what doesn't, and what is worth a spike. Nothing from it has been adopted.*

## Verdict in one line

A few real hardware facts and three good ideas. Most of the rest is speculative architecture that contradicts last-mile's core rule (only decisions travel; the ProArt renders) or assumes hardware we don't have. The code was never compiled or run and should not be used.

## Holds up, and matters

| Claim | Check | What it means for last-mile |
|---|---|---|
| The Thor's "2,000+ TFLOPS" are FP4 AI tensor ops, not graphics FP32 | correct (NVIDIA's spec) | The Thor is an inference box, not a renderer. That matches our split: the ProArt renders, the Thor thinks and speaks |
| **MIG**: the GPU can be partitioned into isolated instances | **verified on our Thor**: `nvidia-smi` reports `mig.mode.current = Disabled` (supported, not on), JetPack 7.2.1 | **The most useful item.** We have measured GPU contention three times this week: decision refusals rose 8 -> 24 while the realizer worked; escalation latency doubled on a busy box (570 -> 986 ms); skald's Riva restarted under GPU pressure. A MIG partition for the game's runtime models (d1-2B, clef-flash, ears, realizer) apart from voice rendering and studio jobs would give the game a guaranteed slice. **Spike candidate** (see below) |
| Mouth and expression should follow the **audio sample clock**, not the frame tick | sound practice | Agrees with Vox Score §13 (tier 0 drives lip shapes from the voice's L0). Lock the face to the audio device's sample counter so voice and face never drift |
| Send the *edit*, not the result ("erasure fields") | consistent with our design | That is already last-mile's rule: deterministic lockstep means both sides apply the same decision. Nothing new to build |
| A cloud System Two that answers in slow bursts, with local voice smoothing the gaps | consistent | Matches skald's Lane C and our never-wait rule: a late director falls back, the voice never stutters |

## Doesn't hold, or contradicts the design

- **"NVLink to the ProArt" and "zero-copy DMA into the ProArt's VRAM":** a laptop on Wi-Fi or Ethernet has neither NVLink nor RDMA. Data reaches the ProArt over TCP/UDP. We measured 5-8 ms RTT over Wi-Fi.
- **Rendering on the Thor and streaming latents or G-buffers to the ProArt** (neural frame generation, latent G-buffers, 4D Gaussian splat streams, "Chrono-Tensor" speculative frames): this inverts the design. last-mile sends decisions (bytes), and each machine renders from the deterministic sim. Streaming latents costs orders of magnitude more bandwidth and breaks replay.
- **"The vector DB canon runs as a continuous matmul in the tensor cores":** this is a description of brute-force vector search. skald's Qdrant + GPU embeddings already do it.
- **The "wave equation continuum", "Lean-verified state engine" and "harmonic loop":** metaphor presented as engineering. No measurable claim to test.
- **Latency figures** such as "well under 50 ms per token" for voiced NPC replies: our measurements are 164 ms p50 for a move decision, about 570 ms for a wording, and about 640 ms or more to render a line in skald. Don't plan against unsourced numbers.
- **The C++/CUDA blueprints:** not compiled and not run. Treat them as illustration.

## Worth a spike

1. **MIG for the game's runtime models.** Measure the decision latency p50/p95 and the refusals with and without a dedicated instance, under a studio load. Enabling MIG needs a GPU reset and every model service restarted into its instance, so this is a planned maintenance window with Casey's OK, not a seat job.
2. **Audio-clock-locked lip shapes,** when the §13 engine tier exists. Small, and it falls out of Vox Score L0.

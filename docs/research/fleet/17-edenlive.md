# EdenLive — what it is, and whether it can be last-mile's "make it look better" renderer (2026-10-30)

Measured, not guessed. Sources: the VAST-AI/EdenLive tech report (35 pp., `assets.vast-eden.ai/assets/edenlive/EdenLive-tech-report.pdf`, read in full), the EdenLive repo and its `NOTICE` (Apache-2.0) at the trial's pinned commit, the Oct 27 Thor trial's own logs and artefacts, a fresh Thor measurement of a real last-mile restyle, the fleet notes [12](12-thor-models.md), [13](13-gemini-architecture-review.md) and [10](../forclaude2/10-piano-roll-and-edenlive.md), and NVIDIA/Black Forest Labs/Qwen model cards. Nothing was cloned, installed or moved; the one side effect is noted in Part 7.

Artefacts this note produced live in [`docs/notes/eden/`](../../notes/eden/).

---

## The verdict

**No — not as last-mile's live renderer, and not on the Thor. Yes — as an offline replay renderer, and yes — in the cloud at $0.0035 per second of video.**

1. **EdenLive is not a world model.** It never advances state. It is a *renderer*: it takes a proxy video (layout, camera, motion), a target first frame (the look) and a text prompt, and emits a photoreal video that follows the proxy. last-mile's simulation stays upstream and untouched. Coverage that calls it a "5B real-time world model" is wrong, and the distinction matters: the determinism argument for last-mile is **safe**, and the real risk is **readability**, not determinism.
2. **The "24.7 fps" number is an RTX 5090 number.** The tech report's own table is 1× H100 at **73.52 fps** (832×480) and **29.80 fps** (1280×704); the 5090 figure comes from the LightX2V path. The Thor measured **0.59 fps** on the same job — **~124× slower than the H100**, and ~40× short of 24 fps even counting only generation time.
3. **The Thor cannot reach those numbers by tuning.** The fast profiles are arch-locked to SM90/SM120; the Thor is SM110. SageAttention's CUDA kernels target SM80/86/89/90/120, FA4 is SM90-only, and LightX2V's `fp8-f16-accum` is an RTX 5090 (SM120) profile. The Thor has no SageAttention CUDA build, no FA4, and no legal quant profile in the published table.
4. **The licence is fine — the fleet doc has it wrong.** `12-thor-models.md` calls EdenLive "license unknown — gated trial, likely non-commercial" and "license unpromising for shipping". The actual per-component terms are Apache-2.0 for `dit-*`, `vae-*`, `umt5/`, `scheduler-*`, `decoder-*`; MIT for `clip/`; **DINOv3 License (Meta)** for `vision-encoder/`; and FLUX Non-Commercial only for `flux-c2f/`, the workbench's first-frame renderer, which the trial never used. **There is no non-commercial trap in the 41 GiB we actually own.** That correction is the single most actionable thing in this note.
5. **It cannot read last-mile's game.** EdenLive's own limitations section says it "leaves the mapping between reference objects and their intended identities implicit" and has "no explicit control over which levels of reference detail to preserve". last-mile's 2D map encodes identity in *colour* (orange diamond = agent, grey = rock, dark green = scrub). I measured a restyle of that map on the Thor: the map came back as an unreadable tiled texture and the agents were gone. The plain greybox is the readable layer; a generative restyle is the layer that destroys readability.
6. **But the quality is real, and it is spectacular.** The trial's 2-second output turned a flat-shaded greybox into a photoreal lava cavern with a glowing weapon. Layout was preserved (measured structure correlation 0.167 aligned vs 0.108 misaligned) and it was temporally stable. It is a genuinely good *offline* re-render.
7. **D14 was right, and this note proves it.** `design/decisions.md` D14 already reads: *"EdenLive-class per-frame neural rendering needs a 5090-class GPU."* The measurement agrees. Meanwhile `docs/superpowers/specs/2026-10-28-last-mile-m1-design.md` already books the slot: *"Out of M1 (planned later): … **EdenLive replays (spike first)**"*. That is where this belongs — and the spike has now happened.

---

## Part 1 — What EdenLive actually is

### 1.1 It is a renderer, not a simulator

EdenLive conditions on three inputs (tech report §3.1):

| Input | What it is | What it controls |
|---|---|---|
| **Reference video** | a *proxy video* — a simplified rendering by a coding agent, game engine or workbench that carries scene layout, camera motion and coarse dynamics | geometry and motion |
| **Target first frame** | one image that sets the *look* | appearance |
| **Text prompt** | a static scene sentence, plus timestamped dynamic sentences | style, and per-event semantics |

It emits RGB frames at 480p or 720p and 24 fps. The upstream engine still owns the world state and advances it; EdenLive only renders. That is the "decouple the evolution of the world state from its visual rendering" principle from Project Eden, and it is exactly last-mile's own split (D10: only decisions travel; D14: the Thor pre-renders and the 4050 composites).

Internally: a **fine-tuned DINOv3** encoder turns the reference video into a dense control feature grid; a **Wan2.1/Wan2.2 video DiT** (5B or 14B) generates latents; **UMT5** encodes the text; a **Wan VAE** (offline) or **LightTAE/TAEHV** (real-time) decodes to pixels. The streaming variant is distilled to **four denoising steps per chunk** with a bounded KV cache (18 latent frames, 6 of them a permanent attention sink) and no classifier-free guidance.

### 1.2 Three different products hide under one name

This is the trap in the whole question, and the trial measured the wrong one:

| Path | What it is | Hardware | Has the shelf subset? |
|---|---|---|---|
| `edenlive/` — **offline batch** | the `python -m edenlive.scripts.infer` path the Oct 27 trial ran | any CUDA box | ✅ yes |
| `realtime/` — **real-time serving** | FlashDreams-based WebRTC service; the real "real-time renderer" | GPU + streaming | ❌ **needs `decoder-5b` (LightTAE), which we do not have** |
| **LightX2V** | the quantised inference stack behind the 24.7 fps claim | SM90 / SM120 | ❌ different weights + kernels |

`realtime/deploy.conf` lists the tree it reads: `dit-5b/`, `vae-5b/`, `decoder-5b/`, `umt5/`, `clip/`, `vision-encoder/`, `flux-c2f/`. Our 41 GiB shelf set is **dit-5b, vae-5b, umt5, scheduler-5b, vision-encoder** — the offline set. **The real-time path cannot run from what we own**, and its fast path additionally needs `decoder-5b` plus the LightX2V kernel stack.

### 1.3 Measured performance (tech report §5.4, H100)

Tables 2 and 3, after warmup, timed *from model-side condition processing to decoded frames in CPU memory* — proxy rendering, offline preprocessing, video-file encoding and network transmission are **outside** the timed interval.

**Table 2 — inference throughput on NVIDIA H100**

| Method | Resolution | H100 GPUs | Latent frames/chunk | FPS |
|---|---|---|---|---|
| **EdenLive 5B** | 832×480 | 1 | 3 | **73.52** |
| **EdenLive 5B** | 1280×704 | 1 | 3 | **29.80** |
| EdenLive 14B | 832×480 | 8 | 3 | 26.76 |
| AgentGarten | 832×480 | 1 | 4 | 36.14 |
| StreamRender-H3 | 1344×768 | 8 | 9→3/2 | 2.56 |
| StreamCWM | 1344×768 | 7+1 | 5 | 1.69 |

**Table 3 — latency, throughput, memory**

| Metric | 5B (1×H100) 832×480 | 5B (1×H100) 1280×704 | 14B (8×H100) 832×480 |
|---|---|---|---|
| Generation throughput (FPS) | 80.68 | 31.04 | 28.81 |
| Inference throughput (FPS) | 73.52 | 29.80 | 26.76 |
| **First-frame latency (ms)** | **826.36** | 1161.79 | 1714.00 |
| **First-chunk latency (ms)** | **826.38** | 1161.81 | 1714.02 |
| **Inter-chunk gap P95 (ms)** | **148.24** | 391.54 | 429.90 |
| RTF | 0.2172 | 0.5359 | 0.5966 |
| Peak allocated (GiB) | 28.53 | 36.78 | 50.83 |
| Peak reserved (GiB) | 29.34 | 38.94 | 60.67 |

Two facts from the report that are load-bearing for last-mile:

- **The serving pipeline has a one-chunk lookahead, so "input-to-display delay ≈ two chunk durations."** One chunk is 3 latent frames = 12 display frames = **0.5 s of video at 24 fps**. Two chunks = **1.0 s of video** of added latency, minimum, on their hardware.
- **HUD text and numbers are composited over the generated video, not generated by it** (§3.2): "exact text and numerical indicators can be rendered directly from 𝑠_t and composited over the generated video." This is a gift to last-mile — the HUD, the piano roll and the tick counters stay exact and crisp, and the neural layer only carries the picture.
- The **event-sentence text condition** is the same shape as last-mile's piano roll: the program emits a short sentence per salient event, the sentence is appended to the static scene prompt, and the updated embeddings replace only the cross-attention KV cache.

---

## Part 2 — What the Oct 27 Thor trial actually measured

The trial (`~/eden-trial`, 2026-10-27) ran the offline path on the Thor: 49 frames at 832×480, 24 fps, `wan22_5b_720p_few_step.yaml`, the bundled `cryo-crossing` sample as the proxy. 49 frames = **2.04 s of video**.

### 2.1 The numbers, re-derived from the logs

| | run1 | run2 (rc=0, completed) |
|---|---|---|
| Wall time | 921.6 s | **442.5 s** |
| Child peak RSS | 30.92 GiB | **29.12 GiB** |
| Pre-generation phase | 594 s | **360 s (81% of the run)** |
| Generation + decode + save | 325.6 s | **82.5 s** |
| Seconds per frame (generation) | 6.64 | **1.68** |
| **FPS (generation only)** | 0.150 | **0.594** |
| FPS (end-to-end) | 0.053 | 0.111 |
| RTF vs real time (generation) | 160× | **40×** |

**The split matters more than the total.** Of run2's 442.5 s, only 82.5 s was generation. The other 360 s was reading weights off the cold USB shelf: the transformer reloaded in **84 s on run1 and 2 s on run2** — the signature of a cold page cache over a spinning USB NTFS disk. **A first spike on the Thor pays ~6–8 minutes of shelf read before the first frame.**

The run2 generation phase covers 5 chunks (12,12,12,12,1 frames) × 4 denoising steps = 20 steps in 82.5 s → **4.1 s per denoising step**.

Run1 is 4× slower than run2 in the generation phase alone. The most likely cause is first-call Triton JIT plus a cold page cache on the transformer; I could not separate the two from the logs alone.

Note also that **the trial's peak RSS (29.12 GiB) matches the H100's peak reserved memory (29.34 GiB) almost exactly.** Memory was never the constraint. Time was.

### 2.2 What it produced — the audit nobody had done

I extracted all 49 output frames and the 13-frame 6 fps proxy and compared them.

**Quality: genuinely excellent.** See [`compare-trial-proxy-vs-edenlive.png`](../../notes/eden/compare-trial-proxy-vs-edenlive.png). The flat-shaded greybox — orange lava plane, purple basalt walls, a cyan-and-white crate — became a photoreal lava cavern with a first-person cryo-blaster, glowing embers, heat haze and wet rock. For a 5B model at 4 steps that is a startling result, and it is the honest face of EdenLive.

**Layout: preserved, but only coarsely.** Measured on gradient maps (256×141), correlated proxy frame *k* against output frame *j*:

| | mean r |
|---|---|
| aligned (j = 4k) | **0.167** (n=13, min 0.095) |
| misaligned (|j − 4k| > 6) | 0.108 (n=484, sd 0.073) |

Separation is real but modest, and the absolute correlation is low because appearance is deliberately replaced (§4.2.2 of the report uses domain-adversarial training to suppress the reference's own appearance; the look comes from the first frame). The arch, the floor and the obstacles stay in place. Where the proxy camera moves fast — proxy frame 12 — the correspondence collapses entirely (r ≈ −0.02).

**Identity: invented.** The proxy's cyan-and-white crate became a *hand-held weapon* in first person. The model decided what the object was. Nothing in the conditioning told it to. This is the report's own first limitation, made visible.

**Temporal stability: good.** Four consecutive frames show no strobing or drift; the extra high-frequency energy in the output (12.96 vs the proxy's 2.56) is animated lava, embers and glow — content that is *added*, not flicker. See [`compare-edenlive-consecutive-frames.png`](../../notes/eden/compare-edenlive-consecutive-frames.png).

---

## Part 3 — Why the Thor cannot reach the vendor's numbers

### 3.1 The architecture gap is the wall, not the FLOPs

| | EdenLive's fast path | This Thor |
|---|---|---|
| Architecture | Blackwell **SM120** (RTX 5090) / Hopper **SM90** (H100) | Blackwell **SM110** |
| Attention | **FlashAttention-4** (SM90-only), SageAttention 2 | **neither available** — PyTorch SDPA fallback |
| Quant profile | `fp8-f16-accum` (RTX 5090), `fp8-sgl`/`int8-sgl` (H100/H800, RTX 40) | **no profile in LightX2V's table lists SM110** |

SageAttention's upstream CUDA extensions target **SM80/SM86/SM89/SM90/SM120**; its prebuilt wheels are x86-64 only; the Triton path (which would be portable) is the slower v1-class kernel. The EdenLive README states the default 5B bidirectional profile "uses FA4 temporal text attention and **requires SM90**, BF16, flash-attn-4==4.0.0b31, and nvidia-cutlass-dsl==4.7.1."

**The Thor is SM110. There is no FA4, no SageAttention CUDA kernel and no legal quant profile for it.** Every fast path in EdenLive's stack is arch-gated off the Thor.

### 3.2 The hardware gap, measured

| | RTX 5090 | This Thor (measured / spec) |
|---|---|---|
| Memory bandwidth | 1792 GB/s | **273 GB/s** peak spec (LPDDR5X, 256-bit @ 4266 MHz) |
| Achievable copy bandwidth | — | **97–106 GB/s** measured on this box, under load |
| FP32 | ~105 TFLOPS | **8.06 TFLOPS** max |
| FP16 dense | — | ~258 TFLOPS |
| FP8 dense | — | 517 TFLOPS |
| GPU | 21 760 CUDA cores | 2560 CUDA cores, 20 SMs, 1.57 GHz |

*(Thor copy bandwidth measured in-place with `torch.cuda` on the trial venv: 4 GiB device-to-device copy at 82.2 ms → 97.3 GB/s effective, 1 GiB at 18.9 ms → 105.7 GB/s. The GPU was at 96–98% utilisation throughout from the five live last-mile services, so this is a floor, not the ceiling.)*

At batch 1 a video DiT step is bandwidth-bound, and the 5090's advantage over the Thor's *peak* is already 6.6×; against what the Thor actually achieves while serving the live game, it is ~18×.

### 3.3 The ceiling on the Thor

Even a perfect Thor build — `torch.compile` + CUDA-graph capture (both disabled in the trial's config), SageAttention-Triton, an FP8 or INT8 path, a dedicated GPU — would have to close a **40× gap** in generation throughput to reach 24 fps at 832×480. A generous 5–8× from optimisation leaves it **5–8× short**. At 24 fps the Thor would need a chunk (12 frames) in 500 ms; the trial took 20 s for the same 12 frames.

**Verdict: the Thor is an EdenLive *offline* box. It is not an EdenLive real-time box and will not become one.**

---

## Part 4 — Licence, and the correction the fleet docs need

From `~/eden-trial/EdenLive/NOTICE` (Apache-2.0 code) and the model card, verbatim in substance:

| Component | Licence | Commercial? | In our 41 GiB? |
|---|---|---|---|
| `dit-5b/`, `dit-14b/` | Apache-2.0 (derived from Wan2.1/2.2) | ✅ yes | ✅ dit-5b |
| `vae-5b/`, `vae-14b/` | Apache-2.0 | ✅ yes | ✅ vae-5b |
| `umt5/` (text encoder + tokenizer) | Apache-2.0 | ✅ yes | ✅ yes |
| `scheduler-5b/` | Apache-2.0 | ✅ yes | ✅ yes |
| `decoder-5b/`, `decoder-14b/` | Apache-2.0 (LightX2V LightTAE) | ✅ yes | ❌ **not downloaded** |
| `clip/` | MIT (LAION CLIP ViT-H/14 via OpenCLIP) | ✅ yes | ❌ not downloaded |
| `vision-encoder/` | **DINOv3 License (Meta Platforms, Inc.)** | ⚠️ see below | ✅ yes |
| `flux-c2f/` | **FLUX Non-Commercial License v2.1** (FLUX.2 [klein] **9B** + a Vast LoRA) | ❌ **no** | ❌ not downloaded |

Plus, from the same NOTICE: the `realtime/` tree is a trimmed fork of NVIDIA FlashDreams (Apache-2.0), and `edenlive/` builds on Wan2.1/2.2 and Hugging Face Diffusers (both Apache-2.0), redistributing neither.

**On DINOv3.** The DINOv3 licence is a royalty-free, worldwide, non-exclusive, non-transferable licence to use, reproduce, distribute, copy, make derivative works of and modify — **no commercial-use prohibition**. Its obligations: redistribute under the same terms, display "Built with DINOv3", acknowledge DINOv3 in publications, comply with Trade Controls and a prohibited-use list, and acknowledge DINOv3 in research. **The exact `vision-encoder/` licence file shipped with the weights must be read before shipping** — I could not, because the shelf is not mounted (Part 7).

**On `flux-c2f/`.** Non-commercial, non-production, and the NOTICE says "public end-user deployment may require separate authorization from Black Forest Labs". **But it is only the workbench's first-frame renderer** — it makes the "look" image for the real-time service. We do not need it: we supply our own first frame (in the measurement below, a SDXL restyle). **Its non-commercial licence never touches the 41 GiB we own.**

### 4.1 What must be corrected in the fleet docs

`12-thor-models.md` says three things that are now wrong:

| Line | Says | Should say |
|---|---|---|
| 91 | "license unknown — gated trial, likely non-commercial" | **Apache-2.0** for dit/vae/umt5/scheduler; **MIT** for clip; **DINOv3 License** for vision-encoder. Nothing non-commercial in the set we hold. |
| 140 | "trial concluded; license unpromising for shipping" | The licence was never the problem. The **speed** was: 0.59 fps on the Thor. |
| 215 | "EdenLive (unknown/gated)" in the licence-tripwire list | **Remove from the tripwire list.** The only NC component (`flux-c2f`) is not on disk. |

And `10-piano-roll-and-edenlive.md` left two open items. Both are now closed:
- *"Spike EdenLive 5B few-step on the Thor"* → **done** 2026-10-27. 0.59 fps at 832×480; 442.5 s for 49 frames; peak RSS 29.12 GiB.
- *"Read the tech report before the spike"* → **done** for this note (35 pp., read in full). Its prediction — *"Real-time 24 fps on the Thor is unlikely"* — **holds, and by a wider margin than "unlikely": it is 40× short.**

---

## Part 5 — Fit for last-mile, tier by tier

### 5.1 What last-mile already decided

- **D10** — only decisions travel; deterministic lockstep sim on every node.
- **D11 / D13** — the sim never waits; agents perceive the score, never pixels.
- **D14** — *"The Thor pre-renders ahead and the 4050 composites: GI baked into vertex colours, neural vistas, impostor sprites, generated textures; progressive chunk fidelity L0–L3."* Rationale: *"**EdenLive-class per-frame neural rendering needs a 5090-class GPU**."*
- **M1 design §10** — *"Out of M1 (planned later): … **EdenLive replays (spike first)**."*

D14's pre-render-ahead shape means a slow renderer is *architecturally allowed*, provided L0 (the plain game) is always available to composite. So the tiers below are about **how far ahead** you can afford to render, not about whether a neural renderer may exist.

### 5.2 Tier A — restyle the live gameplay view · **NO**

- Throughput: 0.59 fps vs 24 fps needed = **40× short** (generation only).
- Latency: the first chunk (12 frames) takes **20 s**; two chunks of lookahead would be **40 s**. The never-wait rule tolerates a late *decision*, not a late *picture* — the picture is the game.
- Readability: the model invents object identity and cannot be told to preserve it. In the 2D map that means the agents stop being agents.
- **Even on a 5090 this tier would be a bad trade**: the tech report's own input-to-display delay is ~1.0 s of video (two chunk durations), and the identity ambiguity is unchanged by hardware.

### 5.3 Tier B — pre-render-ahead for the live round (D14's L1–L3) · **NO on the Thor**

The arithmetic kills it. A round of T seconds of video needs T × 40 s of Thor time at the measured rate (RTF 40×, i.e. weights already loaded; add ~6 min of shelf read for the first run):

| Video to pre-render | Thor time needed (warm) | Verdict |
|---|---|---|
| 2 s (one chunk) | 80 s | fine |
| 30 s (a title card / interstitial) | 20 min | fine |
| 60 s | 40 min | borderline |
| 10 min (a round) | **6.7 hours** | ✗ |

The Thor can pre-render *short* neural passages ahead of time. It cannot cover a round. And the read-ahead window shrinks every time the live services load the GPU — the measurement above was taken at 98% utilisation.

### 5.4 Tier C — offline replay re-render · **YES**

This is the M1 spec's slot, and it is the one the evidence supports.

- The sim is deterministic and replayable from the decision stream (D10, D69), so a *recorded* round can be re-rendered after the fact, on any machine, with no latency constraint.
- The quality is outstanding (Part 2.2) and the layout survives well enough for a replay nobody is reading for gameplay facts.
- Cost on the Thor: 442.5 s per 2.04 s of video is fine for offline work — a 60-second replay is **~40 minutes** of generation (plus the ~6-minute shelf read on the first run), or **~20 seconds** on a dedicated 5090-class box.
- The licence is clean for it.
- The artifact to build is a **replay pipeline**, not a live renderer: recorded proxy video → EdenLive → an mp4 the ProArt can play back beside the live view.

**The one hard requirement for this tier: the real-time path needs `decoder-5b`.** The offline path (which is what the trial ran) does not, but the offline path is batch-only. If we want the *real-time* service for replays, we must also fetch `decoder-5b` (and `clip/`).

### 5.5 Tier D — cloud, real-time · **YES, at a price**

fal.ai hosts VAST's own endpoints, and they are the real-time ones:

- `vast/eden-live-5b-video` — **$0.0035 per second of video generated**, minimum $0.035 per session.
- `vast/eden-live-5b-game` — *"Real-time games rendered using VAST's Eden Live 5B"* — a **WebRTC realtime contract** (`fal.realtime.open`), which is the shape last-mile already streams in.

| Usage | Cost |
|---|---|
| 2 s (one chunk) | $0.007 |
| 60 s (an interstitial) | $0.21 |
| 10 min (a round) | $2.10 |
| 30 min | $6.30 |
| 1 hour | $12.60 |

This is the only path that gives last-mile EdenLive-class rendering **at 24 fps**, and the price is not the obstacle. The obstacles are: (a) WAN round-trip plus queue on top of the ~1.0 s two-chunk display delay — plausibly 1.5–3 s, which the never-wait rule can absorb *only* because L0 is always there to composite; (b) the model is a third party seeing last-mile's game; (c) the fal endpoint's own terms are a separate licence from the open weights and would need their own review before shipping.

---

## Part 6 — The alternatives already on the box

Measured on the Thor at 1024×1024, from `~/image-studio/README.md` (fast lane, 2026-10-23; quiet-window figures), plus the licence column from `~/image-studio/MODELS-MANIFEST.tsv`:

| Model | Steps | Warm s/img | Cold (time-to-first) | Licence | Cap-free? |
|---|---|---|---|---|---|
| Hyper-SD 1-step LoRA | 1 | **1.5** | 7.6 s | ByteDance Hyper-SD | ✅ |
| SDXL-Turbo | 1 | **1.51** | 7.06 s | Stability AI Community | ❌ |
| SDXL Lightning 4-step LoRA | 4 | **2.5** | 5.6 s | OpenRAIL++-M | ❌ |
| LCM-LoRA SDXL | 6 | 3.0 | 6.6 s | OpenRAIL++-M | ❌ |
| SDXL Lightning 8-step LoRA | 8 | 3.5 | 6.6 s | OpenRAIL++-M | ❌ |
| **SDXL base (the keeper)** | 30 | **11.2** | 13.1 s | CreativeML OpenRAIL++-M | ❌ (use-based) |
| **FLUX.2 [klein] 4B** | — | on disk (7.75 GB), 13 GB VRAM, sub-second on a 3090 | — | **Apache-2.0** | ✅ |
| Qwen-Image-Edit (20B) | — | on disk (40.9 GB) | — | **Apache-2.0** | ✅ |
| Z-Image-Turbo NVFP4 | 8 | on disk (4.2 GB) | — | Apache-2.0 | ✅ (Thor-native FP4) |

Two corrections for the fleet docs here as well: **FLUX.2 [klein] 4B is Apache-2.0** ("Open weights available for commercial use under the Apache 2.0 license"), not the Kommunity/<$5 M licence that `12-thor-models.md` lists for the FLUX.1/3 family — it is the **9B** variant that is non-commercial, and that is the one EdenLive's `flux-c2f/` ships. And the `image-studio` README's model table is stale in the other direction: it says FLUX.2 klein and Qwen-Image-Edit are "not resident yet", but both are on disk now.

### 6.1 My measurement: one real restyle of last-mile on the Thor

Per the brief — measure one restyle of `docs/notes/accept-3d.png` and `accept-2d.png`: wall time and memory.

**Recipe:** ComfyUI 0.42.0 on the Thor, `sd_xl_base_1.0.safetensors` + `cn-depth-sdxl.safetensors` + `DepthAnythingV2Preprocessor` (depth lock, strength 0.85) → KSampler 30 steps, dpmpp_2m/karras, cfg 7 → 832×480. Input scaled from 2880×1620. Outputs: [`accept-3d-depth-sdxl.png`](../../notes/eden/accept-3d-depth-sdxl.png), [`accept-2d-depth-sdxl.png`](../../notes/eden/accept-2d-depth-sdxl.png).

| | accept-3d | accept-2d |
|---|---|---|
| Wall time | **198.8 s (cold)** | **53.6 s (warm)** |
| Peak ComfyUI GPU memory | 10 595 MiB (baseline 102 → **+10.2 GiB**) | 11 755 MiB (baseline 9 347 → **+2.4 GiB**) |
| Sampler time alone | 22 s (30 steps, 1.34 s/it) | — |

The cold run includes SDXL + ControlNet + depth-model load; the warm run is the honest per-frame cost. Note the Thor was at 96–98% utilisation from the five live last-mile services throughout — the quiet-window SDXL baseline is 11.2 s at 1024×1024, so contention cost roughly 5×. **Plan for ~54 s per restyled frame on a contended Thor, or ~11–20 s on a quiet one.**

**And here is the finding that matters.** See [`compare-plain-vs-restyle-thor.png`](../../notes/eden/compare-plain-vs-restyle-thor.png).

- **3D view:** the greybox became a textured, lit scene — mossy blocks stayed put (cn-depth did its job), the floor got sand and a rock border, the sky went white. **But the orange agent characters were gone**: the model read them as *torches* and rendered them as flames on top of the blocks. The most gameplay-critical objects in the frame were re-imagined as scenery.
- **2D map view:** catastrophic. The sparse top-down map came back as a **dense repeating tiled texture**; the grey rocks, the dark scrub and the yellow agents all dissolved into it. The map is unreadable. The HUD text was also restyled into illegible pseudo-lettering.

This is the same failure mode as EdenLive's identity ambiguity, and worse because a still-image model has no temporal context to stabilise it. The mechanism is a resolution floor: at 832×480 the map's 1–2 px scrub speckle and small agent marks are sub-pixel, and the model replaces the region with plausible texture. **A generative restyle destroys exactly the information last-mile's game is made of.**

**The comparison that settles it:**

| | cost per frame on the Thor | preserves the gameplay reading? |
|---|---|---|
| EdenLive 5B (5B DiT, 4 steps, 12 frames/chunk) | 1.68 s/frame, but video-wide and temporally coherent | ✗ identity invented; no control knob to ask otherwise |
| SDXL + cn-depth, one frame | 54 s contended / ~11–20 s quiet | ✗ characters lost, map unreadable |
| **The plain greybox** | **0 s** | **✅ it *is* the reading** |

The plain game is not a degraded fallback for the neural version. **It is the only version that carries the game's meaning**, and D14 already says the Thor pre-renders *ahead* and the 4050 composites. The right build is: plain L0 always; neural L1–L3 for passages that do not need to be read (interstitials, the pre-round intro, replays), pre-rendered with hours of lead time, on hardware that can afford it.

---

## Part 7 — What I could not verify, and side effects

**Not verified — the weights were unreachable.** `~/eden-trial/models/EdenLive` is a symlink to `/media/caseydigennaro/TOSHIBA EXT/cold-shelf/...`. **The external drive is not mounted**: the box rebooted at 00:59 on 2026-10-30 and udisks cleaned up the mount point; `readlink -f` on the path returns nothing. Mounting it (`udisksctl mount`, `sudo mount`) is blocked by this session's permission gate, which cannot prompt for approval in a headless run. **I did not read the per-component licence files inside `vision-encoder/`, and I could not measure the real-time path or re-run the offline path.** The attempt is recorded, and it fails exactly where it should:

```
FileNotFoundError: model.components.tokenizer resolves to
  models/EdenLive/umt5/tokenizer
which does not exist. ... rc=0 end=11:15:28UTC     # 25 s to fail
```

**Not verified:** the `realtime/` WebRTC service end to end; LightX2V's actual fps on any GPU (I relied on the published ComfyUI-5090 numbers); the tech report's remaining evaluation sections (long-horizon comparisons, ablations beyond the encoder table); whether the DINOv3 licence file shipped in `vision-encoder/` differs from Meta's published text.

**Side effect to disclose:** `comfyui_controlnet_aux` looked for its depth checkpoint at `ckpts/depth-anything/Depth-Anything-V2-Large/` and, not finding the local copy there, **downloaded it from Hugging Face** (~1.3 GB) into the ComfyUI ckpts directory during my first measurement. I had symlinked the existing `~/models/depth-anything/depth_anything_v2_vitl.pth` to the flat path the node also accepts; the sub-path it preferred was already on disk but in the wrong place. Nothing was installed and no project file was moved; the download is a duplicate of a file the box already had. **Note the licence flag on that weight** — `12-thor-models.md` marks Depth Anything V2 ViT-L as "Apache-2.0 (weights CC-BY-NC for large variants — ViT-L ckpt here predates the split; **re-verify before shipping**)".

**Also carried over from the trial:** `decord==0.6.0` has no aarch64 wheel, so the trial's video-io step failed and **decord is still missing** on the Thor. The render path itself completed (run2 wrote an mp4) and PyAV 17.1.0 is installed, but a production path should not depend on decord.

**On the note in the brief about `:8017`:** confirmed — `~/eden-trial/golem/harbourmaster/serve_step.py` is the EdenGolem prototype viewer (stdlib `http.server` with a `/vendor/` map for three.js and rapier) and its `--port` default is **8017**. It serves the Harbourmaster greybox the trial's zcode agent built; all four stages passed (intake, scene, behavior, acceptance), WON at 28.3 s, LOST at 6.8 s. That game is a *proxy generator*, which is precisely the role EdenLive wants — and precisely the role last-mile's own ProArt already plays, better and deterministically.

---

## Part 8 — What to do with the 41 GiB

**Keep it cold. Do not delete it.**

`12-thor-models.md` proposed deleting it, on the rationale "license unpromising for shipping". That rationale is false (Part 4). The reasons to keep it:

1. It is the **only** copy of the offline EdenLive path, and Tier C (replay re-render) is the one tier the evidence supports and the M1 spec already books.
2. It is **already cold** — the shelf is the right tier for a 41 GiB set used a handful of times a year. Nothing changes.
3. Deleting it costs one `rsync` of 41 GiB to re-fetch. Keeping it costs one directory on a 2 TB disk.

**Two things to fix in the fleet docs:** the licence claim (Part 4.1) and the "EdenLive (unknown/gated)" tripwire entry. Both are one-line edits.

**What to build next, in order:**
1. **The replay spike** the M1 spec asks for: record a last-mile round, build the proxy video from it (I have a working scaffold in `docs/notes/eden/last-mile-3d/` — recording.mp4, prompts.json, manifest.csv, and a cover.png built from the SDXL restyle), and re-render it. **Do this on a 5090-class box, not the Thor.**
2. **Decide the cloud question early**, because the cloud real-time game endpoint is the only path to live 24 fps, and it is $2 for a round.
3. **Do not** spend more Thor time on the live path. The 40× gap is architectural, not a tuning problem.

---

## Sources

**Primary**
- EdenLive tech report — "EdenLive: Real-Time Generative Rendering from Any Visual Reference", VAST AI, 35 pp., `https://assets.vast-eden.ai/assets/edenlive/EdenLive-tech-report.pdf` (read in full; §§3.2, 4.2.2, 4.4, 5.4, 5.5, 7)
- EdenLive repo + `NOTICE` + `LICENSE` (Apache-2.0) at `~/eden-trial/EdenLive`, commit `0b78d21`; `edenlive/configs/wan22_5b_720p_few_step.yaml`, `realtime/deploy.conf`, `examples/samples/manifest.csv`, `examples/samples/cryo-crossing/prompts.json`
- Oct 27 Thor trial: `~/eden-trial/renders/run1.log`, `run2.log`, `run2/save/inference/base/{resolved.yaml,input.json,cryo-crossing.mp4,cryo-crossing_proxy.mp4}`, `~/eden-trial/golem/seat-log.md`, `~/eden-trial/run_render.sh`

**Measured on this box, 2026-10-30**
- `nvidia-smi`; `torch.cuda.get_device_properties` → NVIDIA Thor, capability **11.0**, 122.8 GiB, 20 SMs; `torch.cuda.mem_get_info`; device-to-device copy bandwidth 97.3–105.7 GB/s under load
- Thor restyle measurement: ComfyUI 0.42.0 + SDXL base + `cn-depth-sdxl` + DepthAnythingV2, 832×480, 30 steps — 198.8 s cold / 53.6 s warm, peak 10 595 / 11 755 MiB. Outputs and scripts in `docs/notes/eden/`
- Trial-output audit: 49 output frames + 13 proxy frames extracted with PyAV; gradient-map correlation aligned 0.167 vs misaligned 0.108 (n=484, sd 0.073); high-frequency energy 12.96 vs 2.56

**Model cards and licences**
- `https://huggingface.co/VAST-AI/EdenLive` (model card; weights distributed per component)
- `https://huggingface.co/black-forest-labs/FLUX.2-klein-4B` — Apache-2.0 (9B is non-commercial)
- `https://huggingface.co/Qwen/Qwen-Image-Edit` — Apache-2.0
- `https://huggingface.co/lightx2v/Autoencoders` — LightTAE, Apache-2.0
- Meta DINOv3 License Agreement
- `https://fal.ai/models/vast/eden-live-5b-video` — $0.0035/s, $0.035 minimum; `vast/eden-live-5b-game` — WebRTC realtime

**Hardware**
- NVIDIA Jetson Thor / T5000: 128 GB 256-bit LPDDR5X @ 273 GB/s; 2070 TFLOPS sparse FP4; 8.064 TFLOPS FP32; `https://developer.nvidia.com/blog/introducing-nvidia-jetson-thor-the-ultimate-platform-for-physical-ai/`
- SageAttention upstream arch support (SM80/86/89/90/120); `https://github.com/thu-ml/SageAttention`
- LightX2V quantisation matrix and the ComfyUI-5090 profile measurements (16.8 fps int8+SageAttention, 11.0 fps bf16+SageAttention, 7.9 fps plain PyTorch kernels, 21.1–25.9 GiB peak GPU)

**Inherited last-mile context**
- `design/decisions.md` — D10, D11, D13, D14, D18, D69
- `docs/superpowers/specs/2026-10-28-last-mile-m1-design.md` §10
- `docs/research/fleet/12-thor-models.md`, `13-gemini-architecture-review.md`, `docs/research/forclaude2/10-piano-roll-and-edenlive.md`
- `~/image-studio/README.md`, `~/image-studio/MODELS-MANIFEST.tsv`

---

## Appendix — artefacts this note produced

Under `docs/notes/eden/`:

| File | What it is |
|---|---|
| `compare-trial-proxy-vs-edenlive.png` | trial proxy vs EdenLive output, 3 timepoints |
| `compare-edenlive-consecutive-frames.png` | 4 consecutive output frames (temporal stability) |
| `compare-plain-vs-restyle-thor.png` | last-mile plain vs my Thor SDXL+cn-depth restyle |
| `accept-3d-depth-sdxl.png`, `accept-2d-depth-sdxl.png` | the measured restyles |
| `last-mile-3d/recording.mp4` | 49-frame, 1280×704, 24 fps proxy for last-mile's 3D view (slow push-in over `accept-3d.png`, HUD cropped off) |
| `last-mile-3d/cover.png` | the "target first frame" — the SDXL restyle, as the look EdenLive would propagate |
| `last-mile-3d/prompts.json`, `prompts.txt` | static + timestamped event prompts in EdenLive's own format |
| `last-mile-3d/manifest.csv`, `manifest.csv` | the manifest row that points at them |
| `last-mile-3d/proxy-frame-000.png` | the cropped proxy frame |

With the shelf mounted, the last-mile proxy re-render is one command:

```bash
cd ~/eden-trial/EdenLive && ~/eden-trial/venv/bin/python -m edenlive.scripts.infer \
  --config edenlive/configs/wan22_5b_720p_few_step.yaml \
  data.dataset.params.path=<repo>/docs/notes/eden/manifest.csv \
  data.dataset.params.root=<repo>/docs/notes/eden \
  data.dataset.params.max_frames=49 inference.num_frames=49 \
  inference.max_batches=1 inference.width=832 inference.height=480 \
  inference.output_dir=<out>
```

On this Thor that takes ~442 s (Part 2.1). On a 5090-class box, seconds.

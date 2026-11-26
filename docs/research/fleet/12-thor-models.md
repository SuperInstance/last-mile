# The Thor's model inventory (hot, warm, cold) — and what last-mile should add (2026-10-29)

Measured, not guessed: `~/lm-services/last-mile.sh status`, `sudo ss -ltnp`, `nvidia-smi`, `systemctl --user`, `sudo docker images` / `docker inspect`, `du` over `/`, `~`, and the whole external drive, plus a file-level census of every weight file > 200 MB under `/home` (`*.gguf *.safetensors *.nemo *.onnx *.engine *.plan *.pt *.ckpt *.bin`). Machine-readable companion: [thor-models-2026-10-29.json](thor-models-2026-10-29.json). Nothing was deleted or moved; Part 1's proposals are proposals.

Drives: **NVMe** `/` 936 GB, 655 GB used, 234 GB free (the warm tier). **USB HDD** `/media/caseydigennaro/TOSHIBA EXT` 1.9 TB, 378 GB used (the cold tier, spinning, slow to read). Memory: 122 GiB unified, 45 GiB used, 76 GiB available at measure time (`last-mile.sh status` reports "memory available: 75 GiB").

---

## Part 1 — the inventory

### Hot (loaded and serving now)

Five `llama-server` processes hold **19.2 GiB of GPU memory** in total (nvidia-smi, MiB): plus the Riva/Triton speech server inside the `skald-riva` container (its allocation is not itemized by `nvidia-smi` on L4T) and the GNOME desktop (~0.15 GiB).

| Model | Path | Disk | GPU | Port / service | Job | Quant / format | License | last-mile uses it |
|---|---|---|---|---|---|---|---|---|
| d1-2B | `~/models/d1-2B` | 2.0 GiB | 2516 MiB | 8094 `d1-2b.service` | System One reflex decisions (D62 default fast tier) | Q8_0 GGUF + mmproj | LFM Open License v1.0 (commercial only under $10 M revenue) | **yes** |
| clef-flash | `~/models/clef-flash` | 7.2 GiB | 7590 MiB | 8096 `clef-flash.service` | decision escalation champion (D71 second opinion) | Q4_K_M GGUF + BF16 mmproj | Apache-2.0 (fleet doc 04; community GGUF port of Cloudflare clef-flash) | **yes** |
| LFM2-1.2B | `~/models/LFM2-1.2B` | 1.2 GiB | 1476 MiB | 8092 `lm-realizer.service` | wording realizer for agent lines | Q8_0 GGUF | LFM Open License v1.0 | **yes** |
| LFM3-Audio-2B | `~/models/LFM3-Audio-2B` | 2.1 GiB | 3719 MiB | 8103 llama-server behind `lm-ears` :8102 | speech-to-text + intent probe | Q8_0 GGUF + mmproj | LFM Open License v1.0 | **yes** |
| d1-3B | `~/models/d1-3B` | 3.3 GiB | 3930 MiB | 8093 `d1-3b.service` | decision model; index says "slower than d1-2B and no better here"; **not in last-mile's port map** | Q8_0 GGUF + mmproj | LFM Open License v1.0 | no |
| Riva Magpie TTS Multilingual 357M | `~/src/skald-riva/model-repo-official` (bind-mounted at `/data/models`) | 4.1 GiB repo | in container | 50000/50051 → skald :8000 `skald-daemon.service` | TTS: agent voices via skald (D73 pre-rendered bank) | RMIR/Triton repo (encoder.plan/.onnx + model.ckpt) | NVIDIA Riva/NGC terms; **production deployment requires NVIDIA AI Enterprise** (dev free) | **yes** |
| (serving infra) riva-speech:2.26.0-l4t-aarch64 | docker image, running as `skald-riva` | 32.2 GiB | — | 50000/50051 | the Riva/Triton server itself | docker | NVIDIA container license | (hosts the above) |

Services defined but **down** at measure time (the Thor's own index at :8100 confirms): d1-omni-600M :8095, decision tier :8097, capture :8098, geometry (MoGe) :8099, coder (Qwen3-Coder-30B) :8001, jev-rack :8077. `ollama.service` runs on :11434 but nothing uses it; `~/.ollama` is empty.

**Hot totals: 19.9 GiB of weights + 32.2 GiB serving image on disk; 19.2 GiB itemized GPU memory** (of 45 GiB total system memory in use).

### Warm (NVMe, not loaded)

**Decision / LLM (`~/models`, plus labs)**

| Model | Path | Disk | Job | License | Notes |
|---|---|---|---|---|---|
| Qwen3-Coder-30B-A3B-Instruct | `~/models/Qwen3-Coder-30B-A3B-Instruct-GGUF` | 18.1 GiB | the coder seat (:8001, down) | Apache-2.0 | actively used for development; 82 tok/s per fleet doc 10 |
| d1-omni-600M | `~/models/d1-omni-600M` | 0.6 GiB | triage gate (D-series) | LFM Open v1.0 | service down |
| Qwen3-0.6B ("tiny-smoke") | `~/models/tiny-smoke` | 0.4 GiB | smoke-test model | Apache-2.0 | |
| LFM2.5-8B-A1B | `~/liquid-lab/models` | 4.8 GiB | Liquid trial (planner class) | LFM Open v1.0 | |
| diffusiongemma-26B-A4B-it | `~/diffusion-lab/models` | 16.8 GiB | diffusion-LM trial | Gemma terms | trial only |
| gemma-3-1b-it | `~/diffusion-lab/models` | 0.8 GiB | small wording trial | Gemma terms | |

**Perception / vision**

| Model | Path | Disk | Job | License |
|---|---|---|---|---|
| MoGe ViT-L | `~/models/moge-vitl` | 1.2 GiB | photo → depth + intrinsics (geometry :8099, down; 72 ms warm) | MIT (model card) |
| Depth Anything V2 ViT-L | `~/models/depth-anything` | 1.3 GiB | monocular depth | Apache-2.0 (weights CC-BY-NC for large variants — ViT-L ckpt here predates the split; **re-verify before shipping**) |
| DINOv2-giant | `~/models/dinov2-giant-local` | 4.3 GiB | vision features (sitka experiments) | Apache-2.0 |
| DINOv2-large | `~/.cache/huggingface` | 2.3 GiB | vision features | Apache-2.0 |
| VGGT-1B | `~/sitka-distill/lab/ff-recon/vggt-1b-model.pt` | 4.7 GiB | feed-forward geometry (ff-recon) | CC-BY-NC 4.0 (Meta) — **check before any shipped use** |
| I-JEPA ViT-H / V-JEPA 2 ViT-L | `~/jev-rack/weights` | 2.5 + 1.3 GiB | perception staves experiments (:8077, down) | Meta JEPA weights CC-BY-NC 4.0 — **non-commercial** |
| Bria RMBG 1.4 | `~/.rembg/models/bria-rmbg.onnx` | 1.0 GiB | background removal | bria-rmbg license — **non-commercial for 1.4** |
| CLIP ViT-L/14-336 | `~/.cache/clip` | 0.9 GiB | embeddings (older experiments) | MIT |

**3D generation (`~/models`)**

| Model | Disk | Job | License |
|---|---|---|---|
| Hunyuan3D-2 (dit + paint + delight) | 14 GiB | image → mesh + texture | Tencent Hunyuan Community License |
| Hunyuan3D-2.1 (dit + paintpbr + vae) | 14 GiB | image → mesh + PBR texture | Tencent Hunyuan 3D 2.1 Community License: **free commercial only at ≤ 1 M MAU, and expressly excluded in EU/UK/South Korea** |
| TRELLIS-image-large | 3.1 GiB | image → 3D (structured latents) | MIT |
| TripoSG | 7.5 GiB | image → mesh (diffusion transformer + DINOv2) | MIT |
| stable-fast-3d (SF3D) | 3.8 GiB | fast image → textured mesh | Stability AI Community License (**free commercial under $1 M revenue**) |
| stable-point-aware-3d (SPAR3D) | 6.9 GiB | image → 3D with point-stage editability | Stability AI Community License ($1 M cap) |
| TripoSR | 1.6 GiB | instant draft meshes | MIT |

**Image studio (`~/image-studio/ComfyUI/models`, 187.5 GiB total)**

| Asset | Disk | License / note |
|---|---|---|
| qwen_image_bf16 + qwen_image_edit_bf16 | 38.0 + 38.0 GiB | Apache-2.0 (Qwen-Image 1.0) — legal to ship; bf16 doubles the footprint vs fp8 |
| flux1-dev | 22.2 GiB | **FLUX.1-dev Non-Commercial License — dead end for a shipped game** |
| flux1-schnell | 22.2 GiB | Apache-2.0 |
| flux-2-klein-4b | 7.2 GiB | FLUX.2 klein — non-commercial free tier |
| z_image_turbo_nvfp4 | 4.2 GiB | (Z-Image turbo, NVFP4) — check model card; likely permissive 2026 release |
| text encoders (qwen_2.5_vl_7b 15.5 GiB, qwen_3_4b 7.5 GiB, t5xxl_fp8 4.6 GiB, clip_l 0.2 GiB) | 27.8 GiB | mostly Apache/MIT (VL encoders ship inside Qwen-Image) |
| checkpoints: sd_xl_base + sd_xl_turbo | 13.0 GiB | Open RAIL++-M — commercial, no cap |
| controlnet (cn-canny, cn-depth SDXL) | 9.4 GiB | varies; SDXL-controlnet ecosystem generally Open RAIL |
| clip_vision CLIP-ViT-H (laion2B) | 2.4 GiB | open_clip weights — check lineage |
| loras (Hyper-SDXL, LCM, Lightning ×2), vae, ipadapter, upscale | 3.9 GiB | mixed permissive |

**Speech build tree (`~/src/skald-riva`, 14.5 GiB beyond the hot repo)**

| Path | Disk | What it is |
|---|---|---|
| `model-repo` | 2.1 GiB | earlier self-built "Magpie-Opensource" Triton repo (superseded by the official multilingual repo now mounted) |
| `models` | 2.4 GiB | build inputs: `magpie_tts_multilingual_357m.nemo` (1.4 GiB), two codec .nemo (0.6 + 0.4) |
| `prebuilt` | 3.3 GiB | NVIDIA prebuilt TRT-LLM Magpie tarball for Jetson (`…jetson_2602_deployed.tar.gz`) — spare copy of the deployed engines |
| `work` | 2.9 GiB | RMIR build intermediates + logs |

**Other warm weight sets**: EdenLive (VAST-AI) trial `~/eden-trial/models/EdenLive` **41.1 GiB** (dit-5b, umt5, vae, vision encoder; license unknown — gated trial, likely non-commercial); sitka-distill model corpora (`tiny-models` 0.8 GiB, depth sets ~3 GiB) inside its 35.3 GiB tree; `~/ml` venv 8.1 GiB; caches: `~/.cache/uv` 7.7 GiB, `pip` 1.4 GiB, `flare-cache` venvs 12.8 GiB.

**Docker images not serving anything** (57.8 GiB): `magpie-tts-multilingual` NIM **50.1 GiB** (the first TTS attempt; the Riva container now serves instead), `nv-embedqa-e5-v5` NIM 6.9 GiB (never referenced in the repo), qdrant 0.3 + cuda-base 0.4 + nats/busybox/curl 0.1.

**System ollama** (`/usr/share/ollama/.ollama`): `llama3.2` (blobs 18 + 1.9 GiB, 20.0 GiB total). Nothing on the box references ollama; the daemon idles on :11434.

**Warm totals (weights + model-serving images on the 936 GB NVMe, excluding venvs/corpora/caches): ~497 GiB in total, of which the hot subset is 52.1 GiB** (19.9 weights + 32.2 Riva image), leaving ~445 GiB warm-only. `~/models` (106.2 GiB) and ComfyUI (187.5 GiB) are the two big blocks.

### Cold (external USB HDD, 377.2 GiB used)

| Path | Disk | Contents | License where known |
|---|---|---|---|
| `cold-shelf/home/caseydigennaro/models` | **197.9 GiB** | GLiDE 48.6 · Qwen3-Coder-Next-NVFP4 44.4 · Nemotron-3-Nano-Omni-30B-A3B-Reasoning-NVFP4 20.9 · Nanbeige4.3-33B-NVFP4 20.5 · Qwen3.6-27B-NVFP4 20.4 · iLLaDA-8B-Instruct 15.4 · Sumi-7B 14.5 · Cola-DLM 8.7 · HybridDiffusion-2B 4.5 | Qwen = Apache-2.0; Nemotron = NVIDIA Open Model License; others unverified |
| `cold-shelf/home/caseydigennaro/coder-stack/edgellm-work/qwen36` | 37.6 GiB | TensorRT Edge-LLM engine builds for Qwen3.6 (the coder-stack work) | — |
| `cold-shelf/ollama-store` | 69.7 GiB | ollama blobs: qwen3.5, nemotron-3.5-lightning, nemotron-3-nano | Apache-2.0 / NVIDIA OML |
| `cold-shelf/docker-images` | 23.4 GiB | tarballs: vllm 26.03, vllm-openai 0.20.0, deepstream 9.2 samples, cuda-base, node, busybox, curl | — |
| `cold-shelf/home/caseydigennaro/liquid-lab/models` | 13.5 GiB | LFM2.5-24B-A2B-Q4_K_M.gguf | LFM Open v1.0 |
| `sitka-streetview.tar` | 19.6 GiB | streetview capture corpus (not a model) | — |
| `nvidia-stack` | 13.7 GiB | `venvs.tar` + `IsaacLab-gitclone-stalled-partial` (**dead partial**) | — |
| `image-studio`, `backup`, `sitka-distill` | 1.9 GiB | older copies/backup | — |

### Duplicates and dead files

- **Magpie TTS exists four times on NVMe**: the live official repo (4.1), the superseded self-built repo (2.1), the prebuilt engine tarball (3.3), the .nemo build inputs (2.4), plus its own copy inside the 50.1 GiB NIM image. ~58 GiB of redundancy.
- **LFM2-1.2B twice**: Q8_0 serving copy (1.2) + a Q4_K_M in `liquid-lab` (0.7).
- **Dead files**: `liquid-lab/models/stray-partial.gguf` (7 MB, partial); HF cache stubs (`models--briaai--RMBG-1.4`, `--nvidia--GR00T-N1.7-3B`, `--VAST-AI--EdenLive`, `--GadflyII--Qwen3-Coder-Next-NVFP4`, `--cyankiwi--…AWQ-4bit`, `--facebook--dino-vitb16/ijepa_vith14_1k/VGGT-1B`, `--Ruicheng--moge-vitl` — refs only, no blobs); `Systran/faster-whisper-tiny.en` partial (2.5 MB); `IsaacLab-gitclone-stalled-partial` on cold; `magpie_deployable_1.0.download` (a 404-JSON body kept as a file).
- **format duplication**: Qwen3.6-27B weights cold (20.4 GiB) **and** their TRT engines cold (37.6 GiB); Qwen3-Coder as GGUF warm and as NVFP4 cold (44.4 GiB, a different generation — Coder-Next).

### Proposals (nothing executed)

**Delete — ~108 GiB freed on NVMe (≈130 GiB with the optional caches)**

| What | Bytes | Reason |
|---|---|---|
| docker image `nvcr.io/nim/nvidia/magpie-tts-multilingual` | 50.1 GiB | superseded by the riva-speech deployment; re-pullable from NGC |
| docker image `nvcr.io/nim/nvidia/nv-embedqa-e5-v5` | 6.9 GiB | never referenced by last-mile; re-pullable from NGC |
| system ollama `llama3.2` blobs (+ optionally disable `ollama.service`) | 20.0 GiB | nothing references ollama; frees the daemon's memory too |
| `skald-riva/model-repo`, `prebuilt`, `work` | 8.3 GiB | superseded build artifacts; the official repo + `models/` inputs remain |
| `flux1-dev.safetensors` | 22.2 GiB | non-commercial license — dead weight for a game that may ship |
| `liquid-lab` LFM2-1.2B Q4 + `stray-partial.gguf` | 0.7 GiB | duplicate quant of the serving Q8; dead partial |
| HF-cache stubs + faster-whisper partial | ~0.1 GiB | dead refs |
| (optional, caches) `~/.cache/uv` 7.7, `pip` 1.4, `diffusion-lab/flare-cache` 12.8 | 21.9 GiB | re-creatable caches |
| (on cold, next time it's mounted) `IsaacLab-gitclone-stalled-partial` | (part of 13.7) | stalled clone, dead |

**Go cold — ~122 GiB moved off NVMe**

| What | Bytes | Reason |
|---|---|---|
| `qwen_image_edit_bf16` (keep base warm) | 38.0 GiB | half of a 76 GiB bf16 pair; fp8/GGUF re-download is cheap if needed |
| EdenLive trial | 41.1 GiB | trial concluded; license unpromising for shipping |
| diffusiongemma-26B | 16.8 GiB | trial, rarely loaded |
| Hunyuan3D-2 (v2-0) | 14.0 GiB | superseded by 2.1 (and both carry the territory-limited Tencent license — see Part 2) |
| dinov2-giant-local | 4.3 GiB | experiment weights; TripoSG bundles its own DINOv2 |
| VGGT-1B (ff-recon artifact) | 4.7 GiB | CC-BY-NC; archive with sitka-distill |
| d1-3B weights **and stop the service** | 3.3 GiB (+ 3.9 GiB GPU) | "slower and no better" per the Thor index; not in last-mile's port map — frees ~4 GiB of hot memory immediately |

**Come warm (later, on demand)**: the `coder-stack/edgellm-work/qwen36` TRT engines (37.6 GiB) the day the System Two director tier is built on Qwen3.6 — engines are already compiled for this box; nothing else on cold earns the trip back.

---

## Part 2 — the mix last-mile should run

Two classes, kept separate. **Runtime** = in the game loop, latency-bound, small, always/often hot. **Studio** = offline, quality over speed, loaded on demand. License rule (per the brief; note `docs/research/thor-transfer/03-licences.md` is referenced by the brief but does not exist in this repo — the rule as stated: last-mile may ship, so non-commercial is a dead end). Sources are linked per row; every "measured" number below is from a published Thor/Jetson benchmark unless marked otherwise.

### Runtime

| Slot | Today | Pick (2026) | Why / numbers | Size | License | Source |
|---|---|---|---|---|---|---|
| System One decisions | d1-2B Q8, clef-flash Q4 | **open-weight d1-3B / d1-2B (Liquid, 2026-10-07)** — typed, calibrated answers in one forward pass, no token loop; keep clef-flash as escalation until it's beaten | vendor-published **16 ms/decision on AGX Thor, 262 decisions/s packed**; built on LFM2.5-VL-3B + SigLIP2 | ~2 GB Q4 | LFM Open v1.0 (**$10 M revenue cap**) | [HF d1-3B](https://huggingface.co/LiquidAI/d1-3B) |
| (Apache firewall alternative) | — | **Qwen3.5-2B/4B** | TRT Edge-LLM native on Thor: **125–127 tok/s decode, 18 ms prefill (NVFP4)**; the no-license-risk exit if the studio approaches the LFM $10 M cap | 1.5–2.5 GB | Apache-2.0 | [TRT benchmarks](https://nvidia.github.io/TensorRT-Edge-LLM/latest/user_guide/performance/performance-benchmarks.html) |
| Wording realizer | LFM2-1.2B | **LFM3-1.7B** (2026-10-27), same stack | ~25% faster than LFM2-1.2B at 8K ctx, official GGUF, 256K ctx | ~1 GB Q4 | LFM Open v1.0 | [Liquid](https://www.liquid.ai/blog/lfm3) |
| Speech-to-text | LFM3-Audio-2B | keep, and add **Nemotron-3.5-ASR-streaming-0.6B** | true streaming, 40 locales, **prompt-conditionable — bias the game vocabulary at runtime**; GGML port exists (RTF 0.0066 on RTX 5090); Parakeet TDT v3 as offline second (RTF 0.045 measured on Orin Nano) | ~1 GB | NVIDIA Open Model License (no cap) / CC-BY-4.0 | [HF](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) · [Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) |
| TTS | Riva Magpie 357M | keep for skald pre-render; **migrate runtime speech to Kokoro-82M + Chatterbox Turbo/Pocket TTS** before shipping | Magpie first-audio 55–70 ms but **Riva production deployment requires NVIDIA AI Enterprise (~$4.5 k/GPU/yr)** and has a known crackling bug on Thor; Kokoro/Chatterbox are Apache/MIT, ONNX on aarch64, sub-150 ms class | 0.1–2 GB | Apache-2.0 / MIT | [NIM perf](https://docs.nvidia.com/nim/speech/26.05.0/reference/performances/tts/tts-performance-table.html) · [Kokoro](https://huggingface.co/hexgrad/Kokoro-82M) · [Chatterbox](https://github.com/resemble-ai/chatterbox) |
| Audio surprise (Vox Score §2/§6) | none | **CLAP (Apache checkpoint)**, BEATs second | embedding-drift surprise + zero-shot event probes in one tiny model; sub-10 ms/frame CPU-class | ~150 MB | Apache-2.0 (per checkpoint; AudioMAE and Meta JEPA weights are CC-BY-NC — out) | [LAION CLAP](https://github.com/LAION-AI/CLAP) |
| Face/pose/gaze (§12 staves) | none | **MediaPipe Tasks Face Landmarker (478 pts + 52 blendshapes) + Pose Landmarker**, iris gaze | 30+ FPS CPU-class, exactly the ARKit-blendshape stream §12 wants; NVIDIA Audio2Face/Maxine are x86-only (already verified in §12) | ~50 MB | Apache-2.0 | [MediaPipe](https://developers.google.com/edge/mediapipe/solutions/vision/pose_landmarker) |
| Depth (world) | MoGe ViT-L (geometry svc) | **Depth Anything V2 Small** now, **DA3-SMALL** pilot | **29.3 ms median on Thor (TRT)**; multi-frame consistency is DA3's win; keep MoGe for metric intrinsics (72 ms warm, MIT) | ~100 MB | Apache-2.0 (small variants only — Large is CC-BY-NC) | [embedl Thor](https://embedl.com/depth-anything/Depth-Anything-V2-Small-hf/thor) |
| Segmentation | none | **EfficientTAM-S** | SAM2-class tracking at **126.8 FPS on Thor (TRT), encoder 4.65 ms** | ~0.2 GB | Apache-2.0 | [Jetson port](https://github.com/yunyangx/efficient-track-anything) |
| Agents' sight | none | **Qwen3-VL-2B-Instruct** | **151 tok/s decode, 12.7–29 ms prefill, 916 MB GPU (TRT INT4) on Thor**; official GGUF + mmproj for llama.cpp | 1.6 GB Q4 | Apache-2.0 | [TRT benchmarks](https://nvidia.github.io/TensorRT-Edge-LLM/latest/user_guide/performance/performance-benchmarks.html) |
| Canon RAG embeddings | none | **Qwen3-Embedding-0.6B** (BGE-M3 second) | official GGUF, runs in llama.cpp `--embedding` on aarch64; 32K ctx, 1024-dim | 0.4 GB | Apache-2.0 | [GGUF](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF) |

### Studio / build (offline)

| Slot | Pick | Numbers / fit | Size | License | Source |
|---|---|---|---|---|---|
| Image generation | **FLUX.3 klein** (2026-10-15) first; **SDXL 1.0** (already on disk) as the no-cap workhorse | ComfyUI-native, <9 s/image on an RTX 4070-class GPU; whole set fits 122 GiB unified | ~14 GB FP8 | FLUX Kommunity License: **free commercial under $5 M revenue**; SDXL = Open RAIL++-M, no cap | [BFL](https://bfl.ai/blog/flux-3-image-klein) |
| Image editing / retexture | **Qwen-Image-Edit 1.0** (already on disk, Apache-2.0) | best open editor; bf16 pair is 76 GiB — consider fp8/GGUF copies | 22–76 GB | Apache-2.0 (⚠ Qwen-Image-**2.1** is research-NC — do not "upgrade" blindly) | [Qwen-Image](https://huggingface.co/Qwen/Qwen-Image) |
| Fast drafts | **SANA-Sprint** | NVIDIA-efficiency line, laptop-GPU class | ~3 GB | NVIDIA Open Model License | [NVlabs/Sana](https://github.com/NVlabs/Sana) |
| 3D mesh | **TRELLIS.2-4B** (MIT; needs an sm_110 porting sprint for its Triton/custom ops) first; **TripoSG** (MIT, already on disk) as the painless fallback | PBR GLB out (base color/rough/metal/opacity), 512³–1536³ | ~10 GB | MIT | [TRELLIS.2](https://github.com/microsoft/TRELLIS.2) |
| ⚠ Hunyuan3D-2.1 (on disk) | keep for experiments only | Tencent Community License: ≤ 1 M MAU free, **excluded in EU/UK/South Korea** — unusable for a game sold there | 14 GiB | Tencent community | [LICENSE](https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1/blob/main/LICENSE) |
| Texture / PBR | **MV-Adapter** (mesh texturing) + **StableMaterials** (tileable PBR) + Marigold V2 (albedo from photos) | SDXL-based, SDPA-only — no flash-attn fight | 2–3 GB each | Apache-2.0 / OpenRAIL | [MV-Adapter](https://github.com/huanngzh/MV-Adapter) |
| Skybox/HDRI | **SDXL + MultiDiffusion/tiled + 360° LoRA** (all local; SDXL already on disk) | equirect 2:1, seam-tiled; derive IBL by baking | uses SDXL | Open RAIL++-M (check each LoRA) | [diffusers panorama](https://huggingface.co/docs/diffusers/v0.25.0/api/pipelines/panorama) |
| Motion (§13 tier-1 packs) | **Kimodo** (NVIDIA, 2026) first; **MoMask** second | text→motion with pose/path/end-effector constraints — literally the gamepad tier; 30-joint SOMA, ≤10 s clips; retarget via Blender, ship only your own curves | 1.1 GB | code Apache-2.0, weights NVIDIA OML (commercial-ready) | [Kimodo](https://huggingface.co/nvidia/Kimodo-SOMA-RP-v1) |
| Voice design (consent-only) | **Chatterbox Turbo** (instant clone, MIT) + **CosyVoice 3 0.5B** (fine-tuned signature voices, Apache-2.0) | offline in skald's pipeline; XTTS-v2 (CPML), F5-TTS base (CC-BY-NC), Fish/OpenAudio (CC-BY-NC-SA) are **dead ends** — their outputs are not cleanly commercial | 2–3 GB | MIT / Apache-2.0 | [Chatterbox](https://github.com/resemble-ai/chatterbox) · [CosyVoice3](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512) |
| Music | **ACE-Step 1.5** (MIT) workhorse; MiniMax Music 3.0 top-quality (community license, attribution); DiffRhythm 2 (Apache) backup | full songs w/ vocals, <2 s/song on A100-class; stems → vertical layering in FMOD/Wwise for the adaptive score | 8–22 GB | MIT / community / Apache-2.0 | [ACE-Step 1.5](https://github.com/ace-step/ACE-Step-1.5) |
| SFX | **Stable Audio Open Small / 1.0** (341M–1.2B) | only quality SFX lane that ships; **MMAudio, AudioLDM2, AudioGen, MusicGen are all CC-BY-NC — dead ends** | 0.7–8 GB | Stability Community ($1 M revenue cap) | [SAO Small](https://stability.ai/news-updates/stability-ai-and-arm-release-stable-audio-open-small-enabling-real-world-deployment-for-on-device-audio-control) |
| System Two director (in-game, speed) | **Qwen3.6-35B-A3B** | Apache-2.0, 3B active → 80+ tok/s class on Thor (TRT Edge-LLM: Qwen3-30B-A3B NVFP4 = 84 tok/s measured; **our own NVFP4 Qwen3.6 engines are already on the cold shelf**), 262K ctx + vision | 21 GB Q4 / 37 GB Q8 | Apache-2.0 | [HF](https://huggingface.co/Qwen/Qwen3.6-35B-A3B) |
| System Two authoring (offline, quality) | **Qwen3.5-122B-A10B** Q5/Q6 first; **GLM-4.5-Air** Q6 (MIT) second; gpt-oss-120b MXFP4 (63 GB, Apache) as reserve | GPQA 85.5 / SWE-V 72.0; ~15–25 tok/s decode on 10 B active; fits the ~92 GiB model budget with ~30 GiB left for OS+game; GLM-5.x/DeepSeek-V4/Kimi K3 don't fit, full stop | 85–93 GB | Apache-2.0 / MIT | [HF](https://huggingface.co/Qwen/Qwen3.5-122B-A10B) |

### Recommended mix

**Hot (always loaded) — ~24–30 GiB of the 122 GiB, leaving >85 GiB for the game:**
d1-2B → open d1 (2–4 GB GPU) · clef-flash escalation (7.4 GB, until open d1 beats it) · LFM2-1.2B → LFM3-1.7B (~1.5 GB) · LFM3-Audio-2B (3.6 GB) · Riva Magpie (skald pre-render) · *new:* Qwen3-VL-2B sight (~1 GB) · CLAP (~0.2 GB) · DA2-Small TRT (~0.3 GB) · EfficientTAM-S (~0.5 GB) · MediaPipe landmarker (CPU) · Qwen3-Embedding-0.6B (~0.5 GB) · Nemotron-3.5-ASR streaming (~1 GB). **Drop d1-3B from the hot set** (+3.9 GiB GPU back).

**Warm (NVMe, loaded on demand) — ~230 GiB disk after the proposed prune:** the studio set above (FLUX.3 klein 14 + SDXL stack ~36 + Qwen-Image fp8 ~22 + SANA 3 + TRELLIS.2 10 + TripoSG 7.5 + MV-Adapter/StableMaterials 5 + Kimodo/MoMask 2 + ACE-Step 8 + Stable Audio 1–8 + Chatterbox/CosyVoice 5) · System Two director Qwen3.6-35B Q8 37 (or the local NVFP4 engines) · authoring whale Qwen3.5-122B Q5 85 (load when authoring) · kept: Qwen3-Coder-30B 18, MoGe 1.2, TripoSR/SF3D/SPAR3D 12.3, LFM2.5-8B 4.8, d1-omni-600M 0.6.

**Cold:** everything archived above, plus after adoption the displaced incumbents (clef-flash GGUF, LFM2-1.2B, Magpie build tree, Hunyuan3D pair if TRELLIS.2 lands).

**Download first, by value to last-mile:**
1. **open d1-3B/d1-2B GGUF** (~2 GB) — the decision tier is the game's spine; 16 ms/decision measured on this exact box.
2. **Qwen3-VL-2B GGUF + mmproj** (1.6 GB) — agents' sight; Apache, Thor-proven.
3. **Qwen3-Embedding-0.6B GGUF** (0.4 GB) — canon RAG.
4. **Nemotron-3.5-ASR-streaming-0.6B + Parakeet TDT v3** (~1.8 GB) — vocabulary-biasable ears.
5. **EfficientTAM-S + DA2-Small** (~0.4 GB) — world perception, both Thor-benchmarked.
6. **CLAP audio tower** (~0.15 GB) — Vox Score's surprise channel (E-experiments §6).
7. **MediaPipe Tasks landmarker models** (~50 MB) — the §12 face/gaze/body staves.
8. **Qwen3.6-35B-A3B GGUF** (21–37 GB) — System Two director (and try the local NVFP4 engines first).
9. **FLUX.3 klein FP8** (14 GB) — studio image quality step.
10. **TRELLIS.2** (10 GB + a porting sprint) — 3D with PBR, MIT-clean.
11. **Kimodo + MoMask** (2 GB) — §13 motion packs.
12. **ACE-Step 1.5** (8 GB) — music.
13. **Chatterbox Turbo + CosyVoice 3** (5 GB) — consent-only voice design.
14. **Qwen3.5-122B-A10B Q5** (85 GB) — the offline authoring whale.
15. *(optional)* gpt-oss-20b (13 GB) — cheap fallback tier for the router.

License tripwires for what's already on disk: **flux1-dev (NC — delete candidate), Hunyuan3D-2/2.1 (EU/UK/SK exclusion + MAU cap), SPAR3D/SF3D ($1 M Stability cap), I-JEPA/V-JEPA (CC-BY-NC), RMBG-1.4 (NC), VGGT (CC-BY-NC), EdenLive (unknown/gated), Riva runtime (NVAIE for production), everything Liquid (LFM Open v1.0, $10 M revenue cap).** If last-mile ships under a publisher, the Apache/MIT lane (Qwen3.5/Qwen3-VL/Qwen-Image/TRELLIS/TripoSG/Chatterbox/Kokoro/ACE-Step/Kimodo) is the only fully cap-free path.

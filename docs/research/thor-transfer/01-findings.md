# 01 — The measurements

Every number here was measured on the Jetson AGX Thor on **2026-10-29**, or read from a primary
source on the same day. Where a number came from a document rather than a measurement, it says so.

## 1.1 The generation lane

| Quantity | Value | How it was measured |
|---|---|---|
| SPAR3D, one photograph → textured mesh | **31.9 s** | one end-to-end run on the Thor |
| SPAR3D peak VRAM | **10,576 MB** | sampled over the run |
| SPAR3D raw output | 10,326 vertices / 55,332 indices | before the finishing lane |
| Finishing lane (decimate, 512 px texture, meshopt) | **1.3 s** | one run |
| Frame-model bake (matrix into vertices, via Blender) | **0.4 s** | one run |
| Published payload | **169,132 B**, 10,000 tris, 512 px WebP | `model.glb` in the place tree |
| Payload read back | 11,135 verts / 9,999 tris, normals unit, UVs in range | decoded with three.js's own decoder — **PASS** |
| MoGe depth, one query, warm | **78 ms** | the geometry service on the Thor |
| MoGe depth, round trip at 768 px | **223 ms** | including fetch and upload |
| Relief lane (photograph → depth → UV-painted grid mesh) | **1.6–3.3 s**, **48–50 KB** | the cheap 2.5D path |
| Whole lane, photograph → served tier | **under 60 s** | excluding model load |

> SPAR3D's published default is "10.5GB of VRAM". The measured **10,576 MB** is that figure, so the
> number is not an artefact of this box.

## 1.2 The asset tiers, measured

One capture — St Michael's Cathedral, Sitka — with four tiers in one place file:

| Tier | Bytes | What it is | When it is drawn |
|---|---|---|---|
| `splat` | 193,512 | a gaussian field (`.spz`) | within 140 m |
| `model` | 169,132 | a real triangle mesh (`.glb`, meshopt) | at and beyond 140 m |
| `impostor` | 70,214 | a large photograph (`.webp`) | only when there is no mesh |
| `card` | 26,360 | a small photograph (`.webp`) | the lite profile's only tier |

**The ladder swap was measured, not assumed.** Standing at 138 m from the capture's footprint
centroid, the game fetched the `splat`; standing at 142 m on the same bearing, it fetched the
`model`. The client measures distance to the capture's footprint centroid, and the threshold is
`SPLAT_NEAR_M = 140`.

**Six standings, 6/6 clean.** At 138 m and at 142 m, plus four more bearings at 142 m:

- one asset tier fetched per run, always HTTP **200**
- `ready 7, verified 7, mismatched 0, errors 0` on every run
- **zero** page errors, HTTP errors or failed requests
- settle 5.9–23.0 s; frames 51–108 KB

## 1.3 The framing check — three numbers that must match exactly

A capture's tiers share **one** placement. A generated mesh cannot choose where it stands; it must
be authored into the capture's frame. The check is three numbers, and all three matched exactly:

| Quantity | Value | Must equal |
|---|---|---|
| Base centre (world) | `[-1.153, 0, -15.894]` | the capture's placement |
| Height | **14.592 m** | the capture's `heightM` |
| Front bearing | **190.7°** | the capture's `frontAzimuthDeg` |

When they match, the mesh and the capture's gaussian field share a silhouette under one renderer at
one bearing. **When they do not, the game draws the mesh in the wrong place, at the wrong size,
facing the wrong way — and every log line still reads green.** There is no error to read.

## 1.4 The vouching record

The full chain, verified end to end in production on 2026-10-29:

```
place file fact -> sha256 + byte count + content type
  -> the server serves the bytes
    -> the client verifies the sha256 BEFORE parsing
      -> a mismatch is refused before a single byte is parsed
```

Verified against the deployed site: `GET .../model.glb` → **200**, `content-type:
model/gltf-binary`, and the returned bytes hash to `bdc6919d18894947d008b9ceac9c1f3ccd18fd4c896ea648ba6456e8238f87a8`
— exactly what the place file names. The rule that makes this work: **nothing reaches Bundle bytes
without being in the content address.**

## 1.5 Decision models, on the Thor, against a fixed judge

The judge is fixed and was not modified for these runs. 62 text answers, 31 photo answers.

| Model | Text | Photo | Latency (3-question pack) | Licence |
|---|---|---|---|---|
| d1-omni-600M | 66.1% | 12.9% | 15.4 ms | LFM Open License v1.0 |
| **d1-2B** | **98.4%** | 71.0% | 60.3 ms | LFM Open License v1.0 |
| d1-3B | 93.5% | 67.7% | 90.1 ms | LFM Open License v1.0 |
| **clef-flash** | **100%** | **87.1%** | 284.4 ms | Apache-2.0 |
| Cloud Jev | 100% | 83.9% | 114 ms | $0.0006/run; a **sample**, not a fact |
| MiniMax-M3 | — | — | — | $0.0075–0.0233/run |

Two facts here matter more than the percentages:

- **How a pack of questions scales is not settled, and spike S1 must measure it.** Upstream claims a
  whole pack in one pass — d1's model card says it "returns calibrated answers in one forward pass"
  and takes "any number of questions about one state in one call"; Clef's README says it "scores all
  options of all questions jointly". **The Thor's measurement does not show flat scaling.** The
  600M was flat from one question to three (15.0 → 15.4 ms), but eight questions cost **83.0 ms**,
  roughly 5× the three-question cost. Either the serving path batches at a limit, or the upstream
  claim does not hold at this size and precision. **Do not assume either.** This is exactly the
  quantity S1 needs.
- **clef's one-pass pack was not faster per pack than d1 on this GPU** (284.4 ms vs 60.3 ms for
  three questions). One pass is not one cheap pass.
- **Local answers are reproducible; cloud answers are samples.** Two local runs produced identical
  answers with zero spread. The cloud Jev photo arm scored 83.9% — and that measurement had been
  *wrong before*, and the cause was **the harness** (the image pipeline), not the model.

## 1.6 The image floor

The cloud Jev API refuses any image whose shorter side is under **280 px**, with HTTP 400. Vision
therefore needs a minimum image size, and a harness that resizes to the wrong dimension gets an
error, not a wrong answer — and can look like "the model is bad" when it is the pipeline.

## 1.7 The Thor, measured today

| Quantity | Value | Note |
|---|---|---|
| RAM total | 122 GiB | |
| RAM available | **31 GiB** | was ~42 GiB during the 2026-10-28 audit; the decision services are now resident |
| NVMe total | 936 GiB | |
| NVMe free | **249 GiB (72% used)** | last-mile documents 184 GiB (80% used), measured 2026-10-28 |
| Services answering | 8000, 8001, 8017, 8077, **8093–8099**, **8100**, 8188, 8190, 8200, 11434, 18789, 2206, 8788 | 8092 is **down** |
| GPU memory held by other tenants | ~54.5 GB | from last-mile's 2026-10-28 audit; SPAR3D's 10.5 GB has to fit in what is left |

The **live index on :8100** answers "what is here, right now" on every request: the services, the
asset tiers with their hashes, the tools, the proofs, the decision models, and live memory and disk.
It is the discovery surface last-mile's one-shot service list is not.

## 1.8 The harness reality

Measured with the playtest probe, headless under software GL (`--use-gl=swiftshader`):

- the world streams **4,063** terrain tiles; the screenshot at the default 30 s timeout fails with
  `page.screenshot: Timeout 30000ms exceeded` and produces **no frame** — raised to 180 s
- the probe could not open a baked place at all, only a coordinate lookup, so it could never reach a
  place that carries a capture

These are facts about the **headless probe**, not about the game on the ProArt's desktop GPU.

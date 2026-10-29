# 04 — Hazards, and the measured facts that have moved

Things that will bite the Thor, in the order they are likely to bite.

## 4.1 The memory watchdog sheds the decision tier first

The Thor runs a memory watchdog. When `MemAvailable` falls below **15 GiB** it sheds load, and the
order is deliberate: **the local decision tier goes first.** The registered consults on `:8001` and
`:8003` are protected.

On 2026-10-29 a 6.6 GiB benchmark pushed the box over the line and took **all four local decision
services down at once**. The local decision tier is the reflex rung — and D11 says a late decision
falls back to a reflex. **If the reflex rung is what the watchdog sheds, "never wait" has no floor
under it.**

For last-mile this is a design constraint, not a bug to fix: if the Thor is to run the reflex tier
while also pre-rendering, the two must have a memory budget agreed in advance, and the shed order
must be checked against it. Today the shed order is: decision tier first, registered consults last.

## 4.2 The llama.cpp build is pinned, and it is too old for LFM3

- The Thor's `llama.cpp/build` is **0.7.0-dev**, and it **cannot load LFM3**. LFM3 needs
  **≥0.8.0-dev**.
- A working **0.8.0-dev** build exists at `~/llama.cpp-upstream/build-up`.
- The 0.7.0-dev build is **depended on by a running service and must not be rebuilt**.

last-mile's `docs/research/fleet/01-measured-facts.md` lists `:8092` as a llama-server running
**LFM2-1.2B** as the System-1 candidate for members' think requests. **:8092 is down right now**
(measured 2026-10-29: no answer). Ollama on `:11434` answers.

So the fastest local model last-mile planned to use is unavailable, and the route to restoring a
model family it wants is constrained by a build that cannot be rebuilt in place.

## 4.2b The chosen member model does not load on the Thor's build

**This is the most urgent item in this folder.**

- last-mile's **M1 design spec has already chosen the member model**:
  `docs/superpowers/specs/2026-10-28-last-mile-m1-design.md:101` — "member gates: **Liquid d1-2B**
  (local on the Thor)". `docs/research/fleet/04-decision-models.md:24` records that d1 is **LFM3**-based.
- The running build is **0.7.0-dev**, and **0.7.0-dev cannot load LFM3** — it needs ≥0.8.0-dev, which
  exists at `~/llama.cpp-upstream/build-up` but is a different worktree.
- The 0.7.0-dev build **must not be rebuilt** (a running service depends on it).

So the spec's chosen member model **does not run on the Thor's current build**, and the obvious fix
is forbidden. This needs a decision — which build the Thor runs, and what happens to the member model
when it changes — and it is not written down anywhere today.

## 4.2c The latency numbers in last-mile's own research are not Thor numbers

| last-mile says | file:line | Measured on the Thor |
|---|---|---|
| "d1-3B: **~16 ms on Jetson AGX Thor**; 3 questions ≈ 1.3× one" | `fleet/04-decision-models.md:24` | **d1-3B: 90.1 ms** for a 3-question pack (the quote is unqualified as to pack size) |
| "**Clef-flash** … ~39 ms median on **Workers AI**" | `fleet/04-decision-models.md:26` | **284.4 ms** on the Thor for a 3-question pack — **7× worse**; the 39 ms is a *cloud* number |
| "d1 takes about **1.3×** as long for 3 questions as for 1" | `fleet/05-where-this-leads.md:25` | 600M was flat 1→3 (15.0 → 15.4 ms) but **8 questions cost 83.0 ms** |

`fleet/05-where-this-leads.md:25` is the sentence that makes presence mode affordable — "a model that
answers 8 questions over the changed region is cheap enough to run like a video". It rests on a
**vendor number treated as a measured one**. That is the single sentence to re-measure.

## 4.3 Cycles with denoising renders pure black on this Thor

Blender's **Cycles with denoising ON** renders **pure black** on this box. EEVEE works. The
standing guard is `util.BLENDER_CYCLES_USE_DENOISING = False`.

last-mile's `docs/research/forclaude2/11-prerender-ahead.md` lists **"Path-traced GI bake on the
Thor"** as a spike. A path-traced bake on this Thor hits this exact trap, and it looks like a
renderer bug rather than a denoiser bug: a black frame, no error, no warning.

## 4.4 VRAM on the Thor is not the Thor's

last-mile's 2026-10-28 audit records **~54.5 GB of GPU memory held** by the 30B coder, ComfyUI, the
JEVA rack and the 1.2B. SPAR3D needs **10.5 GB**. That is a third of what is already held.

"Always-on" generation is therefore a scheduling problem, not a capacity problem: the Thor can
generate, but not while everything else runs. This is the same shape as D16's least-use-first
fair-share queue, applied to the generator instead of the agents.

## 4.5 Gated checkpoints break CI

`stabilityai/stable-point-aware-3d` and `Ruicheng/MoGe-2` need authentication. An unauthenticated
fetch of `Ruicheng/MoGe-2` returns **HTTP 401**. Any pipeline step that re-fetches a gated
checkpoint fails the first time a token expires or is not forwarded. Fetch once into the local
cache; serve from the cache.

## 4.6 The headless probe is not the game

Measured with the playtest probe, headless under software GL:

- **4,063** terrain tiles stream into a first look at a whole place
- the screenshot at the default 30 s timeout **fails and produces no frame** — `page.screenshot:
  Timeout 30000ms exceeded` — raised to 180 s
- the software renderer runs at roughly 1–3 fps

These are facts about the **headless harness on a machine without a GPU in the browser's hands** —
not about the ProArt's RTX 4050. Do not read them as a performance estimate for last-mile's renderer.

## 4.7 The measured facts last-mile has stale

| last-mile says | Measured 2026-10-29 | Where |
|---|---|---|
| "the NVMe has 184 GiB free (80% used)" | **249 GiB free, 72% used** of 936 GiB | `docs/research/fleet/01-measured-facts.md` |
| "RAM 122 GiB total, ~42 GiB available during the audit" | 122 GiB total, **31 GiB available** — the decision services are now resident | `docs/research/fleet/01-measured-facts.md` |
| Thor services: 8001, 8092, 11434, 8077, 8188/8190/8200, 18789, 2206/8788 | also **8093, 8094, 8095, 8096, 8097, 8098, 8099, 8100**, 8000, 8017 | `docs/research/fleet/01-measured-facts.md` |
| `:8092` running LFM2-1.2B as the System-1 candidate | **:8092 is down** | same |
| "later estimates should cite those files" | there is now a **live index on `:8100`** that answers on every request — a better source than a snapshot | new |

The direction of the disk correction is worth stating: last-mile's 80%-used figure made the NVMe
look tighter than it is. It is still the standing constraint — **72% used is not comfortable for a
machine that will hold a content-addressed asset bank** — but it is 65 GiB more room than the doc
says, and the trend over one day was 90 GiB *freed*, not consumed.

## 4.8 The discovery surface

The Thor now serves a **live index on `:8100`**:

```
curl -s http://thor.local:8100/index.json
```

It answers, on every request: the services and their ports, the asset tiers with their paths, byte
counts and sha256s, the tools the Thor can run, the proofs that exist and whether their scripts are
present, the decision models on disk, and live memory and disk.

It is the answer to last-mile's real problem with a hand-written service list: **a snapshot of a
machine's ports goes stale within a day**. The index is 9 services, 5 tools, 6 proofs and 17 tiers
as of 2026-10-29 12:49. Any last-mile agent that needs to know what the Thor can do should ask the
Thor, not read a file.

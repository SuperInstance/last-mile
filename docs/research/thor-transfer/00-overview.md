# Thor transfer — what the Otherhere lane found, and where it lands

**Read this first.** This folder is the transfer from a second SuperInstance project — `otherhere`
— into last-mile. In one day of work on the Jetson AGX Thor, that project built a working lane
from **one photograph to a mesh the game opens**, and measured almost everything it touched. Most
of what it learned is not about games. It is about what the Thor can and cannot do, what a
generated asset must satisfy before it is allowed into a world, and what a decision model costs.

last-mile already has the architecture. **This folder adds three things:** measured numbers where
last-mile has guesses, one generator that is licence-clean for what last-mile intends to do, and
a delivery format that needs a step last-mile does not have yet.

| File | What is in it |
|---|---|
| [`01-findings.md`](01-findings.md) | The measurements. Every number, with where it came from and when. |
| [`02-mechanisms.md`](02-mechanisms.md) | The four transferable **mechanisms** — the parts that are not just numbers. |
| [`03-licences.md`](03-licences.md) | The licence and gating matrix, and the decision it forces. |
| [`04-hazards.md`](04-hazards.md) | What will bite the Thor, and the measured facts last-mile has stale. |
| [`05-decision-inputs.md`](05-decision-inputs.md) | Proposed entries for `docs/design/decisions.md`. **Nothing is decided here.** |
| [`06-what-to-change.md`](06-what-to-change.md) | The work list: what last-mile states today that the measurements contradict, and where. |
| [`BRIEFING.md`](BRIEFING.md) | For the agent doing the work: what is on the Thor, how to call it, what to do first. |

## The short version

**1. The generator last-mile named has a licence problem; the one that was actually measured has a
revenue ceiling.** last-mile's `docs/research/forclaude2/11-prerender-ahead.md` proposes TRELLIS for
the decimated-asset spike. TRELLIS's Hugging Face card says MIT, but the TRELLIS repository's own
maintainer says the *rendering* components — `diffoctreerast`, `nvdiffrast`, the modified Flexicubes
and the Gaussian splatting renderer — are not available for commercial use. SPAR3D, which was
measured on this Thor (31.9 s, 10,576 MB peak VRAM, 169,132 B output), is under Stability's
Community License: commercial below US$1M revenue, with registration and a "Powered by Stability AI"
notice. Neither is wrong; they are different ceilings. See [`03-licences.md`](03-licences.md).

**2. The delivery format is right; the renderer needs a step.** A meshopt-compressed GLB is 169 KB
for 10,000 triangles and a browser opens it with three.js's own bundled decoder — no wasm fetch, no
Basis transcoder. **Released Bevy 0.20.1 cannot read it** — ❌ in `bevy_gltf`'s support table, no
`meshopt` feature, no decoder in the loader. The way through is either a plugin that rewrites the
GLB in memory before `bevy_gltf` sees it (pure Rust, no C++ toolchain — a worked example exists in
<https://github.com/paulrobello/hex-world>), or pinning a Bevy build that includes the upstream
support that was cut from the 0.20 branch. See [`02-mechanisms.md`](02-mechanisms.md).

**3. The tier ladder is the same idea as last-mile's L0–L3, and it failed in an instructive way.**
The Otherhere lane shipped a four-tier asset ladder — gaussian field near, real mesh beyond,
photograph beyond that, card for the lite profile. The mesh tier was fetched, hash-verified and
drawn in the running game on six separate bearings, with zero errors. **It was also invisible**:
the ladder gave it only the range beyond 140 m, where a 14.6 m building is a few dozen pixels and,
from most bearings, behind the hill. The lesson is not the ladder. It is that **a fidelity ladder's
thresholds have to be measured against screen pixels, not chosen.** See [`02-mechanisms.md`](02-mechanisms.md).

**4. Three of last-mile's measured facts are stale.** last-mile documents the Thor as having
184 GiB free (80% used), measured 2026-10-28. It has 249 GiB free (72% used) today. last-mile's
service list omits the five services that now answer on the Thor, and names one (8092) that is
down. See [`04-hazards.md`](04-hazards.md).

**5. Spike S1 can be answered from measurements, not estimated — and the shape of the cost is not
settled.** Upstream claims a whole pack of questions in one forward pass (d1: "any number of questions
about one state in one call"; Clef: "scores all options of all questions jointly"). **The Thor's
measurement does not show flat scaling**: the 600M was flat from one question to three (15.0 →
15.4 ms), but eight questions cost **83.0 ms**. last-mile's own research currently treats a
*vendor* number — "d1 takes about 1.3× as long for 3 questions as for 1" — as a measured one. **Do not
assume either.** There are also **no upstream Jetson latency figures** for either family. See
[`01-findings.md`](01-findings.md).

**6. The M1 spec's chosen member model does not load on the Thor's build.** The spec names
**Liquid d1-2B** as the member model; d1 is LFM3-based; the Thor's running `llama.cpp` is
**0.7.0-dev**, which cannot load LFM3, and must not be rebuilt because a running service depends on
it. This is the most urgent item in the folder. See [`04-hazards.md`](04-hazards.md) §4.2b.

**7. At HEAD, last-mile contained none of this.** An audit of the whole repo found **0 of 19**
findings present, **2 contradicted** and **12 stale numbers**. The architecture was right; what was
missing was measurement, licences, and one renderer fact. The full work list is
[`06-what-to-change.md`](06-what-to-change.md).

## What this folder does not claim

- **It does not decide anything.** last-mile's decisions are append-only and belong to Casey.
  [`05-decision-inputs.md`](05-decision-inputs.md) is input to that log, not a change to it.
- **It does not claim SPAR3D replaces last-mile's generator.** SPAR3D reconstructs **one photographed
  object**. last-mile's "seed + params *are* the asset" rule (D19) and the Thor's Foundry on :8200
  are the right path for archetypes. SPAR3D is the answer for *subjects that exist in the world*.
- **It does not measure Bevy.** The Bevy findings in this folder come from Bevy's own documentation
  and from another Bevy project's source. Nothing here was run on last-mile's engine.
- **It does not cover the Orin's archive role beyond one constraint.** The Orin has 7 GiB of RAM.
  SPAR3D needs 10.5 GB of VRAM. The Orin stores and verifies; it does not generate.

## Provenance

Measured on the Thor (`thor.local`, 192.168.0.78) on 2026-10-29 unless stated. Upstream facts were
checked against primary sources on 2026-10-29; the URLs are in the files that use them. The source
project is <https://github.com/SuperInstance/Otherhere> — the model tier and its ADR are
[`ADR-0023`](https://github.com/SuperInstance/Otherhere/blob/main/docs/adr/0023-the-client-gains-a-model-tier.md)
and [`ADR-0024`](https://github.com/SuperInstance/Otherhere/blob/main/docs/adr/0024-a-generated-tier-is-framed-into-its-capture.md).

# 03 — Licences, gating, and what they decide

Checked against primary sources on 2026-10-29. The Hugging Face metadata was read from the model
API; the licence terms were read from the licence files themselves.

**Why this file exists.** last-mile's `docs/research/forclaude2/11-prerender-ahead.md` proposes
TRELLIS for the decimated-asset spike, and last-mile's licensing posture is currently only a line in
`docs/research/fleet/04-decision-models.md` — that the licences are "fine for a personal project;
record it". If last-mile is ever going to ship, that is not enough: a generator that cannot ship
commercially is not a slower option, it is a dead end. So the licences are the first gate, ahead of
quality and speed.

## 3.1 The matrix

| Model / library | Gated | Licence | Commercial use | The catch |
|---|---|---|---|---|
| **SPAR3D** (`stabilityai/stable-point-aware-3d`) | **yes**, auto-approval | Stability AI **Community License** | **yes**, if annual revenue < **US$1M** | registration required; a **"Powered by Stability AI"** notice must ship; **may not be used to train a foundational generative model**; Trade Control Laws apply |
| stable-fast-3d (SPAR3D's base) | yes | Stability AI Community License | same | same |
| **TripoSR** (`stabilityai/TripoSR`) | no | **MIT** | yes | **untextured mesh only** — no PBR texture |
| **TRELLIS-image-large** | no | MIT on the HF card | **contested** — see 3.2 | repo maintainer: the *rendering* components are not commercially available; ~16 GB VRAM |
| **TRELLIS.2-4B** | no | MIT on the HF card | contested | same; **≥24 GB VRAM** |
| **Hunyuan3D-2** | no | Tencent Hunyuan Community | **restricted by territory** | **does not apply in the EU, UK or South Korea**; use outside the Territory is unlicensed; **>1M MAU requires a separate licence from Tencent** |
| **Depth-Anything-V2-Large** | no | **CC-BY-NC-4.0** | **no — non-commercial** | the Large checkpoint is not usable in a commercial product |
| **Depth-Anything-V2-Small** | no | **Apache-2.0** | yes | this is the one to use |
| **microsoft/MoGe** (the depth service) | GitHub; the `Ruicheng/MoGe-2` HF weights need auth | **MIT** (code) | yes | the code is clean; confirm the weights' terms before shipping them |
| JEPA / V-JEPA 2 weights | varies | **CC-BY-NC-4.0** | **no** | already flagged in last-mile's own research |
| **Liquid AI d1** family (`d1-2B`, `d1-3B`, `d1-omni-600M`) | no | **LFM Open License v1.0** | check the v1.0 terms; last-mile records it as free below **$10M** revenue | **not MIT** — but **more permissive than SPAR3D's $1M**. The same family last-mile's own research names as the System-1 class, and the M1 spec has already chosen d1-2B |
| **Cloudflare Clef** (`clef`, `clef-flash`) | no | **Apache-2.0** | yes | the cleanest decision-model licence in the set |
| **meshoptimizer** (C++ reference) | no | **MIT** | yes | |
| **meshoptimizer-rs** (Rust port, v0.2.15) | no | **MIT** | yes | pure Rust, byte-identical output; covers simplification, meshlets, cluster LOD **and decode** |

## 3.2 The TRELLIS conflict, stated plainly

Two primary sources disagree:

- The **Hugging Face model card** for `microsoft/TRELLIS-image-large` and `microsoft/TRELLIS.2-4B`
  lists the licence as **MIT**, and NVIDIA's NIM model card describes TRELLIS as "ready for
  non-commercial/commercial use".
- The **TRELLIS repository's own maintainer** states that the components related to rendering — the
  3D Gaussian renderer, the radiance-field renderer, and the mesh/GLB extraction path (diffoctreerast,
  nvdiffrast, modified Flexicubes) — are **not available for commercial use**, and that TRELLIS is a
  research project for which "commercial use isn't the aim". No patent waivers are granted.

Primary source: <https://github.com/microsoft/TRELLIS> and its issue tracker (the maintainer's
statement about commercial availability of the rendering components). A community fork,
`kg-git-dev/trellis-refactored`, replaced `nvdiffrast` with PyTorch3D precisely so the mesh path
becomes commercially usable — which is evidence that the concern is real and that it is fixable,
but it is also a fork to maintain.

**Consequence for last-mile.** TRELLIS is usable as-is for research and for non-commercial output.
For a commercial product, it needs the renderer rewritten (as the community fork did) or a different
extraction path. This is a real engineering task, not a checkbox.

## 3.3 What the licences decide

**If the Thor generates textures for a commercially-shipping world:**

| Option | Verdict |
|---|---|
| SPAR3D | Clean below $1M revenue. Registration and a "Powered by Stability AI" notice are real but cheap. **The revenue ceiling is a decision, not a wall** — and it forbids using outputs to train a foundational model, which last-mile does not want to do anyway. |
| TripoSR | Cleanest licence, but no texture — it would need last-mile's own UV/paint lane, which **exists and was measured**: the relief lane paints a photograph onto a depth grid by UV in 1.6–3.3 s for 48–50 KB. |
| Hunyuan3D-2 | **Out** while last-mile can ship in the EU or UK. |
| TRELLIS | Out until the renderer is replaced or a licence is negotiated. |

**If the Thor generates geometry only** (depth, relief, LOD), every option is clean: MoGe is MIT,
Depth-Anything-V2-**Small** is Apache-2.0, meshoptimizer is MIT.

> **Correction to an earlier reading.** The Depth-Anything-V2 Large→Small swap has been carried
> around as a disk-saving measure. It is a **licence** requirement: Large is CC-BY-NC-4.0 and
> cannot ship in a commercial product at all. Small is Apache-2.0. The swap is mandatory, not tidy.

## 3.4 Gating and attribution, as operational facts

- **SPAR3D's weights are gated** on Hugging Face (auto-approval, but you must accept the licence and
  give name, email, country, organisation and intended use). An unauthenticated fetch fails. On
  this Thor the weights are already present, so the gate has been cleared once.
- **`Ruicheng/MoGe-2` returns HTTP 401 unauthenticated.** The geometry service on the Thor already
  has them.
- Gated models are a **supply-chain risk for CI**. Any pipeline step that re-fetches a gated
  checkpoint will break the first time the token expires. The Otherhere lane fetches the weights
  once and serves from the local cache; last-mile's CI should do the same.
- **MIT still means shipping the notice.** `meshoptimizer` and `meshoptimizer-rs` are MIT, which
  requires the copyright and permission notice in copies. The hex-world rewrite plugin — the
  worked example for the Bevy loader step — **ships no NOTICE or attribution file**. If last-mile
  copies that pattern it must add the notice, or it is shipping a licence violation.
- **SPAR3D's attribution is a product obligation, not a file obligation.** The Community License
  requires shipping a copy of the agreement, a `Notice` text file with a fixed sentence, and
  **prominently displaying "Powered by Stability AI"** on a website, user interface, blog post,
  about page, or product documentation. That is a user-visible requirement, so it is a design input
  and not just paperwork.

## 3.5 Sources

- SPAR3D licence: <https://github.com/Stability-AI/stable-point-aware-3d/blob/main/LICENSE.md>
  · card: <https://huggingface.co/stabilityai/stable-point-aware-3d>
- TRELLIS: <https://github.com/microsoft/TRELLIS> · the commercial-use question is
  [issue #41](https://github.com/microsoft/TRELLIS/issues/41) · cards:
  <https://huggingface.co/microsoft/TRELLIS-image-large>,
  <https://huggingface.co/microsoft/TRELLIS.2-4B> · NVIDIA's conflicting card:
  <https://build.nvidia.com/microsoft/trellis/modelcard>
- Hunyuan3D-2 licence: <https://huggingface.co/tencent/Hunyuan3D-2/blob/main/LICENSE>
- Depth-Anything-V2: <https://huggingface.co/depth-anything/Depth-Anything-V2-Large-hf> (CC-BY-NC-4.0),
  <https://huggingface.co/depth-anything/Depth-Anything-V2-Small-hf> (Apache-2.0)
- MoGe: <https://github.com/microsoft/MoGe> (MIT)
- d1: <https://huggingface.co/LiquidAI/d1-2B> (LFM Open License v1.0) · Clef:
  <https://huggingface.co/Cloudflare/clef-flash> (Apache-2.0)
- meshoptimizer: <https://github.com/zeux/meshoptimizer> (MIT) · Rust port:
  <https://crates.io/crates/meshoptimizer-rs> (MIT, v0.2.15)
- Bevy's glTF extension table: <https://docs.rs/bevy/latest/bevy/gltf/> ·
  Bevy's meshopt PRs: [#25953](https://github.com/bevyengine/bevy/pull/25953),
  [#26318](https://github.com/bevyengine/bevy/pull/26318)
- The glTF registry: <https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md> ·
  `EXT_meshopt_compression`: `extensions/2.0/Vendor/EXT_meshopt_compression/` ·
  `KHR_meshopt_compression`: `extensions/2.0/Khronos/KHR_meshopt_compression/` ·
  `KHR_gaussian_splatting`: `extensions/2.0/Khronos/KHR_gaussian_splatting/`

# 11: EdenLive-class results at 4050 scale: the Thor pre-renders, the ProArt composites

EdenLive does per-frame neural rendering, which needs a 5090-class GPU. We can get a similar effect (simple game underneath, rich look on top) by moving the expensive rendering **ahead of time** onto the Thor, then using the 4050 as a fast **compositor** of pre-baked parts.

## The core idea: spend neural and offline compute where things change slowly

| Layer of the image | How fast it changes | Who renders it |
|---|---|---|
| Distant vistas, sky, terrain silhouettes | Seconds to minutes | **Thor**, pre-rendered, neural allowed |
| Static scenery lighting (GI, AO, bounce light) | Never, until an edit | **Thor**, baked per chunk |
| Surface detail (textures, decals, grime) | Never | **Thor**, generated once, then cached |
| Mid- and far-distance agents and props | Per animation frame, but small on screen | **Thor** pre-renders sprite/impostor sheets; the **4050** plays them back |
| Near agents, player, effects, UI | Every frame | **4050**, rasterized live |

This is the old rule: **pre-render what's static or far, draw live only what's near and moving.**

## Retro precedents
- **Pre-rendered backdrops plus real-time characters:** Resident Evil (1996), Final Fantasy VII (1997), Myst. Static scenes were rendered offline at high quality, and low-poly characters walked through them with depth masks.
- **Pre-rendered sprites from 3D:** Donkey Kong Country (1994) rendered SGI models into SNES sprites. Doom and Daggerfall used 8-direction billboard sprites.
- **Baked lighting:** Quake (1996) lightmaps. Offline radiosity on a big machine, played back for free on a Pentium.
- **Streaming ahead:** Crash Bandicoot streamed level data along the path the player was about to take.

## Pre-render techniques, mapped to Thor → ProArt

1. **Lighting baked into vertex colors (biggest win, lowest cost).** The Thor path-traces global illumination for each chunk ahead of the player and writes the result **into vertex colors**. Our retro-modern style is already vertex-colored, so the 4050 gets near-path-traced lighting essentially for free. Dynamic objects get a cheap probe sample from the baked chunk.
2. **Neural vistas (EdenLive-lite).** The Thor renders a **greybox panorama** of the far field from the player's predicted position, then stylizes it with its image model (img2img, our palette and style prompt). The 4050 shows it as a parallax-layered skybox. It refreshes every few seconds as the player moves, which is latency-tolerant because distant things barely change. This is where the "generative renderer" look actually shows up on our hardware.
3. **Impostor sprite sheets.** For each generated or decimated asset and each agent animation, the Thor renders 8–16 view angles into a sprite atlas, stylized consistently. The 4050 draws mid and far agents and props as billboards. The full mesh appears only up close.
4. **Generated surface textures.** The Thor's image model makes palette-quantized texture tiles and decals per biome and material. The ProArt holds them as compressed texture arrays (BC7), so VRAM stays predictable.
5. **Pre-voiced barks and lines** (already planned in 03) follow the same pattern for audio.

## Progressive chunk fidelity (graceful degradation)
Every chunk the ProArt can reach exists at increasing fidelity levels. **Any level is playable.** The Thor upgrades chunks as fast as it can, and lateness only costs looks, never gameplay:

| Level | Content | Source | Deadline |
|---|---|---|---|
| **L0** | Geometry from the seed, flat vertex colors | ProArt, instantly (deterministic procgen) | None, always available |
| **L1** | Baked GI in vertex colors | Thor | Before the chunk is within view distance |
| **L2** | Textures, decals, set dressing from the liked-asset bank | Thor, plus the Orin cache | Before the chunk is near |
| **L3** | Neural vista layer covering this region | Thor | Whenever ready |

The same **deadline plus fallback** pattern as AI decisions (08, lesson 7): the game never waits on the Thor.

## Predicting "a few rooms ahead"
- The ProArt sends the Thor the player's position, velocity and the squad's objective. The Thor ranks likely next chunks: along the heading, toward the objective, along corridors.
- The world is seed-deterministic, so the Thor generates the *same* L0 geometry itself and bakes on top of it with no geometry transfer. Only bake results travel: vertex colors, texture IDs, panoramas.
- Budget: rank by probability × time until needed, and cancel bakes when the player turns away.

## Reusing what worked and what you liked (the asset bank)
- Everything generated gets a **content-addressed ID**. It's the MIDI "program number", or PLATO's redefined glyph (08): define it once, then refer to it by ID forever.
- The **Orin** holds the bank on its 2 TB NVMe: assets, textures, sprite sheets and baked lighting, keyed by hash. It's the ROM cartridge or sample library.
- **Your feedback is a lane in the log.** 👍/👎 on assets, rooms and vistas (the same hook as utterances in 09) adjusts the weights procgen uses when choosing set dressing, and the prompts the Thor uses for new generation. Over time the world drifts toward your taste, and liked assets are reused instead of regenerated. That's cheaper *and* better.
- Revisiting a place reloads its baked chunk from the bank instead of re-baking.

## What the 4050 does live (kept small, and pinned to a VRAM budget)
- Rasterize near geometry (vertex colors plus a texture array), billboards, and the panorama layers
- Render at a low internal resolution, then upscale. Bevy has DLSS support (NVIDIA RTX, Vulkan, Windows/Linux), and the 4050 qualifies. Alternatively, an integer upscale for a crisper retro look.
- Cheap style passes: palette quantization, ordered dithering, outline or edge detection, fog. All per-pixel trivial.
- **VRAM budget sketch (6 GB):** render targets ~0.3 GB · near-chunk mesh pool ~0.5 GB · texture arrays ~1.5 GB · sprite atlases ~1 GB · panorama layers ~0.3 GB · headroom for driver, OS and the compositor.

## Spikes needed
- Path-traced GI bake on the Thor: time per chunk at our density (OptiX or CUDA)
- Img2img vista stylization on the Thor with the installed image models: seconds per panorama, and is the style consistent?
- Sprite-sheet rendering of a decimated TRELLIS asset (or other 3D generator), with style matching
- Bevy DLSS on the 4050: what it costs in frame time at our internal resolution

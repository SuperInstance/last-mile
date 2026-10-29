# last-mile M1 Phase 6 (3D + archive) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. In this repo the work is seat-driven: ZCode and MiniMax Code implement, and Claude reviews and commits.

**Goal:** the M1 loop runs on all three boxes. Two deliverables:

- **3D retro-modern view (L0).** A low-poly, vertex-coloured 3D view of the world on the ProArt, rendered at a low internal resolution and upscaled (integer by default, DLSS optional), with agents as instanced low-poly figures in squad colours.
- **Orin archive.** `lm-archive` ingests pushed round logs on the Orin, replays every round headless, verifies every state hash, and hosts the cross-architecture golden check — all while yielding to whatever else the Orin is doing.

**Architecture:**

- A new crate **`lm-render`** holds the 3D view's *pure* half, testable in WSL with no GPU: the palette, the grid→mesh builder, the greedy mesher, the chunk structure hash, and the agent figure builder. It depends on `lm-core` only (no Bevy), so the geometry is unit-testable headlessly.
- **`lm-host`** gains a `view3d` module: a Bevy 0.20.1 plugin that turns `lm-render`'s mesh data into `Mesh` assets with `Mesh::ATTRIBUTE_COLOR`, spawns one `Mesh3d` per chunk, spawns agents as instanced figures, and drives the low-resolution camera.
- A new crate **`lm-archive`** is a small std-only service for the Orin: a `std::net::TcpListener` accept loop, one `std::thread::spawn` per connection, framing over the existing `lm-proto` codec, atomic log writes, and a verification worker that replays each round and compares hashes.

**Tech Stack:** Rust 1.99; `bevy = "=0.20.1"` (already pinned by Phase 2), `bevy_egui = "=0.43.1"`; `lm-core`, `lm-proto`, `lm-tools` (lib) from Phase 1; `lm-archive` adds **no new dependencies** beyond `lm-core`/`lm-tools` — std-only networking.

**Spec:** spec §6 (3D retro-modern, L0 chunks, clean proxy-friendly output), §7 (Orin archive), §10 phase 6. Decisions D2 (retro-modern), D14 (prerender ahead, chunk fidelity L0–L3), D46 (world-anchored structure-hash caches), D48 (procgen grammar), D50.

---

## Global Constraints

- **`lm-core` stays untouched.** Nothing in this phase may change a Phase 1 golden hash. The 3D view **reads** the world; it never writes to it. The archive **replays**; it never mutates a log.
- **Floats are host-only.** `lm-render` and `lm-host` may use `f32` for metre conversion and transforms, exactly like `lm-host-model::map`. Positions come from `lm-core`'s fixed-point `Pos` and are converted once, at the render boundary. `lm-archive` uses **no floats at all** — it is a verifier, so it inherits `lm-core`'s discipline (`#![deny(clippy::float_arithmetic)]`, `#![forbid(unsafe_code)]`).
- **L0 only.** Only L0 chunks render (spec §6). L1 (baked GI in vertex colours), L2 (textures/set dressing) and L3 (neural vistas) are M2. **The renderer must not grow a path for them**: L0 geometry comes from the seed's grid and nothing else. D14's fidelity ladder is *not* started here.
- **Clean proxy-friendly output.** Flat material classes only: one `StandardMaterial` per material class, `unlit` or flat-lit, **no textures, no normal maps, no PBR features**. Every surface's identity is carried by **vertex colour**, so a future neural renderer can read the same buffer. This is a hard constraint, not a style preference.
- **The 3D view is a *view*, not a second source of truth.** It renders the same `Sim` the 2D map renders. Any disagreement between the 2D map and the 3D view is a bug in the 3D view.
- **Interpolation happens in the render layer only.** The sim ticks at `tick_hz`; the 3D view interpolates between `prev` and `cur` positions using the same overstep fraction as the 2D map. Positions are never extrapolated and never fed back.
- **Nothing in the render path may block the sim thread.** Chunk meshing and agent figure building happen on the render side or in a worker; the sim thread only publishes positions.
- **The Orin archive is strictly read-only over the log.** It stores, replays and reports. It never rewrites a log, never "fixes" a mismatch, and never edits a hash. A mismatch is reported, with the first divergent tick.
- **The archive yields to the desktop.** It runs at a lower CPU priority and an idle I/O class by default, and both are parameters.
- **The archive adds no async runtime.** `std::net` + `std::thread` only. No tokio, no async, no `unsafe`.
- **Secrets:** the archive service has no credentials and no network egress. It accepts connections on the LAN and writes to its own store. It never calls out.
- **Every tunable is a parameter** (D19): internal resolution, upscale mode, palette, mesh budget, archive store path, listen address, nice/ionice values, and the archive buffer cap all have defaults listed in the tasks.
- **Commits as in Phase 1** (conventional prefixes, `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`), and `scripts/check.sh` green in debug and release before each merge.

---

## Review Focus

1. **A chunk mesh is wrong** (a wall is see-through, a floor is missing, a quad is doubled): the mesh builder is the seam. It must be verified by *counting quads against the grid*, not by eyeballing a PNG → `walls_get_faces`, `floor_covers_every_floor_cell`, `no_quad_crosses_a_wall` (Task 3).
2. **The camera moves and the world swims.** Chunks are anchored in **world space**, never in screen space, and a chunk's mesh is cached by its **structure hash** (D46), not by visibility. A follow camera that re-keys per frame is the exact failure E1 measured (68% of tiles re-keyed per frame) → `chunk_cache_is_world_anchored` (Task 4).
3. **The first frame after load takes 35–40 s.** `night-street` measured that shader compile on an RTX 4060; a mid-round stall is a **bug**, not a loading inconvenience. Pipelines and chunk meshes must be warm before the round starts → `load_warms_before_first_round`, `no_pipeline_compile_during_round` (Task 6).
4. **The internal resolution is wrong** (upscaled by a non-integer factor, so the retro look is mush, or the blit is upside down): the blit must be an **integer** nearest-neighbour scale by default, and the image must not be flipped → `integer_upscale_is_exact`, `blit_is_not_flipped` (Task 7).
5. **The Orin pushes a truncated or corrupt log** (a half-written file, a wrong magic, a broken hash chain): the archive must reject it with a *named* reason and keep serving, never crash and never report a false OK → `truncated_log_rejected`, `broken_chain_rejected`, `bad_magic_rejected` (Task 10).
6. **The Orin is busy** (the desktop needs the CPU): the archive must stay responsive to new connections and finish a verification without starving the box → `verification_is_yielded`, `accept_loop_stays_responsive` (Task 11).
7. **x86 and ARM disagree.** The Orin runs the same golden check; a mismatch means the sim is not portable, and that is the single most important thing this phase can discover → `golden_ok_on_orin` (Task 12).

---

## File Structure

```
crates/lm-render/
  Cargo.toml                          deps: lm-core (no bevy)
  src/lib.rs                          module tree; RenderConfig (+ defaults)
  src/palette.rs                      the shared palette: squad colours, material classes
  src/mesh.rs                         ChunkMesh: grid cells -> quads (positions, normals, colours)
  src/greedy.rs                       greedy mesher: merge coplanar same-material quads
  src/hash.rs                         chunk structure hash (D46): world tile + structure -> u64
  src/figure.rs                       agent figure: low-poly mesh + squad colour
crates/lm-host/
  src/view3d/mod.rs                   NEW: the View3d plugin; mode switch
  src/view3d/chunks.rs                chunk entities: Mesh3d + MeshMaterial3d per chunk
  src/view3d/agents.rs                agent entities: instanced figures, interpolated transforms
  src/view3d/upscale.rs               internal-resolution camera + integer nearest-neighbour blit
  src/view3d/warmup.rs                pipeline/mesh warmup before the round starts
crates/lm-archive/
  Cargo.toml                          deps: lm-core, lm-tools (lib); std-only networking
  src/lib.rs                          module tree; ArchiveConfig (+ defaults)
  src/server.rs                       TcpListener accept loop; thread per connection
  src/protocol.rs                     framed transport over lm-proto's Msg codec
  src/store.rs                        atomic log writes (temp + rename); the on-disk layout
  src/verify.rs                       headless replay + hash comparison; the report
  src/priority.rs                     nice/ionice via std::process (no libc)
  src/bin/lm-archive.rs               the service binary
scripts/win-build.sh                  (Phase 2) used for every visual check in this phase
docs/notes/bevy-0.20.md               (Phase 2) the API facts; extended here with the 3D ones
```

---

### Task 1: The palette and the render config

**Files:** Create `crates/lm-render/{Cargo.toml,src/lib.rs,src/palette.rs}`.

**Interfaces:**

- `RenderConfig` (serde, `Default`), read from an optional `render.toml`:

| field | default | field | default |
|---|---|---|---|
| `internal_width` | 960 | `internal_height` | 540 |
| `upscale` | `"integer"` (`"integer" \| "dlss" \| "off"`) | `palette` | `"default"` |
| `view_distance_chunks` | 3 | `fog_near_m` / `fog_far_m` | 40 / 120 |
| `wall_height_m` | 2.6 | `cover_height_m` | 1.1 |
| `greedy` | `true` | `max_chunks` | 64 |
| `dlss_quality` | `"quality"` | `dlss_project_id` | `""` (required when `upscale = "dlss"`) |

- `Palette` (serde, `Default`): the **single shared palette** (D2). Squad colours are indexed by `squad: u8` and are read by the 2D map, the piano roll and the 3D view alike, so a squad is the same colour everywhere:
  - `squad_colors: [Color; 4]` — four hues, maximally separated, all readable against the floor colour.
  - `material_colors: { floor, wall, cover, beacon_held, beacon_free }` — the **flat material classes** (proxy-friendly output).
  - `leader_rim: Color` — the leader's marker, distinct in luminance as well as hue so it survives colour-blindness and greyscale.
- `Palette::squad_color(squad: u8) -> Color`; `Palette::class_color(Class) -> Color`; `Palette::agent_color(squad: u8, leader: bool) -> Color`.

- [ ] Tests:
  - `palette_is_deterministic` (same config → same colours);
  - `squad_colors_are_distinct` (the four squad colours differ pairwise in RGB);
  - `squad_colors_survive_greyscale` (their luminances differ pairwise — the leader rim stays readable);
  - `class_colors_are_flat` (every material class is exactly one colour, no gradient).

### Task 2: The chunk mesh builder (grid → quads)

**Files:** Create `crates/lm-render/src/mesh.rs`.

**Interfaces:**

- The world is `lm_core::world::World`: `grid: Grid` of `CellKind::{Floor, Wall, Cover}` cells, `cell_units: i32` units per cell edge, `UNITS_PER_M = 1024`. The grid is `world_chunks.0 * chunk_m / cell_m` cells per axis (default 8×8 chunks of 32 m at 2 m/cell → **128×128 cells**). A chunk is `chunk_m / cell_m` cells per axis (default **16×16 cells** = 32 m).
- `ChunkCoord { cx: u8, cy: u8 }` — chunk indices, world-anchored.
- `ChunkMesh { positions: Vec<[f32;3]>, normals: Vec<[f32;3]>, colors: Vec<[f32;4]>, indices: Vec<u32> }` — plain data, no Bevy.
- `build_chunk(world: &World, chunk: ChunkCoord, cfg: &RenderConfig, palette: &Palette) -> ChunkMesh`:
  - **Walls** are boxes: a `Wall` cell emits a full-height box (four side quads + a top quad; no bottom, it is never seen). A wall block's side faces are emitted **only where the neighbour is not a wall**, so a solid block yields 4 quads, not 4 per cell.
  - **Cover** is a low box: four side quads + a top quad, at `cover_height_m`.
  - **Floor** is a single quad per cell at y = 0.
  - **Colours** come from the palette's material classes, with a **deterministic per-cell value jitter** (a small hash-derived factor) so flat shading still reads as texture. The jitter is derived from the cell coordinates, so it is identical on every machine and every run.
  - **Normals** are per-face, flat (no smoothing): the proxy-friendly requirement.
- `ChunkMesh::quad_count() -> usize`, `ChunkMesh::triangle_count() -> usize`.
- [ ] Tests:
  - `floor_covers_every_floor_cell` (the floor quad count equals the number of `Floor` cells in the chunk);
  - `walls_get_faces` (a 2×2 block of `Wall` cells produces 4 side quads + 1 top quad, **not** 16 sides);
  - `no_quad_crosses_a_wall` (every emitted side quad's footprint lies in a cell whose neighbour is not a wall);
  - `mesh_is_deterministic` (two builds of the same chunk are byte-identical);
  - `colors_are_in_the_palette` (every vertex colour is one of the class colours, times the jitter factor);
  - `chunk_edge_is_seamless` (two adjacent chunks share the same edge quad positions — no gaps and no overlap).

### Task 3: The greedy mesher

**Files:** Create `crates/lm-render/src/greedy.rs`.

**Interfaces:**

- Greedy meshing merges coplanar, same-material quads into the fewest possible larger quads (Mikola Lysenko's "Greedy Meshing", 0fps, 2012; the Rust shape is `building_blocks_mesh::greedy_quads` — <https://docs.rs/building_blocks_mesh/latest/building_blocks_mesh/greedy_quads/>).
- `greedy_merge(quads: &[RawQuad]) -> Vec<RawQuad>` where `RawQuad { origin: [i32;3], u_axis: [i32;3], v_axis: [i32;3], u_len: i32, v_len: i32, material: u8 }` — a face in **integer cell space**, so merging is exact and never accumulates float error.
- The greedy pass runs **per material class**, so a merged quad never spans two colours. It runs **per face direction** (the six axis directions), so a merged quad is always planar.
- `build_chunk` calls `greedy_merge` when `cfg.greedy` is true and returns the unmerged quads otherwise. **The greedy output must be visually identical to the unmerged output** — that is the whole risk of the task, and it is testable.
- [ ] Tests:
  - `greedy_reduces_quad_count` (a 16×16 open floor becomes 1 quad, not 256);
  - `greedy_preserves_geometry` (the merged quads cover exactly the same total area as the unmerged ones — compared as a set of covered cell footprints);
  - `greedy_never_merges_across_materials` (no merged quad contains two different material ids);
  - `greedy_is_deterministic`;
  - `greedy_matches_unmerged_visually` (the two meshes produce the same rendered pixels in a headless rasteriser stub — a footprint comparison, not a GPU check).

### Task 4: The chunk structure hash (D46)

**Files:** Create `crates/lm-render/src/hash.rs`.

**Interfaces:**

- `structure_hash(world: &World, chunk: ChunkCoord, cfg: &RenderConfig) -> u64` — an FNV-1a-64 over the chunk's **structure only**: the `CellKind` tag of every cell in the chunk, in canonical row-major order, plus the wall/cover heights and the palette version. It does **not** include positions, ticks, or anything else that changes per round.
- `ChunkKey { cx: u8, cy: u8, structure: u64 }` — the **world-anchored** cache key (D46). The key contains the **world tile**, never a screen tile.
- `ChunkCache { map: HashMap<ChunkKey, Handle<Mesh>> }` with `get_or_build(...)`. The cache is **content-addressed**: the same structure at a different world position produces the same hash and therefore **reuses the same mesh asset** — which is the whole point of D46 (E1 measured 95.6% LRU hit rates for world-anchored keys against ≤61% for view-anchored).
- [ ] Tests:
  - `hash_is_deterministic` (same chunk → same hash);
  - `hash_changes_with_structure` (adding one wall cell changes the hash);
  - `hash_ignores_non_structure` (the hash is independent of tick, agent positions and squad count);
  - `chunk_cache_is_world_anchored` (two chunks with identical structure **share one mesh asset**; the cache is keyed by world tile + structure, never by screen position);
  - `cache_survives_camera_movement` (panning the camera 50 times re-keys no chunk — the direct E1 regression).

### Task 5: The agent figure

**Files:** Create `crates/lm-render/src/figure.rs`.

**Interfaces:**

- `build_figure(palette: &Palette) -> FigureMesh` — a low-poly humanoid: a body box, a head box, and two leg boxes (about 40 triangles). **One mesh, shared by every agent.** The squad colour is *not* baked into the mesh; it comes from the per-instance material, so one mesh serves every agent (instancing) and the palette can change without rebuilding geometry.
- `FigureMesh { positions, normals, indices }` — positions in metres, origin at the feet, +Z forward.
- [ ] Tests:
  - `figure_is_low_poly` (≤ 64 triangles);
  - `figure_has_a_head` (the head box's vertices are above the body box's);
  - `figure_is_shared` (every agent uses the same `FigureMesh` — one asset, many instances);
  - `figure_origin_is_at_the_feet` (the minimum Y is 0).

### Task 6: The Bevy 3D view — chunks, agents, warmup

**Files:** Create `crates/lm-host/src/view3d/{mod.rs,chunks.rs,agents.rs,warmup.rs}`. Modify `crates/lm-host/src/main.rs`.

**Interfaces (Bevy 0.20.1 API, verified against the 0.20.1 sources in `~/.cargo/registry/src`):**

- **Meshes.** A `Mesh` is built with `Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())` (`bevy_mesh-0.20.1/src/mesh.rs:412`) and filled with `mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, …)` / `insert_attribute(Mesh::ATTRIBUTE_NORMAL, …)` / `insert_attribute(Mesh::ATTRIBUTE_COLOR, …)` (`mesh.rs:445`) plus `mesh.insert_indices(Indices::U32(…))` (`mesh.rs:757`). `Mesh::ATTRIBUTE_COLOR` is `bevy_mesh-0.20.1/src/mesh.rs:388`; it is a `Float32x4` (`VertexAttributeValues`, `bevy_mesh-0.20.1/src/vertex.rs:321`). **`Mesh::try_insert_attribute` (not `insert_attribute`) is used inside asset-building code**, because `insert_attribute` panics if the mesh has already been extracted to the render world (`mesh.rs:440-452`).
- **Materials.** One `StandardMaterial` per material class, e.g. `StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() }` — `unlit` is `bevy_pbr-0.20.1/src/pbr_material.rs:677`. The material is `base_color: Color::WHITE` because **the vertex colour carries the identity**: `bevy_pbr` adds the `VERTEX_COLORS` shader define automatically when the mesh's attribute layout contains `ATTRIBUTE_COLOR` (`bevy_pbr-0.20.1/src/render/mesh.rs:3465-3468`), and the PBR fragment shader multiplies it into the base colour (`bevy_pbr-0.20.1/src/render/pbr_fragment.wesl:51`). This is the mechanism the whole "flat material classes + vertex colours" design depends on, and it is what makes the output proxy-friendly: no texture is ever sampled.
- **Spawning.** `Mesh3d(handle)` is `bevy_mesh-0.20.1/src/components.rs:103`; `MeshMaterial3d(handle)` is `bevy_pbr-0.20.1/src/mesh_material.rs:41`. `bevy 0.20` has **no `MaterialMeshBundle`** — spawn the two components.
- **Agents as instances.** Bevy 0.20 **batches automatically**: entities with the same mesh and material are merged into one GPU instance buffer by `bevy_render`'s batching pipeline (`bevy_render-0.20.1/src/batching/mod.rs`; `NoAutomaticBatching` is the opt-out). So N agents sharing one `FigureMesh` and one `StandardMaterial` per squad are **instanced by the engine** — there is no `Instance` component to write. Each agent entity still owns a `Transform`, which is what makes the interpolation trivial: the agent's `Transform` is set from the lerped position each frame.
- **Camera.** `Camera3d::default()` plus a `Transform` (`bevy_camera-0.20.1/src/lib.rs` prelude re-exports `Camera`, `Camera2d`, `Camera3d`, `PerspectiveProjection`).
- **Lights.** One `DirectionalLight` (the sun) plus a weak `AmbientLight`. Flat, unlit materials need no light, but the agent figures use a simple lit material so they read as solid.
- **Interpolation.** The 3D view reads the same `prev_positions` the 2D map uses (`lm-host-model::map::MapModel::build(…, alpha)`) and lerps the same way. The agent's `Transform.translation` is set from the lerped position in metres, and `Transform.rotation` from the interpolated facing.
- **Warmup (the `night-street` lesson).** `fleet/06-code-generated-3d.md` records 35–40 s to first frame on an RTX 4060 from shader compilation. Two mitigations, both required:
  1. **The materials and shaders are fixed**, not generated per round. There is exactly one `StandardMaterial` per class and one figure material per squad; nothing is compiled mid-round.
  2. **The warmup happens before the round.** At load, the app builds one chunk mesh and one figure mesh and spawns them **off-camera** for one frame, so `bevy_render`'s `PipelineCache` compiles the pipelines then. Bevy exposes the compile state as `PipelineCache::get_render_pipeline_state(id) -> &CachedPipelineState` (`bevy_render-0.20.1/src/render_resource/pipeline_cache.rs:294`), with `CachedPipelineState::{Queued, Creating(..), Ok(..), Err(..)}` (`pipeline_cache.rs:48`). The warmup screen waits until the state is `Ok` before the round starts.
  - A `NoAutomaticBatching` opt-out is **not** needed; but `Mesh3d` entities must not be added and removed every frame, or the pipeline cache churns.
- [ ] Tests (model-side, in `lm-render`, no GPU):
  - `chunk_entities_match_visible_chunks` (the set of spawned chunk entities equals the set of chunks within `view_distance_chunks`);
  - `agent_entities_match_roster` (one entity per agent in the roster, no more);
  - `agent_transform_interpolates` (at alpha 0.5 the figure sits halfway between two ticks' positions);
  - `agent_colour_is_the_squad_colour` (the material of agent *i* is its squad's colour);
  - `warmup_covers_every_material_class` (every class's material is compiled before the round starts);
- [ ] **Visual acceptance (screenshot PNG, via `scripts/win-build.sh` from Phase 2):**
  - `bash scripts/win-build.sh phase6-3d run -p lm-host --release -- --seed 1 --view 3d --screenshot C:\\Users\\casey\\src\\shot-3d.png --frames 120`. The reviewer opens the PNG and confirms: a low-poly world of boxes, agents visible as coloured figures, no missing floor, and the retro low-resolution look.
  - Repeat with `--view 3d --window 1280x720` and `--window 640x360`.
  - The 3D view must show **the same world as the 2D map for the same seed** — the reviewer compares the two screenshots.

### Task 7: Internal resolution and integer upscale

**Files:** Create `crates/lm-host/src/view3d/upscale.rs`.

**Interfaces:**

- Two cameras:
  - The **world camera** renders the 3D scene into an offscreen `Image` at `internal_width × internal_height` (default 960×540). The mechanism is Bevy 0.20's `RenderTarget`: `RenderTarget::Image(ImageRenderTarget { handle, scale_factor: 1.0 })` — `bevy_camera-0.20.1/src/camera.rs:893` and `:987`; the `Image` is created with `Image::new_target_texture(w, h, TextureFormat::Rgba8Unorm, Some(TextureFormat::Rgba8UnormSrgb))` (`bevy_image-0.20.1/src/image.rs:1253`), which sets `TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT` automatically. The official 0.20.1 example `bevy-0.20.1/examples/3d/render_to_texture.rs` is the reference for this wiring.
  - The **blit camera** draws that image to the window with a **nearest-neighbour** sampler at an **integer** scale.
- **Why an integer scale:** D2's retro-modern look needs whole-pixel cells. The scale is `floor(min(window_w / internal_w, window_h / internal_h))`, clamped to ≥ 1, and the remainder is **letterboxed** (the clear colour shows), never stretched.
- **The blit.** Bevy 0.20's supported full-screen path is the `FullscreenMaterial` trait (`bevy_core_pipeline-0.20.1/src/fullscreen_material.rs:80`) with `FullscreenMaterialPlugin` (`:49`), which draws a fullscreen triangle using `bevy_core_pipeline`'s `fullscreen_vertex_shader.wesl`. The blit material samples the offscreen image with a nearest sampler and applies the integer scale + letterbox in the fragment shader. **Note for the implementer:** Bevy 0.20 removed `ShaderMaterial`; the material is a `Material` impl with `fn fragment_shader() -> ShaderRef` (`bevy_pbr-0.20.1/src/material.rs:95-160`).
- **DLSS (optional, `upscale = "dlss"`).** Bevy 0.20 has DLSS in `bevy_anti_alias` behind the `dlss` feature: the `Dlss` component (`bevy_anti_alias-0.20.1/src/dlss/mod.rs:205`), `DlssInitPlugin` (`:60`) and `DlssPlugin` (`:132`), and the `DlssProjectId` resource (`:388`) that **must** be inserted before `DlssInitPlugin` (the plugin panics otherwise — `mod.rs:66`). Support resources are `DlssSuperResolutionSupported` (`:393`) and `DlssRayReconstructionSupported` (`:310`). `DlssPlugin::finish` checks `AdditionalVulkanFeatures` for support and **returns early if unsupported**, so DLSS degrades to off rather than failing. **DLSS is RTX-only** (NVIDIA, Vulkan, Windows/Linux) and needs the `dlss_wgpu` crate (v6.0.0), which is **not** in the local cargo registry — it must be fetched from crates.io (verified reachable: `index.crates.io/dl/ss/dlss_wgpu` → 200, `dlss_wgpu-6.0.0.crate` → 200). It is therefore an **optional feature flag**, off by default, so the main build stays offline-buildable.
- [ ] Tests (model-side, in `lm-render`):
  - `integer_scale_is_exact` (for every window size in a table, the computed scale is an integer ≥ 1 and the blit covers `scale × internal` pixels exactly);
  - `letterbox_is_centred` (the remainder is split evenly, or the extra pixel goes right/down deterministically);
  - `upscaled_size_never_exceeds_the_window`;
  - `dlss_requires_a_project_id` (`upscale = "dlss"` with an empty `dlss_project_id` is a **config error at startup**, not a crash at first frame);
- [ ] **Visual acceptance:** the same `--screenshot` run as Task 6, plus a run at `--window 1280x720` with `internal = 640x360`, where the reviewer confirms the pixels are **chunky and uniform in size** (the integer upscale working) rather than smeared (a fractional upscale).

### Task 8: The 3D view's seam with the host (mode switch, fog, camera)

**Files:** Modify `crates/lm-host/src/main.rs`; create `crates/lm-host/src/view3d/mod.rs`.

**Interfaces:**

- The host gains a **view mode** parameter: `--view 2d | 3d | both` (default `2d` until Phase 6's done-when). `3d` hides the 2D map plugin and shows the 3D view; `both` shows the 3D view with the egui HUD and piano roll still on top (the piano roll is the M1 debugger and must keep working in 3D).
- **Fog of war in 3D.** The 2D map's fog rule (spectator sees all; a possessed agent sees only its squad's knowledge) is reused **verbatim**: the 3D view draws only what the viewer may know. Chunks outside the viewer's knowledge are not drawn at all; chunks inside it but beyond `fog_far_m` are drawn with a fog colour that ramps to the background.
- **Camera.** Free camera (WASD + drag, scroll to zoom) and follow-an-agent, matching the 2D view's controls so the human does not learn two schemes.
- **The 3D view reads the same `Sim`.** It is a consumer of `HostLoop`, never a second simulation.
- [ ] Tests:
  - `fog_matches_the_2d_map` (the same viewer state yields the same set of visible chunks in the 2D map model and the 3D view model — one shared function, one test);
  - `possessed_view_hides_unseen_chunks`;
  - `spectator_sees_every_chunk`;
  - `view_mode_switches_without_a_restart` (switching 2d↔3d does not reset the sim tick or the round state);
- [ ] **Visual acceptance:** `bash scripts/win-build.sh phase6-3d run -p lm-host --release -- --seed 1 --view both --screenshot C:\\Users\\casey\\src\\shot-3d-both.png --frames 120`. The reviewer confirms the piano roll and HUD are still drawn over the 3D view.

### Task 9: `lm-archive` — the crate, the store, the protocol

**Files:** Create `crates/lm-archive/{Cargo.toml,src/lib.rs,src/store.rs,src/protocol.rs,src/bin/lm-archive.rs}`.

**Interfaces:**

- `ArchiveConfig` (serde, `Default`), from an optional `archive.toml`:

| field | default | field | default |
|---|---|---|---|
| `listen` | `"0.0.0.0:8790"` | `store_dir` | `"/var/lib/last-mile/archive"` |
| `max_log_bytes` | 256 MiB | `verify_on_arrival` | `true` |
| `retain_rounds` | 0 (keep all) | `nice` | 10 |
| `ionice_class` | 3 (idle) | `workers` | 1 |

- **The store.** `store.rs` owns the on-disk layout:
  - `<store_dir>/rounds/<seed>-<unix_secs>.lml` — the log, written **atomically**: write to `<name>.lml.tmp`, then `std::fs::rename` (atomic on Linux within a filesystem; `crates/lm-archive/src/store.rs`). A partially-written log is therefore **never** visible to the verifier.
  - `<store_dir>/reports/<name>.json` — the verification report.
  - `<store_dir>/state.json` — the service's own state (last verified round, counters), rewritten atomically the same way.
- **The protocol.** The ProArt pushes the **header plus the stamped decision stream**, which is exactly `lm_proto::Msg` — `Msg::Header(Header)`, `Msg::Stamped(Stamped)`, `Msg::Bye { reason }` (`crates/lm-proto/src/lib.rs:43`). `lm_proto::encode`/`decode` (`lib.rs:78`/`:88`) are the existing postcard codec, and `decode` **rejects trailing bytes** (`lib.rs:89-92`). The archive **reuses it and adds no new codec**: framing over the wire is `len: u32 LE` + postcard bytes, mirroring `lm-core`'s own blob framing (`crates/lm-core/src/log.rs:281`).
  - `handle_connection(stream: TcpStream, cfg: &ArchiveConfig) -> Result<IngestReport, IngestError>` — reads framed messages until `Msg::Bye`, reconstructs the `RoundLog` (header + segments), writes it atomically, and (when `verify_on_arrival`) hands it to the verifier.
- [ ] Tests:
  - `framed_roundtrip` (header + decisions + bye → the same `RoundLog`);
  - `trailing_bytes_rejected` (a frame with extra bytes is refused, matching `lm-proto`'s rule);
  - `atomic_write_leaves_no_partial` (a `kill -9` style abort leaves only the `.tmp`, never a half `.lml`);
  - `store_layout_is_flat` (the on-disk paths are exactly as specified above);
  - `max_log_bytes_is_enforced` (an oversized push is refused **before** it is buffered);
  - `unknown_message_rejected` (a `Msg` this build does not understand is refused with a named reason, not ignored).

### Task 10: The verifier (replay every round, verify every hash)

**Files:** Create `crates/lm-archive/src/verify.rs`.

**Interfaces:**

- This is the heart of the crate, and it is **already written**: `lm_tools::runner::replay(log) -> Result<Vec<(Tick, u64)>, ReplayError>` (`crates/lm-tools/src/runner.rs:197`) re-runs the sim over the log's stamped decisions and compares **every** state hash. `ReplayError::Diverged { tick, expected, got }` (`runner.rs:80`) carries the **first divergent tick** — exactly what spec §7 asks to be reported.
- `verify_log(log: &RoundLog) -> VerifyReport` where `VerifyReport { ok: bool, hashes_checked: usize, first_divergence: Option<(Tick, u64, u64)>, header_hash: u64 }`.
- `verify_file(path: &Path) -> VerifyReport` — reads the log with `lm_tools::runner::read_log` (`runner.rs:247`), which verifies the **hash chain** before returning (`crates/lm-core/src/log.rs:222`), then replays it.
- The report is written to `<store_dir>/reports/<name>.json` and is the **report to the ProArt HUD** that spec §7 requires. The wire format for that report is `lm_proto::Msg::StateHash { tick, hash }` for the success case and a new `Msg::Bye { reason }` with a descriptive reason for the failure case (a mismatch is reported, never silently swallowed).
- [ ] Tests:
  - `verify_accepts_a_good_log` (a log produced by `lm_tools::runner::run` verifies clean, with `hashes_checked` equal to the number of recorded hashes);
  - `verify_catches_a_tampered_hash` (one hash byte changed → `ok == false`, `first_divergence` is that tick, and the reported expected/got are the two values);
  - `verify_catches_a_tampered_decision` (a decision's payload changed → divergence at the tick it applies);
  - `truncated_log_rejected` (a truncated file → a named error, **not** a false OK);
  - `broken_chain_rejected` (a segment's `prev` altered → `LogError::ChainBroken`, not a false OK);
  - `bad_magic_rejected` (`LogError::BadMagic`);
  - `no_false_ok` (every rejection path leaves `ok == false` and a reason — the property that matters most here).

### Task 11: The service loop, the yielding, and the concurrency

**Files:** Create `crates/lm-archive/src/server.rs` and `crates/lm-archive/src/priority.rs`.

**Interfaces:**

- **The accept loop.** `std::net::TcpListener::bind(&cfg.listen)?` then a loop over `listener.accept()`. **One `std::thread::spawn` per connection** (`std::thread::spawn` — `std::net::TcpListener::accept` returns `(TcpStream, SocketAddr)`). Each connection sets `stream.set_read_timeout(Some(Duration::from_secs(cfg.read_timeout_s)))` and `stream.set_nodelay(true)` so a stalled peer cannot pin a thread forever.
- **The verification worker** is a **separate thread** with a bounded queue, so a slow replay never blocks the accept loop (spec §8: "Orin unreachable → the ProArt buffers logs locally and flushes on reconnect" — the Orin must never become the bottleneck that stalls the ProArt).
- **Yielding to the desktop.** There is **no std API** for `setpriority`/`ioprio_set` — `std::os::unix::process::CommandExt` exposes only `uid`, `gid`, `groups`, `pre_exec`, `exec`, `arg0`, `process_group`, `chroot`, `setsid` (verified against the rustdoc generated by rustc 1.99.1). `libc` **is** in the local registry (0.2.191), but the crate stays std-only by design, so the priority is applied by **wrapping the command**:
  - `nice -n <cfg.nice>` — raises niceness (lowers CPU priority). Range −20 (most favourable) to 19 (least favourable); `nice -n N` **adds** N to the current niceness. Default **10**, which is `nice`'s own default and a conventional "background" value.
  - `ionice -c <cfg.ionice_class>` — I/O scheduling class; **3 = idle** (I/O only when nothing else is pending). Default **3**.
  - Both binaries exist on this machine (`/usr/bin/nice`, `/usr/bin/ionice`); they must be confirmed present on the Orin before deployment. `priority.rs` provides `pub fn apply_to_self(cfg: &ArchiveConfig) -> io::Result<()>` which **re-execs** the process under `nice`/`ionice` (guarded by an env var so it happens exactly once), and a `pub fn is_low_priority() -> bool` that reads `/proc/self/stat` field 19 to **verify** the priority actually took (because `nice` can only raise niceness; the value may be capped at 19).
  - **The "desktop" premise is unverified in the repo** — the spec only says the Orin "archives and verifies". Confirm on the box what the Orin actually runs (`ps -eo comm | grep -iE 'xorg|wayland|gnome|kde'`) before assuming it must yield. The yielding is on by default either way, because it is harmless insurance.
- [ ] Tests:
  - `accept_loop_stays_responsive` (while a verification is in flight, a new connection is still accepted and greeted promptly);
  - `verification_is_yielded` (the service's own `/proc/self/stat` niceness is ≥ the configured value, and the I/O class is idle);
  - `slow_peer_does_not_pin_a_thread` (a connection that sends nothing is dropped after `read_timeout_s`);
  - `oversized_push_is_refused_early` (the connection is closed before the whole body is buffered);
  - `service_survives_a_bad_connection` (a client that disconnects mid-frame leaves the service running and accepting);
  - `re_exec_is_idempotent` (the env-var guard prevents an exec loop).

### Task 12: The aarch64 build and the cross-architecture golden check

**Files:** Create `scripts/orin-build.sh`; extend `crates/lm-archive` with a `golden` subcommand.

**Interfaces:**

- **The build.** `rustup target add aarch64-unknown-linux-gnu` installs the target's std (the toolchain is pinned to 1.99 by `rust-toolchain.toml`; `static.rust-lang.org` is reachable). A **cross-linker is required**: `cargo build --target aarch64-unknown-linux-gnu` needs an aarch64 linker, and this machine has **no** `aarch64-linux-gnu-gcc`, no `cross`, no `zig`, and no `.cargo/config.toml`.
  - **Decision: build natively on the Orin, not by cross-compiling.** This matches the repo's existing strategy (the Phase 3 plan's cross-arch handoff is "ask the Thor's agent to `git clone` the repo and `cargo run --release --bin lm -- golden`"), avoids the glibc-version skew of a cross-built binary, and needs no new toolchain on the ProArt. `scripts/orin-build.sh` therefore **drives the Orin over SSH**: clone/pull the branch, `cargo build --release`, and run the golden check. The alternative (cross-compile with `gcc-aarch64-linux-gnu` + a `.cargo/config.toml` linker, or `cargo-zigbuild` for a static musl binary) is recorded in the plan as the fallback if the Orin cannot build Rust.
  - **The cross-arch golden check runs on the Orin** and is the x86↔ARM proof (spec §9, D10/D26). The existing mechanism is already in place: `cargo run --release --bin lm -- golden` compares the three pinned seeds in `tests/golden.toml` against a fresh 3000-tick run and prints `OK 3/3` on success (exit 0) — verified: the committed `tests/golden.toml` and a regenerated file are byte-identical. `lm-archive` gains a `golden` subcommand that runs the same check **natively on the Orin** and writes its result into the store's report directory, so the ProArt HUD can show the ARM verdict.
  - **Known asymmetry (already handled in Phase 1):** two tests in `crates/lm-tools/src/main.rs` are `#[cfg(target_arch = "x86_64")]`-gated because the host libm's `sin` may differ on other architectures; the cross-arch check is the pinned `TRIG_TABLE_HASH`. This is why the golden check, not the test suite, is the ARM gate.
- [ ] Tests:
  - `golden_ok_on_orin` (on the Orin: `cargo run --release --bin lm -- golden` prints `OK 3/3` and exits 0) — **the phase's headline check**;
  - `golden_report_is_recorded` (the Orin's result lands in the store's report directory, and the ProArt can read it);
  - `build_script_is_idempotent` (running `scripts/orin-build.sh` twice is safe and reports the same result);
  - `golden_failure_is_reported` (a deliberately wrong pinned hash produces a **non-zero exit and a named mismatch**, not a silent pass — the property that makes the check worth running);
- [ ] Human acceptance (Casey, on the ProArt): the full M1 loop on all three boxes —
  1. `lm-host --seed 1 --view 3d` on the ProArt: play a round in 3D;
  2. the round's `.lml` reaches the Orin (either pushed over the TCP link or copied);
  3. the Orin verifies it and the ProArt's HUD shows the verification verdict;
  4. the Orin runs `lm golden` and reports `OK 3/3`;
  5. **the loop closes**: a round played on the ProArt, verified on the Orin, replayed bit-exact on the ProArt.

---

## Done-when (spec §10 phase 6)

**Phase 6 is done when the full M1 loop runs on all three boxes.** Concretely:

1. The 3D retro-modern view runs on the ProArt at L0: low-poly, vertex-coloured, low internal resolution with an integer upscale, agents as instanced squad-coloured figures, and the world identical to the 2D map for the same seed.
2. `lm-archive` runs on the Orin, receives pushed logs, and **verifies every state hash** of every round, reporting the first divergent tick on a mismatch.
3. The cross-architecture golden check passes on the Orin: `OK 3/3`.
4. The loop closes end to end: a round is played on the ProArt, archived and verified on the Orin, and replays bit-exact.

**Out of M1 (recorded so nobody builds it early):** L1–L3 fidelity (baked GI in vertex colours, textures, neural vistas), the asset bank and 👍/👎, the Thor's prerender-ahead (D14), delta regeneration (D48's repair path), a palette-quantisation/dithering pass, and DLSS as a default rather than an opt-in.

### Later (handoffs)

- **The Thor's prerender-ahead (D14)** consumes the same chunk structure hash this phase defines: the Thor generates the *same* L0 geometry from the seed and bakes on top of it, so only bake results travel. The `ChunkKey` is the seam.
- **The asset bank on the Orin** (research/11) is a content-addressed directory keyed by the same structure hash, so a liked chunk is reused rather than regenerated.
- **QUIC transport** replaces the TCP stream when `lm-proto` grows one: the archive's framing already mirrors `lm-proto`'s message boundaries, so the swap is one module.

---

### Sources

- Spec: `docs/superpowers/specs/2026-10-28-last-mile-m1-design.md` §2, §6, §7, §9, §10, §11.
- Decisions: `docs/design/decisions.md` — D2, D14, D46, D48, D50.
- Plans (format + precedent): `docs/superpowers/plans/2026-10-28-m1-phase{2-host,3-agents,5-learning}.md`.
- Research: `docs/research/forclaude2/11-prerender-ahead.md`; `docs/research/fleet/06-code-generated-3d.md` (the 35–40 s shader-compile measurement); `docs/research/fleet/05-where-this-leads.md` (E1/D46, E3/D48); `docs/research/poc/tiles/RESULTS.md` (E1 numbers).
- Bevy 0.20.1 (read from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`):
  - `bevy_mesh-0.20.1/src/mesh.rs:412` (`Mesh::new`), `:445` (`insert_attribute`), `:757` (`insert_indices`), `:388` (`ATTRIBUTE_COLOR`); `bevy_mesh-0.20.1/src/components.rs:103` (`Mesh3d`); `bevy_mesh-0.20.1/src/vertex.rs:321` (`VertexAttributeValues`).
  - `bevy_pbr-0.20.1/src/pbr_material.rs:677` (`StandardMaterial::unlit`); `bevy_pbr-0.20.1/src/mesh_material.rs:41` (`MeshMaterial3d`); `bevy_pbr-0.20.1/src/render/mesh.rs:3465-3468` (`VERTEX_COLORS` define); `bevy_pbr-0.20.1/src/render/pbr_fragment.wesl:51` (vertex colour × base colour); `bevy_pbr-0.20.1/src/material.rs:95-160` (the `Material` trait; **`ShaderMaterial` is gone in 0.20**).
  - `bevy_camera-0.20.1/src/camera.rs:893` (`RenderTarget`), `:987` (`ImageRenderTarget`), `:385` (`Camera`); `bevy_camera-0.20.1/src/components.rs:16` (`Camera2d`); `bevy_camera-0.20.1/src/visibility/render_layers.rs:20` (`RenderLayers`).
  - `bevy_image-0.20.1/src/image.rs:1253` (`Image::new_target_texture`), `:714` (`ImageSampler::nearest`).
  - `bevy_core_pipeline-0.20.1/src/fullscreen_material.rs:80` (`FullscreenMaterial`), `:49` (`FullscreenMaterialPlugin`); `bevy_core_pipeline-0.20.1/src/fullscreen_vertex_shader.wesl` (the fullscreen triangle).
  - `bevy_anti_alias-0.20.1/src/dlss/mod.rs:60` (`DlssInitPlugin`), `:132` (`DlssPlugin`), `:205` (`Dlss`), `:388` (`DlssProjectId`), `:393`/`:310` (the support resources); `bevy_anti_alias-0.20.1/Cargo.toml:106-108` (`dlss_wgpu` v6.0.0, optional).
  - `bevy_render-0.20.1/src/render_resource/pipeline_cache.rs:48` (`CachedPipelineState`), `:294` (`get_render_pipeline_state`); `bevy_render-0.20.1/src/batching/mod.rs` (automatic GPU instancing; `NoAutomaticBatching`).
  - `bevy-0.20.1/examples/3d/render_to_texture.rs` (the render-target reference).
- Greedy meshing: Mikola Lysenko, "Greedy Meshing" (0fps, 2012) — <https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/>; the Rust shape in `building_blocks_mesh` — <https://docs.rs/building_blocks_mesh/latest/building_blocks_mesh/greedy_quads/>.
- `lm-core`/`lm-tools` (read in the repo): `crates/lm-core/src/log.rs` (the `LMLOG1` format, `Recorder`, `RoundLog::verify`), `crates/lm-core/src/hash.rs` (FNV-1a-64), `crates/lm-core/src/world/{grid,gen,mod}.rs` (`Grid`, `CellKind`, `World`), `crates/lm-tools/src/runner.rs` (`run`, `replay`, `read_log`, `golden_hashes`), `crates/lm-tools/src/main.rs` (`lm golden` → `OK 3/3`), `crates/lm-proto/src/lib.rs` (`Msg`, `encode`, `decode`).
- Toolchain facts verified locally: `rustc 1.99.1`; `rustup target list --installed` → x86_64 only; **no** aarch64 cross-linker, `cross` or `zig` installed; `/usr/bin/nice` and `/usr/bin/ionice` present; `dlss_wgpu` 6.0.0 fetchable from crates.io but **not** in the local registry (hence an optional feature).

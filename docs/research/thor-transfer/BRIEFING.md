# BRIEFING — for the agent working on last-mile

**Read `docs/design/decisions.md` first**, then `docs/research/forclaude2/00-overview.md`, then this.
Everything in this folder is *input* to the decision log; nothing here has been decided, and the
log is append-only.

This briefing is about one thing: **the Jetson Thor has a working asset lane and a measured
decision tier, and last-mile can use both.** It also says plainly what last-mile should *not* copy.

---

## 1. What the Thor can do today

Measured 2026-10-29. Ask the Thor rather than trusting this table — see §2.

| Capability | Where | Measured |
|---|---|---|
| **Photograph → textured triangle mesh** | SPAR3D, on the Thor | 31.9 s, 10,576 MB peak VRAM, 169,132 B finished |
| **Photograph → depth** | the geometry service, `:8099` | 78 ms warm, 223 ms round trip at 768 px |
| **Depth → UV-painted relief mesh** | the relief lane | 1.6–3.3 s, 48–50 KB |
| **Any mesh → game-ready, meshopt-compressed** | the finishing lane | 1.3 s, 192,000 B budget |
| **Author a generated mesh into a subject's frame** | the framing tool | 0.4 s, checked by three exact numbers |
| **Serve assets by hash, vouching for them** | the capture service, `:8098` | 17 tiers, 4 place-vouched, 0 problems |
| **Describe itself** | the live index, `:8100` | 9 services, 5 tools, 6 proofs, 17 tiers |
| **Answer typed questions fast** | the decision tier, `:8097` | d1-2B 98.4% text / 71.0% photo; clef-flash 100% / 87.1% |

Everything is LAN-open with no API key. The Thor is `thor.local` (192.168.0.78). **`proart.local`
does not resolve** — use the IP.

---

## 2. Ask the Thor, do not read a list

```bash
curl -s http://thor.local:8100/index.json | python3 -m json.tool | head -60
```

It answers, live: the services and their ports, the asset tiers with paths, byte counts and
sha256s, the tools, the proofs and whether their scripts are present, the decision models on disk,
and live memory and disk.

**Do not copy this table into a file.** A hand-written list of a machine's ports went stale in one
day: five services appeared, one went down, and the disk figure moved by 65 GiB.

---

## 3. What to build, in this order

### 3.1 Decide the delivery format — first, because everything else is load-bearing on it

A generated mesh ships as a **GLB with `EXT_meshopt_compression`**: 169 KB for 10,000 triangles,
opened in a browser by three.js's own bundled decoder, no wasm fetch and no CDN.

**Released Bevy 0.20.1 cannot read it** — the extension is ❌ in `bevy_gltf`'s support table, there
is no `meshopt` feature, and the loader has no decoder at all. Two paths:

- **(a) Rewrite the GLB before `bevy_gltf` sees it.** A plugin using the pure-Rust
  `meshoptimizer-rs` decoder — **no C++ toolchain**. Worked example:
  <https://github.com/paulrobello/hex-world> (`meshopt_glb.rs`, `MeshoptGlbPlugin`). ⚠ It ships **no
  NOTICE file** for meshoptimizer, which MIT requires — if you copy the pattern, ship the notice.
- **(b) Pin a Bevy build with upstream's support.** Bevy `main` has it (PR #25953, #26318) but it was
  **cut from the 0.20 release branch**, and it sits behind a non-default `meshopt` feature that pulls
  a **C++ FFI** crate. That means an unreleased Bevy and a C++ build step on the ProArt.

**Path (a) is the smaller commitment.** Do this **before** the generator work: it sets the byte
budget D16 plans against and the ProArt's 6 GB of VRAM.

**Write `EXT_meshopt_compression`**, not `KHR_`. `EXT` is the ratified one ("Complete, Ratified by
the Khronos Group"); `KHR_meshopt_compression` is a Release Candidate **superset** with an upgraded
v1 vertex format, not a rename, and the two must not be mixed on one buffer. three.js reads both.

### 3.2 Give the asset bank a vouching record

last-mile's asset ID is a hash. A hash needs a record that names the bytes:

```
{ path, sha256, bytes, contentType }   + the parameters that produced it
```

and the reader must verify the hash **before** handing bytes to a renderer. This is what turns the
Orin's bank from a pile of blobs into a bank that can refuse. The measured chain to copy is in
[`01-findings.md`](01-findings.md) §1.4.

### 3.3 Put a frame check in front of every bake

The Thor bakes onto geometry last-mile derives from a seed. The bake must land on *that* geometry.
If the two machines round the seed's coordinates differently, the bake lands in the wrong place —
**and nothing says so**, because a misplaced bake is a picture, not an error.

The mechanism and the three numbers that must match exactly are in
[`02-mechanisms.md`](02-mechanisms.md) §M2.

### 3.4 Derive the L0–L3 thresholds from pixels, not from round numbers

last-mile's ladder is L0–L3 with "any level is playable". The Otherhere lane built the same shape
and shipped a tier that was fetched, verified and drawn 6/6 — **and invisible**, because its
threshold was 140 m, and at 140 m the subject was 68 pixels tall and behind a hill.

A threshold must be derived from the **on-screen size** of the thing it replaces. See
[`02-mechanisms.md`](02-mechanisms.md) §M3.

### 3.5 Answer spike S1 from the measurements

S1 asks for decision-model latency on the Thor: 16 agents × 8 gates, p99, other tenants loaded.
What is already measured:

- **How a pack scales is not settled.** Upstream claims one pass for any number of questions (d1:
  "one forward pass … any number of questions about one state in one call"; Clef: "scores all
  options of all questions jointly"). **The Thor's measurement does not show flat scaling** — the
  600M was flat from 1 to 3 questions (15.0 → 15.4 ms) but 8 questions cost **83.0 ms**. Either the
  serving path batches at a limit or the upstream claim does not hold at this size. **Do not assume
  either way — this is the quantity S1 exists to measure.**
- **clef's one-pass pack was not faster per pack than d1 on this GPU** (284.4 ms vs 60.3 ms for
  three questions).
- **Local answers are reproducible** — identical across runs, zero spread. **Cloud answers are
  samples.** A cloud reading can be cached only as a sample, with a provenance note.
- The **image floor** is 280 px on the shorter side; below that the API returns HTTP 400.
- **No Jetson latency figures exist upstream for either family** — d1's card quotes 5.7 ms per
  decision on an H100, Clef's README quotes a single H200. Neither is a Thor number.

S1 still needs the p99 under load — that part is not measured and the agent should measure it.

---

## 4. The traps

1. **The memory watchdog sheds the decision tier first** when `MemAvailable` drops below 15 GiB.
   On 2026-10-29 a 6.6 GiB benchmark took all four local decision services down at once. D11 says a
   late decision falls back to a reflex — **the reflex rung is what the watchdog sheds.**
2. **`:8092` is down.** It was last-mile's LFM2-1.2B System-1 candidate. Ollama on `:11434` answers.
3. **The Thor's llama.cpp build is 0.7.0-dev and cannot load LFM3** (needs ≥0.8.0-dev). A working
   0.8.0-dev build is at `~/llama.cpp-upstream/build-up`. **Do not rebuild `llama.cpp/build`** — a
   running service depends on it.
4. **Blender's Cycles with denoising ON renders pure black on this Thor.** EEVEE works. last-mile's
   GI-bake spike ("path-traced GI bake on the Thor") will hit this and it looks like a renderer bug.
5. **VRAM is not the Thor's.** ~54.5 GB is held by other tenants; SPAR3D wants 10.5 GB. Generation is
   a scheduling problem, not a capacity problem.
6. **Gated checkpoints break CI.** `stabilityai/stable-point-aware-3d` and `Ruicheng/MoGe-2` need
   auth; an unauthenticated fetch of the latter is HTTP 401. Fetch once into a local cache.
7. **The headless probe runs under software GL.** 4,063 terrain tiles, 1–3 fps, screenshots need
   180 s. That is the harness, not the game.

---

## 5. What NOT to take

- **The whole-hog pipeline.** last-mile's generator need is archetypes, and last-mile already has the
  right rule for that: **seed + params *are* the asset** (D19), served by the Thor's Foundry on
  `:8200`. SPAR3D reconstructs **one photographed object**. It is the answer for subjects that
  exist in the world, not for the world's vocabulary.
- **The splat tier.** A gaussian field is right for a *photographed* place. last-mile's world is
  generated; it has no photographs to splat.
- **H3 cells, the local frame, the place file, the capsule format.** Those are the Otherhere
  project's world model. last-mile has its own.
- **The sculpting workflow.** The finishing lane is a tool, not a workflow to reproduce.
- **Any number in this folder as a design target.** They are measurements of one subject on one
  machine on one day.

---

## 6. Where things are

**On the Thor:**

```
~/sitka-distill/pipeline     the generation lane: depth-place, finish-asset, publish-model,
                              frame-model, asset-capture, lib/
~/d1-lab                     the lab: proofs and instruments, incl. cathedral/ (the worked example)
~/models                     the decision models' GGUF weights
~/llama.cpp/build            0.7.0-dev — DO NOT REBUILD (a service depends on it)
~/llama.cpp-upstream/build-up 0.8.0-dev — the upstream worktree; use this one
```

**The proofs worth running before you trust anything:**

| Script | What it proves |
|---|---|
| `~/d1-lab/verify_meshopt_glb.mjs` | a finished GLB decodes the way the browser will |
| `~/d1-lab/prove_model_in_game.mjs` | the running game fetched, hash-verified and drew a tier |
| `~/d1-lab/look_capture.mjs` | a capture and a generated mesh, one renderer, one bearing |
| `~/d1-lab/measure_splat.mjs` | the splat's own extent, so a mesh can be sized against it |

**The source project** is <https://github.com/SuperInstance/Otherhere>. Its
[`docs/proart-handoff.md`](https://github.com/SuperInstance/Otherhere/blob/main/docs/proart-handoff.md)
is the two-machine map, and ADR-0023 / ADR-0024 are the decisions behind the model tier and the
framing rule.

---

## 7. Where this lands in the code that is already being written

last-mile's Phase 6 branch, `phase6-3d`, is building the 3D and archive work that most of this
folder is about. Three things there, and one handoff that already exists.

**Read `docs/handoffs/orin-archive-and-golden.md` on `phase6-3d` first.** It is already a handoff for
an agent running on the Orin or the Thor: the ARM golden check, the pinned rounds, and the commands.
This folder does not repeat it.

Three observations about the branch, each with something to do:

| What is there | The observation |
|---|---|
| `crates/lm-archive/src/store.rs` — the store keeps `<store>/rounds/*.lml`, `<store>/reports/*.json`, `<store>/state.json` | The README says the Orin holds "round logs, **content-addressed asset bank**, headless replay verification". The archive implements the round logs and the verification. **The asset bank is not there yet.** It is the place where this folder's vouching record belongs. |
| `crates/lm-archive/src/verify.rs` — every failure is refused by name, and the module's own heading says "No false OK" | This is exactly the refusal discipline the vouching record needs, already written and working. **The asset bank should inherit it rather than invent a second way to fail.** |
| `crates/lm-render/src/hash.rs:41` — `structure_hash(world, chunk, cfg) -> u64`, an FNV-1a over the chunk's cell tags and two float bit patterns | This is a **cache key**, and a good one: world-anchored, stable, and cheap. It is not a byte-level content address for a payload that ships. A u64 FNV hash does not carry a byte count or a content type, and cannot refuse a truncated file. **The render cache and the asset bank want different hashes of the same thing.** |

The third row is the one to get right. A chunk's structure hash answers "is this the same chunk in
the same shape". A shipped asset's record answers "is this the same file, whole". last-mile needs
both, and it already has the first.

---

## 8. The one thing to get right

Everything else in this folder is a number or a licence. **The one thing that is a discipline:**
a generated asset that lands in the wrong place, or a tier that is drawn but invisible, **produces
no error.** Both are content, not lookups. The only defence is a check that compares — the frame
against the subject's placement, the threshold against the screen, the hash against the record —
and refuses when they disagree.

# 02 — Four mechanisms, and where each one lands

Numbers expire. These do not. Four patterns came out of the Otherhere lane that are not specific to
that game and that last-mile already has a place for.

---

## M1 — The vouching record: a hash plus the record that names it

**What it is.** A content address is only half a fact. The other half is a *record* that names the
bytes, and a client that refuses the payload if the two disagree.

```
the record   -> { path, sha256, bytes, contentType }
the server   -> serves the bytes
the client   -> verifies sha256 BEFORE parsing; a mismatch is refused before a byte is parsed
```

The rule that makes it work: **nothing reaches the world without being in the content address.**
Measured end to end in production on 2026-10-29 — the place file named the sha256, the server
served the bytes, the client verified, and the round trip returned exactly the named hash.

**Where it lands.** last-mile's `docs/research/forclaude2/11-prerender-ahead.md` already says
"everything generated gets a content-addressed ID" and that "the Orin holds the bank", and D46
already keys caches by "world tile + structure hash". What last-mile does not have is the second
half: **the record**.

An ID alone makes the Orin a pile of blobs with names. The record makes it a bank that can refuse:
the record says which file, how many bytes, what content type, and which hash — so a corrupt or
truncated transfer is caught by the reader, not discovered later as a hole in the world. And because
the record is a *file*, it is reviewable: a change to the record is a change to the bank, and it
shows up in a diff.

**What to take.** Extend last-mile's asset ID into a record with the same five fields, and make the
Orin's archive reader verify the hash before handing bytes to a renderer. The Orin already has the
disk; it needs the refusal.

---

## M2 — The framing rule: a generated asset must be authored into the frame it will live in

**What it is.** When several representations of one subject share one placement, a generated
representation **cannot choose where it stands**. It has to be authored into that frame, or it lands
in the wrong place, at the wrong size, facing the wrong way.

The check is three numbers, and they must match **exactly**:

| Quantity | What it must equal |
|---|---|
| Base centre (world) | the subject's placement |
| Height | the subject's declared height |
| Front bearing | the subject's declared principal face |

**The failure is silent, and that is the point.** In the measured case the transform was wrong by
1.153 m in x and 15.894 m in z. The game fetched the payload, verified its hash, drew it, and every
log line read green. Nothing said "this is in the wrong place" because nothing in the pipeline
compares the mesh's base against the subject's placement — until you add that comparison.

**Where it lands.** last-mile's `docs/research/forclaude2/11-prerender-ahead.md` says: "the world
is seed-deterministic, so the Thor generates the same L0 geometry itself and **bakes on top of it
with no geometry transfer**."

That sentence is last-mile's **risk case**, not its general case. Normally the frame never has to
travel, because both machines derive it from the same seed. But the Thor's bake is *content* — a
texture, a vertex colour, a vista — and content that is applied to geometry must land on that
geometry. If the Thor and the ProArt round the seed's coordinates differently, snap cells differently,
or apply the local frame differently, the bake lands in the wrong place. And a wrong bake is not an
error: it is a picture of a wall that does not exist.

**What to take.** The mechanism is a **cross-machine agreement check on the frame**, before any bake
is published. last-mile already has the vocabulary for this: D10 says the machines "cannot trust
each other's arithmetic" and D46 keys caches by "world tile + structure hash". The bake needs the
same discipline as the cache key — **the structure hash is what makes the bake addressable**, and
the frame check is what makes it land.

Concretely: the Thor's bake output should carry the same structure hash the ProArt will look it up
by, and a bake whose frame does not match the frame the ProArt derives from the same seed should be
refused rather than drawn.

---

## M3 — A tier ladder is a set of thresholds, and thresholds are measurements

**What it is.** last-mile's ladder is L0–L3 with "any level is playable" and "lateness only costs
looks". The Otherhere lane built the same shape — splat near, mesh beyond, photograph beyond that,
card for the lite profile — and **measured** it: 138 m fetched the splat, 142 m fetched the mesh,
same bearing, four metres apart.

**Then it failed, and the failure is the lesson.** The mesh tier was fetched, hash-verified and
drawn on six bearings, 6/6 clean, zero errors. **It was invisible.** The ladder gave the mesh only
the range beyond 140 m, and at 140 m a 14.6 m building is **68 pixels tall** in a 1280×800 frame at
a 70° vertical FOV — and from most bearings the town's hill was in front of it.

So the threshold was not a measurement. It was a round number, and a round number is a guess.

**What to take.** The rule is: **a fidelity threshold is derived from the on-screen size of the thing
it is hiding.** For last-mile, L0→L1→L2→L3 needs a screen-pixel budget per level, computed from the
chunk's size and the camera distance at which the next level appears. A chunk that is 4 m tall at
120 m is 24 px at 70° over 800 px — and if L2 is supposed to be an improvement over L1, the
improvement has to exist in those 24 pixels.

The same discipline applies to D11's "never wait" and to research/11's "lateness only costs looks":
**lateness only costs looks if the looks exist at the distance the player is at.** A ladder that
replaces a good tier with a worse one because of a threshold that was never measured against the
camera is not "never wait" — it is "never good".

---

## M4 — A machine should be able to describe itself

**What it is.** The Thor serves a live index on `:8100`. On every request it answers: the services
and their ports, the asset tiers with paths and hashes, the tools, the proofs and whether their
scripts are present, the decision models on disk, and live memory and disk.

**Where it lands.** last-mile's `docs/research/fleet/01-measured-facts.md` is a hand-written list of
the Thor's services, measured once on 2026-10-28, with the instruction "later estimates should cite
those files". One day later that list was already wrong in both directions: five services were
missing, one was down, the disk figure was off by 65 GiB, and available RAM was 11 GiB lower.

**What to take.** If the last-mile agent is to dial a service on the Thor, it should ask the Thor.
The index is ~40 lines of Node on top of the services it describes. It is not a clever thing; it is
just the difference between a fact and a memory.

---

## M5 — The delivery format is right, and the renderer needs one of two steps

Not a mechanism so much as a **finding with a consequence**.

A 10,000-triangle textured mesh compresses to **169,132 B** as a GLB with
`EXT_meshopt_compression`, and a browser opens it with **three.js's own bundled decoder** — no wasm
fetch, no Basis transcoder, no CDN. `meshoptimizer` upstream is MIT, and the Rust port
`meshoptimizer-rs` (v0.2.15, MIT) is byte-identical to the reference and covers simplification,
meshlets, **cluster LOD**, and **decoding** EXT buffer views.

**The consequence for last-mile.** last-mile's renderer is **Bevy**, and the released Bevy 0.20.1
does not support it:

```
| `KHR_draco_mesh_compression` | ❌ |
| `KHR_mesh_quantization`      | ❌ |
| `EXT_meshopt_compression`    | ❌ |
```

`bevy_gltf` 0.20.1 has **no `meshopt` feature and no decoder at all** in its `GLTFLoader`. The file
the Thor produced cannot simply be dropped into `assets/`.

**Two paths, and the choice is worth making early because both change the build story:**

| Path | What it costs |
|---|---|
| **(a) Rewrite the GLB before `bevy_gltf` sees it** | A plugin using the pure-Rust `meshoptimizer-rs` decoder — **no C++ toolchain**, no Bevy fork. There is a worked example: <https://github.com/paulrobello/hex-world> (`crates/hex-assets/src/meshopt_glb.rs`, `MeshoptGlbPlugin`, `meshoptimizer-rs = "=0.2.1"`, Bevy `=0.20.1` — i.e. exactly the Bevy that cannot read meshopt, which is why it rewrites) |
| **(b) Pin a Bevy build that includes the upstream support** | Bevy `main` **has** meshopt support — PR [bevyengine/bevy#25953](https://github.com/bevyengine/bevy/pull/25953) (merged 2026-10-15) and [#26318](https://github.com/bevyengine/bevy/pull/26318) (merged 2026-10-27) — but it was **cut from the 0.20 release branch**, and it sits behind a non-default `meshopt` feature that pulls a **C++ FFI** crate (`meshopt` v0.7.0). So (b) means tracking an unreleased Bevy and taking a C++ build step on the ProArt |

**What to take.** Path (a) is the smaller commitment and it is a few hundred lines, not a research
project. Doing it early also buys `meshoptimizer-rs`'s **cluster LOD**, which is the natural
primitive for L0–L3. **But pick one and write it down**, because the choice sets the byte budget D16
plans against and the ProArt's 6 GB of VRAM.

**One trap in the precedent.** hex-world ships **no NOTICE or attribution file** for meshoptimizer.
meshoptimizer is MIT, which requires the copyright notice and permission notice to be included in
copies. If last-mile copies that pattern, it must ship the notice. (hex-world is an independent MIT
project by Paul Robello — not a SuperInstance project — so it is a pattern to reuse, not prior art
from this org.)

**One naming trap.** The registry status is the opposite of what it looks like at a glance:

- `EXT_meshopt_compression` is **"Complete, Ratified by the Khronos Group"** — it lives under
  `extensions/2.0/Vendor/`.
- `KHR_meshopt_compression` is **not a rename**. It is a **derived superset** at **Release
  Candidate** status: an upgraded v1 vertex format and a new `COLOR` filter. Its version 0 is
  bit-identical to `EXT`, and existing EXT assets convert losslessly by changing the extension
  string in the glTF JSON. It **must not be mixed with `EXT_` on the same buffer view.**

three.js's `GLTFLoader` registers **both** names and the bundled decoder is codec-level only, so it
handles either. **Practical advice: write `EXT_meshopt_compression`.** It is ratified, it is what the
browser reads, and it is what the measured payload already uses. Reach for `KHR_` version 1 only if
the better compression is worth requiring newer readers on both ends.

**And a companion fact for the splat tier:** `KHR_gaussian_splatting` is also ratified ("Complete,
Ratatified by the Khronos Group"), specifying 3D Gaussian splats as point primitives with a graceful
sparse-point-cloud fallback. Released Bevy 0.20.1 lists it ❌ as well — so glTF-carried splats have
the same loader story.


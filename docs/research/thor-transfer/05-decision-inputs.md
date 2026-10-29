# 05 — Decision inputs

**Nothing here is decided.** last-mile's decision log is append-only and belongs to Casey. This file
is written in the log's own row format so that anything accepted can be pasted in; the reasoning
under each proposal says why it is being offered.

Source: the Otherhere/Thor lane, measured 2026-10-29. Full evidence in
[`01-findings.md`](01-findings.md) and [`02-mechanisms.md`](02-mechanisms.md).

---

## Proposed new entries

| # | Date | Statement | Reason | Source |
|---|---|---|---|---|
| **D49** | 2026-10-29 | **Generated assets ship as a GLB with `EXT_meshopt_compression`** (the ratified Khronos extension; not the `KHR_` superset). The Bevy renderer gets a plugin that decodes and rewrites the GLB in memory before `bevy_gltf` sees it, using the pure-Rust `meshoptimizer-rs` (MIT, no C++ toolchain) | 169,132 B for 10,000 tris; a browser opens it with three.js's own bundled decoder; **released Bevy 0.20.1 has no meshopt support at all** (❌, no feature, no decoder). Bevy `main` has it behind a non-default `meshopt` feature that pulls a C++ FFI crate, but it was cut from the 0.20 branch — so the alternative is an unreleased Bevy plus a C++ build step. Doing this first sets the byte budget D16 plans against. **Ship meshoptimizer's MIT NOTICE** — the hex-world precedent does not | [02](02-mechanisms.md#m5--the-delivery-format-is-right-and-the-renderer-needs-one-of-two-steps) |
| **D50** | 2026-10-29 | **The asset bank's entries are records, not hashes**: `{ path, sha256, bytes, contentType, params }`. A reader verifies the hash **before** handing bytes to a renderer | An ID alone makes the Orin a pile of blobs with names; the record makes it a bank that can refuse. A corrupt transfer is caught by the reader, not found later as a hole in the world. Extends D46's world-anchored keys | [02](02-mechanisms.md#m1--the-vouching-record-a-hash-plus-the-record-that-names-it) |
| **D51** | 2026-10-29 | **A pre-rendered bake carries the structure hash it was made for and is refused if its frame does not match the frame the ProArt derives from the same seed** | A misplaced bake produces **no error** — it is a picture, not a lookup. The Thor bakes onto geometry it generates itself, so the frame normally never travels, which is exactly why the agreement check has to be explicit | [02](02-mechanisms.md#m2--the-framing-rule-a-generated-asset-must-be-authored-into-the-frame-it-will-live-in) |
| **D52** | 2026-10-29 | **A fidelity threshold is derived from the on-screen size of the thing it replaces**, not chosen. L0–L3 get a pixel budget per level | The Otherhere ladder shipped a mesh tier that was fetched, verified and drawn 6/6 clean and **invisible**: its 140 m threshold put a 14.6 m building at 68 px, behind a hill. "Lateness only costs looks" is only true if the looks exist at the player's distance | [02](02-mechanisms.md#m3--a-tier-ladder-is-a-set-of-thresholds-and-thresholds-are-measurements) |
| **D53** | 2026-10-29 | **The Thor describes itself: agents ask `:8100` what is there, they do not read a file.** No hand-written service list in the repo | A snapshot of a machine's ports went stale in one day: five services appeared, one went down, disk moved 65 GiB, available RAM moved 11 GiB | [04](04-hazards.md#48-the-discovery-surface) |
| **D54** | 2026-10-29 | **A licence gate precedes a quality gate.** Any model the Thor runs has its commercial ceiling checked before its quality is benchmarked | Hunyuan3D-2 does not apply in the EU, UK or South Korea; Depth-Anything-V2 **Large** is CC-BY-NC-4.0 (non-commercial); TRELLIS's rendering components are non-commercial per its own maintainer despite an MIT card; SPAR3D is commercial below US$1M revenue with registration and attribution. A generator that cannot ship is not a slower option | [03](03-licences.md) |
| **D55** | 2026-10-29 | **Local decision answers are reproducible and cacheable as facts; cloud answers are samples and carry a provenance note.** Local answers were identical across runs with zero spread | D25's reflex cache and D32's held-out eval both depend on an answer meaning the same thing twice. A cloud reading can only be cached as a sample | [01](01-findings.md#15-decision-models-on-the-thor-against-a-fixed-judge) |
| **D56** | 2026-10-29 | **The reflex tier has an explicit memory floor, and the watchdog's shed order is a design input, not an implementation detail** | The Thor's watchdog sheds the local decision tier first below 15 GiB `MemAvailable`; on 2026-10-29 a 6.6 GiB benchmark took all four local decision services down. D11's fallback to reflex has no floor under it if the reflex tier is what gets shed | [04](04-hazards.md#41-the-memory-watchdog-sheds-the-decision-tier-first) |

---

## Proposed amendments to existing entries

| Amends | Statement | Reason |
|---|---|---|
| **D35** (provisional models) | The model list is provisional *and* one member is currently unavailable: `:8092` (LFM2-1.2B) is down as of 2026-10-29; Ollama on `:11434` answers. Measured System-1 candidates on the Thor against a fixed judge: d1-2B 98.4% text / 71.0% photo at 60.3 ms for a 3-question pack; clef-flash 100% / 87.1% at 284.4 ms; d1-3B is not better than 2B on that judge; d1-omni-600M is too weak (66.1% / 12.9%). D37's class was right; the members are now candidates with numbers | The fleet doc's system-1 candidates were LFM2-1.2B and qwen3.5:9b. One is down, and the family last-mile's own research names (d1, Clef) has never been measured on this hardware |
| **D42** (spike S1 caveat) | S1's re-measurement should settle **how a pack of questions scales**, because upstream and the Thor disagree: d1's card claims "one forward pass … any number of questions about one state in one call" and Clef "scores all options of all questions jointly", but on the Thor the 600M was flat from 1 to 3 questions (15.0 → 15.4 ms) and 8 questions cost 83.0 ms. Measure it; do not assume either | S1 asks for p99 at 16 agents × 8 gates. The shape of the cost is the thing being asked, and there is **no upstream Jetson figure** for either family — d1 quotes 5.7 ms on an H100, Clef quotes a single H200 |
| **fleet/01** (measured facts) | Supersede the Thor's Stage 0 snapshot: **249 GiB free (72% used)** of 936 GiB, not 184 GiB (80%); **31 GiB RAM available**, not ~42; services additionally on **8093–8099 and 8100**; **`:8092` down**; and a **live index on `:8100`** that supersedes a snapshot | Every number in that table was measured 2026-10-28 and moved by 2026-10-29 |

---

## Open questions this folder raises but does not answer

1. **Does last-mile want photographed subjects at all?** SPAR3D reconstructs one photographed
   object. If the world is procedural, the answer is the Thor's Foundry (`:8200`, seed + params) and
   not SPAR3D. The licence question only matters if the answer is yes.
2. **Which renderer reads the meshopt GLB?** D49 assumes a rewrite plugin. The alternative is to
   ship uncompressed GLBs and pay the bytes — D16's budget has to answer this.
3. **What is the pixel budget per fidelity level?** D52 says derive it; it does not say what it is.
   That needs the camera FOV and the chunk sizes, which is last-mile's to know.
4. **Is the Orin allowed to verify?** D50 needs the archive to hash on read. The Orin has 7 GiB of
   RAM and 1.9 TiB of NVMe, and cannot generate (SPAR3D needs 10.5 GB of VRAM) — but verifying is
   cheap and the machine is otherwise idle.

# 06 — What to change, and where

The audit of this repo against the 2026-10-29 Thor measurements. Every line number is against
`28da80f` (2026-10-28). **Nothing here has been changed** — this is the work list.

Last-mile's decisions are append-only, so a number that is wrong is **superseded, not rewritten**.
The amendments in [`05-decision-inputs.md`](05-decision-inputs.md) are written in the log's own row
format.

## 6.1 The headline: at HEAD, **0 of 19 findings were present**

| | count |
|---|---|
| Findings already in last-mile | **0** |
| Findings last-mile **contradicts** | **2** (the d1 pack cost; "everything stays resident") |
| Findings last-mile states as **stale numbers** | **12** |
| Findings that were pure **gaps** | **5** |

The architecture was right. What was missing was measurement, licences, and one renderer fact.

---

## 6.2 The two contradictions — fix these first

### (a) `fleet/05-where-this-leads.md:25`

> "Decision models answer up to dozens of typed questions in one pass: Clef allows 64, and **d1 takes
> about 1.3× as long for 3 questions as for 1.** A model that answers 8 questions over the changed
> region is cheap enough to run like a video."

**Measured:** the 600M was flat from 1 to 3 questions (15.0 → 15.4 ms) but **8 questions cost 83.0 ms**
— about 5× the three-question cost. Upstream claims one pass for any number of questions; the Thor
does not behave that way at eight. **This sentence is what makes presence mode affordable**, and
D38's presence-mode defaults rest on it. It is a vendor number presented as a measurement.

### (b) `forclaude2/04-hardware-reality.md:14`

> "128 GB of unified memory lets **them all stay loaded at the same time**."

**Measured:** the Thor's watchdog sheds the **local decision tier first** below 15 GiB
`MemAvailable`. On 2026-10-29 a 6.6 GiB benchmark drove the box over the line and took all four
local decision services down. D11's "late thinking falls back to reflexes" has no floor under it,
because the reflex rung is what gets shed.

---

## 6.3 The stale numbers

| What last-mile says | file:line | Measured 2026-10-29 |
|---|---|---|
| "the NVMe has **184 GiB free (80% used)**" | `fleet/01-measured-facts.md:41` | **249 GiB free, 72% used** of 936 GiB |
| "RAM: 122 GiB total, about **42 GiB available**" | `fleet/01-measured-facts.md:38` | **~31 GiB available** |
| The service table — 7 rows | `fleet/01-measured-facts.md:25-35` | also **8000, 8017, 8093–8099, 8100**; **8092 down** |
| "8092 \| llama-server: **LFM2-1.2B** \| System-1 candidate" | `fleet/01-measured-facts.md:30` | **8092 is down**; Ollama `:11434` answers |
| "LFM2-1.2B (:8092) \| **Already resident, fast, small**" | `fleet/02-lessons.md:140` | down |
| "d1-3B: **~16 ms on Jetson AGX Thor**" | `fleet/04-decision-models.md:24` | **90.1 ms** for a 3-question pack |
| "**Clef-flash** … ~39 ms median on Workers AI" | `fleet/04-decision-models.md:26` | **284.4 ms** on the Thor — a cloud number, not a Thor number |
| "later estimates should cite those files" | `fleet/01-measured-facts.md:3` | superseded by the **live index on `:8100`** |
| Jev's inputs are "**text + schema**" | `fleet/04-decision-models.md:28` | Jev has a measured **photo** arm (83.9%) — and the earlier photo result was the **harness** being wrong, not the model |
| "about **54.5 GB** is held by the 30B, ComfyUI, the JEV rack and the 1.2B" | `fleet/01-measured-facts.md:38` | the 1.2B is down, so the total is no longer exact |

**An internal contradiction that is not about the Thor:** last-mile says the Orin has **8 GB**
(`forclaude2/04-hardware-reality.md:29`) in one place and **7 GiB** (`fleet/01-measured-facts.md:46`)
in another, and "**2 TB NVMe**" in a third place against "1.9 TiB". The measurements confirm **7 GiB
and 1.9 TiB with 1.7 TiB free**. Pick one and use it.

---

## 6.4 The gaps — nothing in the repo mentions these

| Gap | Where it lands |
|---|---|
| **No licence gate anywhere.** The only licence notes are `fleet/00-overview.md:64` (openSMILE) and `fleet/04-decision-models.md:142` ("fine for a personal project; record it") | [`03-licences.md`](03-licences.md) |
| **TRELLIS named with no licence note**, as the sprite-sheet spike's generator | `forclaude2/11-prerender-ahead.md:63` → [`03-licences.md`](03-licences.md) §3.2 |
| **No glTF loader / extension table anywhere.** D1 says "Bevy (Rust)" with **no version pinned** | [`02-mechanisms.md`](02-mechanisms.md) §M5 → and pin a version |
| **The L0–L3 table has no byte budget and no distance threshold** — Content, Source, Deadline only | `forclaude2/11-prerender-ahead.md:32-39` → [`01-findings.md`](01-findings.md) §1.2, §1.3 |
| **"the Thor generates the same L0 geometry itself and bakes on top of it with no geometry transfer"** — the mechanism, with no agreement check | `forclaude2/11-prerender-ahead.md:45` → [`02-mechanisms.md`](02-mechanisms.md) §M2 |
| **"content-addressed ID" / "keyed by hash"** — no record, no byte count, no pre-parse verify, no refusal | `forclaude2/11-prerender-ahead.md:49-50` → [`02-mechanisms.md`](02-mechanisms.md) §M1 |
| **The GI-bake spike** ("path-traced GI bake on the Thor") with no renderer hazard | `forclaude2/11-prerender-ahead.md:61` → [`04-hazards.md`](04-hazards.md) §4.3 |
| **The vision path has no minimum-resolution constraint** | `fleet/04-decision-models.md:101-114` → [`01-findings.md`](01-findings.md) §1.6 |
| **The asset bank is an M2 stub** — "a content-addressed directory layout only" | `spec:170` |

---

## 6.5 The docs that would need editing

Ordered by how much the measurements change them.

1. `docs/research/fleet/01-measured-facts.md` — disk, RAM, the service table, 8092, and "cite those
   files". **Everything here is a snapshot; the live index supersedes it.**
2. `docs/research/fleet/04-decision-models.md` — the d1/Clef latency rows, Jev's inputs, S1's text,
   and the licensing line at 142.
3. `docs/research/fleet/05-where-this-leads.md` — **line 25**, and the Jev rows at 82 and 104.
4. `docs/research/forclaude2/11-prerender-ahead.md` — line 45 (frame check), lines 49-50 (the
   vouching record), lines 32-39 (thresholds and bytes), line 61 (Cycles), line 63 (TRELLIS's
   licence), line 58 (the VRAM budget).
5. `docs/research/forclaude2/04-hardware-reality.md` — **line 14** (all resident) and **line 29**
   (the Orin's RAM).
6. `docs/design/decisions.md` — D1, D14, D35, D37, D39, D41, D46, plus the new entries from
   [`05-decision-inputs.md`](05-decision-inputs.md).
7. `docs/superpowers/specs/2026-10-28-last-mile-m1-design.md` — the model block at 100-106, the asset
   bank at 170, the VRAM budget at 166, `agent.models` at 249, and the open items at 262-263.
8. `docs/superpowers/plans/2026-10-28-m1-phase1-core.md:641` — the phase-3 model-client row.
9. `docs/research/fleet/02-lessons.md` — the `:8092` row at 140.
10. `docs/research/poc/jev/RESULTS.md` — E4 is text-only; the photo arm and the harness cause are
    missing.
11. `README.md` — the Orin's NVMe, and the Thor's role.
12. `docs/research/fleet/00-overview.md` and `docs/research/forclaude2/00-overview.md` — summary rows.

---

## 6.6 Three things worth a decision, in order

1. **Which llama.cpp does the Thor run?** The M1 spec chose d1-2B; d1 is LFM3-based; 0.7.0-dev cannot
   load LFM3; and 0.7.0-dev must not be rebuilt. Everything about presence mode's economics sits on
   this answer. See [`04-hazards.md`](04-hazards.md) §4.2b.
2. **Which Bevy version?** D1 names Bevy without a version, and the answer decides whether the
   meshopt GLB needs a rewrite plugin or an unreleased Bevy with a C++ feature. See
   [`02-mechanisms.md`](02-mechanisms.md) §M5.
3. **Does last-mile want photographed subjects at all?** If not, the generator question closes
   itself and the licence question with it — and the Thor's Foundry (`:8200`, seed + params) is the
   whole answer. See [`05-decision-inputs.md`](05-decision-inputs.md).

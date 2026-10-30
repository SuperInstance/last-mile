# The grand plan: a world you teach by talking to it, that shows its work

*2026-10-30. The top-level plan. Under it: docs/ROADMAP-AHEAD.md (schemas, open questions, job waves), docs/HANDOFF.md (the state now and how to operate), and the designs (sandbox-on-otherhere.md, questions-as-experience.md, vox-score.md). Revisit at every milestone boundary.*

## 1. The vision in one paragraph

A paintball match on a real street in Sitka, played by comic AI squads with personalities, commanded by voice. A kid discovers they can change the game by saying so. Bench, the AI co-developer, answers on the radio: he names the change, predicts its effect, tests it on unseen matches, and shows whether he was right. The kid keeps it or puts it back. Underneath, every agent's knowledge is **a readable library of questions** answered by small local models and written by large cloud models, every change is **a sealed, tested prediction**, and every performance (voice, face, motion) is **a score** any renderer can play. The game is how people find their way in. The platform is teachable characters that show their work.

## 2. What must be true (the five pillars)

| Pillar | Means | Proven so far | Still to prove |
|---|---|---|---|
| **P1 Fun to watch** | contact, comedy, readable consequence, a camera that finds the action | `advance` makes squads meet; urgent speech in fights | most rounds fight (shared target); auto-camera; personalities land; paint reads |
| **P2 Talk changes the world** | voice → closed vocabulary → typed, bounded change → test → keep or undo | ears (179 ms), the realizer, the learning flow, 16-seed sign test | the Vibe gate; Bench's radio loop under 5 s; put-it-back |
| **P3 Experience is questions** | knowledge as a versioned, evidenced question library; decompose before escalate | every behaviour change tonight was a question change | decompose beats escalate; the ledger; cloud-written questions pass tests |
| **P4 Honest learning** | sealed predictions, held-out seeds, an evidence trail, deterministic replay | D69 bit-exact; seed farm; refusals logged | a kid-facing Glitch Meter that's real, not decorative |
| **P5 Safe and personal** | voice stays local; nothing about a child is a training target; a per-player profile only by consent | local ears and voices; prosody-only storage tier designed | parent/teacher view; a privacy review; Magnus's playtest |

Every milestone below advances at least one pillar and must not regress any.

## 3. Milestones, with exit criteria

| | Name | Exit criteria (all must hold) | Main risks |
|---|---|---|---|
| **M1** | Close the foundation | in-flight merges landed; D75-D78 recorded; ACCEPTANCE.md refreshed; **Casey's hands-on pass** done; his findings fixed | merge conflicts; the Thor busy during his pass |
| **M2a** | A match worth watching | ≥70% of rounds have contact (100-seed sweep + 3 Thor rounds); draws < 40%; auto-camera captures every fight in a 5-round review; 6 personality patches with audible, distinct lines; paint splats in the hashed state; golden re-pinned with reasons | shared target breaks decisiveness; personalities hurt win rates |
| **M2b** | The sandbox slice | a kid (Casey first) completes the first-5-minutes storyboard; "make the paint bouncy" → named block → sealed prediction → 16-seed verdict → keep/undo in **≤ 5 s** perceived (radio framing); 10 blocks; one Bolt offer per round; replay OK after changes; nothing outside a block's bounds ever applies | the 16-seed test is too slow (needs a pre-screen); the vocabulary feels like a menu |
| **M2c** | The Otherhere stage | the match plays on Lincoln Street in the chosen client (Three.js or Bevy) at legible distances; the never-wait fallback with no restyle; optional restyle rung measured | client choice; grid ↔ place mapping; tier legibility |
| **M3** | Experience that grows | the question ledger live (evidence DAG); decompose-before-escalate applied wherever it wins; per-question routing; ≥1 cloud-written question adopted by test; the Hodge triage used online | gains don't generalize across doctrines |
| **M4** | People | Magnus playtests and his feedback lands as tested changes; a parent/teacher view; a privacy review passed; two players on two squads in lockstep | kids' privacy obligations; multiplayer determinism over a network |

**Sequencing:** M1 → M2a ∥ (M3 research) → M2b → M2c ∥ M3 build → M4. M2a and M2b share the starting game; M3's research runs underneath from now, and its first wins (decomposition, routing) feed M2b's Vibe gate and Bench.

## 4. The work streams

Each stream has an owner type and a home machine. Research runs one step ahead of build.

| Stream | What | Research now | Build next | Home |
|---|---|---|---|---|
| **S1 Stage and watchability** | doctrine, shared target, camera, paint, personalities, readable art | sharedtarget, resde (DE), autocam | paint state, persona A/B, camera polish | Thor (measure) + ProArt (host) |
| **S2 Talk-to-change** | Vibe gate, vibe.toml, Bench, Bolt, proposal UI, undo | res-prior (kids' tools, COPPA), reslucineer (TALKSITKA, self-evolve) | the Vibe gate + 10 blocks; the Bench radio loop | ProArt + Thor |
| **S3 Questions as experience** | decomposition, Hodge triage, routing, ledger, cloud-written questions | decomp, resdec (decompose repos + rea ledger) | the ledger format; offline Hodge on the pack corpus; routing | Thor + pi |
| **S4 Honest learning** | 16 seeds + sign test, faster pre-screen, Glitch Meter, RL complement | rlspike, seed farm follow-ups | apply (D78), the pre-screen, the stability meter | Thor (cores) |
| **S5 Performance (Vox Score)** | marks, conformer, polisher, the full score (face/body) | conform (E10), the E6 sweep (batch) | marks for Bench and Bolt's delivery; the conformer in the voice ladder | Thor |
| **S6 Stage tech** | Otherhere client, grid ↔ frame, restyle | res-eden, a Three.js spike | the stage slice | ProArt + Thor |
| **S7 Platform and ops** | merges, soak, MIG, batched server, model fit, licences | MIG done (doc) | scheduler upkeep; soak triage; the MIG window if Casey approves | all |
| **S8 People** | Casey's acceptance, Magnus, parents | — | the guide (accept2), playtest scripts, the parent view | — |

## 5. People and roles

- **Casey:** product owner. Decides at the gates in §8. Plays every milestone first.
- **Magnus:** prospective co-designer and first kid playtester (ask Casey first). His ecosystem (TALKSITKA, craftmind-herding/fishing/studio, self-evolve) is prior art to honour and, with consent, to build with.
- **Claude (director):** keeps the plan, writes briefs and schemas ahead, reviews, merges, records decisions, stays three moves ahead. Conserves its own tokens (resets daily).
- **zcode seats (GLM-5.3 main, GLM-5.3-Flash):** implementation and measurement. The main model for research (Flash spawns sub-agents and exits).
- **mcode seats (MiniMax):** long investigations and research when its quota allows (it ran out on 2026-10-30).
- **pi (local Qwen3-Coder / Glimmer / ProArt qwen2.5-coder):** free, mechanical work with exact tests. Glimmer reviews Qwen's drafts.

## 6. Machines (what each is for)

- **Thor:** runtime models (System One, ears, realizer, voices), GPU research and measurement, the seed farm, the soak, free batch jobs. Keep it **quiet when measuring latency**.
- **ProArt:** the Windows host, UI, screenshots, builds; ≤2 seats (memory); the local Ollama rung.
- **Orin:** archive and verify (logs, the pack corpus, replays) when the NVMe fills; candidate home for Magnus's Orin-native pieces.
- **Cloud:** seats (zcode, mcode) and, later, the System Two question writer.

## 7. Budgets and how to spend them

- **Claude tokens:** reserve them for briefs, reviews, decisions and plans; never for reading what a seat can summarize.
- **zcode:** ~8 concurrent seats before 429s, so stagger launches by 2 min; the scheduler enforces caps.
- **mcode:** a daily/plan quota, so use it for research and investigations; hand off to zcode with CONTINUE notes when it runs out.
- **The Thor's GPU:** free experimentation; but don't measure latency on a loaded box.
- **The ProArt's memory:** ≤2 seats; unload Ollama when idle.

## 8. Decision gates (Casey)

| Gate | When | The question |
|---|---|---|
| G1 | end of M1 | accept M1? |
| G2 | after sharedtarget + persona A/B | make the shared target and the personality cast default? |
| G3 | before M2b build | the Vibe Dictionary v0 (10 blocks) and Bench's voice |
| G4 | before M2c | Three.js (Otherhere, browser/iPad) or the Bevy host? |
| G5 | before any kid plays | privacy review + Magnus as co-designer and playtester |
| G6 | any time | MIG window; AGPL (default no); the project's license |

## 9. Risks and mitigations

| Risk | Mitigation |
|---|---|
| The match still isn't fun to watch | P1 first (M2a before M2b); measure contact; Casey and Magnus judge by eye |
| The 16-seed test is too slow for a kid | radio framing; a fast pre-screen (fewer ticks + guards, or an LLM filter) before the full test; test in the background while play continues |
| The Thor is overloaded, so latency spikes in play | quiet-Thor rule during play; MIG experiment; the batched server when headroom exists |
| Seats produce plausible but wrong work | verify key numbers; a second reviewer on research (mcode + zcode agreed on nuggets); pi pairs |
| Kids' privacy | voice local, nothing stored by default, the closed vocabulary, a parent view, a review before any kid plays |
| Licence traps (Riva NVAIE, the Liquid $10M cap, AGPL) | ship-safe alternatives listed (Kokoro/Chatterbox, Qwen); re-implement, don't vendor |
| Scope creep from new ideas | every idea enters as a hypothesis with one track in ROADMAP-AHEAD §3; build only what a milestone's exit criteria need |

## 10. The next 72 hours

1. Land the in-flight merges; record D75-D78; rebuild the play build; finish accept2. **→ G1: Casey's pass.**
2. Wave 1 research lands; write wave 2 briefs the same hour (persona A/B, paint state, Vibe gate, offline Hodge, routing, cloud-question spike).
3. Start M2a build: paint state + shared target (if it wins) + auto-camera, measured against M2a's exit criteria.
4. Draft the Vibe Dictionary v0 and Bench's radio lines for **G3**, informed by res-prior and TALKSITKA.
5. Keep the free GPU busy (the corpus, sweeps) and the soak green.

# last-mile and the whole-house quilt

*whole-house-quilt ([repo](https://github.com/SuperInstance/whole-house-quilt)) is Casey's umbrella design for the same four tiers: ProArt at the screen, Thor and Orin in the house, the subscriptions, and rentals. Its first build was formally adjudicated on 2026-10-28. last-mile should fit inside it, not compete with it.*

## Where they agree (already shared doctrine)

| Quilt doctrine | last-mile equivalent |
|---|---|
| "The world is text"; renders are replaceable projections (§21) | The score is the game; renderers are instruments (D8, D13) |
| Reaction never leaves the screen; the front end never waits on the wrong tier (§15) | The sim never waits; late thinking falls back to reflexes (D11) |
| Subscriptions are compute pools metered by quota, not dollars (§14–15) | Per-provider allotments with an exchange table (D16) |
| Decision models are the dice; big minds author the tables (§22) | D23 |
| Generators up, instances down; jobs descend into watched machinery (§16–19) | D25 reflex promotion; D35 operator tier |
| Staging ladder: play on the metal, judge samples, the long render takes the night (§26) | L0–L3 chunk fidelity and prerender-ahead (D14); neural replays overnight |
| Seed + params are the asset; geometry is a cache (Foundry) | Content-addressed asset bank (research/11) |
| Persistence classes: Eternal / Persistent / Ephemeral (plato-ghostable) | Round logs are Persistent and regenerable sim state is Ephemeral (fits D10: the log of decisions *is* the round) |

## Where they differ: who holds the engine

The quilt places **engine cells on the Thor**, with rendering at the ProArt. Its field room runs a headless Bevy cell next to its resident characters. last-mile places the **sequencer and authority on the ProArt** (D18), with mirror sims elsewhere.

These are different answers to different questions, and **both stay valid**:

- The quilt's field room is an agent-only world, and its clients (the characters) live on the Thor. Putting the engine next to them keeps the reaction path off the Wi-Fi.
- last-mile has a **human in the loop as a player**. The human's input must never cross the Thor's 80–112 ms p99 radio tail. Because last-mile is decisions-only lockstep (D10), *every* node runs the engine anyway. The only thing placed is the **sequencer**, the node that stamps final ticks.
  - Agent decisions are already scheduled with lead (D11, adaptive per D34), so they absorb the Thor → ProArt tail.
  - Human decisions on the ProArt pay nothing.

**Resolution (D36):**
- The sequencer node is a parameter (`net.sequencer_node`).
- Default `proart` whenever a human is playing.
- `thor` is legitimate for **agent-only rounds**, such as overnight seasons, tournaments and training, where nobody sits at the chair. Those can then run while the ProArt sleeps, matching the quilt's "night works twice".
- The mirror protocol is identical either way.

## How last-mile plugs into the quilt (seams, not dependencies)

M1 takes **no runtime dependency** on quilt components; the inventory is still being audited. It defines seams so the two can meet later:

1. **last-mile is a quilt room.** In quilt terms it's a *field room*:
   - its stationed residents are the squads;
   - its far side is the deterministic sim;
   - its tick policy is the round.
2. **A bridge-console face.** `lm-host` can emit change-only `TICK`-shaped status, plus a heartbeat (⚙ `quilt.heartbeat_s` = 60), so the quilt's bridge console can show "round 7, squad A leads 2–1, agents on reflex". Missing heartbeats are the alarm, raised by the receiver.
3. **The fabric purse.** When the quilt's quota ledger exists (its component 6), `lm-router` should read its plan health and reserve rules instead of duplicating them. Until then, `lm-router` meters by observation (01).
4. **Contention class.** During a round, last-mile's System-1 requests are quilt *reaction/conversation* class. Between rounds, re-harness work is *production* class. This lets the Thor's resident GPU tenants (ComfyUI, Foundry, the 30B) give way to a live round.
5. **ROLL records.** D23's logged `p[]` has the same shape as the quilt's `ROLL {table, options, p[], chose, seed, inputs_hash}`. Use the same field names so the quilt's dice-drift tooling can read last-mile logs.

## What last-mile can give back

- A **real-time, human-in-the-loop room**, which the quilt's first slice doesn't have.
- **Cross-architecture determinism evidence:** the x86 ↔ ARM golden vectors in D26.
- A tested **least-use-first local-model queue** that the quilt's fabric could reuse for its own resident cells.

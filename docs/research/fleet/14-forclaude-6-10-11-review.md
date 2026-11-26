# Review: forclaude6, forclaude10, forclaude11

**Sources.** Three conversations in `~/lm-inbox/`, read in full: `forclaude6.md` (2,176 lines — a "pure
vector embedding space" architecture for a physical excavator), `forclaude10.md` (6,644 lines — a
"sub-microsecond state-interchange engine" handoff for a senior engineer), `forclaude11.md` (2,049 lines
— a paper on "FLUX", a non-floating-point, hardware-locked coordination substrate).

**Question.** Which parts of these three conversations are worth carrying into last-mile, and which are
not.

**Verdict in one line.** These are one design written three times — a GPU-resident, lockstep,
hexagon-lattice engine for a game we do not have — and almost none of the architecture is adoptable; what
*is* adoptable is five small changes (three measurements, one fence we do not yet have, and one config
change that settles an open spike), and the corpus's single best idea (the expensive output is
pre-validated off-line and the hot path runs a table) is the idea we already shipped for voices (D73).

**A note on the numbers.** Both the forclaude10 and forclaude11 documents make performance claims in the
hundreds of nanoseconds, and both back them with a host-side mock of the GPU kernel. There is no CUDA, no
C++ and no `.cu` file in this repository, and the "hardware-level substrate verification" phase
(`forclaude10.md:3313-3326`) compiles and runs that mock. Every sub-microsecond figure below is therefore
a measurement of a simulation of a measurement. Our own scale: the sim tick is 33 ms (D20), one think is
164 ms at the median (D74).

---

## 1. Already in last-mile

The good ideas in these conversations are, in the main, ideas we had already taken and made concrete.
This is worth recording, because it means we should not spend the next month re-deriving them.

| The claim | Where it came from | Where we have it |
|---|---|---|
| FNV-1a hash chain as the receipt for every finalized state | `forclaude10.md:2495` — "an append-only, tamper-evident hash chain using the FNV-1a 64-bit non-cryptographic integer hashing algorithm"; `forclaude11.md:4` | `crates/lm-learn/src/ledger.rs` (FNV-1a 64 over canonical JSON with `prev` links); `crates/lm-archive/src/verify.rs` walks the chain and names the first diverging tick. **Ours is a tamper-evident chain, not a signature** — the documents' "cryptographically signed ledger" is not what FNV-1a is. |
| Eliminate floating-point arithmetic entirely | `forclaude11.md:4` — "we eliminate floating-point arithmetic entirely"; `forclaude10.md:628` — "uses no external libraries, floating-point operations, or trigonometric calls" | `clippy::float_arithmetic` denied in `crates/lm-core/src/lib.rs:9`, `crates/lm-proto/src/lib.rs:15`, `crates/lm-archive/src/lib.rs:36`; the fixed-point type is `crates/lm-core/src/fixed.rs` (`Pos` in subunits, `Angle` Q14, `isqrt`). D10, D69. |
| A rejection carries its reason; it is refused, never repaired | `forclaude10.md:52` — "the state is auto-rejected inside the GPU cache line, preventing an invalid action from ever triggering host-side processing or dropping an unverified block into your jev-receipts ledger"; `forclaude11.md:218-227` | D26. `crates/lm-core/src/opal.rs:77` — "Every variant is a **named** refusal (D26)"; `crates/lm-agents/src/gates.rs:679` — "refuse the whole pack instead of taking the part that fits". |
| The fallback is a pre-validated table, not a model | `forclaude11.md:24` — "falling back to pre-validated, mathematically safe look-up matrices stored within hardware __constant__ cache lines"; `forclaude10.md:1494` — "Hardwired, pre-validated geometric licks (Stored directly on-chip for instant safety)" | D45. `crates/lm-core/src/agent.rs:45` — "The safe branch: known-free cells only, ending in cover or a hold." The reflex plays it; it never travels (D10). |
| Drop the late block rather than wait for it | `forclaude10.md:594-596` — "it uses an internal Eisenstein Snap to cleanly drop the delayed block, update its local timeline tracker, and maintain the current performance flow without missing a beat" | D11 (the sim never waits; a late answer takes the safe branch), D26 (a refusal is an outcome), `crates/lm-core/src/sim.rs:76-80` (`reflex_refused` / `reflex_out_of_fuel` / `reflex_error`). |
| Only the decision travels; the state is re-derived | `forclaude10.md:4242` — "The ProArt shouldn't send long streams of decimal floating-point vectors tracking every minor frame update… pack the resulting 4D coordinates into a single 32-bit integer word, and blast it across the network" | D10; `crates/lm-proto` (postcard, varint, no float); D69 (the Thor reproduces the ProArt's state bit-for-bit). The hexagonal grid is wrong for us; the principle is ours. |
| The write path must never be in the producer's path | `forclaude10.md:5786` — "the resulting file-system I/O barriers will cause backpressure, flooding the L2 cache lines of our persistent CudaClaw kernels" | `crates/lm-core/src/log.rs:507-542` — the `Recorder` seals segments in memory (`close_segment` only hashes and pushes to a `Vec`); the file write happens after the round. We already had this. |
| The two expensive requests must not wait on each other | `forclaude10.md:55` and `forclaude10.md:141` — the actor/critic warp split | Two worker threads per mind (`crates/lm-agents/src/mind.rs`); `crates/lm-agents/tests/talk_on_thor.md:246` — "The talk pack never waits behind the move pack, and the move pack never waits behind the talk pack". |
| The second opinion is a cost, and a cost that is not measured is not managed | `forclaude10.md:1544` — the fence as policy, not as a constant | `crates/lm-agents/tests/talk_on_thor.md:249` — the escalation is logged with gate, both confidences and latency. |
| Pre-render the expensive output off the hot path and run a table inline | `forclaude10.md:1494`, `forclaude11.md:24` | D73 — skald pre-renders voices into a content-addressed bank; the game never renders inline. This is the corpus's best idea and it is ours: at ~640 ms per line, a line rendered inline would cost nineteen ticks of a thirty-three-millisecond tick by itself. |
| The decision record carries the situation it was made under, and what it cost | the context-key idea in both documents | `crates/lm-core/src/decision.rs:127-134` — `Provenance::System1 { model, tokens, latency_ms, situation_key }`. Already in the record. |
| Fence the model's GPU from the round's GPU with MIG | `forclaude10.md:3470-3490`; `forclaude10.md:6373` — "Hardware MIG Fencing Allocations" | Already a spike: `docs/research/fleet/13-gemini-architecture-review.md` (spike 1). MIG is supported on our Thor and disabled. N5 below sharpens it with a reason and a measurement. |

---

## 2. Nuggets worth adopting

Ranked by value. Each is a thing we could do next week, that would produce a number, and that does not
disturb "only decisions travel", "never wait", or bit-exactness.

### N1 — Measure the wait where it happens, not at the boundary

> `forclaude10.md:3023` — "In high-stakes distributed edge applications on the Jetson Thor, relying on
> traditional high-level diagnostic loggers introduces system calls that taint our profiling data. To
> protect the timing invariants of our sub-microsecond runtime loops, the benchmark tracking must be
> computed using direct CPU-register cycle counting and inline statistical metrics with zero memory
> allocations."
>
> `forclaude10.md:3067-3069` — "// Jetson Thor ARM64 Reference System Timer commonly scales at 100MHz. //
> 1 cycle = 10 nanoseconds." (followed by `let total_elapsed_ns = (end_cycles - start_cycles) * 10;`)

**What it is.** The *method* is sound: time the operation inside the process that performs it, with a
counter that does not enter the kernel, rather than stamping the boundary of an HTTP call. The
implementation that comes with it is wrong in a way we should not copy — see the warning below.

**Why it helps.** The move pack is 164 ms at the median and 262 ms at p95 (D74;
`crates/lm-agents/tests/talk_on_thor.md:276`). That single number is measured around the HTTP call — in
`post_pack`, `let latency_ms = ms(started.elapsed());` (`crates/lm-agents/src/sysone.rs:1314`, carried on
the answer at `sysone.rs:434-435`) — so it covers the Thor's own scoring, the LAN, and our own queueing,
all three together. We can bound two of them: the d1 server scores about 11 ms per option, sequentially
(D70), and the move question is one question over ~8 options, which is most of the 82 ms it costs on an
idle box (`talk_on_thor.md:243`); Wi-Fi RTT is 5-8 ms. The one term we have never measured is **the wait
in our own least-use-first queue** — and it exists, because the request already carries the tick it asked
on and its deadline tick (`crates/lm-agents/src/queue.rs:44-47`), but the queue counts in sim ticks, not
wall clock (`crates/lm-agents/src/queue.rs:21`), so the wait never appears in any millisecond figure.

That number decides a live disagreement. D71 wants escalations near 4%; the Thor measurement puts them
at 16.1% of asks (`talk_on_thor.md:320`), carried by two ten-option talk gates, and the suggested fix is
a threshold change from 0.50 to 0.55 (`talk_on_thor.md:332-333`). We are tuning a number against a
latency we have not decomposed.

**Smallest experiment.** The request already knows the tick it asked on and the tick it was sent on.
Record the difference in wall-clock milliseconds next to the existing `latency_ms` on the ask's record,
and print the split from the existing three-seed Thor harness
(`cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture`). One round tells us
whether the move pack's 164 ms is server-side scoring (likely), queueing (unknown), or both.

**Crate.** `lm-agents` (`queue.rs`, `mind.rs`), `lm-learn` (the metric).

**Compatible.** The timestamps ride the ask's record, not the decision; nothing waits; the hash chain
does not see them. The queue's behaviour is tick-based and stays that way.

**Warning — do not copy the constant.** `forclaude10.md:3067` hardcodes a 100 MHz reference counter.
The counter frequency is a platform property the kernel reports, not a constant, and every nanosecond
that harness prints is scaled by it: at 24 MHz the same cycle count is understated by 4×. Read the
frequency from the kernel, or use `clock_gettime(CLOCK_MONOTONIC)`. The method is worth taking; the
constant is the bug.

### N2 — One byte that names which invariant failed, and it must be in the record

> `forclaude10.md:52` — "If the mask is non-zero (mask != 0x00), the state is auto-rejected inside the
> GPU cache line, preventing an invalid action from ever triggering host-side processing or dropping an
> unverified block into your jev-receipts ledger."
>
> `forclaude11.md:218-227` — `uint8_t error_mask = (__popc(peer_mask) > 1) ? 0x01 : 0x00;` …
> `error_mask |= 0x02; // Forbidden path intrusion` … `token.flux_error_mask = error_mask;`
>
> `forclaude11.md:419` — "pub flux_error_mask: u8, // 8-bit bitwise safety validation mask (0x00 == PASS)"

**What it is.** A rejection is one byte of reason, not a boolean, and the rejection is a *record*, not a
counter.

**Why it helps.** D26 makes a refusal an outcome and D64 makes the round a chain of files, so the reason
for a refusal is exactly the kind of thing that should be in the log. We already do this in two places:
the notation layer names every variant (`crates/lm-core/src/opal.rs:77`), and the reflex's failures are
counted by kind (`crates/lm-learn/src/metrics.rs:356-360` reads `reflex_refused`, `reflex_out_of_fuel`,
`reflex_error`). The model-answer path stops at the boundary: `crates/lm-agents/src/agents.rs:148-149`
does `self.stats.refusals += popped.refused_budget.len()` and `self.stats.expired += popped.expired.len()`,
and the round log carries no refusal record at all.

So the learner sees "8 refusals per round" (`talk_on_thor.md:133-137`) and cannot tell a timeout (a
deadline problem) from a stale menu (a cache problem) from a budget refusal (a scheduling problem) from a
schema break (a prompt problem). Four different fixes, one number. We have already paid for this
blindness once: the pack-level confidence rule refused 91% of packs and the cause — one weak gate folding
the whole pack — was invisible until it was measured (`talk_on_thor.md:108-112`, and D72).

**Smallest experiment.** Give the model's refusal a name (the `Refuse`-shaped error already exists in
`crates/lm-agents/src/gates.rs`), write it into the round log beside the decision it would have replaced,
and split the soak CSV's existing `refusals` column into the names. Run the three Thor seeds. If the
names are one population, the rule is wrong in a way we cannot see today.

**Crate.** `lm-agents` (`gates.rs`, `agents.rs`), `lm-core` (`log.rs`), `lm-learn` (`metrics.rs`).

**Compatible.** The reason code rides the decision record. It adds no waiting and no travel. It does
change what is hashed, so the golden hashes pinned in `tests/golden.toml` (D52) must be re-pinned in the
same commit.

### N3 — A decision with two deadlines has none

> `forclaude11.md:24` — "Implementing a hard latency firewall (e.g., 350 microseconds). If deep neural
> context transformations bottleneck, the cocapn-escalation gate drops execution out of heavy inference
> paths instantly, falling back to pre-validated, mathematically safe look-up matrices stored within
> hardware __constant__ cache lines."
>
> `forclaude10.md:1544` — "We cannot drop frames, we cannot spike queue latency past our 1ms boundary,
> and we cannot cause thread backpressure inside our tminus-dispatcher timeline. If our deep, multi-modal
> neural layers … take longer than 350 microseconds to resolve…"

**What it is.** Every expensive path gets exactly one fence, and what sits under the fence is
pre-validated and free. (The 350 µs number is wrong for us by three orders of magnitude — see §3.)

**Why it helps.** Our safe branch is already the table the conversations describe:
`crates/lm-core/src/agent.rs:45` — "The safe branch: known-free cells only, ending in cover or a hold."
The shape is ours. What is missing is a fence on the *second* opinion. clef-flash adds 593 ms at the
median and 1300 ms at p95 to a pack that was already 180 ms (`talk_on_thor.md:316`, and 335-339: "The
added latency is 593 ms, not the +25 ms D71 measured"), and it fires on 16.1% of asks. A decision whose
escalation returns after its apply tick has two deadlines, and the safe branch covers only one of them:
the pack was on time, the second opinion was not, and the move is lost with nothing recorded.

**Smallest experiment.** Give the escalation its own deadline — the decision's apply tick. If the second
opinion has not answered by then, the move takes the safe branch and the round records a named refusal
("escalation missed", which is N2's mechanism). The harness already counts escalations and refusals, so
the measurement is one run of the three seeds: how many moves are lost to a missed escalation today.

**Crate.** `lm-agents` (`mind.rs`, `sysone.rs`), `lm-core` (the safe branch).

**Compatible.** Nothing waits; the fallback is code, not a model; the refusal is a decision with
`Provenance::Reflex`, which never travels.

### N4 — Every decision has a terminal state, and a lost send must be visible

> `forclaude11.md:849-857` — "// Flags parameter 0x4000 (MSG_DONTWAIT) guarantees the simulation canvas
> loop never stalls." followed by `sendto(…, MSG_DONTWAIT, …)` **with the return value discarded**.
>
> `forclaude11.md:633` — `let bytes = recv(socket_fd, target_slot_ptr as *mut std::os::raw::c_void, 32, 0x4000);`
> — the ring's tail advances only when exactly 32 bytes arrive.
>
> `forclaude11.md:419-427` — the token carries `token_status` (`SIM_STATUS_EMPTY` / `_PRE_CONCEIVED` /
> `_VERIFIED` / `_DISPATCHED`).

**What it is.** A decision's journey ends in exactly one of {applied, late, refused, lost}, and all four
are in the record.

**Why it helps.** "Never wait" is half the rule. The other half is "never lose silently". Our sequencer
refuses by name and the round log is sealed per segment (`crates/lm-core/src/log.rs:532-542`), so the
ProArt side has the machinery — but nothing today counts the decisions that were *asked for* against the
decisions that were *accepted*, *late* or *refused*. The conversations' transport drops a datagram on a
full socket buffer and says nothing; `MSG_DONTWAIT` does not prevent a drop, it converts a block into a
failure, and the code throws the failure away. We cannot afford that: a decision that vanishes is a round
that cannot be replayed from its own log (D52's golden hashes) and cannot be learned from (D64).

**Smallest experiment.** Publish four counters per round — decisions asked, decisions accepted by the
sequencer, decisions refused by name, decisions that arrived after their apply tick — and assert that
accepted + refused + late == asked. Put them in the soak CSV next to the existing `decisions` and
`refusals` columns. If the assertion fails, there is a silent drop somewhere and we have found it with a
test instead of a mystery.

**Crate.** `lm-host-model` (`host_loop.rs`), `lm-agents` (`gates.rs`), `lm-core` (`log.rs`).

**Compatible.** The counters live in the round's own summary. No decision waits, and nothing in the
hashed state changes.

### N5 — Fence the model's GPU from the round's GPU — and do it for the right reason

> `forclaude10.md:3471-3473` — "To protect our sub-microsecond note validation deadlines from being
> interrupted by heavy multi-modal visual context processing tasks, we configure the Jetson Thor using
> hardware-enforced Multi-Instance GPU (MIG) grouping partitions", with MIG instance 0 running the heavy
> multi-modal work and instance 1 running the bit-exact checks.
>
> `forclaude10.md:6373` — "Hardware MIG Fencing Allocations".

**What it is.** Partition the Thor's GPU so the model's inference and the round's own GPU work cannot
contend. We have this as a spike candidate; the contribution here is *the reason to do it* and a way to
settle it cheaply.

**Why it helps.** The Thor is the game's critical path and its latency is not what we think it is. The
eleven-question pack costs 600 ms on an idle Thor and 1400 ms on a busy one
(`talk_on_thor.md:221-222`); the escalation adds 593 ms on top (`talk_on_thor.md:316`). If MIG is what
separates "idle" from "busy", it is worth roughly 2× on the talk pack — and the talk pack is what sets
the line rate. The Gemini review already proposes the spike
(`docs/research/fleet/13-gemini-architecture-review.md`); what it lacks is a number to aim at.

**Smallest experiment.** MIG is supported on our Thor and disabled — turn it on, put the d1 server on one
instance, and run the existing three-seed harness
(`cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture`). Compare the move pack
p50/p95 (164/262 ms) and the pack cost (600-1400 ms) against today's numbers. A config change and one
test run settles the spike.

**Crate.** `lm-agents` (the harness), the Thor's deployment (outside this repo), `lm-learn` (the metric).

**Compatible.** MIG partitions the GPU; it does not touch the sim, the log or the wire. Determinism is
unaffected — D69 was proven across architectures, and a MIG instance is the same ISA in a different
slice.

---

## 3. Wrong, or contradicts the design

Each with the reason: a measurement, a hardware fact, or a design rule.

1. **"450-650 nanoseconds", "less than 30 nanoseconds" jitter, "1,000,000 state updates" — the number is
   a mock.** `forclaude11.md:4` — "We validate this architecture under a continuous saturation stress-load
   of 1,000,000 state updates, demonstrating an invariant end-to-end processing roundtrip latency of
   450–650 nanoseconds"; `forclaude10.md:3171-3172`. The harness that produces it is
   `forclaude10.md:3249` — "// MOCK KERNEL INTERCEPT: Simulate the sub-microsecond Blackwell verification
   pass" — and the 1,000,000-iteration loop at `forclaude10.md:4009` is a host-side loop that writes a
   token and then *simulates* the GPU's verdict (`forclaude10.md:4052-4056` — "// MOCK BARE-METAL INTERCEPT
   BRIDGE — Simulates the exact register execution responses of our raw PTX assembly kernel"). It
   measures a CPU loop, possibly on a non-Thor box, and converts cycles to nanoseconds with the hardcoded
   100 MHz of `forclaude10.md:3067`. **Reason:** our tick is 33 ms (D20) and a think is 164 ms (D74) —
   400,000× the claimed figure. There is no CUDA, C++ or `.cu` file in this repository, and no
   `tests/thor_saturation_test.rs`; the "hardware-level substrate verification" phase
   (`forclaude10.md:3313-3326`) compiles and runs the mock.

2. **`sm_100` is the wrong architecture for the Thor.** `forclaude11.md:181`, `forclaude11.md:438`,
   `forclaude10.md:6372` — "Targets: NVIDIA Jetson Thor (AGX Thor T5000 Module / sm_100
   microarchitecture)". **Reason (hardware fact):** the Thor's T5000 is compute capability **11.0
   (sm_110)**; sm_100 is the Blackwell datacenter part (B100/B200). Every `-arch=sm_100` build line in
   both documents produces a binary for the wrong GPU.

3. **The Eisenstein norm is written with two different signs, and the documents disagree.** `forclaude11.md:89`
   — `N(P) = (r+iω)(r+iω²) = r² − r·i + i²`; `forclaude10.md:2792` and `forclaude10.md:2886` — "||z||² =
   r^2 + r*i + i^2". **Reason (mathematics):** for ω = e^{2πi/3}, `N(a+bω) = a² − ab + b²`. One of the two
   documents is wrong about its own algebra, and both use it as the basis of the proximity check.

4. **"We eliminate floating-point arithmetic entirely" — the record carries two floats.**
   `forclaude11.md:4`; `forclaude10.md:628` ("uses no external libraries, floating-point operations, or
   trigonometric calls") — against `forclaude10.md:482-483` (`pub float liquid_time_drift; pub float
   jepa_vibe_gradient;`), `forclaude10.md:644` (`float vibe_gradient`), `forclaude10.md:695-696`
   (`token.liquid_time_drift = 0.0f; token.jepa_vibe_gradient = vibe_gradient;`).
   **Reason (design rule):** the same corpus claims bit-exactness elsewhere — `forclaude10.md:286`
   ("Validates agent states with bit-exact, zero-drift INT8 error masks"), `forclaude10.md:612` ("FLUX
   runs bit-exact integer constraint filters"), `forclaude6.md:499` ("the exact same bit-for-bit integer
   coordinates"). A record with two IEEE floats, written by one host and read by another, is exactly the
   drift the design claims to remove — and it is the one thing D69 spent real hardware proving we do not
   have.

5. **forclaude6's "zero floating-point math in the safety-critical hot path" is false of its own code.**
   `forclaude6.md:498` — against `forclaude6.md:713` — `let c = ((a * a + b * b) as f64).sqrt().round() as i64;`.
   **Reason (measurement + mathematics):** the quantizer that is supposed to *be* the integer path uses
   an f64 square root; and since `c = round(√(a²+b²))`, the "exact Pythagorean integer triples
   (a² + b² = c²)" claimed at `forclaude6.md:97` is not what the code computes.

6. **forclaude6's checksum is not reproducible across Rust versions.** `forclaude6.md:840-850` —
   `std::collections::hash_map::DefaultHasher::new()` — against the claims at `forclaude6.md:588` ("the
   exact same bit-for-bit output regardless of whether they execute on a server-grade GPU, an embedded ARM
   processor, or a standard laptop CPU") and the struct's own field comment at `forclaude6.md:696` —
   "Checksum integrity hash for cross-platform verification". **Reason (hardware/software fact):** Rust's
   default hasher is explicitly not guaranteed stable across Rust releases. Our D52 golden hashes and D69
   cross-architecture proof hold only because we pinned FNV-1a (`crates/lm-core/src/hash.rs`).

7. **"Complete elimination of a centralized master system clock" contradicts D10, D18, D20 and D69.**
   `forclaude11.md:21` and `forclaude11.md:58` — "Complete elimination of a centralized master system
   clock. Every agent squad or track-character tracks its own timeline via localized countdown
   primitives"; `forclaude11.md:164`, `forclaude11.md:359`. **Reason (design rule):** our design has one
   clock and one authority — the ProArt is the sequencer and the authority (D18), the tick is 30 Hz
   (D20), and the Thor reproduces the ProArt's state bit-for-bit (D69). Per-agent countdowns do not
   produce a shared state; they produce N states.

8. **"The ProArt is a rendering pipeline; the Jetson Thor is the bit-exact, register-locked neural
   substrate" inverts D18.** `forclaude10.md:4132-4133`; the three-node layout at
   `forclaude10.md:4255-4280`. **Reason (design rule):** ours is the opposite — the ProArt holds the
   round log and the hash chain and is the authority; the Thor runs a mirror. If the Thor were the
   authority, both "only decisions travel" (D10) and the golden hashes (D52) would have to move.

9. **A deadline expressed in raw `cntvct_el0` cycles is meaningless between two machines.**
   `forclaude10.md:4290` — "pub target_deadline_hw: u64, // t-minus absolute target clock cycle reference
   (cntvct_el0)". **Reason (hardware fact):** the ProArt's generic timer and the Thor's are different
   counters with different frequencies and different origins. Our shared unit is the tick (D20).

10. **"You Have Abolished the Network Stack … treating remote edge hardware as local, stack-accessible
    memory addresses" is not possible.** `forclaude10.md:5781-5783`.
    **Reason (hardware fact):** the Thor is a separate machine on a LAN; there is no memory-mapped window
    across an Ethernet link. The documents' own transport code (`forclaude11.md:849-857`) uses UDP, which
    is the opposite of what the prose claims.

11. **The two ends of the "zero-copy shared memory" ring buffer do not agree on the layout.**
    `forclaude11.md:427` (`timeline: [LastMileNetworkToken; SIM_QUEUE_SLOTS]`) vs `forclaude11.md:1859-1863`
    (`timeline: [SimulationToken; 256]`); the C kernel reads a u32 from
    `&ctx->timeline[slot].sector_r` (`forclaude11.md:213-214`) — a field that exists only in the C struct
    (`forclaude11.md:1074-1075`) — while the Rust struct has `packed_4d_coordinate: u32` at offset 8
    (`forclaude11.md:1847-1856`). The upper two bytes are also given different names in the two
    documents: `forclaude11.md:414` ("[ trajectory_w | actuator_gate_z | sector_i | sector_r ]") vs
    `forclaude10.md:4294` ("[ voice_w | tensor_z | lattice_i | lattice_r ]").
    **Reason (design rule):** our wire format has one canonical byte layout that both ends agree on
    (D9, `crates/lm-proto`).

12. **`MSG_DONTWAIT` plus a discarded `sendto` return value is silent loss.**
    `forclaude11.md:849-857`. **Reason (design rule):** a decision that vanishes is a round that cannot be
    replayed from its log (D52) or learned from (D64). See N4, which is the fix.

13. **The 350 µs firewall is three orders of magnitude off our world.** `forclaude11.md:24`,
    `forclaude11.md:296`, `forclaude10.md:1544`. **Reason (measurement):** one d1 question costs ~11 ms per
    option, sequentially (D70); the move pack is 164 ms at the median (D74); the eleven-question pack is
    600-1400 ms. A 350 µs fence fires before the first token is out of the model.

14. **The hexagon lattice does not fit our grid.** `forclaude6.md:97`, `forclaude11.md:89`,
    `forclaude10.md:2886`. **Reason (design rule):** ours is a square grid with integer cells, Chebyshev
    moves and jump-point search (`crates/lm-core/src/fixed.rs`). Changing the lattice would change the
    moves, the path costs, the "known-free cells" of the safe branch, and every golden hash (D52).

15. **"Zero serialization. Zero tokens. Zero drift." / "dropping text entirely and moving the entire
    operational stack into a pure vector embedding space."** `forclaude6.md:178`, `forclaude6.md:116`.
    **Reason (design rule):** D12 and D13 are the opposite by design — the utterance *is* the score, and
    the agents perceive the score, never pixels. Dropping text drops the game's core mechanic.

16. **The frequency domain is wrong, and the two documents give different numbers for the same tier.**
    `forclaude6.md:170` — "Synchronous (≥ 1 kHz)" — vs `forclaude6.md:426` — "~200–500 Hz (2–5 ms)".
    **Reason (measurement):** our tick is 30 Hz (D20); both figures are off by a factor of 7-30 and
    neither is sourced.

17. **The proximity check detects co-location, not proximity.** `forclaude11.md:96` defines the key as
    `K_λ = R_T & 0x00FFFFFF`; the kernel at `forclaude11.md:214-217` compares all 32 bits of the word
    (`__match_any_sync(CONVERGED_MASK, current_reg)`) — including the `w` byte the proof leaves out — and
    `match.any.sync` reports *exact equality*. **Reason (mathematics):** two agents one cell apart have
    different keys and are not flagged. The proof's `O(1)` derivation (`forclaude11.md:104-106`) is also
    circular: the "2 clock cycles" figure assumes one cycle for `match.any` and one for `popc`, and the
    `vabsdiff4` "1 cycle" claim (`forclaude11.md:124`) describes a legacy instruction that Blackwell
    emulates in a sequence.

---

## 4. Unfalsifiable

Metaphor with no measurable claim. Kept short on purpose — these cost nothing to skip and everything to
misread.

- "The music protocol is our exact diagnostic monitor" / "if your integer constraints are misaligned, you
  will immediately hear the system glitch" (`forclaude10.md:619`). Nothing to measure, and our audio path
  is the utterance, not a MIDI port.
- "The model doesn't get bigger; the exocortex gets richer" (`forclaude10.md:4307`, `forclaude11.md:319`).
  D64 says something adjacent and measurable; this does not.
- "We have turned the human ear into the ultimate high-performance debugger" (`forclaude10.md:620`).
- "Absolute Present-Tense Fluidity", "total engineering closure", "a pure, zero-latency extension of the
  human mind", "absolute real-time precision" (`forclaude10.md:5780-5784`, `forclaude10.md:1080`).
- "Geometry Over Probability … Safety becomes a physical mathematical invariant, not a statistical guess"
  (`forclaude10.md:1097`). D59's "find hard, check cheap" is the measurable version.
- The vocabulary of `forclaude6` — "autopoiesis" (`forclaude6.md:1934`), "latent stigmergy"
  (`forclaude6.md:2092-2094`), "apoptotic pruning" (`forclaude6.md:2106`), the "epigenetic mask"
  (`forclaude6.md:2118-2119`), `HolonomyChecker` (`forclaude6.md:147`). The `HolonomyChecker` is a real
  idea in the documents (closed-loop conservation: return to a baseline and the error must be zero), and
  our `crates/lm-core/src/fixed.rs` is the integer arithmetic that would make it checkable — but as
  written there is no claim to falsify.

---

## What was not done

- **No code from the conversations was run, copied, or built.** Every snippet was read as text.
- **No file other than this one was edited, and nothing was committed.**
- The three conversations were read in full. The sub-microsecond claims were checked against the
  documents' own harness code rather than against the hardware, because the harness never touches the
  hardware.
- `docs/research/thor-transfer/` exists only on `origin/main`; the Thor-specific findings here were
  cross-checked against `docs/research/fleet/01-measured-facts.md`, D74, and
  `crates/lm-agents/tests/talk_on_thor.md`.

Done. `schedproto` is built in this directory, std-only, and `cargo test --release` is green.

## Report

**Tests — 32 passed, 0 failed** (26 unit + 6 property)

Unit: FNV-1a known vectors · Q16 conversion · smoother moves ¼ of the error · deadband blocks a 163-count lead while an 8 314-count lead switches · hysteresis keeps the incumbent inside the deadband · on-time/early arrival keep the tick · late ⇒ next tick + lateness counted · stale window inclusive (15 accepted / 16 dropped) · rotation fairness · canonical order · log round-trip for all four kinds · byte cost per kind · truncated log rejected · split on record boundary · `ActionOn` moves one cell · lane action applies every tick · intent follows plan · **expiry → safe branch** · **refused replacement → safe branch** · **new intent → plan**.

Property (50 seeds, xorshift64\*, 200–400 decisions each, on-time + late + stale mixed): the live hash sequence equals the decode-and-rerun sequence; decoded records re-encode to the identical bytes; every stream really contains stale drops and late stamps; the log is canonically ordered; and both kill tests — truncate at a random record boundary → replay prefix → append the rest → final hash matches the full replay, and the stronger variant that resumes *from the truncated state* (the cut can land mid-tick) → whole hash sequence and final hash match.

**Bytes per decision** — header `len4 + tag1 + tick8 + source2 + seq4` = 19 B + payload, 6 B magic per file:

| Kind | Bytes |
|---|---|
| ActionOn | 20 |
| Lane | 24 |
| Marker | 23 |
| Intent | 25 + 4 per waypoint (33 for 1+1) |

**Log size** — 16 agents, 4 Hz decisions + 10 Hz lanes, 10 min (18 000 ticks): 38 400 decisions; lanes 6 000 fleet-wide → **912 000 B (0.87 MiB)**, or 96 000 per agent → **3 072 000 B (2.93 MiB)**. The generator produced exactly 3 072 006 B (incl. magic), matching the analytic figure.

**Replay speed** — 3.07 MiB log, 134 400 decisions, 18 000 ticks: **3.88 ms → 4 643 115 ticks/s**, 756 MiB/s log throughput, ≈155 000× headroom over the 30 Hz wall clock.

Interpretations I had to fix (all documented in code and in the report): "late → next tick" = `arrival_tick + 1`; deadband `6554` = `ceil(0.10 × 65536)`; a replacement is *refused* when a live unexpired intent has a different first waypoint (the agent drops to the safe branch, keeping the live intent); movement precedence per tick is `ActionOn` → plan → safe → lane-chosen action.

Files: [out.md](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/out.md) (full report), [src/lib.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/lib.rs), [src/seq.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/seq.rs), [src/lane.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/lane.rs), [src/world.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/world.rs), [src/log.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/log.rs), [src/replay.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/src/replay.rs), [tests/properties.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/tests/properties.rs), [examples/report.rs](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/sched/examples/report.rs).
y_of_log_reproduces_the_live_hash_sequence` — the hash sequence
   produced live by the driver equals the one from decoding + re-running the
   log, tick count and final hash included.
2. `replay_from_decoded_records_matches_replay_from_bytes` — replay from the
   decoded record list == replay from the bytes; the decoded records re-encode
   to the identical byte string (the log is canonical).
3. `stale_decisions_are_dropped_and_late_ones_are_counted` — every stream
   really contains stale drops and late stamps.
4. `the_log_is_written_in_canonical_order`.
5. `kill_test_truncate_at_a_record_boundary_and_replay_the_prefix` — truncate
   at a random record boundary, replay the prefix, append the rest back:
   the replay equals the full replay **and the final hash matches**.
6. `kill_test_resume_from_the_truncated_state` — the stronger variant: replay
   only the prefix, then continue from that exact state with the remaining
   bytes (the cut may fall in the middle of a tick): the whole hash sequence
   and the final hash match the full replay.

## 3. Bytes per decision by kind

Fixed record framing: `len:u32` (4) + `tag:u8` (1) + `apply_tick:u64` (8) +
`source:u16` (2) + `seq:u32` (4) = **19 B** header, plus the payload; the file
starts with the 6-byte magic `SPLOG\x01`.

| Kind | Payload | Bytes / decision |
|---|---|---|
| `ActionOn{action}` | `action:u8` | **20** |
| `Lane{gate,p}` | `gate:u8` + `p[4]` | **24** |
| `Marker(u32)` | `value:u32` | **23** |
| `Intent{ttl,plan,safe}` | `ttl:u16` + 2×`u16` lens + 4 B/waypoint | **25 + 4 × (plan+safe)** (33 B for 1+1 waypoints) |

## 4. Log size — 16 agents, 4 Hz decisions + 10 Hz lanes, 10 minutes

30 ticks/s → **18 000 ticks**; decisions = 16 × 4 Hz × 600 s = **38 400**;
lanes = 10 Hz × 600 s = **6 000** (fleet-wide) or 16 × 10 Hz × 600 s =
**96 000** (per agent). Hashes: one every 30 ticks → 600 × u64 = 4 800 B.

| Variant | Log size |
|---|---|
| lanes 10 Hz fleet-wide | 38 400×20 + 6 000×24 = **912 000 B (0.87 MiB)** |
| lanes 10 Hz per agent | 38 400×20 + 96 000×24 = **3 072 000 B (2.93 MiB)** |
| + 1 marker per tick | 3 486 000 B (3.32 MiB) |

Measured: the generator built the per-agent-lanes scenario log at
**3 072 006 B** (3 072 000 + 6 magic) with 134 400 records — matches the
analytic figure exactly.

## 5. Replay speed

Replay = decode the log + re-run the world + hash every 30 ticks, release
build, best of 5 runs over the 10-minute per-agent-lanes log (3.07 MiB,
134 400 decisions, 18 000 ticks):

| Metric | Value |
|---|---|
| replay time | **3.88 ms** |
| **ticks / s** | **4 643 115** |
| decisions / s | 34 668 590 |
| log throughput | 756 MiB/s |
| headroom vs. 30 Hz real time | **≈ 155 000×** |

Replay is ~155 000× faster than the 30 Hz wall clock, and the 10-minute
scenario consumes 2.93 MiB — well inside a "log and replay" budget for phase 1.

## 6. Open points for phase 1

* The hash covers the full mutable world (positions, modes, intent TTLs and
  paths, lane scores, chosen actions, markers, tick) — the state is small
  enough to snapshot whole, so a checkpoint/restart scheme can be built on it.
* The log framing carries a `u32` length per record purely to make
  truncation/splitting safe; if the format is frozen for production, a
  fixed-size record table or a chunked container (e.g. per-tick frames) would
  compress better than 20–24 B/decision.
* `apply_tick` is absolute (u64) in the log; the same log replays on any
  machine as long as the world parameters (agents, gates) are fixed.
* Nothing here is persisted across process restarts yet — the prototype keeps
  the log in a `Vec<u8>` and the world in memory, by design.
EXIT 0

# Determinism audit — lm-core / lm-proto (branch `phase1-core` @ 5e0ce79)

Scope: every file under `crates/lm-core/src/` plus `crates/lm-proto/src/lib.rs`, read in full against the plan's Global Constraints (`docs/superpowers/plans/2026-10-28-m1-phase1-core.md`). No repository file was modified and no cargo was run. `sim.rs` / `script.rs` from the plan's file layout do not exist yet (later tasks), so Rhai is not wired into any audited path.

## Verdict

**No cross-machine or cross-run divergence was found.** Every computation is integer math with explicit little-endian encodings; all containers in sim state are `BTreeMap`/`BTreeSet`/`Vec`; every sort key and heap key is a total order; there is no clock, environment read, pointer value, or float anywhere (grep for `f32|f64` over both crates: zero hits). The committed `SIN_Q14` table was independently re-hashed outside the repo: 16 385 entries, monotone 0→16 384, FNV-1a over the LE `i32` bytes = `0xeb883dc1fe4f9e5d`, matching `TRIG_TABLE_HASH`. Debug and release builds produce identical state (only `debug_assert` differs, and `overflow-checks = true` in both profiles).

The findings below are therefore **reachable panics and invariant gaps from external input** (a header's `Params`/`start_tick`, decision payloads) — crash/DoS risks to a long round, not divergence — plus canonical-encoding nits flagged per the brief.

## Findings, by severity

### RISK 1 — u32 overflow in worldgen density targets, reachable from a hostile/typo'd header
`crates/lm-core/src/world/gen.rs:58` (and identically `gen.rs:85`)

```rust
let wall_target = half_cells * u32::from(p.wall_density_pct) / 100;
```

`half_cells = u32::from(w) * u32::from(half)` reaches ~2.1 × 10⁹ at the parameter maximum (`world_chunks (255,255), chunk_m 255, cell_m 1` → `w = 65025`), and the multiply-by-percentage happens before the division. `Params::validate` (params.rs:149) puts **no bound on world size in cells or on density**, so a header with e.g. `world_chunks (51,51), chunk_m 255, cell_m 1, wall_density_pct 51` passes validation, allocates a feasible ~169 MB grid, then panics: `half_cells = 13005 × 6502 = 84 558 510`, and `84 558 510 × 51 = 4 312 484 010 > u32::MAX`. With `overflow-checks = true` this is a release-mode panic — a crashed round on every node that builds the world. (The same unbounded shape also allows a 4.2 GB `Grid::new` allocation attempt at grid.rs:89, which aborts on OOM before the overflow even fires.)

**Fix:** compute the targets in `u64` (`u64::from(half_cells) * u64::from(p.wall_density_pct) / 100`), and have `Params::validate` refuse a total cell count above a sane ceiling, naming the field.

### RISK 2 — `set_intent` expiry add overflows when `start_tick` is near `u64::MAX`
`crates/lm-core/src/agent.rs:116`

```rust
expires: now + u64::from(i.ttl_ticks),
```

`start_tick` is a `u64` that travels in the `Header` (header.rs:73) and is fully external. A round whose header sets `start_tick` near `u64::MAX` panics on the first `Kind::Intent` decision applied at `now = start_tick` with any `ttl_ticks ≥ 1`. The codebase already treats the identical pattern defensively ten files over: the stun in the same tick pipeline uses `now.saturating_add(u64::from(p.stun_ticks))` (rules/beacons.rs:141), as does `sync_intent`'s consumer `move_step` via `now.saturating_sub(*at)` (agent.rs:194).

**Fix:** `now.saturating_add(u64::from(i.ttl_ticks))` — an intent expiring at `u64::MAX` reads as "never expires", the same semantic the stun path already chose.

### NIT 3 — usize values inside hashed data
`crates/lm-core/src/agent.rs:275` hashes a `usize` field directly: `h.u64(self.idx as u64)`. Length prefixes hash a `usize` narrowed to `u32` at header.rs:121, 125, 136 (`write_str`), decision.rs:202, 207, 218, 229, 312, agent.rs:265, 269, 312, 329, lane.rs:150, view.rs:147, 151, 335, rules/mod.rs:74, rules/beacons.rs:225, 229.

Both targets are LP64 so the bytes are identical on ProArt and Jetson, and no collection can exceed 4 G entries in phase 1, so this cannot diverge today. It is flagged because the crate's own rule is "explicit encodings, never layout": a 32-bit `usize` (a future embedded tool build) or a >4 G `String` would silently collide hashes. **Fix:** make `ActiveIntent::idx` a `u32` and use `u32::try_from(v.len()).unwrap_or(u32::MAX)` in the length prefixes.

### NIT 4 — silent `as i32` truncation in `split_units` breaks the `Pos` roundtrip for extreme inputs
`crates/lm-core/src/fixed.rs:101` — `carry as i32` where `carry = units.div_euclid(32_768)` is an `i64`. For `|units| ≥ 2^47` (reachable only through the `pub fn Pos::from_units`, e.g. via `offset`'s saturated `i64` extremes) the chunk index wraps, so `from_units(x, y).to_units() != (x, y)` and the documented invariant "two equal positions have one representation" silently breaks. All in-tree callers are bounded (grid centres ≤ ~2^34), so this is pub-API misuse only — and note the crate's `deny` lints don't cover `cast_possible_truncation`, so nothing guards this class. **Fix:** `i32::try_from(carry).unwrap_or(i32::MAX)` (or saturate), keeping the roundtrip total.

### NIT 5 — a refused >256-option lane sample still mutates conditioner state
`crates/lm-core/src/lane.rs:80-85` — `update` calls `self.reset(menu_id, values.len())` **before** the `values.len() > MAX_OPTIONS` refusal, so a 257-wide `Kind::Lane` decision (decision.rs:92 `values: Vec<u8>` — arbitrary width off the wire) reallocates the lanes and re-points `menu_id` on a call documented to "make no selection". The outcome stays deterministic on all machines, but a rejected decision changes the state hash, which will confuse a future divergence-bisect ("this node processed a decision that was refused"). **Fix:** move the `> MAX_OPTIONS` check above the reset so a refused sample leaves the conditioner untouched.

### NIT 6 — `speed_upt` above one cell per tick lets the motor hop over walls
`crates/lm-core/src/agent.rs:213-233` — `move_step` validates only the *final* cell of the whole axis-separated step (`grid.walkable(next_cell)`), not the cells crossed mid-step. With `speed_upt ≥ cell_units` (default 136 vs 2 048, but `Params::validate` puts no bound on the ratio) a single tick can jump `speed_upt / cell_units` cells and land beyond a wall. Deterministic on every machine, but it voids the module's own "the agent never enters a wall" guarantee under exactly the "nonsense parameters" class the plan's Review Focus 5 calls out. **Fix:** refuse `speed_upt > cell_units` in `Params::validate`, or advance cell-by-cell within the budget.

### NIT 7 — `expect` sites in non-test code (all currently unreachable)
`crates/lm-core/src/world/path.rs:210` (`parent[cur].expect("every open node has a parent")`) is the only one resting on an algorithmic invariant rather than a type fact: it holds because only heap-popped nodes are reconstructed and `g` strictly decreases along parent links, so the walk must terminate at `start`. `log.rs:145` and `log.rs:282`, plus `lm-proto/src/lib.rs:79`, `expect` postcard `to_allocvec` into a `Vec` — infallible for these pure-data types. None is reachable from wire input today; they become live the moment a type grows an IO-backed sink or the pathfinder is reused outside `jps`. **Fix:** leave with a comment, or convert to error returns if `jps` ever gains external callers.

### NIT 8 — `Recorder` tick counter overflows in the one-segment configuration
`crates/lm-core/src/log.rs:377` — `self.current_ticks += 1` is `u32`; with `segment_ticks = 0` ("the whole round is one segment") it never resets and panics after 2³² `record()` calls. Unreachable at 30 Hz inside any real round (4.5 years of ticks), flagged only because the same file is otherwise meticulous about width. **Fix:** make it `u64` (or `saturating_add`).

## Checked and clean

- **Floats:** zero `f32`/`f64` occurrences in either crate; the three `deny` lints are present at both crate roots; the trig table is committed bytes, never recomputed at runtime, and `gen-trig` lives outside `lm-core`.
- **Trig pin:** `SIN_Q14` re-hashed externally — 16 385 entries, `0xeb883dc1fe4f9e5d`, matches `TRIG_TABLE_HASH`; quadrant/octant reductions in `fixed.rs` index it without interpolation.
- **Unordered containers / iteration order:** no `HashMap`/`HashSet` anywhere; `Header::scripts`, `Sequencer::{pending,last_seq}`, `Agent::lanes`, `SquadKnowledge::last_seen` are all `BTree*`; the two `BinaryHeap`s (path.rs) order on total keys `(f, h, y, x)` that are unique per cell, so pop order among equals is immaterial.
- **Sorts:** the only sim sort is `sched.rs:154` `sort_by_key` over `(group, rot, rank, seq)` — a total order whose key is unique per decision (`seq` is strictly increasing per source), so stable vs unstable is moot; no `sort_unstable` in the crate.
- **StateHash coverage:** field-by-field check of every impl — `Params` (all 29 fields), `Header`, `RosterEntry`, `Agent` (all 11, including the derived `route` cache), `ActiveIntent`, `Beacons`, `SquadKnowledge`, `LaneConditioner`, `World`, `Grid`, `Decision`/`Stamped`/`Kind`/`Provenance` — found no missed field and no non-declaration-order hashing; the `Segment` hash deliberately covers the postcard body plus `prev`, consistent with its serde form.
- **Serde-vs-hash:** postcard varints over integer/string data only; `decode` (lm-proto) uses `take_from_bytes` + trailing check and is panic-free on garbage (property-tested at lm-proto/src/lib.rs:222-232); log reading never trusts a file length for one big allocation (chunked `read_body`, log.rs:311-328) and names the damaged blob on truncation.
- **Division by zero:** every denominator is a non-zero constant, a `max(1)` clamp (`grid.rs:141,153`, gen.rs:33), or validated (`cell_m`, `pairs + 1 ≥ 1`, `n_agents` zero-guarded in `rot`).
- **Loops:** the worldgen connectivity `loop` (gen.rs:168-175) terminates — carving only floors cells, so reachability grows monotonically and each pass connects ≥ 1 required cell; Bresenham, JPS scans and the flood fill are bounded by the grid extent; `isqrt` Newton converges from above without overflow at `u64::MAX`.
- **Workspace rules:** `lm-core` depends only on `serde` + `postcard`; `lm-proto` adds `thiserror` + `lm-core`; `overflow-checks = true` in both dev and release profiles, so an overflow crashes loudly everywhere rather than wrapping on one machine only.

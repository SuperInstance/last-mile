Done — the BRIEF.md prototype is built and green: **`cargo test --release`: 24 passed, 0 failed.**

## Short report

**Layout** — `cargo init --lib --name fixproto` plus `build.rs` (integer-only table generator: π via Machin, `atan(2^-i)` via series, CORDIC gain via integer Newton inverse-sqrt chain, rotation/vectoring CORDIC for the tables) and `src/{position,angle,fixedmath,grid,astar,jps,fnv}.rs`. Std only; no floats anywhere in runtime code or `build.rs` (f64 appears only inside test modules, to measure error).

**Measured results**

| Metric | Result |
|---|---|
| Trig max abs error vs f64 | 8.72e-5 = **1.43 Q14 LSB** (sin and cos, all 65,536 angles) |
| `angle_to` vs f64 `atan2` | max **1.49** angle units (1 unit = 1/65536 turn ≈ 0.0055°), mean ≈ 0 |
| Golden hash (10,000 moves) | **`0xc1885dc540668e0a`** (printed by the test, asserted as a const) |
| JPS vs A*, 200 seeded 64×64 maps (25% walls, 198 solvable) | **identical path cost on every map**; mean expansions **A\* 381.2 vs JPS 165.6** (JPS = 0.43× A*) |
| Timing (`Instant`, release) | movement `step` ≈ **9.2 ns/op**; A* ≈ **42.9 µs/map**; JPS ≈ **22.5 µs/map** |

**Key de-risk finding**: the classic JPS pruning rules are wrong under strict no-corner-cutting (a diagonal step needs *both* orthogonal cells walkable). The classic forced-neighbors are illegal steps there, and the straight-scan stop rule must shift one cell down-ray ("wall beside the *previous* cell, opening beside current"), with forced successors `(0,±s)` **and** `(dx,±s)` for straight-incoming nodes. That derivation is backed by 65,536 exhaustive 4×4 wall configurations, 3,000 random maps (densities 5–50%), a hand-built adversarial offset-wall corridor, and the brief's 200 seeded maps — cost equality with A* and step-legality of every reconstructed path asserted throughout.

**aarch64 watch-items** (all addressed explicitly):
- **Overflow**: FNV uses `wrapping_mul`; angle negation uses `wrapping_neg`; chunk/offset carries and `dist_sq` use `checked_*` and fail loudly instead of silently wrapping (exact for |chunk| < ~92,000). Rust's debug-panic/release-wrap behaviour is target-independent.
- **Endianness**: hash input is `i32::to_le_bytes` explicitly, so it's byte-identical on any host; tables are numeric constants. (aarch64 is LE anyway.)
- **Shifts/division**: `>>` on signed i64 is arithmetic by Rust's spec (no arch variance); `/` truncates toward zero — defined. No `unsafe`, no transmute, no float→int casts in runtime code, so there's no libm/rounding divergence between x86-64 and aarch64.
EXIT 0

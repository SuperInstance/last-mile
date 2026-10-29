//! Fixed-point math for the simulation.
//!
//! Every quantity in `lm-core` is an integer. Positions are counts of 1/1024 m
//! units, angles are 1/65536 of a turn and trigonometry is Q14 (16384 is 1.0).
//! No float ever enters this crate.

use crate::trig_table::SIN_Q14;

/// Units per metre.
pub const UNITS_PER_M: i32 = 1024;
/// Units per 32 m chunk.
pub const CHUNK_UNITS: i32 = 32_768;
/// Angle units in a full turn.
pub const ANGLE_TURN: u32 = 65_536;
/// Angle units in a quadrant.
pub const ANGLE_QUADRANT: u32 = 16_384;
/// Angle units in a half turn.
pub const ANGLE_HALF_TURN: u32 = ANGLE_TURN / 2;
/// Angle units in an octant.
pub const ANGLE_OCTANT: u32 = 8_192;
/// Q14 scale: 16384 is 1.0.
pub const Q14_ONE: i32 = 16_384;

/// A position in the world: a chunk plus an offset inside that chunk.
///
/// `off` is always in `0..CHUNK_UNITS` once the position has been
/// [`Pos::normalize`]d, so two equal positions have one representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pos {
    /// Chunk coordinate per axis.
    pub chunk: [i32; 3],
    /// Offset inside the chunk per axis, in units.
    pub off: [i32; 3],
}

impl Pos {
    /// The origin.
    pub const fn zero() -> Self {
        Self {
            chunk: [0; 3],
            off: [0; 3],
        }
    }

    /// A position from absolute unit coordinates on the x/y plane.
    pub fn from_units(x: i64, y: i64) -> Pos {
        let (cx, ox) = split_units(x);
        let (cy, oy) = split_units(y);
        Pos {
            chunk: [cx, cy, 0],
            off: [ox, oy, 0],
        }
    }

    /// Absolute unit coordinates on the x/y plane.
    pub fn to_units(&self) -> (i64, i64) {
        (
            i64::from(self.chunk[0]) * i64::from(CHUNK_UNITS) + i64::from(self.off[0]),
            i64::from(self.chunk[1]) * i64::from(CHUNK_UNITS) + i64::from(self.off[1]),
        )
    }

    /// Carry every offset into its chunk so that `0 <= off < CHUNK_UNITS`.
    pub fn normalize(self) -> Pos {
        let (c0, o0) = carry_axis(self.chunk[0], self.off[0]);
        let (c1, o1) = carry_axis(self.chunk[1], self.off[1]);
        let (c2, o2) = carry_axis(self.chunk[2], self.off[2]);
        Pos {
            chunk: [c0, c1, c2],
            off: [o0, o1, o2],
        }
    }

    /// Move by a delta in units and renormalize.
    pub fn offset(self, dx: i64, dy: i64) -> Pos {
        let (x, y) = self.to_units();
        Pos::from_units(x.saturating_add(dx), y.saturating_add(dy))
    }
}

/// Squared distance between two positions on the x/y plane, in units squared.
pub fn dist2(a: Pos, b: Pos) -> i64 {
    let (ax, ay) = a.to_units();
    let (bx, by) = b.to_units();
    let dx = i128::from(ax) - i128::from(bx);
    let dy = i128::from(ay) - i128::from(by);
    let d2 = dx * dx + dy * dy;
    if d2 > i128::from(i64::MAX) {
        i64::MAX
    } else {
        d2 as i64
    }
}

/// Split an absolute unit coordinate into a chunk index and an in-chunk offset.
fn split_units(units: i64) -> (i32, i32) {
    let chunk_units = i64::from(CHUNK_UNITS);
    let carry = units.div_euclid(chunk_units);
    let offset = units.rem_euclid(chunk_units);
    (carry as i32, offset as i32)
}

/// Carry an in-chunk offset into its chunk.
fn carry_axis(chunk: i32, off: i32) -> (i32, i32) {
    let chunk_units = i64::from(CHUNK_UNITS);
    let carry = i64::from(off).div_euclid(chunk_units);
    let offset = i64::from(off).rem_euclid(chunk_units);
    (chunk.saturating_add(carry as i32), offset as i32)
}

/// An angle: 65536 units make a full turn, counter-clockwise from +x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Angle(pub u16);

impl Angle {
    /// Sine in Q14: 16384 is 1.0.
    pub fn sin_q14(self) -> i32 {
        let a = u32::from(self.0);
        let quadrant = a / ANGLE_QUADRANT;
        let r = a % ANGLE_QUADRANT;
        match quadrant {
            0 => SIN_Q14[r as usize],
            1 => SIN_Q14[(ANGLE_QUADRANT - r) as usize],
            2 => -SIN_Q14[r as usize],
            _ => -SIN_Q14[(ANGLE_QUADRANT - r) as usize],
        }
    }

    /// Cosine in Q14: 16384 is 1.0.
    pub fn cos_q14(self) -> i32 {
        Angle(self.0.wrapping_add(ANGLE_QUADRANT as u16)).sin_q14()
    }
}

/// Integer square root: the largest `r` with `r * r <= n`.
pub fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    // Start above the root, then converge with Newton's method.
    let bits = 64 - n.leading_zeros();
    let mut x = 1u64 << (bits / 2 + 1);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// The angle of the direction `(dx, dy)`, rounded to the nearest angle unit.
pub fn angle_to(dx: i64, dy: i64) -> Angle {
    if dx == 0 && dy == 0 {
        return Angle(0);
    }
    let quadrant = quadrant_angle(dx.saturating_abs(), dy.saturating_abs());
    let q = i64::from(quadrant);
    let half_turn = i64::from(ANGLE_HALF_TURN);
    let turn = i64::from(ANGLE_TURN);
    let angle = match (dx < 0, dy < 0) {
        (false, false) => q,
        (true, false) => (half_turn - q).rem_euclid(turn),
        (false, true) => (turn - q).rem_euclid(turn),
        (true, true) => (half_turn + q).rem_euclid(turn),
    };
    Angle(angle as u16)
}

/// The angle of a vector in the first quadrant, in `0..=ANGLE_QUADRANT`.
fn quadrant_angle(ax: i64, ay: i64) -> u16 {
    if ax == 0 {
        return ANGLE_QUADRANT as u16;
    }
    if ay == 0 {
        return 0;
    }
    if ax >= ay {
        octant_angle(ax, ay)
    } else {
        ANGLE_QUADRANT as u16 - octant_angle(ay, ax)
    }
}

/// The angle of a vector inside the first octant (`mx >= my >= 0`), in
/// `0..=ANGLE_OCTANT`, found by binary search on the table's tangent ratio.
fn octant_angle(mx: i64, my: i64) -> u16 {
    debug_assert!(mx > 0 && my >= 0 && my <= mx);
    let mut lo = 0u16;
    let mut hi = ANGLE_OCTANT as u16;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if octant_cross(mid, mx, my) <= 0 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    // `lo` is the last angle at or below the target; take whichever of it and
    // its neighbour is closer to the direction.
    let below = octant_cross(lo, mx, my).abs();
    let above = if lo < ANGLE_OCTANT as u16 {
        octant_cross(lo + 1, mx, my).abs()
    } else {
        i128::MAX
    };
    if above < below {
        lo + 1
    } else {
        lo
    }
}

/// `sin(theta) * mx - cos(theta) * my`, the sign of which says whether the
/// direction `(mx, my)` lies above or below the angle `theta`.
fn octant_cross(theta: u16, mx: i64, my: i64) -> i128 {
    let s = i128::from(Angle(theta).sin_q14());
    let c = i128::from(Angle(theta).cos_q14());
    s * i128::from(mx) - c * i128::from(my)
}

/// Walk `units` units from `p` along the direction `a`.
pub fn step_along(p: Pos, a: Angle, units: i32) -> Pos {
    let dx = q14_scale(a.cos_q14(), units);
    let dy = q14_scale(a.sin_q14(), units);
    p.offset(dx, dy)
}

/// Scale a Q14 value by a unit count, rounding half away from zero.
fn q14_scale(value: i32, units: i32) -> i64 {
    let product = i64::from(value) * i64::from(units);
    let half = i64::from(Q14_ONE / 2);
    if product >= 0 {
        (product + half) >> 14
    } else {
        -((-product + half) >> 14)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Fnv64;
    use crate::trig_table::{SIN_Q14, TRIG_TABLE_HASH};

    #[test]
    fn normalize_carries_offset() {
        assert_eq!(
            Pos::from_units(32_768 + 5, -1).normalize(),
            Pos {
                chunk: [1, -1, 0],
                off: [5, 32_767, 0]
            }
        );
    }

    #[test]
    fn units_roundtrip() {
        for (x, y) in [(0, 0), (123_456, -7), (-1, -1)] {
            assert_eq!(Pos::from_units(x, y).to_units(), (x, y));
        }
    }

    #[test]
    fn dist2_across_chunks() {
        assert_eq!(
            dist2(Pos::from_units(32_760, 0), Pos::from_units(32_776, 0)),
            256
        );
    }

    #[test]
    fn trig_quadrants() {
        assert_eq!(Angle(0).sin_q14(), 0);
        assert_eq!(Angle(16_384).sin_q14(), 16_384);
        assert_eq!(Angle(32_768).cos_q14(), -16_384);
    }

    #[test]
    fn trig_table_hash_pinned() {
        let mut h = Fnv64::new();
        for v in SIN_Q14 {
            h.i32(v);
        }
        assert_eq!(h.finish(), TRIG_TABLE_HASH);
    }

    #[test]
    #[allow(clippy::unnecessary_map_or)]
    fn isqrt_exact() {
        for n in [0u64, 1, 2, 3, 4, 99, 100, u32::MAX as u64, u64::MAX] {
            let r = isqrt(n);
            assert!(
                r * r <= n && (r + 1).checked_mul(r + 1).map_or(true, |s| s > n),
                "isqrt({n}) = {r}"
            );
        }
    }

    #[test]
    fn angle_to_cardinals() {
        assert_eq!(angle_to(1, 0), Angle(0));
        assert_eq!(angle_to(0, 1), Angle(16_384));
        assert_eq!(angle_to(-1, 0), Angle(32_768));
    }

    #[test]
    fn offset_moves_and_normalizes() {
        let p = Pos::from_units(32_768 + 5, 0);
        assert_eq!(p.offset(10, -10).to_units(), (32_783, -10));
        assert_eq!(Pos::zero().offset(-1, 0).to_units(), (-1, 0));
    }

    #[test]
    fn angle_to_diagonals_and_quadrants() {
        assert_eq!(angle_to(1, 1), Angle(8_192));
        assert_eq!(angle_to(0, -1), Angle(49_152));
        assert_eq!(angle_to(-1, -1), Angle(40_960));
        assert_eq!(angle_to(0, 0), Angle(0));
    }

    #[test]
    fn step_along_follows_the_angle() {
        let p = Pos::from_units(0, 0);
        assert_eq!(step_along(p, Angle(0), 100).to_units(), (100, 0));
        assert_eq!(step_along(p, Angle(16_384), 100).to_units(), (0, 100));
        assert_eq!(step_along(p, Angle(32_768), 100).to_units(), (-100, 0));
    }
}

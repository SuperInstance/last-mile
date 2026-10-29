//! Chunked fixed-point world position (1/1024 m units, 32 m chunks).

use crate::angle;

/// One meter in fixed-point units.
pub const UNITS_PER_METER: i32 = 1024;
/// Chunk edge length in units: 32 m * 1024.
pub const CHUNK_SIZE_UNITS: i32 = 32 * UNITS_PER_METER;

/// A world position: chunk coordinates plus an offset inside the chunk.
/// A position is *canonical* once `off` lies in `[0, CHUNK_SIZE_UNITS)`;
/// `normalize()` restores that invariant, all other ops return canonical
/// positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Position {
    pub chunk: (i32, i32),
    pub off: (i32, i32),
}

fn carry(off: i32) -> (i32, i32) {
    // div_euclid/rem_euclid give the floor-carry behaviour we want for negatives.
    // |carry| <= 65536 and carry * CHUNK fits i32 even at the i32::MIN extreme.
    let c = off.div_euclid(CHUNK_SIZE_UNITS);
    (c, off - c * CHUNK_SIZE_UNITS)
}

impl Position {
    pub const fn origin() -> Self {
        Position { chunk: (0, 0), off: (0, 0) }
    }

    /// Carry out-of-range offsets into the chunk coordinates.
    pub fn normalize(&mut self) {
        let (cx, rx) = carry(self.off.0);
        let (cy, ry) = carry(self.off.1);
        self.chunk.0 = self.chunk.0.checked_add(cx).expect("chunk x overflow");
        self.chunk.1 = self.chunk.1.checked_add(cy).expect("chunk y overflow");
        self.off = (rx, ry);
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    /// Shift by a delta in units; returns a canonical position.
    pub fn add_delta(self, dx: i32, dy: i32) -> Self {
        // Widen before adding so an unnormalized or extreme offset cannot
        // overflow the i32 offset fields mid-computation.
        let ox = self.off.0 as i64 + dx as i64;
        let oy = self.off.1 as i64 + dy as i64;
        let cx = ox.div_euclid(CHUNK_SIZE_UNITS as i64) as i32;
        let cy = oy.div_euclid(CHUNK_SIZE_UNITS as i64) as i32;
        Position {
            chunk: (
                self.chunk.0.checked_add(cx).expect("chunk x overflow"),
                self.chunk.1.checked_add(cy).expect("chunk y overflow"),
            ),
            off: (
                (ox - cx as i64 * CHUNK_SIZE_UNITS as i64) as i32,
                (oy - cy as i64 * CHUNK_SIZE_UNITS as i64) as i32,
            ),
        }
    }

    pub fn sub_delta(self, dx: i32, dy: i32) -> Self {
        self.add_delta(dx.wrapping_neg(), dy.wrapping_neg())
    }

    /// Absolute world coordinates in units.
    pub fn global(&self) -> (i64, i64) {
        (
            self.chunk.0 as i64 * CHUNK_SIZE_UNITS as i64 + self.off.0 as i64,
            self.chunk.1 as i64 * CHUNK_SIZE_UNITS as i64 + self.off.1 as i64,
        )
    }

    /// Exact squared distance in units^2. Exact while both global coordinates
    /// stay below ~3.04e9 (|chunk| < ~92000); overflow is a hard error, not a wrap.
    pub fn dist_sq(&self, other: &Position) -> i64 {
        let (ax, ay) = self.global();
        let (bx, by) = other.global();
        let dx = ax - bx;
        let dy = ay - by;
        let dx2 = dx.checked_mul(dx).expect("dist_sq overflow (dx)");
        let dy2 = dy.checked_mul(dy).expect("dist_sq overflow (dy)");
        dx2.checked_add(dy2).expect("dist_sq overflow (sum)")
    }

    /// Advance one tick at `speed` (units/tick) along `angle`. Fully integer,
    /// deterministic, returns a canonical position.
    pub fn step(self, angle: u16, speed_units_per_tick: i32) -> Self {
        let (c, s) = angle::sin_cos(angle);
        let dx = round_div_i64(c as i64 * speed_units_per_tick as i64, 16384);
        let dy = round_div_i64(s as i64 * speed_units_per_tick as i64, 16384);
        self.add_delta(dx as i32, dy as i32)
    }
}

/// Divide by d rounding half away from zero (d > 0), truncating division otherwise.
pub fn round_div_i64(n: i64, d: i64) -> i64 {
    if n >= 0 {
        (n + d / 2) / d
    } else {
        (n - d / 2) / d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_carries_both_directions() {
        let mut p = Position { chunk: (0, 0), off: (-1, -1) };
        p.normalize();
        assert_eq!(p, Position { chunk: (-1, -1), off: (32767, 32767) });

        let mut p = Position { chunk: (5, 5), off: (32768, -32769) };
        p.normalize();
        // 32768 -> chunk +1, off 0; -32769 -> chunk -2, off 32767
        assert_eq!(p, Position { chunk: (6, 3), off: (0, 32767) });

        let mut p = Position { chunk: (0, 0), off: (i32::MIN, i32::MIN) };
        p.normalize();
        assert_eq!(p.chunk, (-65536, -65536));
        assert_eq!(p.off, (0, 0));
    }

    #[test]
    fn add_sub_roundtrip() {
        let p = Position { chunk: (3, -4), off: (100, 32000) };
        let q = p.add_delta(4096, -5000);
        assert!(q.off.0 >= 0 && q.off.0 < CHUNK_SIZE_UNITS);
        assert!(q.off.1 >= 0 && q.off.1 < CHUNK_SIZE_UNITS);
        assert_eq!(q.sub_delta(4096, -5000), p);
    }

    #[test]
    fn dist_sq_across_chunks() {
        let a = Position::origin();
        let b = Position { chunk: (0, 0), off: (3, 4) };
        assert_eq!(a.dist_sq(&b), 25);

        // c sits at global (32773, -65543): carry x by +1 chunk, y by -3 chunks
        let c = Position::origin().add_delta(32768 + 5, -(2 * 32768) - 7);
        assert_eq!(c, Position { chunk: (1, -3), off: (5, 32761) });
        assert_eq!(a.dist_sq(&c), 32773 * 32773 + 65543 * 65543);
    }

    #[test]
    fn step_moves_about_one_speed() {
        let mut rng = crate::grid::Xorshift64Star::new(0xBEEF_0000_0000_0002);
        for _ in 0..100_000 {
            let a = rng.next_u64() as u16;
            let speed = 1 + (rng.next_u64() % 2048) as i32;
            let p0 = Position::origin().add_delta(
                (rng.next_u64() % 30000) as i32 * 97 % 65536 - 32768,
                (rng.next_u64() % 30000) as i32 * 97 % 65536 - 32768,
            );
            let p1 = p0.step(a, speed);
            // canonical invariant
            assert!(p1.off.0 >= 0 && p1.off.0 < CHUNK_SIZE_UNITS);
            assert!(p1.off.1 >= 0 && p1.off.1 < CHUNK_SIZE_UNITS);
            // displacement magnitude within rounding of speed
            let d = p0.dist_sq(&p1);
            assert!(d <= (speed as i64 + 2) * (speed as i64 + 2), "overshoot {d} speed {speed}");
            assert!(d >= (speed as i64 - 2) * (speed as i64 - 2).max(0), "undershoot {d} speed {speed}");
        }
    }
}

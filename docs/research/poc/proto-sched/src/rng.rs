//! xorshift64* — the deterministic PRNG used by the property tests.

/// xorshift64* (Vigna). Seed 0 is remapped so the generator never degenerates.
#[derive(Clone, Debug)]
pub struct XorShift64Star {
    state: u64,
}

impl XorShift64Star {
    pub fn new(seed: u64) -> Self {
        XorShift64Star { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish value in `0..n` (0 when `n == 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }

    /// Uniform-ish value in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

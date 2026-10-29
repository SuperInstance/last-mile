//! Small exact integer math helpers.

/// floor(sqrt(n)) by integer Newton iteration.
/// Converges from an upper bound derived from the bit length, so the classic
/// `y >= x` termination returns the exact floor. No overflow: x starts at
/// most 2^32 for u64 inputs and (x + n/x) stays below 2^33.
pub fn isqrt_u64(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let bits = 64 - n.leading_zeros();
    let mut x = 1u64 << ((bits + 1) / 2); // >= floor(sqrt(n))
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Xorshift64Star;

    #[test]
    fn isqrt_exhaustive_small() {
        for n in 0..(1u64 << 20) {
            let r = isqrt_u64(n);
            assert!(r * r <= n, "n={n} r={r}");
            assert!((r + 1) * (r + 1) > n, "n={n} r={r}");
        }
    }

    #[test]
    fn isqrt_random_and_edges() {
        let mut rng = Xorshift64Star::new(0x5EED_5123_5123_0004);
        for _ in 0..500_000 {
            let n = rng.next_u64();
            let r = isqrt_u64(n);
            assert!(r * r <= n && (r + 1).checked_mul(r + 1).map_or(true, |v| v > n), "n={n}");
        }
        for n in [u64::MAX, u64::MAX - 1, (1u64 << 63) + 12345, 1 << 62] {
            let r = isqrt_u64(n);
            assert!(r * r <= n);
            // (r+1)^2 may overflow u64 exactly when it exceeds u64::MAX, i.e. always here
            assert!((r + 1).checked_mul(r + 1).map_or(true, |v| v > n), "n={n} r={r}");
        }
        assert_eq!(isqrt_u64(u64::MAX), 4294967295);
        assert_eq!(isqrt_u64(0), 0);
        assert_eq!(isqrt_u64(1), 1);
        assert_eq!(isqrt_u64(3), 1);
        assert_eq!(isqrt_u64(4), 2);
    }
}

//! Deterministic RNG streams, one per (seed, domain, id).
//!
//! Every random draw in the simulation comes from a named stream, so two
//! machines that play the same round draw the same numbers in the same order.
//! There is no thread RNG and no clock: the stream seed is the hash of
//! (seed, domain, id) and the generator is SplitMix64.

use crate::hash::Fnv64;

/// The SplitMix64 gamma constant.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// What a stream is for. The domain keeps unrelated draws apart even when
/// they share a seed and an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Domain {
    /// World generation.
    Worldgen = 0,
    /// Agent behaviour.
    Agent = 1,
    /// Menu rolls.
    Roll = 2,
    /// Rules and scoring.
    Rules = 3,
}

/// A SplitMix64 generator seeded for one named stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// The stream for `(seed, domain, id)`.
    pub fn stream(seed: u64, domain: Domain, id: u64) -> Rng {
        let mut h = Fnv64::new();
        h.u64(seed);
        h.write(&[domain as u8]);
        h.u64(id);
        Rng { state: h.finish() }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (`0` when `n` is zero), without modulo bias.
    pub fn below(&mut self, n: u32) -> u32 {
        (((self.next_u64() >> 32) * u64::from(n)) >> 32) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_streams_independent_and_stable() {
        let a = Rng::stream(7, Domain::Agent, 1).next_u64();
        assert_ne!(a, Rng::stream(7, Domain::Agent, 2).next_u64());
        assert_eq!(a, Rng::stream(7, Domain::Agent, 1).next_u64());
    }

    #[test]
    fn below_in_range() {
        let mut r = Rng::stream(1, Domain::Roll, 0);
        for _ in 0..10_000 {
            assert!(r.below(7) < 7);
        }
    }

    #[test]
    fn domains_and_seeds_differ() {
        let a = Rng::stream(7, Domain::Agent, 1).next_u64();
        assert_ne!(a, Rng::stream(7, Domain::Roll, 1).next_u64());
        assert_ne!(a, Rng::stream(8, Domain::Agent, 1).next_u64());
    }

    #[test]
    fn below_zero_is_zero() {
        let mut r = Rng::stream(1, Domain::Roll, 0);
        assert_eq!(r.below(0), 0);
    }
}

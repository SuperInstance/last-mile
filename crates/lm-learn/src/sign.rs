//! The exact one-sided binomial sign test.
//!
//! The pass rule the flow judges a proposal by has two parts: the count of
//! the seeds the change met the sealed bar on, and this — the sign test, the
//! one-sided binomial tail at 1/2 over the per-seed deltas, ties excluded
//! (crates/lm-learn/SEEDFARM.md).
//!
//! Under the null "the change does nothing", every seed's delta is as likely
//! up as down, so the number of seeds that moved the claimed way is a coin
//! flip's count, and the p-value is how unlikely the observed count is. The
//! count carries "how often"; the sign test carries "which way": it refuses
//! a change that meets the bar on many seeds while moving the others the
//! wrong way.
//!
//! ## The arithmetic
//!
//! The decision is made in **integers, never in floats**. The tail is
//! `sum(C(n, j) for j >= k)` over `2^n`, so it is below `1/20` exactly when
//!
//! ```text
//!     20 * sum(C(n, j) for j >= k)  <  2^n
//! ```
//!
//! and every part of that is an integer. Two machines reach the same answer
//! for every seed count (D69's spirit: a verdict must not depend on the
//! machine that read it).
//!
//! The numerator grows with the seed count — `2^n` — so the arithmetic needs
//! more than a fixed-width integer holds. [`BigUint`] is the smallest thing
//! that does: an add, a multiply by a small number, an exact division by a
//! small number, a shift and a comparison.

use std::cmp::Ordering;

use crate::ledger::Direction;

/// The p-value the pass rule compares against: 0.05, as an exact fraction.
pub const ALPHA_NUMERATOR: u64 = 1;
pub const ALPHA_DENOMINATOR: u64 = 20;

/// How many decimal places the p-value is printed with.
pub const PLACES: usize = 4;

/// The sign test over a round's per-seed deltas.
///
/// The ties — the seeds whose delta was zero — are excluded: a seed that did
/// not move at all says nothing about which way the change moved the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignTest {
    /// The seeds that moved at all.
    pub moved: usize,
    /// How many of the seeds that moved moved the claimed way.
    pub claimed: usize,
}

impl SignTest {
    /// The p-value in decimal, truncated (never rounded) at [`PLACES`]
    /// places.
    ///
    /// The text is a rendering, not the decision: the decision is the exact
    /// comparison in [`passes`]. This one is truncated with integer
    /// arithmetic too, so it is the same on every machine.
    pub fn p_value_text(&self) -> String {
        let tail = binomial_tail(self.moved, self.claimed);
        let mut scaled = tail;
        scaled.mul_small(10u64.pow(PLACES as u32));
        // floor(p * 10^PLACES): dividing by 2^n is a shift.
        let whole = scaled.shr_bits(self.moved);
        let digits = whole.to_decimal_string();
        let digits = format!("{digits:0>width$}", width = PLACES + 1);
        let split = digits.len() - PLACES;
        format!("{}.{}", &digits[..split], &digits[split..])
    }
}

/// The sign test of a set of per-seed deltas.
///
/// The claimed way is the prediction's direction: up for [`Direction::Up`],
/// down for [`Direction::Down`]. The deltas are the change the proposal made
/// to the metric, so a positive delta is a rise.
pub fn sign_test(deltas: &[i64], direction: Direction) -> SignTest {
    let mut moved = 0;
    let mut claimed = 0;
    for &delta in deltas {
        if delta == 0 {
            // A tie: the seed did not move, so it is not in the test.
            continue;
        }
        moved += 1;
        let the_claimed_way = match direction {
            Direction::Up => delta > 0,
            Direction::Down => delta < 0,
        };
        if the_claimed_way {
            claimed += 1;
        }
    }
    SignTest { moved, claimed }
}

/// Does the sign test pass: is its p-value below 0.05?
///
/// The comparison is exact — `20 * tail < 2^n` — so there is no float near
/// the verdict and no rounding to argue about.
pub fn passes(test: SignTest) -> bool {
    tail_below(test.moved, test.claimed, ALPHA_NUMERATOR, ALPHA_DENOMINATOR)
}

/// Is `P(X >= k)` for `X` drawn from `Binomial(n, 1/2)` strictly below
/// `num/den`?
///
/// The comparison is exact: the tail is `sum(C(n, j) for j >= k)` over
/// `2^n`, so it is below `num/den` exactly when
/// `tail * den < num * 2^n`, and every part of that is an integer.
fn tail_below(n: usize, k: usize, num: u64, den: u64) -> bool {
    let tail = binomial_tail(n, k);
    let mut left = tail;
    left.mul_small(den);
    let mut right = BigUint::from_u64(num);
    right.shl_bits(n);
    left.cmp_to(&right) == Ordering::Less
}

/// The sum of the binomial coefficients from `k` to `n`: the numerator of
/// `P(X >= k)` for `X` drawn from `Binomial(n, 1/2)`.
fn binomial_tail(n: usize, k: usize) -> BigUint {
    let mut sum = BigUint::zero();
    // C(n, 0) is 1, and C(n, j+1) = C(n, j) * (n - j) / (j + 1).
    let mut coefficient = BigUint::one();
    for j in 0..=n {
        if j >= k {
            sum.add_assign(&coefficient);
        }
        if j < n {
            coefficient.mul_small((n - j) as u64);
            coefficient.div_small_exact((j + 1) as u64);
        }
    }
    sum
}

/// A non-negative integer, as little-endian 32-bit limbs.
///
/// The sign test's tail needs integers of thousands of bits — `2^n` for `n`
/// seeds — which no fixed-width type holds. This is the smallest thing that
/// does: an add, a multiply by a small number, an exact division by a small
/// number, a shift, a comparison and a rendering in decimal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BigUint {
    /// The limbs, least significant first. Empty is zero.
    limbs: Vec<u32>,
}

impl BigUint {
    /// Zero.
    fn zero() -> Self {
        BigUint { limbs: Vec::new() }
    }

    /// One.
    fn one() -> Self {
        BigUint { limbs: vec![1] }
    }

    /// A number that fits in a `u64`.
    fn from_u64(value: u64) -> Self {
        let mut limbs = Vec::new();
        let mut rest = value;
        while rest > 0 {
            limbs.push((rest & 0xffff_ffff) as u32);
            rest >>= 32;
        }
        BigUint { limbs }
    }

    /// Is this zero?
    fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Drop the leading zero limbs, so the length is the number's size.
    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    /// Add `other` to this number.
    fn add_assign(&mut self, other: &BigUint) {
        let mut carry = 0u64;
        let length = self.limbs.len().max(other.limbs.len());
        self.limbs.resize(length, 0);
        for index in 0..length {
            let sum = self.limbs[index] as u64
                + other.limbs.get(index).copied().unwrap_or(0) as u64
                + carry;
            self.limbs[index] = (sum & 0xffff_ffff) as u32;
            carry = sum >> 32;
        }
        if carry > 0 {
            self.limbs.push(carry as u32);
        }
    }

    /// Multiply this number by a small one.
    fn mul_small(&mut self, factor: u64) {
        let mut carry = 0u128;
        for limb in self.limbs.iter_mut() {
            let product = (*limb as u128) * (factor as u128) + carry;
            *limb = (product & 0xffff_ffff) as u32;
            carry = product >> 32;
        }
        while carry > 0 {
            self.limbs.push((carry & 0xffff_ffff) as u32);
            carry >>= 32;
        }
    }

    /// Divide this number by a small one, exactly.
    ///
    /// The division the binomial recurrence makes is exact, and this says so
    /// rather than rounding it away.
    fn div_small_exact(&mut self, divisor: u64) {
        let remainder = self.divmod_small(divisor);
        assert_eq!(
            remainder, 0,
            "the binomial coefficient's division by {divisor} is not exact"
        );
    }

    /// Divide this number by a small one, and hand back the remainder.
    fn divmod_small(&mut self, divisor: u64) -> u64 {
        let mut remainder = 0u128;
        for index in (0..self.limbs.len()).rev() {
            let current = (remainder << 32) | self.limbs[index] as u128;
            self.limbs[index] = (current / divisor as u128) as u32;
            remainder = current % divisor as u128;
        }
        self.trim();
        remainder as u64
    }

    /// Multiply this number by `2^bits`.
    fn shl_bits(&mut self, bits: usize) {
        let limbs = bits / 32;
        let rest = bits % 32;
        if rest > 0 {
            let mut carry = 0u32;
            for limb in self.limbs.iter_mut() {
                let next_carry = *limb >> (32 - rest);
                *limb = (*limb << rest) | carry;
                carry = next_carry;
            }
            if carry > 0 {
                self.limbs.push(carry);
            }
        }
        if limbs > 0 {
            let mut shifted = vec![0u32; limbs];
            shifted.extend_from_slice(&self.limbs);
            self.limbs = shifted;
        }
    }

    /// This number divided by `2^bits`, rounded down.
    fn shr_bits(&self, bits: usize) -> BigUint {
        let limbs = bits / 32;
        let rest = bits % 32;
        let mut out: Vec<u32> = self.limbs.iter().skip(limbs).copied().collect();
        if rest > 0 {
            for index in 0..out.len() {
                let high = out.get(index + 1).copied().unwrap_or(0);
                out[index] = (out[index] >> rest) | (high << (32 - rest));
            }
        }
        let mut shifted = BigUint { limbs: out };
        shifted.trim();
        shifted
    }

    /// Compare this number with another: the sizes first, then the limbs.
    fn cmp_to(&self, other: &BigUint) -> Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for index in (0..self.limbs.len()).rev() {
            if self.limbs[index] != other.limbs[index] {
                return self.limbs[index].cmp(&other.limbs[index]);
            }
        }
        Ordering::Equal
    }

    /// This number in decimal.
    fn to_decimal_string(&self) -> String {
        if self.is_zero() {
            return "0".to_owned();
        }
        let mut rest = self.clone();
        let mut chunks: Vec<u32> = Vec::new();
        while !rest.is_zero() {
            chunks.push(rest.divmod_small(1_000_000_000) as u32);
        }
        let mut out = String::new();
        for (index, chunk) in chunks.iter().enumerate().rev() {
            if index == chunks.len() - 1 {
                out.push_str(&chunk.to_string());
            } else {
                out.push_str(&format!("{chunk:09}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The p-value of the fairest possible sample at N = 4: 1/16 = 0.0625.
    #[test]
    fn the_fairest_sample_at_four_seeds_is_just_above_the_line() {
        let test = sign_test(&[1, 1, 1, 1], Direction::Up);
        assert_eq!(test.moved, 4);
        assert_eq!(test.claimed, 4);
        assert_eq!(test.p_value_text(), "0.0625");
        assert!(
            !passes(test),
            "four of four is p = 0.0625, which is not below 0.05"
        );
    }

    /// The p-values the seed farm's table prints: exact, and truncated.
    #[test]
    fn the_p_values_are_exact() {
        // 12 of 16: 2517/65536 = 0.0384...
        let twelve_of_sixteen = sign_test(
            &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1],
            Direction::Up,
        );
        assert_eq!(twelve_of_sixteen.moved, 16);
        assert_eq!(twelve_of_sixteen.claimed, 12);
        assert_eq!(twelve_of_sixteen.p_value_text(), "0.0384");
        assert!(passes(twelve_of_sixteen));

        // 11 of 16: 6885/65536 = 0.1050...
        let eleven_of_sixteen = sign_test(
            &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1],
            Direction::Up,
        );
        assert_eq!(eleven_of_sixteen.claimed, 11);
        assert_eq!(eleven_of_sixteen.p_value_text(), "0.1050");
        assert!(!passes(eleven_of_sixteen));

        // 16 of 16: 1/65536 = 0.0000...
        let sixteen = sign_test(&[1; 16], Direction::Up);
        assert_eq!(sixteen.p_value_text(), "0.0000");
        assert!(passes(sixteen));

        // Nothing moved: no evidence, so p = 1.
        let nothing = sign_test(&[0, 0, 0, 0], Direction::Up);
        assert_eq!(nothing.moved, 0);
        assert_eq!(nothing.p_value_text(), "1.0000");
        assert!(!passes(nothing));
    }

    /// The ties are excluded from the test, and a test of only ties has no
    /// population to read.
    #[test]
    fn the_ties_are_excluded() {
        let test = sign_test(&[1, 1, 0, 0], Direction::Up);
        assert_eq!(test.moved, 2);
        assert_eq!(test.claimed, 2);
        // 2 of 2: p = 1/4.
        assert_eq!(test.p_value_text(), "0.2500");
        assert!(!passes(test));
    }

    /// The direction is the prediction's: a rise is not a fall.
    #[test]
    fn the_direction_is_the_prediction_s() {
        let deltas = [1, 1, -1, -1];
        let up = sign_test(&deltas, Direction::Up);
        assert_eq!(up.moved, 4);
        assert_eq!(up.claimed, 2);
        let down = sign_test(&deltas, Direction::Down);
        assert_eq!(down.moved, 4);
        assert_eq!(down.claimed, 2);
        assert!(!passes(up));
        assert!(!passes(down));
    }

    /// The tail is the sum of the binomial coefficients: the number is
    /// checked against a hand-computed one.
    #[test]
    fn the_tail_is_the_sum_of_the_binomial_coefficients() {
        // C(16, j) for j = 12..16: 1820 + 560 + 120 + 16 + 1 = 2517.
        let tail = binomial_tail(16, 12);
        assert_eq!(tail.to_decimal_string(), "2517");
        // The whole row sums to 2^n.
        assert_eq!(binomial_tail(0, 0).to_decimal_string(), "1");
        assert_eq!(binomial_tail(16, 0).to_decimal_string(), "65536");
    }
}

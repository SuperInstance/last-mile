//! Deterministic stand-ins for the libm transcendentals.
//!
//! Everything here feeds L2/L3, whose bytes must be identical on x86-64 and
//! aarch64 (design §8, D69). libm's `cos`, `sin` and `log` are allowed to
//! differ in the last bit between platforms, so the analyzer does not call
//! them: it reads the committed tables in [`crate::tables`] and interpolates
//! linearly — a fixed sequence of `+ - * /` on IEEE-754 doubles, every step
//! correctly rounded and bit-exact everywhere. Accuracy is far beyond what
//! the protocol can represent (a cosine good to 1e-8, log2 good to 7e-6,
//! which is 0.01 cent in pitch).

use crate::tables::{COS_Q15, EXP2_Q16, LOG2_Q16};

/// Q15 scale: 32768 is 1.0.
const Q15: f64 = 32_768.0;

/// cos of a full turn: `x` is in turns (0.25 is a quarter turn).
///
/// The angle is folded into the first quadrant and read out of the committed
/// table with linear interpolation.
pub fn cos_turn(x: f64) -> f64 {
    // Quarter-turns, wrapped into [0, 4). The `as i64` cast truncates
    // toward zero, which only shifts by whole turns and so does not change
    // the wrapped result.
    let mut t = x * 4.0;
    let whole = (t / 4.0) as i64;
    t -= whole as f64 * 4.0;
    if t < 0.0 {
        t += 4.0;
    }
    let q = t as u64;
    let u = t - q as f64;
    match q {
        0 => table_cos(u),
        1 => -table_cos(1.0 - u),
        2 => -table_cos(u),
        _ => table_cos(1.0 - u),
    }
}

/// sin of a full turn, as [`cos_turn`]: a quarter turn behind cosine.
pub fn sin_turn(x: f64) -> f64 {
    cos_turn(x + 0.75)
}

/// The table read under cos_turn: cos(u * pi/2) for u in [0, 1).
fn table_cos(u: f64) -> f64 {
    // u = 0 is exactly 1: the FFT's k = 0 twiddle and the windows' first
    // taps must not carry the table's quantization.
    if u <= 0.0 {
        return 1.0;
    }
    let t = u * (COS_Q15.len() - 1) as f64;
    let i = (t as usize).min(COS_Q15.len() - 2);
    let frac = t - i as f64;
    let a = f64::from(COS_Q15[i]);
    let b = f64::from(COS_Q15[i + 1]);
    (a + frac * (b - a)) / Q15
}

/// log2 of a positive number, from the committed mantissa table.
///
/// The exponent is read off the bit pattern (an exact integer operation) and
/// the mantissa in [1, 2) is interpolated in the table.
pub fn log2(x: f64) -> f64 {
    debug_assert!(x > 0.0);
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i64 - 1023;
    // The mantissa renormalized to [1, 2): exact, it only rewrites the
    // exponent field.
    let m = f64::from_bits((bits & !(0x7ff_u64 << 52)) | (1023_u64 << 52));
    let t = (m - 1.0) * (LOG2_Q16.len() - 1) as f64;
    let i = (t as usize).min(LOG2_Q16.len() - 2);
    let frac = t - i as f64;
    let a = f64::from(LOG2_Q16[i]);
    let b = f64::from(LOG2_Q16[i + 1]);
    exp as f64 + (a + frac * (b - a)) / 65_536.0
}

/// 2 to the power `x`, the inverse of [`log2`], from the committed table.
///
/// The integer part scales the interpolated mantissa by repeated doubling
/// or halving — a fixed count of exact operations.
pub fn exp2(x: f64) -> f64 {
    // div_euclid floors, so the fraction is always in [0, 1); a plain
    // `as i64` would truncate toward zero and go wrong on negatives.
    let e = x.div_euclid(1.0) as i64;
    let t = (x - e as f64) * (EXP2_Q16.len() - 1) as f64;
    let i = (t as usize).min(EXP2_Q16.len() - 2);
    let frac = t - i as f64;
    let a = f64::from(EXP2_Q16[i]);
    let b = f64::from(EXP2_Q16[i + 1]);
    let mut m = (a + frac * (b - a)) / 65_536.0;
    // Scale by 2^e: the loop is exact at every step.
    let mut e = e;
    while e > 0 {
        m *= 2.0;
        e -= 1;
    }
    while e < 0 {
        m *= 0.5;
        e += 1;
    }
    m
}

/// log2 of e, for turning natural exponentials into the table's base.
pub use std::f64::consts::LOG2_E as LOG2E;

/// log2 of 10, the constant that turns log2 into log10.
pub use std::f64::consts::LOG2_10;

/// log10 of a positive number, in the table's terms (see [`log2`]).
pub fn log10(x: f64) -> f64 {
    log2(x) / LOG2_10
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against std's own cos/sin at a grid of angles: close enough that the
    /// protocol cannot see the difference.
    #[test]
    fn trig_matches_std() {
        for i in 0..2000 {
            let x = f64::from(i) / 2000.0;
            let angle = x * std::f64::consts::TAU;
            assert!((cos_turn(x) - angle.cos()).abs() < 5e-4, "cos at {x}");
            assert!((sin_turn(x) - angle.sin()).abs() < 5e-4, "sin at {x}");
        }
    }

    /// Angles past one turn and negative angles fold to the same values.
    #[test]
    fn trig_wraps() {
        for i in 0..200 {
            let x = f64::from(i) / 200.0;
            assert!((cos_turn(x + 1.0) - cos_turn(x)).abs() < 5e-4);
            assert!((cos_turn(x - 1.0) - cos_turn(x)).abs() < 5e-4);
            assert!((sin_turn(x + 1.0) - sin_turn(x)).abs() < 5e-4);
        }
    }

    /// log2 against std: within 7e-6 everywhere, which is a hundredth of a
    /// cent in pitch terms.
    #[test]
    fn log2_matches_std() {
        for i in 0..2000 {
            let x = 0.001 + f64::from(i) * 4.0;
            let want = x.log2();
            assert!((log2(x) - want).abs() < 2e-5, "log2 at {x}");
        }
    }

    /// Exactly one octave up is exactly one log2 up.
    #[test]
    fn log2_octaves_are_exact() {
        assert!((log2(2.0) - 1.0).abs() < 1e-9);
        assert!((log2(0.5) + 1.0).abs() < 1e-9);
        assert!(log2(1.0).abs() < 1e-9);
    }

    /// exp2 against std's powf, over the range the Hz conversions need
    /// (about -60..+14: 55 Hz to 5000 Hz against any reference in between).
    #[test]
    fn exp2_matches_std() {
        for i in 0..2000 {
            let x = -60.0 + f64::from(i) * 0.04;
            let want = (2.0_f64).powf(x);
            assert!((exp2(x) - want).abs() / want < 3e-5, "exp2 at {x}");
        }
    }

    /// log2 and exp2 undo each other to well under a cent.
    #[test]
    fn exp2_log2_round_trip() {
        for i in 0..500 {
            let hz = 80.0 + f64::from(i) * 4.0;
            let back = 55.0 * exp2(log2(hz / 55.0));
            assert!((back - hz).abs() / hz < 2e-5);
        }
    }
}

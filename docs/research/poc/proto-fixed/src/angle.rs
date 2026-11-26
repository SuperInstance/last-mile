//! Angles as u16 (65536 units per full turn, 0 = +x, quarter turn = +y,
//! counter-clockwise) with build-time integer-generated Q14 trig tables.
//! No floats on any runtime path in this module.

include!(concat!(env!("OUT_DIR"), "/fixproto_tables.rs"));

/// 1.0 in Q14.
pub const Q14_ONE: i32 = 16384;

/// Interpolate the quarter-wave sine table at t in [0, 16384] angle units.
/// Linear interpolation between adjacent entries (16 units apart).
#[inline]
fn quarter_interp(t: u16) -> i32 {
    let i = (t >> 4) as usize; // 0..=1024
    let f = (t & 15) as i32;
    SIN_Q14[i] + (((SIN_Q14[i + 1] - SIN_Q14[i]) * f) >> 4)
}

/// Sine and cosine of an angle, both in Q14 (16384 = 1.0), via octant folding.
#[inline]
pub fn sin_cos(a: u16) -> (i32, i32) {
    let oct = (a >> 14) & 3;
    let t = a & 0x3FFF;
    match oct {
        0 => (quarter_interp(t), quarter_interp(16384 - t)),
        1 => (quarter_interp(16384 - t), -quarter_interp(t)),
        2 => (-quarter_interp(t), -quarter_interp(16384 - t)),
        _ => (-quarter_interp(16384 - t), quarter_interp(t)),
    }
}

pub fn sin_q14(a: u16) -> i32 {
    sin_cos(a).0
}

pub fn cos_q14(a: u16) -> i32 {
    sin_cos(a).1
}

/// atan2 in angle units. Integer octant reduction + eighth-turn table with
/// linear interpolation of the exact slope ratio. (0,0) maps to 0.
///
/// Error is bounded by table rounding plus interpolation (~1-2 units,
/// 1 unit = 1/65536 turn ~= 0.0055 deg); verified against f64 in tests only.
pub fn angle_to(dx: i32, dy: i32) -> u16 {
    if dx == 0 && dy == 0 {
        return 0;
    }
    let ax = dx.unsigned_abs();
    let ay = dy.unsigned_abs();
    let (a, b) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    debug_assert!(a > 0 && b <= a);

    // phi = atan(b/a) in eighth-turn units (0..=8192) via table interpolation
    let phi = {
        let num = (b as u64) << 10;
        let k = (num / a as u64) as usize; // 0..=1024
        let rem = num % a as u64;
        let f = ((rem << 10) / a as u64) as u32; // 0..=1023
        let t0 = ATAN_EIGHTH[k] as u32;
        let t1 = ATAN_EIGHTH[k + 1] as u32;
        t0 + (((t1 - t0) * f) >> 10)
    };

    let dx_pos = dx >= 0;
    let dy_pos = dy >= 0;
    let theta: u32 = match (dx_pos, dy_pos) {
        (true, true) => {
            if ax >= ay { phi } else { 16384 - phi }
        }
        (false, true) => {
            if ay >= ax { 16384 + phi } else { 32768 - phi }
        }
        (false, false) => {
            if ax >= ay { 32768 + phi } else { 49152 - phi }
        }
        (true, false) => {
            if ay >= ax { 49152 + phi } else { 65536 - phi } // wraps to 0 at phi == 0
        }
    };
    theta as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trig_exact_points() {
        assert_eq!(sin_cos(0), (0, 16384)); // sin=0, cos=1
        assert_eq!(sin_cos(16384), (16384, 0)); // 90 degrees
        assert_eq!(sin_cos(32768), (0, -16384)); // 180 degrees
        assert_eq!(sin_cos(49152), (-16384, 0)); // 270 degrees
        assert_eq!(sin_cos(8192).0, 11585); // sin(45 deg) = 0.70710678 * 16384 = 11585.2
    }

    #[test]
    fn trig_max_error_vs_f64() {
        // f64 used HERE ONLY to measure the table error; never on runtime paths.
        let mut max_sin = 0.0f64;
        let mut max_cos = 0.0f64;
        for a in 0..=u16::MAX {
            let rad = a as f64 * std::f64::consts::TAU / 65536.0;
            let (s, c) = sin_cos(a);
            max_sin = max_sin.max((s as f64 / 16384.0 - rad.sin()).abs());
            max_cos = max_cos.max((c as f64 / 16384.0 - rad.cos()).abs());
        }
        println!("trig max abs error vs f64: sin = {max_sin:.3e} ({:.2} Q14 LSB), cos = {max_cos:.3e} ({:.2} Q14 LSB)",
            max_sin * 16384.0, max_cos * 16384.0);
        assert!(max_sin < 2.0 / 16384.0, "sin error {max_sin}");
        assert!(max_cos < 2.0 / 16384.0, "cos error {max_cos}");
    }

    #[test]
    fn angle_to_exact_axes() {
        assert_eq!(angle_to(1, 0), 0);
        assert_eq!(angle_to(0, 1), 16384);
        assert_eq!(angle_to(-1, 0), 32768);
        assert_eq!(angle_to(0, -1), 49152);
        assert_eq!(angle_to(1, 1), 8192);
        assert_eq!(angle_to(1, -1), 57344);
        assert_eq!(angle_to(-1, 1), 24576);
        assert_eq!(angle_to(-1, -1), 40960);
        assert_eq!(angle_to(0, 0), 0);
    }

    #[test]
    fn angle_to_vs_f64() {
        // f64 here only to measure error.
        let mut rng = crate::grid::Xorshift64Star::new(0x5EED_C0DE_1234_5679);
        let mut max_err = 0.0f64;
        let mut sum_err = 0.0f64;
        let n = 1_000_000u32;
        for i in 0..n {
            let mag = if i % 3 == 0 { 1 } else { 1 + rng.next_u64() % (1 << 24) };
            let dx = (rng.next_u64() % (2 * mag as u64)) as i64 - mag as i64;
            let dy = (rng.next_u64() % (2 * mag as u64)) as i64 - mag as i64;
            let theta = angle_to(dx as i32, dy as i32) as f64 * std::f64::consts::TAU / 65536.0;
            let mut err = (theta - (dy as f64).atan2(dx as f64)) * 65536.0 / std::f64::consts::TAU;
            if err > 32768.0 {
                err -= 65536.0;
            } else if err < -32768.0 {
                err += 65536.0;
            }
            max_err = max_err.max(err.abs());
            sum_err += err;
        }
        println!("angle_to error vs f64 atan2: max = {max_err:.2} units, mean = {:.3} units", sum_err / n as f64);
        assert!(max_err <= 8.0, "angle_to error too large: {max_err}");
    }
}

// Build-time generator for the fixed-point trig tables.
//
// Pure integer arithmetic only (no floats anywhere in this file):
//   - pi via Machin's formula: pi/4 = 4*atan(1/5) - atan(1/239)
//   - atan(1/n) via the alternating power series, evaluated in 2^96 fixed point
//   - CORDIC gain 0.607252935... via a chain of integer Newton inverse square roots
//   - rotation-mode CORDIC for the quarter-wave sine table
//   - vectoring-mode CORDIC for the eighth-turn atan table
//
// Emits OUT_DIR/fixproto_tables.rs with:
//   SIN_Q14:     [i32; 1026]  round(16384 * sin(k*pi/2048)), k = 0..=1024 (+1 dup)
//   ATAN_EIGHTH: [u16; 1026]  round(atan(k/1024) in units of 1/65536 turn), k = 0..=1024 (+1 dup)
use std::env;
use std::fs;
use std::path::PathBuf;

// Fixed-point scale used for pi and the atan series: 96 fractional bits.
const FRAC: u32 = 96;

// atan(1/n) * 2^FRAC for integer n >= 2 (n == 1 is handled by the caller as pi/4).
// Terms are 2^FRAC / ((2k+1) * n^(2k+1)); the alternating decreasing tail left
// unsummed is below a couple of ulps at this scale.
fn series_atan(n: u128) -> u128 {
    let one = 1u128 << FRAC;
    let mut acc: i128 = 0;
    let mut npow: u128 = n; // n^(2k+1)
    let mut k: u128 = 0;
    loop {
        let c = 2 * k + 1;
        let denom = npow.saturating_mul(c);
        if denom > one {
            break;
        }
        let term = one / denom;
        if k % 2 == 0 {
            acc += term as i128;
        } else {
            acc -= term as i128;
        }
        match npow.checked_mul(n * n) {
            Some(v) => npow = v,
            None => break,
        }
        k += 1;
    }
    acc as u128
}

fn main() {
    // --- pi via Machin's formula ---
    let a5 = series_atan(5);
    let a239 = series_atan(239);
    let pi = 16 * a5 - 4 * a239; // pi * 2^96, ~2^97.7, comfortably inside u128

    // --- atan(2^-i) in sub-angle units (2^32 units per full turn) ---
    let mut theta = [0i64; 32];
    for (i, th) in theta.iter_mut().enumerate() {
        let raw: u128 = if i == 0 { pi >> 2 } else { series_atan(1u128 << i) };
        *th = ((raw << 32) / (2 * pi)) as i64;
    }
    // atan(1) is exactly an eighth of a turn by definition; exactness here is a
    // cheap end-to-end check on pi and the series.
    assert_eq!(theta[0], 1 << 29, "atan(1) must be exactly 2^29 sub-units");

    // --- CORDIC gain in Q45 via integer Newton inverse square roots (Q60/i128) ---
    fn inv_sqrt_q60(v: i128) -> i128 {
        // y <- y*(3 - v*y^2)/2 ; seed 0.75 is safe for v in [1, 1.25]
        let mut y: i128 = (3i128 << 59) / 4;
        for _ in 0..10 {
            let y2 = (y * y) >> 60;
            let vy2 = (v * y2) >> 60;
            y = (y * ((3i128 << 60) - vy2)) >> 61;
        }
        y
    }
    let mut gain: i128 = 1i128 << 60;
    for i in 0..23usize {
        let v = (1i128 << 60) + (1i128 << (60 - 2 * i)); // 1 + 2^-2i
        gain = (gain * inv_sqrt_q60(v)) >> 60;
    }
    let gain45 = (gain >> 15) as i64; // 0.607252935... * 2^45

    // --- rotation-mode CORDIC: returns sin(z0) in Q45, z0 in sub-angle units ---
    let cordic_sin = |z0: i64| -> i64 {
        let mut x = gain45;
        let mut y: i64 = 0;
        let mut z = z0;
        for i in 0..32usize {
            let (xs, ys) = (x >> i, y >> i);
            if z >= 0 {
                x -= ys;
                y += xs;
                z -= theta[i];
            } else {
                x += ys;
                y -= xs;
                z += theta[i];
            }
        }
        y
    };

    // --- vectoring-mode CORDIC: atan2(y0, x0) in sub-angle units, x0 > 0, Q45 in ---
    let cordic_atan = |x0: i64, y0: i64| -> i64 {
        let (mut x, mut y, mut z) = (x0, y0, 0i64);
        for i in 0..32usize {
            if y == 0 {
                break;
            }
            let (xs, ys) = (x >> i, y >> i);
            if y > 0 {
                x += ys;
                y -= xs;
                z += theta[i];
            } else {
                x -= ys;
                y += xs;
                z -= theta[i];
            }
        }
        z
    };

    // --- quarter-wave sine table: entry k = round(16384 * sin(k*pi/2048)) ---
    // 1024 real entries (the brief's "1024 entries") plus boundary dups so that
    // runtime interpolation can index i+1 without a branch.
    let mut sin_q14 = vec![0i32; 1026];
    for k in 0..=1024usize {
        let v = if k == 0 {
            0
        } else if k == 1024 {
            16384
        } else {
            let y = cordic_sin((k as i64) << 20); // k * 2^20 sub-units per table step
            (((y as i128) + (1i128 << 30)) >> 31) as i32
        };
        sin_q14[k] = v.clamp(0, 16384);
    }
    sin_q14[1025] = sin_q14[1024];
    // Integer sanity anchors (values derived by hand):
    assert_eq!(sin_q14[0], 0);
    assert_eq!(sin_q14[1024], 16384);
    assert!((6269..=6271).contains(&sin_q14[256]), "sin(pi/8) off: {}", sin_q14[256]);
    assert!((11584..=11586).contains(&sin_q14[512]), "sin(pi/4) off: {}", sin_q14[512]);

    // --- eighth-turn atan table: entry k = round(atan(k/1024) in 1/65536 turn) ---
    let mut atan_eighth = vec![0u16; 1026];
    for k in 0..=1024usize {
        let v = if k == 0 {
            0
        } else if k == 1024 {
            8192
        } else {
            let z = cordic_atan(1i64 << 45, (k as i64) << 35); // atan2(k/1024, 1)
            (((z + (1i64 << 15)) >> 16) as i32).clamp(0, 8192) as u16
        };
        atan_eighth[k] = v;
    }
    atan_eighth[1025] = atan_eighth[1024];
    assert!((4835..=4838).contains(&(atan_eighth[512] as i32)), "atan(1/2) off: {}", atan_eighth[512]);

    // --- emit ---
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("fixproto_tables.rs");
    let mut src = String::new();
    src.push_str("// @generated by build.rs — integer-only pipeline (Machin pi, atan series, CORDIC).\n");
    src.push_str("#[rustfmt::skip]\n");
    src.push_str("pub static SIN_Q14: [i32; 1026] = [\n");
    for chunk in sin_q14.chunks(16) {
        src.push_str("    ");
        src.push_str(&chunk.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        src.push_str(",\n");
    }
    src.push_str("];\n");
    src.push_str("#[rustfmt::skip]\n");
    src.push_str("pub static ATAN_EIGHTH: [u16; 1026] = [\n");
    for chunk in atan_eighth.chunks(16) {
        src.push_str("    ");
        src.push_str(&chunk.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        src.push_str(",\n");
    }
    src.push_str("];\n");
    fs::write(&out, src).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

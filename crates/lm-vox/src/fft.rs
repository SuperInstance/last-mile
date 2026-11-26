//! A radix-2 Cooley-Tukey FFT, written from the 1965 paper, plus the
//! windows the analyzer puts in front of it.
//!
//! No FFT crate is pulled in: the transform the analyzer needs is a
//! power-of-two complex FFT of at most 2048 points, which is under a hundred
//! lines, and owning it keeps the crate dependency-free (design §10) and the
//! arithmetic a fixed sequence of `+ - *` — bit-exact across machines. The
//! twiddles and the Hann/Hamming windows come from the committed cosine
//! table (see [`crate::math`]), never from libm.

use crate::math::cos_turn;
use crate::math::sin_turn;

/// The power spectrum of a windowed frame: `power[k]` is the squared
/// magnitude of bin `k`, `k = 0..n/2` (the rest of a real signal's spectrum
/// mirrors this half).
///
/// `frame` is the windowed samples; `n` must be a power of two at least as
/// long as the frame (the rest is zero-padded).
pub fn power_spectrum(frame: &[f64], n: usize) -> Vec<f64> {
    debug_assert!(n.is_power_of_two());
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    re[..frame.len().min(n)].copy_from_slice(&frame[..frame.len().min(n)]);
    fft_in_place(&mut re, &mut im, false);
    (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

/// An in-place complex FFT (or inverse, unscaled, with `invert`).
///
/// Iterative, decimation in time, with bit-reversal first — the textbook
/// Cooley-Tukey factoring: a size-n transform is log2(n) passes of n/2
/// butterflies. Sizes are powers of two only; the analyzer never asks for
/// anything else.
pub fn fft_in_place(re: &mut [f64], im: &mut [f64], invert: bool) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    debug_assert_eq!(im.len(), n);
    if n <= 1 {
        return;
    }

    // Bit-reversed reorder: j's bit pattern reversed is where j's value
    // starts out.
    let mut j = 0usize;
    for i in 0..n - 1 {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        // Add the highest bit that fits, carrying down through the lower
        // ones: this counts j up in bit-reversed order.
        let mut m = n >> 1;
        while m >= 2 && j & m != 0 {
            j &= !m;
            m >>= 1;
        }
        j |= m;
    }

    // Butterfly passes, doubling the sub-transform size each time. Twiddles
    // are read from the committed cosine table, so every machine runs the
    // same operations on the same bits.
    let sign = if invert { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let half = len >> 1;
        let stride = 1.0 / len as f64;
        for base in (0..n).step_by(len) {
            for k in 0..half {
                let wr = cos_turn(stride * k as f64);
                let wi = sign * sin_turn(stride * k as f64);
                let i = base + k;
                let i2 = i + half;
                let tr = wr * re[i2] - wi * im[i2];
                let ti = wr * im[i2] + wi * re[i2];
                re[i2] = re[i] - tr;
                im[i2] = im[i] - ti;
                re[i] += tr;
                im[i] += ti;
            }
        }
        len <<= 1;
    }
}

/// The Hann window of length `n`: 0.5 - 0.5 cos(2 pi i / (n-1)), the
/// standard choice for LPC (Markel & Gray, "Linear Prediction of Speech").
pub fn hann(n: usize) -> Vec<f64> {
    if n == 1 {
        return vec![1.0];
    }
    let denom = (n - 1) as f64;
    (0..n)
        .map(|i| 0.5 - 0.5 * cos_turn(i as f64 / denom))
        .collect()
}

/// The Hamming window of length `n`: 0.54 - 0.46 cos(2 pi i / (n-1)), the
/// standard choice for pitch-period analysis windows.
pub fn hamming(n: usize) -> Vec<f64> {
    if n == 1 {
        return vec![1.0];
    }
    let denom = (n - 1) as f64;
    (0..n)
        .map(|i| 0.54 - 0.46 * cos_turn(i as f64 / denom))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DC input puts everything in bin 0.
    #[test]
    fn fft_dc() {
        let n = 256;
        let mut re = vec![1.0; n];
        let mut im = vec![0.0; n];
        fft_in_place(&mut re, &mut im, false);
        // The twiddles come from a Q15 table, so the tolerances carry the
        // table's quantization (about 3e-5 per twiddle), not just
        // rounding: a leak of n * 1e-4 is the table's floor.
        assert!(re[1..].iter().all(|&v| v.abs() < n as f64 * 1e-2));
        assert!((re[0] - n as f64).abs() < 0.5);
    }

    /// A window of a bin-centered sine lands in that bin, and the inverse
    /// transform undoes the forward one.
    #[test]
    fn fft_sine_and_inverse() {
        let n = 512;
        let bin = 37;
        let mut re: Vec<f64> = (0..n)
            .map(|i| sin_turn((i * bin) as f64 / n as f64))
            .collect();
        let want = re.clone();
        let mut im = vec![0.0; n];
        fft_in_place(&mut re, &mut im, false);
        let mag: Vec<f64> = (0..n)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
            .collect();
        let peak = mag
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(peak == bin || peak == n - bin, "peak at {peak}");
        // Every other bin is empty.
        for (k, &m) in mag.iter().enumerate() {
            if k != bin && k != n - bin {
                assert!(m < n as f64 * 2e-2, "bin {k} leaked {m}");
            }
        }

        fft_in_place(&mut re, &mut im, true);
        for i in 0..n {
            assert!((re[i] / (n as f64) - want[i]).abs() < 2e-2);
            assert!(im[i].abs() / (n as f64) < 2e-2);
        }
    }

    /// The power spectrum of a pure bin-centered tone is two spikes.
    #[test]
    fn power_spectrum_tone() {
        let n = 256;
        let bin = 20;
        let frame: Vec<f64> = (0..n)
            .map(|i| sin_turn((i * bin) as f64 / n as f64))
            .collect();
        let power = power_spectrum(&frame, n);
        assert_eq!(power.len(), n / 2 + 1);
        assert!(power[bin] > 0.98 * n as f64 * n as f64 / 4.0);
        for (k, &p) in power.iter().enumerate() {
            if k != bin {
                assert!(p < (n as f64 * 2e-2).powi(2), "bin {k} leaked {p}");
            }
        }
    }

    /// The windows are symmetric and peak at 1 (Hann touches 0 at the ends).
    #[test]
    fn windows_shape() {
        let w = hann(64);
        assert!(w[0].abs() < 1e-4);
        // A 64-tap window's peak sits between taps 31 and 32.
        assert!((w[31] - 1.0).abs() < 2e-3);
        assert!((w[32] - 1.0).abs() < 2e-3);
        for i in 0..32 {
            assert!((w[i] - w[63 - i]).abs() < 1e-4);
        }
        let h = hamming(64);
        assert!((h[31] - 1.0).abs() < 2e-3);
        assert!(h[0] > 0.07 && h[0] < 0.09);
    }
}

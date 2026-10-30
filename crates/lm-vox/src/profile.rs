//! The speaker profile (design §7): the keyboard's velocity curve.
//!
//! L0 and L1 are *relative* to the player the profile describes, so "loud"
//! means loud for this speaker, not loud in absolute terms. The profile is
//! a deterministic running estimate — integer histograms and fixed-point
//! means, no floating point at all — so two machines that saw the same
//! frames hold byte-identical profiles, and the learning flow can rerun on
//! old sidecars and land on the same answer.
//!
//! The file is human-readable by design (`Display` prints every dial), so
//! the player can open it and see how the squad has learned to hear them,
//! and correct it (design §7).

use crate::frame::{Frame, Gesture, Phrase, F0_REF_HZ};
use std::fmt;

/// How many histogram bins the pitch estimate keeps. Each bin is 64 cents —
/// a quarter tone — which is finer than any decision the protocol makes.
const F0_BINS: usize = 64;
/// The bin width of the pitch histogram, in cents.
const F0_BIN_CENTS: i32 = 64;
/// How many histogram bins the loudness estimate keeps: 8 units of the L0
/// log scale, about 3 dB per bin.
const LOUD_BINS: usize = 32;
/// The bin width of the loudness histogram, in L0 units.
const LOUD_BIN: i32 = 8;
/// Fixed-point scale of the running means (attack, glide, tempo): 256 is
/// 1.0.
const Q8: i64 = 256;

/// The player's measured touch (design §7).
///
/// Defaults describe an unmeasured speaker: the median pitch is the
/// population median, the ranges are wide, and every L0 relative reading is
/// taken against those defaults until learning narrows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeakerProfile {
    /// The reference pitch the layer-0 cents field is measured against, in
    /// Hz. The default is 55 (A1); a fitted profile may move it so the
    /// speaker's whole range sits near the i16's center.
    pub f0_ref_hz: u32,
    /// The speaker's median voiced pitch, in cents above `f0_ref_hz`
    /// (i16 on the wire). Population default 2200 ≈ 208 Hz.
    pub median_f0_cents: i16,
    /// The speaker's pitch span, p90 minus p10, in cents. Population
    /// default 700 (about a tritone and a half).
    pub f0_span_cents: i16,
    /// The speaker's quiet p10 level, on the L0 log scale (0-255).
    pub loud_floor: u8,
    /// The speaker's loud p90 level, on the same scale.
    pub loud_ceiling: u8,
    /// The speaker's typical gesture attack, in MS. Population default 40.
    pub typical_attack_ms: u16,
    /// The speaker's typical glide rate, in cents per second, signed.
    pub typical_glide_cents_per_s: i16,
    /// The speaker's median tempo, in tenths of a syllable per second.
    pub tempo_tenths: u16,
    /// How many frames have trained this estimate; 0 means stock defaults.
    pub frames_seen: u64,
    /// How many phrases have trained the tempo mean.
    phrases_seen: u64,
    /// The pitch histogram behind `median_f0_cents` (counts, not rates).
    f0_hist: [u32; F0_BINS],
    /// The loudness histogram behind `loud_floor`/`loud_ceiling`.
    loud_hist: [u32; LOUD_BINS],
    /// The running mean of gesture attacks, in MS * Q8.
    attack_q8: u64,
    /// The running mean of glide rates, in cents/s * Q8.
    glide_q8: i64,
    /// The running mean of tempo, in tenths/s * Q8.
    tempo_q8: u64,
}

impl Default for SpeakerProfile {
    fn default() -> SpeakerProfile {
        SpeakerProfile {
            f0_ref_hz: F0_REF_HZ as u32,
            median_f0_cents: 2200,
            f0_span_cents: 700,
            loud_floor: 84,
            loud_ceiling: 200,
            typical_attack_ms: 40,
            typical_glide_cents_per_s: -120,
            tempo_tenths: 50,
            frames_seen: 0,
            phrases_seen: 0,
            f0_hist: [0; F0_BINS],
            loud_hist: [0; LOUD_BINS],
            attack_q8: 0,
            glide_q8: 0,
            tempo_q8: 0,
        }
    }
}

impl SpeakerProfile {
    /// Fold one utterance's evidence into the profile: every frame trains
    /// the pitch and loudness histograms, every gesture the articulation
    /// means, the phrase the tempo.
    pub fn update(&mut self, frames: &[Frame], gestures: &[Gesture], phrase: &Phrase) {
        for f in frames {
            // Rests carry no pitch or level evidence.
            if f.flags.has(crate::frame::FrameFlags::REST) {
                continue;
            }
            if f.f0_cents > 0 {
                let bin = ((f.f0_cents as i32) / F0_BIN_CENTS) as usize;
                if bin < F0_BINS {
                    self.f0_hist[bin] += 1;
                }
            }
            if f.rms > 0 {
                let bin = (i32::from(f.rms) / LOUD_BIN) as usize;
                if bin < LOUD_BINS {
                    self.loud_hist[bin] += 1;
                }
            }
            self.frames_seen += 1;
        }

        // The articulation means: each gesture contributes equally, whether
        // it is long or short.
        let mut n_gest = 0u64;
        for g in gestures {
            if g.velocity_db == i8::MIN {
                continue;
            }
            let n = n_gest + 1;
            self.attack_q8 = upgrade_mean(self.attack_q8, n_gest, g.attack_ms as i64 * Q8, n);
            let glide = g.glide_cents as i64 * 10_000 / (g.duration_ms.max(1) as i64);
            self.glide_q8 = upgrade_mean_i(self.glide_q8, n_gest, glide * Q8 / 10_000, n);
            n_gest = n;
        }
        if phrase.tempo_tenths > 0 {
            self.phrases_seen += 1;
            self.tempo_q8 = upgrade_mean(
                self.tempo_q8,
                self.phrases_seen - 1,
                i64::from(phrase.tempo_tenths) * Q8,
                self.phrases_seen,
            );
        }

        self.recompute();
    }

    /// Read the dials back off the histograms. Deterministic: the same
    /// counts always give the same numbers. A histogram with no evidence
    /// leaves its dial at the stock default.
    fn recompute(&mut self) {
        if self.f0_hist.iter().sum::<u32>() > 0 {
            let (p10, p90) = hist_span(&self.f0_hist, F0_BIN_CENTS);
            self.f0_span_cents = (p90 - p10) as i16;
            self.median_f0_cents = hist_median(&self.f0_hist, F0_BIN_CENTS) as i16;
        }
        let (floor, ceiling) = hist_span(&self.loud_hist, LOUD_BIN);
        // Only narrow from the stock defaults once there is real evidence;
        // a handful of frames says nothing about a speaker.
        if self.loud_hist.iter().sum::<u32>() >= 100 {
            self.loud_floor = (floor * LOUD_BIN) as u8;
            self.loud_ceiling = ((ceiling + 1) * LOUD_BIN - 1) as u8;
        }
        if self.attack_q8 > 0 {
            self.typical_attack_ms = (self.attack_q8 / Q8 as u64) as u16;
        }
        if self.glide_q8 != 0 {
            self.typical_glide_cents_per_s = (self.glide_q8 / Q8) as i16;
        }
        if self.tempo_q8 > 0 {
            self.tempo_tenths = (self.tempo_q8 / Q8 as u64) as u16;
        }
    }

    /// The profile's reference pitch, as the analyzer wants it.
    pub fn f0_ref(&self) -> f64 {
        f64::from(self.f0_ref_hz)
    }

    /// The speaker's median level on the L0 log scale, the line "loud for
    /// this player" is drawn against. Before any evidence arrives it sits
    /// midway between the stock floor and ceiling.
    pub fn median_loud(&self) -> u8 {
        let total: u32 = self.loud_hist.iter().sum();
        if total == 0 {
            ((u16::from(self.loud_floor) + u16::from(self.loud_ceiling)) / 2) as u8
        } else {
            (hist_median(&self.loud_hist, LOUD_BIN).clamp(0, 255 / LOUD_BIN) * LOUD_BIN) as u8
        }
    }
}

/// The weighted mean of a running mean and one new sample, all integer.
fn upgrade_mean(mean_q8: u64, n: u64, sample_q8: i64, new_n: u64) -> u64 {
    if new_n == 0 {
        return mean_q8;
    }
    let total = mean_q8 as i64 * n as i64 + sample_q8;
    (total / new_n as i64).max(0) as u64
}

/// The signed twin of [`upgrade_mean`].
fn upgrade_mean_i(mean_q8: i64, n: u64, sample_q8: i64, new_n: u64) -> i64 {
    if new_n == 0 {
        return mean_q8;
    }
    (mean_q8 * n as i64 + sample_q8) / new_n as i64
}

/// The median of a histogram, in the quantity's own units (the bin's lower
/// edge plus half a bin).
fn hist_median(hist: &[u32], bin_width: i32) -> i32 {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return 0;
    }
    let half = total / 2;
    let mut seen = 0;
    for (i, &c) in hist.iter().enumerate() {
        seen += c;
        if seen > half {
            return i as i32 * bin_width + bin_width / 2;
        }
    }
    0
}

/// The p10 and p90 of a histogram, in the quantity's own units.
fn hist_span(hist: &[u32], bin_width: i32) -> (i32, i32) {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return (0, 0);
    }
    let p = |q: u32| -> i32 {
        let mut want = total * q / 100;
        if want == 0 {
            want = 1;
        }
        let mut seen = 0;
        for (i, &c) in hist.iter().enumerate() {
            seen += c;
            if seen >= want {
                return i as i32 * bin_width;
            }
        }
        (hist.len() as i32 - 1) * bin_width
    };
    (p(10), p(90))
}

impl fmt::Display for SpeakerProfile {
    /// The human-readable form the player can open and correct (§7).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "profile: median {}Hz span {}c loud {}..{} atk {}ms glide {}/s tempo {}/s over {} frames",
            Frame::cents_to_hz(f64::from(self.median_f0_cents), self.f0_ref()) as i32,
            self.f0_span_cents,
            self.loud_floor,
            self.loud_ceiling,
            self.typical_attack_ms,
            self.typical_glide_cents_per_s,
            self.tempo_tenths / 10,
            self.frames_seen
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{FrameFlags, Phrase};

    /// Voiced frames at a steady pitch put the median where they were.
    #[test]
    fn median_tracks_pitch() {
        let mut p = SpeakerProfile::default();
        let frames: Vec<Frame> = (0..400)
            .map(|_| Frame {
                f0_cents: 1500,
                rms: 150,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        p.update(&frames, &[], &Phrase::default());
        assert!(
            (p.median_f0_cents - 1500).abs() <= 32,
            "{}",
            p.median_f0_cents
        );
        assert_eq!(p.frames_seen, 400);
    }

    /// The same evidence always produces the same profile: two profiles fed
    /// the same utterances agree dial for dial.
    #[test]
    fn update_is_deterministic() {
        let frames: Vec<Frame> = (0..300)
            .map(|i| Frame {
                f0_cents: if i % 5 == 0 { 1800 } else { 1400 },
                rms: 120 + (i % 60) as u8,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        let gestures = vec![Gesture {
            onset_ms: 0,
            duration_ms: 300,
            pitch_target_cents: 1400,
            glide_cents: -300,
            velocity_db: 3,
            attack_ms: 35,
            release_ms: 90,
            noise_burst: false,
        }];
        let phrase = Phrase {
            tempo_tenths: 61,
            ..Phrase::default()
        };
        let mut a = SpeakerProfile::default();
        let mut b = SpeakerProfile::default();
        a.update(&frames, &gestures, &phrase);
        b.update(&frames, &gestures, &phrase);
        assert_eq!(a, b);
        assert_eq!(a.typical_attack_ms, 35);
    }

    /// A rest trains nothing.
    #[test]
    fn rests_are_ignored() {
        let mut p = SpeakerProfile::default();
        let before = p.clone();
        p.update(&[Frame::default(); 50], &[], &Phrase::default());
        assert_eq!(p.frames_seen, 0);
        assert_eq!(p.median_f0_cents, before.median_f0_cents);
    }

    /// The display line names every dial.
    #[test]
    fn display_names_the_dials() {
        let p = SpeakerProfile::default();
        let line = p.to_string();
        assert!(line.starts_with("profile:"));
        assert!(line.contains("atk"));
        assert!(line.contains("tempo"));
    }
}

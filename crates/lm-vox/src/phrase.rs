//! L2: the phrase summary (design §4) — a whole utterance as one synth
//! patch, and the one text line a decision model reads.
//!
//! The summary is derived from L0 and L1 by fixed integer rules (the same
//! frames always summarize to the same patch), and the line is exactly the
//! form §4 shows:
//!
//! ```text
//! vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB
//! ```
//!
//! About 30 tokens, no floating point in sight — the line is what a gate
//! prompt carries.

use crate::frame::{Adsr, Contour, Frame, FrameFlags, Gesture, Lfo, Phrase};
use crate::profile::SpeakerProfile;

/// Summarize one utterance's frames and gestures into its patch.
pub fn phrase_summary(frames: &[Frame], gestures: &[Gesture], profile: &SpeakerProfile) -> Phrase {
    let n = frames.len();
    let live: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.flags.has(FrameFlags::REST))
        .map(|(i, _)| i)
        .collect();
    let duration_ms = if live.is_empty() {
        n as u32 * 10
    } else {
        ((live[live.len() - 1] - live[0] + 1) * 10) as u32
    };

    // The loudness envelope, off the level channel: one unit of the rms
    // scale is 96/255 dB. The attack is the gestures' median, not the
    // distance to the phrase's global peak — a patch's attack says how a
    // note starts, and a three-island utterance would otherwise report a
    // whole second of "attack" because its loudest island came last.
    let mut loud = adsr(frames, |f| i32::from(f.rms), 12, 96);
    let mut attacks: Vec<u32> = gestures
        .iter()
        .filter(|g| g.velocity_db != i8::MIN)
        .map(|g| u32::from(g.attack_ms))
        .collect();
    if !attacks.is_empty() {
        attacks.sort_unstable();
        loud.attack_ms = (attacks[attacks.len() / 2]).min(1000) as u16;
    }
    // The brightness envelope, the same shape on the tilt channel: one unit
    // is 1 dB per octave, and the tilt settles faster.
    let bright = adsr(frames, |f| i32::from(f.tilt), 4, 255);

    // The patch's brightness word: the median tilt over the sounding
    // frames, in dB per octave.
    let bright_tilt_db_oct = if live.is_empty() {
        0
    } else {
        let mut tilts: Vec<i32> = live.iter().map(|&i| i32::from(frames[i].tilt)).collect();
        tilts.sort_unstable();
        (tilts[tilts.len() / 2]).clamp(-127, 127) as i8
    };

    let contour = contour_of(frames);
    let vib = lfo_of(
        &frames
            .iter()
            .map(|f| i32::from(f.f0_cents))
            .collect::<Vec<_>>(),
        1,
        10,
    );
    let trem = lfo_of(
        &frames
            .iter()
            .map(|f| i32::from(f.rms) * 96 / 255)
            .collect::<Vec<_>>(),
        100,
        50,
    );

    // Tempo: sounding gestures over the span they fill, in tenths of a
    // syllable per second.
    let sounding = gestures.iter().filter(|g| g.velocity_db != i8::MIN).count();
    let tempo_tenths = if duration_ms == 0 || sounding == 0 {
        0
    } else {
        ((sounding * 10_000 + duration_ms as usize / 2) / duration_ms as usize) as u16
    };

    // Pauses: silent stretches of at least 200 ms.
    let mut pauses = 0u16;
    let mut pause_frames = 0usize;
    let mut run = 0usize;
    for f in frames {
        if f.flags.has(FrameFlags::REST) {
            run += 1;
        } else {
            if run >= 20 {
                pauses += 1;
            }
            pause_frames += run;
            run = 0;
        }
    }
    if run >= 20 {
        pauses += 1;
        pause_frames += run;
    }
    let pause_frac_255 = (pause_frames * 255).checked_div(n).unwrap_or(0) as u8;

    // Velocity: the utterance's peak against the speaker's median.
    let peak = frames.iter().map(|f| f.rms).max().unwrap_or(0);
    let velocity_db =
        (((i32::from(peak) - i32::from(profile.median_loud())) * 96 + 127) / 255) as i8;

    Phrase {
        duration_ms,
        loud,
        bright,
        bright_tilt_db_oct,
        contour,
        vib,
        trem,
        tempo_tenths,
        pauses,
        pause_frac_255,
        velocity_db,
    }
}

/// An ADSR shape off one integer channel.
///
/// `settle` is how far the channel must fall from its peak (in the
/// channel's own units) before it counts as decayed; `db_per_unit_255`
/// converts a channel difference into the envelope's unit (255 is one dB
/// per unit for tilt, 96 makes the rms scale's 255 units span 96 dB).
fn adsr(
    frames: &[Frame],
    value: impl Fn(&Frame) -> i32,
    settle: i32,
    db_per_unit_255: i32,
) -> Adsr {
    let live: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.flags.has(FrameFlags::REST))
        .map(|(i, _)| i)
        .collect();
    if live.len() < 4 {
        return Adsr::default();
    }
    let val = |i: usize| value(&frames[i]);

    // The channel's peak over the utterance.
    let (peak_pos, peak_val) = live
        .iter()
        .copied()
        .map(|i| (i, val(i)))
        .max_by_key(|&(i, v)| (v, std::cmp::Reverse(i)))
        .unwrap();
    let first = live[0];
    let last_live = live[live.len() - 1];
    // The release runs to the utterance's very end: trailing silence is
    // the sound stopping.
    let last_any = frames.len() - 1;

    // Attack: first sound to the peak.
    let attack_ms = ((peak_pos - first) * 10) as u16;

    // Decay: the peak to where the channel has fallen `settle` units, or
    // to the end.
    let decay_end = live
        .iter()
        .copied()
        .skip_while(|&i| i <= peak_pos)
        .find(|&i| val(i) <= peak_val - settle)
        .unwrap_or(last_live);
    let decay_ms = (decay_end.saturating_sub(peak_pos) * 10) as u16;

    // Sustain: the median of the tail after the decay, against the peak.
    let mut tail: Vec<i32> = (decay_end.max(peak_pos)..=last_live).map(val).collect();
    tail.sort_unstable();
    let median = tail[tail.len() / 2];
    let sustain_db = ((median - peak_val) * db_per_unit_255 / 255).clamp(-60, 0) as i8;

    // Release: from the last frame within twice the settle of the peak, to
    // the utterance's end.
    let floor = peak_val - settle * 2;
    let last_loud = (first..=last_any)
        .rev()
        .find(|&i| val(i) >= floor)
        .unwrap_or(peak_pos);
    let release_ms = ((last_any - last_loud.min(last_any)) * 10) as u16;

    Adsr {
        attack_ms: attack_ms.min(1000),
        decay_ms: decay_ms.min(1000),
        sustain_db,
        release_ms: release_ms.min(1000),
    }
}

/// The pitch contour class: how the voiced cents travel across the
/// utterance.
fn contour_of(frames: &[Frame]) -> Contour {
    let voiced: Vec<i32> = frames
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED))
        .map(|f| i32::from(f.f0_cents))
        .collect();
    if voiced.len() < 6 {
        return Contour::Flat;
    }
    let head_n = voiced.len().min(8);
    let tail_n = voiced.len().min(8);
    let head = voiced[..head_n].iter().sum::<i32>() / head_n as i32;
    let tail = voiced[voiced.len() - tail_n..].iter().sum::<i32>() / tail_n as i32;
    let peak = *voiced.iter().max().unwrap();

    let travel = tail - head;
    if travel > 200 {
        Contour::Rise
    } else if travel < -200 {
        Contour::Fall
    } else if peak - head > 200 && peak - tail > 200 {
        Contour::RiseFall
    } else {
        Contour::Flat
    }
}

/// A low-frequency oscillation on an integer channel: the residual after a
/// 300 ms moving-average detrend, its depth and its rate.
///
/// `unit_scale` converts one channel unit into the depth unit (1 for
/// pitch's cents, 100 for level's hundredths of a dB); `low_at` is the
/// depth that counts as present. Frames of value 0 (unvoiced) hold their
/// last value so the channel does not fake a huge oscillation at every gap.
fn lfo_of(channel: &[i32], unit_scale: i32, low_at: i32) -> Lfo {
    if channel.len() < 30 {
        return Lfo::default();
    }

    // Each contiguous voiced run is detrended on its own: a pause's held
    // value would tilt the moving average at the island edges and fake a
    // swing no voice made.
    let mut residual: Vec<i32> = Vec::new();
    let mut run: Vec<i32> = Vec::new();
    let flush = |run: &mut Vec<i32>, out: &mut Vec<i32>| {
        if run.len() >= 30 {
            let win = 30.min(run.len() / 3);
            for i in 0..run.len() {
                let lo = i.saturating_sub(win / 2);
                let hi = (i + win / 2).min(run.len());
                let mean = run[lo..hi].iter().sum::<i32>() / (hi - lo) as i32;
                out.push((run[i] - mean) * unit_scale);
            }
        }
        run.clear();
    };
    for &v in channel {
        if v != 0 {
            run.push(v);
        } else {
            flush(&mut run, &mut residual);
        }
    }
    flush(&mut run, &mut residual);
    if residual.is_empty() {
        return Lfo::default();
    }

    // Depth: the root of the mean square of the residual, the LFO's size.
    let sq: i64 = residual.iter().map(|&r| i64::from(r) * i64::from(r)).sum();
    let depth = (sq as f64 / residual.len() as f64).sqrt() as i32;

    // Rate: sign changes of the residual over the utterance, halved (each
    // cycle crosses twice).
    let mut crossings = 0i32;
    for i in 1..residual.len() {
        if (residual[i - 1] < 0) != (residual[i] < 0) {
            crossings += 1;
        }
    }
    let seconds = residual.len() as f64 / 100.0;
    let rate_hz = if seconds > 0.0 {
        crossings as f64 / 2.0 / seconds
    } else {
        0.0
    };

    if depth < low_at {
        Lfo::default()
    } else {
        Lfo {
            depth: depth.max(0) as u16,
            rate_centi_hz: ((rate_hz * 100.0) + 0.5) as u16,
        }
    }
}

/// The L2 text line, exactly as §4 shows it.
///
/// ```text
/// vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB
/// ```
pub fn l2_line(p: &Phrase) -> String {
    let dec = if p.loud.decay_ms < 80 {
        "fast"
    } else if p.loud.decay_ms < 160 {
        "med"
    } else {
        "slow"
    };
    let sus = if p.loud.sustain_db < -12 {
        "low"
    } else if p.loud.sustain_db < -5 {
        "mid"
    } else {
        "high"
    };
    let rel = if p.loud.release_ms < 60 {
        "hard"
    } else {
        "soft"
    };
    let contour = match p.contour {
        Contour::Rise => "rise",
        Contour::Fall => "fall",
        Contour::Flat => "flat",
        Contour::RiseFall => "rise-fall",
    };
    let vib_class = lfo_class(p.vib.depth, 10, 50);
    let trem_class = lfo_class(p.trem.depth, 50, 300);
    let bright_class = if p.bright_tilt_db_oct < -4 {
        "low"
    } else if p.bright_tilt_db_oct < 4 {
        "mid"
    } else {
        "high"
    };
    format!(
        "vox: atk {}ms dec {} sus {} rel {} | contour {} | vib {} trem {} | bright {} | tempo {}.{}/s | vel {:+}dB",
        p.loud.attack_ms,
        dec,
        sus,
        rel,
        contour,
        vib_class,
        trem_class,
        bright_class,
        p.tempo_tenths / 10,
        p.tempo_tenths % 10,
        p.velocity_db
    )
}

/// The none/low/deep ladder an LFO's depth is read through.
fn lfo_class(depth: u16, low_at: u16, deep_at: u16) -> &'static str {
    if depth < low_at {
        "none"
    } else if depth < deep_at {
        "low"
    } else {
        "deep"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase_saying_the_spec_example() -> Phrase {
        Phrase {
            duration_ms: 1500,
            loud: Adsr {
                attack_ms: 35,
                decay_ms: 60,
                sustain_db: -14,
                release_ms: 40,
            },
            bright: Adsr {
                attack_ms: 30,
                decay_ms: 50,
                sustain_db: -2,
                release_ms: 40,
            },
            bright_tilt_db_oct: 6,
            contour: Contour::Fall,
            vib: Lfo::default(),
            trem: Lfo {
                depth: 100,
                rate_centi_hz: 500,
            },
            tempo_tenths: 61,
            pauses: 0,
            pause_frac_255: 0,
            velocity_db: 9,
        }
    }

    /// The example line of §4, rebuilt from a phrase that says the same
    /// thing, keeps the exact shape.
    #[test]
    fn the_line_matches_the_spec_shape() {
        assert_eq!(
            l2_line(&phrase_saying_the_spec_example()),
            "vox: atk 35ms dec fast sus low rel hard | contour fall | vib none trem low | bright high | tempo 6.1/s | vel +9dB"
        );
    }

    /// The line's classes move with the dials: every word has a working
    /// range.
    #[test]
    fn the_line_classes_move() {
        let mut p = phrase_saying_the_spec_example();
        p.contour = Contour::RiseFall;
        p.loud.attack_ms = 200;
        p.loud.decay_ms = 300;
        p.loud.sustain_db = -3;
        p.loud.release_ms = 300;
        p.bright_tilt_db_oct = -10;
        p.vib.depth = 80;
        p.trem.depth = 400;
        p.velocity_db = -12;
        p.tempo_tenths = 95;
        assert_eq!(
            l2_line(&p),
            "vox: atk 200ms dec slow sus high rel soft | contour rise-fall | vib deep trem deep | bright low | tempo 9.5/s | vel -12dB"
        );
    }

    /// A steady line summarizes as flat, no vibrato, one gesture's tempo.
    #[test]
    fn steady_line() {
        let frames: Vec<Frame> = (0..100)
            .map(|_| Frame {
                f0_cents: 1500,
                rms: 140,
                tilt: 2,
                noise: 10,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        let gestures = vec![Gesture {
            onset_ms: 0,
            duration_ms: 1000,
            pitch_target_cents: 1500,
            glide_cents: 0,
            velocity_db: 0,
            attack_ms: 40,
            release_ms: 100,
            noise_burst: false,
        }];
        let p = phrase_summary(&frames, &gestures, &SpeakerProfile::default());
        assert_eq!(p.contour, Contour::Flat);
        assert_eq!(p.vib.depth, 0);
        assert_eq!(p.tempo_tenths, 10);
        let line = l2_line(&p);
        assert!(line.contains("tempo 1.0/s"), "{line}");
        assert!(line.contains("contour flat"), "{line}");
    }

    /// A falling line reads as a fall.
    #[test]
    fn falling_line() {
        let frames: Vec<Frame> = (0..100)
            .map(|i| Frame {
                f0_cents: 1800 - i as i16 * 6,
                rms: 140,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        let p = phrase_summary(&frames, &[], &SpeakerProfile::default());
        assert_eq!(p.contour, Contour::Fall);
    }

    /// A sharp attack and a trailing silence read as such.
    #[test]
    fn attack_and_release_read() {
        let mut frames: Vec<Frame> = vec![Frame::default(); 10];
        frames.extend((0..40).map(|i| Frame {
            f0_cents: 1500,
            rms: if i < 3 { 120 + i as u8 * 10 } else { 150 },
            flags: FrameFlags::VOICED,
            ..Frame::default()
        }));
        frames.extend(vec![Frame::default(); 5]);
        let p = phrase_summary(&frames, &[], &SpeakerProfile::default());
        assert!(p.loud.attack_ms <= 50, "{}", p.loud.attack_ms);
        assert_eq!(p.loud.release_ms, 50);
        assert_eq!(p.pauses, 0, "a short trailing silence is no pause");
    }

    /// Pauses longer than 200 ms count, and their share is kept.
    #[test]
    fn pauses_count() {
        let mut frames: Vec<Frame> = vec![Frame::default(); 30];
        frames.extend((0..50).map(|_| Frame {
            f0_cents: 1500,
            rms: 140,
            flags: FrameFlags::VOICED,
            ..Frame::default()
        }));
        frames.extend(vec![Frame::default(); 30]);
        frames.extend((0..50).map(|_| Frame {
            f0_cents: 1500,
            rms: 140,
            flags: FrameFlags::VOICED,
            ..Frame::default()
        }));
        let p = phrase_summary(&frames, &[], &SpeakerProfile::default());
        assert_eq!(p.pauses, 2);
        assert!(
            p.pause_frac_255 > 40 && p.pause_frac_255 < 100,
            "{}",
            p.pause_frac_255
        );
    }
}

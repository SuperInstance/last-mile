//! L1: gesture segmentation (design §4) — L0 frames cut into gestures, the
//! syllable-sized notes a renderer or a gesture matcher reads.
//!
//! A gesture starts where the ear hears a new syllable begin: at a detected
//! onset, at a voicing flip (vowel into fricative and back), when the level
//! falls into a valley and rises again, and around a pause. Pure integer
//! arithmetic on the already-quantized frames — the segmentation is exact
//! on every machine by construction.

use crate::frame::{Frame, FrameFlags, Gesture};
use crate::profile::SpeakerProfile;

/// How long a pause must last to be a pause of its own, in frames (200 ms).
const PAUSE_FRAMES: usize = 20;

/// How far the level must dip below its running peak, in L0 log units
/// (about 3.8 dB), for a valley to close a gesture.
const VALLEY_DIP: u8 = 10;

/// How far the level must recover after a valley for the split to stand,
/// in the same units.
const VALLEY_RISE: u8 = 5;

/// Gestures shorter than this, in frames, fold into their neighbor. A
/// performed syllable bottoms out around 80 ms — shorter spans are
/// measurement ripple (time-scale modification grain joins, smooth slow
/// speech), not notes — and folding them keeps L1's syllable count stable
/// under the conformer's own tempo channel.
const MIN_GESTURE_FRAMES: usize = 8;

/// Segment L0 frames into gestures, measured against the profile.
pub fn segment_gestures(frames: &[Frame], profile: &SpeakerProfile) -> Vec<Gesture> {
    // Cut 0 is always a boundary: the first gesture starts wherever the
    // utterance does, silence or sound.
    let mut cuts = vec![0usize];
    let mut start = 0usize;
    let mut peak = 0u8;

    for i in 0..frames.len() {
        let f = frames[i];
        let rest = f.flags.has(FrameFlags::REST);

        // A pause of its own: silence that runs past the marker.
        if rest {
            let mut run = 0usize;
            while i + run < frames.len() && frames[i + run].flags.has(FrameFlags::REST) {
                run += 1;
            }
            if run >= PAUSE_FRAMES || i == 0 {
                cuts.push(i);
                cuts.push(i + run);
            }
            // Skip the run; the state after it starts fresh.
            start = i + run;
            peak = 0;
            continue;
        }

        if i <= start {
            peak = f.rms;
            continue;
        }

        // A new attack: an onset flag, or sound out of silence.
        if f.flags.has(FrameFlags::ONSET) || frames[i - 1].flags.has(FrameFlags::REST) {
            cuts.push(i);
            start = i;
            peak = f.rms;
            continue;
        }

        // A voicing flip starts a new gesture: vowel into fricative,
        // fricative into vowel.
        if f.flags.has(FrameFlags::VOICED) != frames[i - 1].flags.has(FrameFlags::VOICED) {
            cuts.push(i);
            start = i;
            peak = f.rms;
            continue;
        }

        if f.rms > peak {
            peak = f.rms;
        }
        // A valley: the level fell well under the gesture's peak and has
        // begun to climb again — the syllable's energy trough. (The dip
        // is subtracted from the peak, not added to the frame, so a hot
        // frame near the top of the scale cannot overflow.)
        if f.rms <= peak.saturating_sub(VALLEY_DIP)
            && i + 1 < frames.len()
            && frames[i + 1].rms > f.rms + VALLEY_RISE
        {
            cuts.push(i);
            start = i;
            peak = f.rms;
        }
    }
    cuts.push(frames.len());
    cuts.sort_unstable();
    cuts.dedup();

    // Frame ranges of the candidate gestures.
    let mut spans: Vec<(usize, usize)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
    spans.retain(|&(a, b)| b > a);

    // Fold the too-short gestures into their predecessor.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = merged.last_mut() {
            let short = span.1 - span.0 < MIN_GESTURE_FRAMES;
            let prev_short = last.1 - last.0 < MIN_GESTURE_FRAMES;
            if short || prev_short {
                last.1 = span.1;
                continue;
            }
        }
        merged.push(span);
    }

    merged
        .iter()
        .map(|&(a, b)| gesture_of(a, b, frames, profile))
        .collect()
}

/// Read one gesture's dials off its frames.
fn gesture_of(a: usize, b: usize, frames: &[Frame], profile: &SpeakerProfile) -> Gesture {
    let span = &frames[a..b];
    let voiced: Vec<&Frame> = span
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED))
        .collect();
    let all_rest = span.iter().all(|f| f.flags.has(FrameFlags::REST));

    // The peak level and where it sits.
    let (peak_pos, peak) = span
        .iter()
        .enumerate()
        .max_by_key(|&(i, f)| (f.rms, std::cmp::Reverse(i)))
        .map(|(i, f)| (i, f.rms))
        .unwrap_or((0, 0));

    let median_loud = profile.median_loud();
    let velocity_db = if all_rest {
        i8::MIN
    } else {
        let db = (f64::from(peak) - f64::from(median_loud)) * (96.0 / 255.0);
        db as i8
    };

    // The pitch target: the median voiced cents of the span.
    let pitch_target = if voiced.is_empty() {
        0
    } else {
        let mut cents: Vec<i16> = voiced.iter().map(|f| f.f0_cents).collect();
        cents.sort_unstable();
        cents[cents.len() / 2]
    };

    // The glide: the mean of the last voiced quarter minus the first.
    let glide = if voiced.len() < 4 {
        0
    } else {
        let quarter = voiced.len() / 4;
        let head: i64 = voiced[..quarter]
            .iter()
            .map(|f| i64::from(f.f0_cents))
            .sum();
        let tail: i64 = voiced[voiced.len() - quarter..]
            .iter()
            .map(|f| i64::from(f.f0_cents))
            .sum();
        ((tail - head) / quarter as i64) as i16
    };

    // The articulation times, from the level's shape.
    let first_sound = span
        .iter()
        .position(|f| !f.flags.has(FrameFlags::REST))
        .unwrap_or(0);
    let attack_ms = if peak_pos > first_sound {
        ((peak_pos - first_sound) * 10) as u16
    } else {
        10
    };
    let release_ms = (((b - a) - peak_pos) * 10) as u16;

    // A noise burst at the opening: a fricative or a plosive release.
    let noise_burst = span[..span.len().min(3)]
        .iter()
        .any(|f| u16::from(f.noise) > 150 && !f.flags.has(FrameFlags::VOICED));

    Gesture {
        onset_ms: (a * 10) as u32,
        duration_ms: ((b - a) * 10) as u32,
        pitch_target_cents: pitch_target,
        glide_cents: glide,
        velocity_db,
        attack_ms: attack_ms.min(600),
        release_ms: release_ms.min(600),
        noise_burst,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One voiced island in silence is one gesture with the pitch it was
    /// made with.
    #[test]
    fn one_burst_one_gesture() {
        let mut frames = vec![Frame::default(); 30];
        for f in frames.iter_mut().skip(10).take(20) {
            *f = Frame {
                f0_cents: 1500,
                rms: 140,
                noise: 10,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            };
        }
        let gestures = segment_gestures(&frames, &SpeakerProfile::default());
        assert!(!gestures.is_empty());
        let voiced: Vec<&Gesture> = gestures
            .iter()
            .filter(|g| g.velocity_db != i8::MIN)
            .collect();
        assert_eq!(voiced.len(), 1);
        assert!((voiced[0].pitch_target_cents - 1500).abs() <= 1);
        assert_eq!(voiced[0].onset_ms, 100);
        assert_eq!(voiced[0].duration_ms, 200);
    }

    /// Two bursts separated by a pause are two gestures.
    #[test]
    fn two_bursts_two_gestures() {
        let mut frames = vec![Frame::default(); 200];
        for f in frames.iter_mut().skip(20).take(30) {
            *f = Frame {
                f0_cents: 1500,
                rms: 140,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            };
        }
        for f in frames.iter_mut().skip(120).take(30) {
            *f = Frame {
                f0_cents: 1600,
                rms: 150,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            };
        }
        let gestures = segment_gestures(&frames, &SpeakerProfile::default());
        let voiced: Vec<&Gesture> = gestures
            .iter()
            .filter(|g| g.velocity_db != i8::MIN)
            .collect();
        assert_eq!(voiced.len(), 2);
        assert!((voiced[0].pitch_target_cents - 1500).abs() <= 1);
        assert!((voiced[1].pitch_target_cents - 1600).abs() <= 1);
    }

    /// A falling glide is read as negative cents.
    #[test]
    fn glide_is_signed() {
        let frames: Vec<Frame> = (0..50)
            .map(|i| Frame {
                f0_cents: 1800 - (i as i16) * 8,
                rms: 140,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        let gestures = segment_gestures(&frames, &SpeakerProfile::default());
        assert_eq!(gestures.len(), 1);
        assert!(
            gestures[0].glide_cents < -100,
            "{}",
            gestures[0].glide_cents
        );
    }
}

//! α-β-γ tracking filters on the L0 tracks, in fixed point (fleet 14z's
//! nugget N5, the review of forclaude11's fixed-point filter — the idea,
//! not the nanosecond theater).
//!
//! A tracker-style filter predicts each dial from where it was and how
//! fast it was moving, then corrects the prediction by a fraction of the
//! residual. Two properties make it the right tool between the analyzer's
//! per-frame estimates and the encoder:
//!
//! * **Ramps ride free.** A steady glide (declination, a formant tracking
//!   from a vowel into a consonant) is absorbed by the velocity state, so
//!   the smoothed track follows it with no lag — the property E1's ±10
//!   cents demands and the reason a plain low-pass would fail it.
//! * **Jitter dies.** Estimator noise is uncorrelated frame to frame; the
//!   residual correction averages it toward zero instead of passing it
//!   through, which is what was forcing the running-status encoder out of
//!   its 4-bit deltas (E2's 498 B/s against the 300 B/s target).
//!
//! ## Resets, not targets
//!
//! The filter never smooths *across* a discontinuity. An octave jump in
//! the pitch, a voicing flip, an onset and a rest boundary reset the
//! filter: the new value states itself whole and tracking restarts from
//! it. Smoothing a real onset would be the one error no codec here is
//! allowed to make — the attack is the note — and a reset is also cheaper
//! on the wire than a long slide neither the voice nor the encoder made.
//!
//! ## Arithmetic
//!
//! Integer only, like everything downstream of the analyzer's
//! quantization: the state lives in Q4 units of each field (a sixteenth of
//! a cent, of an L0 log unit), the gains in Q8, and the one multiply per
//! gain is a shift-add rounded half up. No table, no float, no libm — the
//! same frames give the same smoothed track on every machine, and the
//! golden test pins it.

use crate::frame::{Frame, FrameFlags};

/// Fixed-point scale of the gains: 256 is 1.0.
const Q8: i32 = 256;

/// Fixed-point scale of the state: 16 is one field unit.
const Q4: i32 = 16;

/// The velocity clamp, in field units per frame: no dial in the protocol
/// moves a quarter of its range in 10 ms, and a clamp keeps a burst of
/// outliers from winding the state up past it.
const V_MAX: i32 = 64;

/// The acceleration clamp, in field units per frame squared.
const A_MAX: i32 = 32;

/// The gains of one α-β-γ filter, in Q8 (256 is 1.0).
///
/// `alpha` is how much of the residual the position takes this frame,
/// `beta` how much the velocity takes, `gamma` how much the acceleration
/// takes. All three are parameters, so a different voice or a different
/// frame rate can be tuned without touching the filter; the defaults are
/// what E1's tolerances and E2's byte counts on the skald corpus picked
/// ([`SmoothConfig::default`]). At `alpha = 256, beta = gamma = 0` the
/// filter is the identity on a steady track — the honest degenerate case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbgGains {
    /// The position gain, Q8. 0 holds the prediction, 256 snaps to the
    /// measurement.
    pub alpha: i32,
    /// The velocity gain, Q8. Zero gives a plain α filter (no ramp
    /// following); too much rings on steps.
    pub beta: i32,
    /// The acceleration gain, Q8. Small: it exists to take curvature
    /// (a formant accelerating through a consonant), not noise.
    pub gamma: i32,
}

impl AbgGains {
    /// All gains zero: the filter free-runs on its prediction. Only
    /// useful as a placeholder; [`AbgGains::identity`] is the honest no-op.
    pub const ZERO: AbgGains = AbgGains {
        alpha: 0,
        beta: 0,
        gamma: 0,
    };

    /// The degenerate filter: take the measurement, learn nothing. With
    /// these gains `smooth_l0` leaves every track exactly as it arrived,
    /// which is the "smoothing off" configuration E2's before/after
    /// measures against.
    pub const IDENTITY: AbgGains = AbgGains {
        alpha: 256,
        beta: 0,
        gamma: 0,
    };
}

/// One track's filter state, in Q4 field units: position, velocity (units
/// per frame) and acceleration (units per frame²).
#[derive(Clone, Copy, Debug)]
struct Track {
    x: i32,
    v: i32,
    a: i32,
}

impl Track {
    /// A track born on a measurement: sitting on it, no motion.
    fn new(z: i32) -> Track {
        Track {
            x: z * Q4,
            v: 0,
            a: 0,
        }
    }

    /// Where the filter says the track is, rounded to field units.
    fn out(&self) -> i32 {
        (self.x + Q4 / 2) / Q4
    }

    /// One predict-correct cycle against the measurement `z` (field
    /// units), with `g`'s gains. Returns the smoothed value, field units.
    fn step(&mut self, z: i32, g: &AbgGains) -> i32 {
        // Predict: constant acceleration for one frame.
        let x_pred = self.x + self.v + self.a / 2;
        let v_pred = self.v + self.a;
        // Correct: a fraction of the residual goes to each state.
        let r = z * Q4 - x_pred;
        self.x = x_pred + q8(g.alpha, r);
        self.v = (v_pred + q8(g.beta, r)).clamp(-V_MAX * Q4, V_MAX * Q4);
        self.a = (self.a + q8(g.gamma, r)).clamp(-A_MAX * Q4, A_MAX * Q4);
        self.out()
    }
}

/// One gain application: `g/2^8 * r`, rounded half up. The only
/// multiplication in the filter, and it is integer-exact everywhere.
fn q8(g: i32, r: i32) -> i32 {
    (g * r + Q8 / 2) / Q8
}

/// The smoothing pass's knobs. Every gain is a Q8 fraction; the resets are
/// in field units. The defaults are measured (E1's tolerances bound them
/// from above, E2's byte count from below); [`SmoothConfig::off`] is the
/// pass turned into a no-op for before/after measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmoothConfig {
    /// Master switch: `false` passes every frame through untouched (the
    /// pre-smoothing analyzer, bit for bit).
    pub enabled: bool,
    /// The pitch track's gains. Pitch is the channel E1 watches closest
    /// (±10 cents), so it tracks tightest.
    pub f0: AbgGains,
    /// The level track's gains.
    pub level: AbgGains,
    /// The three formant tracks' gains (one set for all three).
    pub formant: AbgGains,
    /// The spectral-tilt track's gains.
    pub tilt: AbgGains,
    /// The noise track's gains.
    pub noise: AbgGains,
    /// A pitch jump this big, in cents, is an octave error or a new note,
    /// not a glide: reset the track instead of chasing it. Half an octave,
    /// the same window the voicing median's octave hysteresis uses.
    pub f0_jump_cents: i32,
    /// A formant jump this big, in L0 units, is the picker switching
    /// candidates, not the tract moving: reset the track.
    pub formant_jump_units: i32,
    /// An rms jump this big, in L0 units, is an articulation the filter
    /// has no business ramping across (a stop closure). Resets are also
    /// forced by onsets and voicing flips; this catches the ones the flags
    /// miss.
    pub level_jump_units: i32,
    /// The most the pitch track may move in one frame, in cents, away
    /// from a reset boundary. This has to sit *above* what a real voice
    /// does — the E4 corpus declinates at up to 18 cents a frame, and a
    /// cap inside that lags the contour by hundreds of cents while saving
    /// nothing (the same fall costs the same delta bytes, spread thinner).
    /// What it clips is estimator and vocoder spikes only; the jitter is
    /// the hold band's job.
    pub f0_slew_cents: i32,
    /// The most a formant track may move in one frame, in L0 units on the
    /// 2-unit grid, away from a reset boundary. Real tract glides sit
    /// under it; faster steps are the picker switching candidates.
    pub formant_slew_units: i32,
    /// The most the level track may move in one frame, in L0 units on the
    /// 2-unit grid, away from a reset boundary; attacks are resets and
    /// state themselves whole. It must stay above the segmentation's
    /// VALLEY_RISE (5), or the smoothed level can never rise fast enough
    /// for a valley to close a gesture.
    pub level_slew_units: i32,
    /// The pitch track's hold band, in cents: away from a boundary the
    /// track does not move at all until the filter has drifted this far
    /// from what it last stated. E1's ±10-cent tolerance budgets the band
    /// plus the grid (6 + 3 cents).
    pub f0_hold_cents: i32,
    /// The level track's hold band, in L0 units (about 2.25 dB): the
    /// micro-dynamics of a neural render inside it are shimmer no
    /// listener tracks at 10 ms, and stating them costs a packet a frame.
    pub level_hold_units: i32,
    /// The formant tracks' hold band, in L0 units (three grid steps,
    /// about 4% on a formant): the picker's residual jitter sits inside
    /// it; a real glide crosses it in a frame or two.
    pub formant_hold_units: i32,
    /// The noise track's hold band, in L0 units. The noise dial is the
    /// one channel read as a class (voiced hiss against fricative roar),
    /// and its pre-smoothing pass held it a full 16; 8 is well inside
    /// that and still lets every real transition through.
    pub noise_hold_units: i32,
}

/// The pitch track's grid, in cents: a third of E1's ±10-cent tolerance,
/// and coarse enough that two grid steps still fit the encoder's 4-bit
/// delta (−8..7).
const F0_GRID: i32 = 3;
/// The level and formant tracks' grid, in L0 units (0.75 dB, 2.8%).
const DIAL_GRID: i32 = 2;

/// Step a non-negative value up to its grid (0 and the not-measured
/// marker stay put).
fn step_pos(v: i32, grid: i32) -> i32 {
    if v > 0 {
        (v + grid - 1) / grid * grid
    } else {
        v
    }
}

/// Emit a track value: on its grid, and — except at a boundary, where a
/// reset states itself whole — moving no more than `slew` grid units from
/// the last emitted value. The filter's internal state runs ahead of a
/// slewed emission and the residual pulls it back, so the track catches up
/// at full slew rate once the burst has passed.
fn slew_step(raw: i32, prev: i32, grid: i32, slew: i32, at_boundary: bool) -> i32 {
    let stepped = step_pos(raw, grid);
    let prev = step_pos(prev, grid);
    if at_boundary || slew <= 0 {
        return stepped;
    }
    prev + (stepped - prev).clamp(-slew, slew)
}

/// A hold band over grid and slew: stay at the last stated value until
/// the filter has drifted `hold` from it. Boundary frames reset the band
/// to the stated value, so attacks and note entries never wait on it.
#[derive(Clone, Copy, Debug)]
struct Hold {
    last: i32,
}

impl Hold {
    fn new(z: i32) -> Hold {
        Hold { last: z }
    }

    fn emit(&mut self, want: i32, hold: i32) -> i32 {
        if (want - self.last).abs() >= hold {
            self.last = want;
        }
        self.last
    }
}

impl Default for SmoothConfig {
    fn default() -> SmoothConfig {
        SmoothConfig {
            enabled: true,
            // Pitch: the gains E2's byte count picked, bounded by E1's
            // ±10 cents — alpha light enough that residual jitter stops
            // reaching the deltas, beta enough that declination and
            // intonation ramps ride with no lag.
            f0: AbgGains {
                alpha: 96,
                beta: 24,
                gamma: 2,
            },
            // Level: attacks are resets, so between resets the track only
            // has envelopes and micro-tremor on it. No velocity state:
            // an envelope's fast rises are attacks (resets) or slew, and
            // a beta here rides ramps past their summits — the +3-4 dB
            // phantom peaks the E4 check caught.
            level: AbgGains {
                alpha: 64,
                beta: 0,
                gamma: 0,
            },
            // Formants: the slowest dials — they glide hundreds of Hz
            // through consonants over many frames, and their estimator
            // jitter rides on top of that.
            formant: AbgGains {
                alpha: 72,
                beta: 16,
                gamma: 1,
            },
            // Tilt: nearly a slow envelope, so no velocity state either —
            // the same ramp-past-the-summit overshoot applied to it (the
            // E4 check caught bright classes flipping mid→high on it).
            tilt: AbgGains {
                alpha: 64,
                beta: 0,
                gamma: 0,
            },
            // Noise: the jitteriest channel and the one no ear follows at
            // 10 ms; it gets the heaviest hand (plus the analyzer's
            // deadband downstream of this pass).
            noise: AbgGains {
                alpha: 48,
                beta: 3,
                gamma: 0,
            },
            f0_jump_cents: 600,
            formant_jump_units: 24,
            level_jump_units: 40,
            // The slew bounds: high enough never to bind a real contour
            // (4000 cents/s on the pitch, 2.25 dB a frame on the level,
            // 5.6% a frame on a formant), low enough to catch estimator
            // spikes.
            f0_slew_cents: 40,
            formant_slew_units: 4,
            level_slew_units: 6,
            // The hold bands: twice a grid step, and within E1's budget
            // on the pitch (6 + 3 < 10 cents).
            f0_hold_cents: 6,
            level_hold_units: 5,
            formant_hold_units: 6,
            noise_hold_units: 8,
        }
    }
}

impl SmoothConfig {
    /// The pass as a no-op: every frame leaves exactly as it arrived. E2's
    /// "before" column and any A/B the analyzer's callers want to run.
    pub fn off() -> SmoothConfig {
        SmoothConfig {
            enabled: false,
            ..SmoothConfig::default()
        }
    }
}

/// The filter bank: one track each for f0, rms, f1, f2, f3, tilt, noise.
#[derive(Clone, Copy, Debug)]
struct Bank {
    f0: Track,
    level: Track,
    f1: Track,
    f2: Track,
    f3: Track,
    tilt: Track,
    noise: Track,
}

impl Bank {
    fn new(f: &Frame) -> Bank {
        Bank {
            f0: Track::new(i32::from(f.f0_cents)),
            level: Track::new(i32::from(f.rms)),
            f1: Track::new(i32::from(f.f1)),
            f2: Track::new(i32::from(f.f2)),
            f3: Track::new(i32::from(f.f3)),
            tilt: Track::new(i32::from(f.tilt)),
            noise: Track::new(i32::from(f.noise)),
        }
    }
}

/// Smooth the L0 tracks: run the α-β-γ bank over the frames in place.
///
/// The flags travel untouched — voicing, onsets and rests are decisions,
/// not dials, and the smoothing must never move a boundary — and every
/// reset boundary snaps the dials to their measured values so real
/// transients state themselves whole instead of sliding through the
/// filter.
///
/// The pitch field carries the last established pitch through unvoiced
/// frames inside a gesture (the larynx does not teleport between a vowel
/// and the fricative beside it; voicing itself is the flag's job, and
/// every reader of L0 — the segmentation, the phrase, the renderer —
/// gates on the flag). Before the first voiced frame of a gesture the
/// field reads 0: no pitch is known.
pub fn smooth_l0(frames: &mut [Frame], cfg: &SmoothConfig) {
    if !cfg.enabled || frames.len() < 3 {
        return;
    }

    let mut bank: Option<Bank> = None;
    // The pitch the voice last settled on: held through unvoiced frames,
    // 0 until the first voiced one.
    let mut carried_f0: i32 = 0;
    // The hold bands on every dial's emission.
    let mut f0_hold = Hold::new(0);
    let mut level_hold = Hold::new(0);
    let mut f1_hold = Hold::new(0);
    let mut f2_hold = Hold::new(0);
    let mut f3_hold = Hold::new(0);
    let mut noise_hold = Hold::new(0);
    // Whether the previous frame was a boundary (reset or track jump):
    // the frame after one may state itself whole.
    let mut prev_boundary = true;
    for i in 0..frames.len() {
        let cur = frames[i];
        let prev = if i > 0 {
            frames[i - 1]
        } else {
            Frame::default()
        };
        let prev_rest = i == 0 || frames[i - 1].flags.has(FrameFlags::REST);
        let cur_rest = cur.flags.has(FrameFlags::REST);
        let voiced_flip = i > 0
            && cur.flags.has(FrameFlags::VOICED) != frames[i - 1].flags.has(FrameFlags::VOICED);
        let onset = cur.flags.has(FrameFlags::ONSET);
        // The reset boundaries: a new note (onset, sound out of rest, a
        // voicing flip) states itself whole and tracking restarts. Rest
        // frames are silence — nothing to smooth, pass them through.
        let reset = onset || voiced_flip || prev_rest || cur_rest;

        let Some(b) = bank.as_mut() else {
            bank = Some(Bank::new(&cur));
            carried_f0 = if cur.flags.has(FrameFlags::VOICED) {
                i32::from(cur.f0_cents)
            } else {
                0
            };
            f0_hold = Hold::new(carried_f0);
            level_hold = Hold::new(i32::from(cur.rms));
            f1_hold = Hold::new(i32::from(cur.f1));
            f2_hold = Hold::new(i32::from(cur.f2));
            f3_hold = Hold::new(i32::from(cur.f3));
            noise_hold = Hold::new(i32::from(cur.noise));
            prev_boundary = true;
            continue;
        };

        if reset {
            *b = Bank::new(&cur);
            // A flip into unvoicing holds the pitch the voice had, not
            // the zero the median pass paints there; a flip into voicing
            // (or an onset, or a rest) states the frame as measured.
            if !cur_rest && !cur.flags.has(FrameFlags::VOICED) && carried_f0 > 0 {
                frames[i].f0_cents = carried_f0 as i16;
                b.f0 = Track::new(carried_f0);
            } else if cur.flags.has(FrameFlags::VOICED) {
                carried_f0 = i32::from(cur.f0_cents);
            } else {
                carried_f0 = 0;
            }
            // A boundary re-arms the hold bands at the stated values: the
            // frame after one may have to catch up fast, and a hold that
            // kept pre-boundary values would release that catch-up one
            // frame too late, when the slew exemption has already expired.
            f0_hold = Hold::new(carried_f0);
            level_hold = Hold::new(i32::from(frames[i].rms));
            f1_hold = Hold::new(i32::from(frames[i].f1));
            f2_hold = Hold::new(i32::from(frames[i].f2));
            f3_hold = Hold::new(i32::from(frames[i].f3));
            noise_hold = Hold::new(i32::from(frames[i].noise));
            prev_boundary = true;
            continue;
        }

        // The pitch track. While voiced, filter (an octave-class jump
        // resets the track: that is a new note or an estimator error, not
        // a glide). While unvoiced, hold the carried pitch steady — a
        // flickering 0↔1700 field through a fricative is the estimator's
        // indecision, and every flicker costs a whole-frame fallback.
        let mut jumped = false;
        if cur.flags.has(FrameFlags::VOICED) {
            let z = i32::from(cur.f0_cents);
            let raw = if (z - b.f0.out()).abs() > cfg.f0_jump_cents {
                b.f0 = Track::new(z);
                jumped = true;
                z
            } else {
                b.f0.step(z, &cfg.f0)
            };
            let slewed = slew_step(
                raw,
                i32::from(prev.f0_cents),
                F0_GRID,
                cfg.f0_slew_cents,
                prev_boundary || jumped,
            );
            frames[i].f0_cents = f0_hold.emit(slewed, cfg.f0_hold_cents) as i16;
            carried_f0 = i32::from(frames[i].f0_cents);
        } else {
            frames[i].f0_cents = carried_f0 as i16;
        }

        let raw_level = if (i32::from(cur.rms) - b.level.out()).abs() > cfg.level_jump_units {
            b.level = Track::new(i32::from(cur.rms));
            jumped = true;
            i32::from(cur.rms)
        } else {
            b.level.step(i32::from(cur.rms), &cfg.level)
        };
        let slewed_level = slew_step(
            raw_level,
            i32::from(prev.rms),
            DIAL_GRID,
            cfg.level_slew_units,
            prev_boundary || jumped,
        );
        frames[i].rms = level_hold
            .emit(slewed_level, cfg.level_hold_units)
            .clamp(0, 255) as u8;

        for (field, track, prev_field, hold) in [
            (
                &mut frames[i].f1,
                &mut b.f1,
                i32::from(prev.f1),
                &mut f1_hold,
            ),
            (
                &mut frames[i].f2,
                &mut b.f2,
                i32::from(prev.f2),
                &mut f2_hold,
            ),
            (
                &mut frames[i].f3,
                &mut b.f3,
                i32::from(prev.f3),
                &mut f3_hold,
            ),
        ] {
            let z = i32::from(*field);
            // A jump here is the picker switching candidates, not the
            // tract moving: the filter's state re-bases to it, but the
            // emission still slews — the glide between two diphone
            // targets is exactly the motion a renderer wants to play.
            let raw = if (z - track.out()).abs() > cfg.formant_jump_units {
                *track = Track::new(z);
                z
            } else {
                track.step(z, &cfg.formant)
            };
            let slewed = slew_step(
                raw,
                prev_field,
                DIAL_GRID,
                cfg.formant_slew_units,
                prev_boundary,
            );
            *field = hold.emit(slewed, cfg.formant_hold_units).clamp(0, 255) as u8;
        }
        frames[i].tilt = b.tilt.step(i32::from(cur.tilt), &cfg.tilt).clamp(-127, 127) as i8;
        // The noise dial rides the same grid and slew as the formants:
        // its jitter is the filter's to kill, and its real transitions
        // sit at voicing flips, which are boundaries.
        let noise_slewed = slew_step(
            b.noise.step(i32::from(cur.noise), &cfg.noise),
            i32::from(prev.noise),
            DIAL_GRID,
            cfg.formant_slew_units,
            prev_boundary,
        );
        frames[i].noise = noise_hold
            .emit(noise_slewed, cfg.noise_hold_units)
            .clamp(0, 255) as u8;

        prev_boundary = jumped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voiced(f0: i16, rms: u8) -> Frame {
        Frame {
            f0_cents: f0,
            rms,
            f1: 100,
            f2: 180,
            f3: 240,
            tilt: -4,
            noise: 20,
            flags: FrameFlags::VOICED,
        }
    }

    /// A ramp rides with no lag beyond the emission budget: a
    /// 3-cents-a-frame declination (the E1 sweep's rate) is followed
    /// within the hold band plus the grid (6 + 3 cents), which is E1's
    /// ±10-cent tolerance with a cent to spare.
    #[test]
    fn a_ramp_rides_free() {
        let cfg = SmoothConfig::default();
        let frames: Vec<Frame> = (0..200)
            .map(|i| voiced(1800 - (i as i16) * 3, 140))
            .collect();
        let mut smoothed = frames.clone();
        smooth_l0(&mut smoothed, &cfg);
        let worst = smoothed[20..]
            .iter()
            .zip(&frames[20..])
            .map(|(s, f)| (i32::from(s.f0_cents) - i32::from(f.f0_cents)).abs())
            .max()
            .unwrap();
        assert!(worst <= 9, "ramp lagged by {worst} cents");
    }

    /// Jitter dies: uncorrelated +-16 cents of estimator noise on a steady
    /// pitch comes out with a much tighter typical deviation. (The extremes
    /// survive — a tracker rightly follows runs of same-sign noise — and
    /// correlated wobble is not jitter at all.)
    #[test]
    fn jitter_dies() {
        let cfg = SmoothConfig::default();
        let mut state = 0xfeed_face_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let frames: Vec<Frame> = (0..300)
            .map(|_| {
                let mut f = voiced(1800, 140);
                // White +-16 cents, in integer quanta.
                f.f0_cents += (((next() >> 16) % 33) as i32 - 16) as i16;
                f
            })
            .collect();
        let mad = |fs: &[Frame]| -> f64 {
            let mut vals: Vec<i32> = fs[20..].iter().map(|f| i32::from(f.f0_cents)).collect();
            vals.sort_unstable();
            let median = vals[vals.len() / 2];
            let mut dev: Vec<i32> = vals.iter().map(|&c| (c - median).abs()).collect();
            dev.sort_unstable();
            f64::from(dev[dev.len() / 2])
        };
        let raw_mad = mad(&frames);
        let mut smoothed = frames.clone();
        smooth_l0(&mut smoothed, &cfg);
        let smooth_mad = mad(&smoothed);
        assert!(
            smooth_mad <= raw_mad * 2.0 / 3.0,
            "typical deviation {smooth_mad} cents against the raw {raw_mad}"
        );
    }

    /// An onset is stated whole, never slid into: the frame after a rest
    /// carries exactly its measured dials.
    #[test]
    fn onsets_survive() {
        let mut frames: Vec<Frame> = vec![Frame::default(); 30];
        frames.extend((0..40).map(|_| voiced(1500, 40)));
        let before = frames.clone();
        let mut smoothed = before.clone();
        smooth_l0(&mut smoothed, &SmoothConfig::default());
        // The first sounding frame is a reset boundary: identical.
        assert_eq!(smoothed[30], before[30]);
        // And an onset flag mid-stream resets too.
        let mut frames2: Vec<Frame> = (0..60).map(|_| voiced(1500, 100)).collect();
        let mut jump = voiced(1900, 200);
        jump.flags.set(FrameFlags::ONSET, true);
        frames2.push(jump);
        frames2.extend((0..40).map(|_| voiced(1900, 200)));
        let mut smoothed2 = frames2.clone();
        smooth_l0(&mut smoothed2, &SmoothConfig::default());
        assert_eq!(smoothed2[60], frames2[60], "the onset frame states itself");
    }

    /// A voicing flip is a reset: the unvoiced frame holds the pitch the
    /// voice had (voicing itself is the flag's job), and the return to
    /// voice states its measured cents whole.
    #[test]
    fn voicing_flips_reset() {
        let mut frames: Vec<Frame> = (0..30).map(|_| voiced(1500, 140)).collect();
        let mut unvoiced = voiced(0, 120);
        unvoiced.flags = FrameFlags(0);
        frames.push(unvoiced);
        frames.extend((0..30).map(|_| voiced(1520, 140)));
        let mut smoothed = frames.clone();
        smooth_l0(&mut smoothed, &SmoothConfig::default());
        assert_eq!(
            smoothed[30].f0_cents, 1500,
            "the carry holds through the flip"
        );
        assert_eq!(
            smoothed[31].f0_cents, 1520,
            "the return to voice states itself"
        );
    }

    /// An unvoiced stretch that never had a pitch reads 0: nothing is
    /// carried before the first voiced frame of a gesture.
    #[test]
    fn no_carry_before_the_first_pitch() {
        let mut frames: Vec<Frame> = (0..20)
            .map(|_| {
                let mut f = voiced(0, 120);
                f.flags = FrameFlags(0);
                f
            })
            .collect();
        frames.extend((0..20).map(|_| voiced(1500, 140)));
        let before = frames.clone();
        let mut smoothed = before.clone();
        smooth_l0(&mut smoothed, &SmoothConfig::default());
        assert!(smoothed[..20].iter().all(|f| f.f0_cents == 0));
        assert_eq!(smoothed[20].f0_cents, 1500);
    }

    /// An octave jump resets the pitch track instead of glissandoing
    /// through it.
    #[test]
    fn octave_jumps_reset() {
        let mut frames: Vec<Frame> = (0..50).map(|_| voiced(1500, 140)).collect();
        frames.extend((0..50).map(|_| voiced(2700, 140)));
        let mut smoothed = frames.clone();
        smooth_l0(&mut smoothed, &SmoothConfig::default());
        assert_eq!(smoothed[50].f0_cents, 2700);
        // And nothing in between slides: the frame before is still 1500.
        assert_eq!(smoothed[49].f0_cents, 1500);
    }

    /// The off configuration is the identity, frame for frame.
    #[test]
    fn off_is_the_identity() {
        let mut frames: Vec<Frame> = vec![Frame::default(); 10];
        frames.extend((0..60).map(|i| voiced(1500 + (i as i16) % 7 * 11 - 33, 130)));
        frames.extend((0..20).map(|_| voiced(1500, 90)));
        let before = frames.clone();
        smooth_l0(&mut frames, &SmoothConfig::off());
        assert_eq!(frames, before);
    }

    /// The rounding and clamps hold the state in range over a long worst
    /// case: heavy noise, full-scale jumps, a 500-frame clip.
    #[test]
    fn state_stays_bounded() {
        let cfg = SmoothConfig::default();
        let mut state = 0x243F_6A88u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let frames: Vec<Frame> = (0..500)
            .map(|i| {
                let r = next();
                let mut f = voiced((800 + (r % 2400) as i32) as i16, ((r >> 8) % 256) as u8);
                f.tilt = ((((r >> 4) % 255) as i32) - 128) as i8;
                f.noise = ((r >> 12) % 256) as u8;
                f.f1 = ((r >> 6) % 256) as u8;
                f.flags = FrameFlags(if i % 17 == 0 { 0x03 } else { 0x01 });
                f
            })
            .collect();
        let mut smoothed = frames.clone();
        smooth_l0(&mut smoothed, &cfg);
        for (s, f) in smoothed.iter().zip(&frames) {
            assert_eq!(s.flags, f.flags, "flags never move");
            assert!((-127..=127).contains(&s.tilt));
            assert!(
                s.f0_cents > 0 && s.f0_cents < 4096,
                "pitch left the voice's range"
            );
        }
    }
}

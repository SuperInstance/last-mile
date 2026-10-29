//! Lane conditioning: the fixed-point smoother that turns a stream of
//! presence-mode probability samples into a stable choice (D38, D43).
//!
//! A gate reports one `u8` per option on every presence tick. Those samples
//! are noisy, so the raw argmax chatters (PoC A: 34 flips per agent-minute).
//! The conditioner keeps one lane per option, smooths it with an exponential
//! moving average in Q16 and only lets the selection move when a challenger
//! clears the deadband, so the choice is stable but still responsive.
//!
//! Everything is integer maths: a sample is scaled to Q16 (`255 * 257 =
//! 65 535`), the lane is a signed Q16 accumulator and the EMA shift is an
//! arithmetic shift, so two machines reach the same lane values from the same
//! samples.

use crate::hash::{Fnv64, StateHash};

/// The Q16 scale of a `u8` sample: `255 * 257 = 65 535`, one below 1.0.
const Q16_SCALE: i64 = 257;

/// The widest menu whose options can be named by a `u8` index.
const MAX_OPTIONS: usize = 256;

/// The largest EMA shift that can be applied to an `i64` without panicking.
/// A shift this large makes the average effectively frozen.
const MAX_ALPHA_SHIFT: u8 = 63;

/// Conditions one menu's options: an exponential moving average per option
/// plus a deadband with hysteresis around the selection (D38, D43).
///
/// The conditioner is fed one sample per option per presence tick and keeps
/// the selection stable: a challenger only takes over when its lane leads the
/// current selection by more than the deadband. A new menu (or a new option
/// count) resets the lanes, so a lane never mixes samples from two menus.
///
/// The tunables come from `Params` (`lane_alpha_shift`, `lane_deadband_q16`)
/// and are passed in, so this type holds no defaults of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneConditioner {
    /// The EMA shift: the lane moves by `1 / 2^alpha_shift` of the gap.
    alpha_shift: u8,
    /// The Q16 width a challenger must lead by before the selection moves.
    deadband_q16: u32,
    /// The menu the lanes belong to, if any.
    menu_id: Option<u64>,
    /// One Q16 lane per option.
    lanes: Vec<i64>,
    /// The option currently selected.
    current: Option<u8>,
}

impl LaneConditioner {
    /// A conditioner with the given EMA shift and deadband.
    ///
    /// `alpha_shift` is clamped to [`MAX_ALPHA_SHIFT`] so that the shift can
    /// never panic, and `deadband_q16` is a Q16 width (`0..=65_535`) that the
    /// caller validates.
    pub fn new(alpha_shift: u8, deadband_q16: u32) -> Self {
        Self {
            alpha_shift: alpha_shift.min(MAX_ALPHA_SHIFT),
            deadband_q16,
            menu_id: None,
            lanes: Vec::new(),
            current: None,
        }
    }

    /// Feed one sample per option and return the selected option.
    ///
    /// A sample is scaled to Q16 and smoothed into its lane:
    /// `lane[i] += (q - lane[i]) >> alpha_shift`. The selected option is the
    /// argmax of the lanes (the lowest index wins ties), but it only moves to
    /// a challenger when that challenger leads by more than the deadband.
    /// After a reset there is nothing to compare against, so the first sample
    /// picks the argmax outright.
    ///
    /// A changed menu id or option count resets the lanes first. A menu with
    /// more than [`MAX_OPTIONS`] options cannot be named by a `u8` index, so
    /// it is refused and no selection is made.
    pub fn update(&mut self, menu_id: u64, values: &[u8]) -> Option<u8> {
        if self.menu_id != Some(menu_id) || self.lanes.len() != values.len() {
            self.reset(menu_id, values.len());
        }
        if values.len() > MAX_OPTIONS {
            return None;
        }

        let shift = self.alpha_shift;
        for (i, &sample) in values.iter().enumerate() {
            let q = i64::from(sample) * Q16_SCALE;
            let lane = self.lanes[i];
            self.lanes[i] = lane + ((q - lane) >> shift);
        }

        let best = self.argmax()?;
        match self.current {
            None => self.current = Some(best),
            Some(current) => {
                let lead = self.lanes[usize::from(best)] - self.lanes[usize::from(current)];
                if lead > i64::from(self.deadband_q16) {
                    self.current = Some(best);
                }
            }
        }
        self.current
    }

    /// The option currently selected, if there is one.
    pub fn current(&self) -> Option<u8> {
        self.current
    }

    /// Forget the lanes and start the menu again from scratch.
    fn reset(&mut self, menu_id: u64, options: usize) {
        self.menu_id = Some(menu_id);
        self.lanes = vec![0; options];
        self.current = None;
    }

    /// The option with the highest lane; the lowest index wins ties.
    fn argmax(&self) -> Option<u8> {
        let mut best: Option<(usize, i64)> = None;
        for (i, &lane) in self.lanes.iter().enumerate() {
            match best {
                Some((_, top)) if lane <= top => {}
                _ => best = Some((i, lane)),
            }
        }
        best.and_then(|(i, _)| u8::try_from(i).ok())
    }
}

impl StateHash for LaneConditioner {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.alpha_shift]);
        h.u32(self.deadband_q16);
        match self.menu_id {
            None => h.write(&[0]),
            Some(menu_id) => {
                h.write(&[1]);
                h.u64(menu_id);
            }
        }
        match self.current {
            None => h.write(&[0]),
            Some(current) => {
                h.write(&[1]);
                h.write(&[current]);
            }
        }
        h.u32(self.lanes.len() as u32);
        for lane in &self.lanes {
            h.i64(*lane);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conditioner used by the plan's tests: α = 1/4, deadband 0.10.
    fn lc() -> LaneConditioner {
        LaneConditioner::new(2, 6_554)
    }

    #[test]
    fn first_sample_is_argmax() {
        let mut l = lc();
        assert_eq!(l.update(1, &[10, 200, 30]), Some(1));
    }

    #[test]
    fn single_noisy_sample_does_not_flip() {
        let mut l = lc();
        for _ in 0..8 {
            l.update(1, &[220, 20, 15]);
        }
        assert_eq!(l.update(1, &[10, 240, 5]), Some(0));
    }

    #[test]
    fn sustained_change_flips() {
        let mut l = lc();
        for _ in 0..8 {
            l.update(1, &[220, 20, 15]);
        }
        let mut last = None;
        for _ in 0..6 {
            last = l.update(1, &[10, 240, 5]);
        }
        assert_eq!(last, Some(1));
    }

    #[test]
    fn menu_change_resets() {
        let mut l = lc();
        l.update(1, &[255, 0]);
        assert_eq!(l.update(2, &[0, 255, 0]), Some(1));
    }

    #[test]
    fn equal_lanes_hold() {
        let mut l = lc();
        l.update(1, &[128, 127]);
        for _ in 0..20 {
            assert_eq!(l.update(1, &[127, 128]), Some(0));
        }
    }

    #[test]
    fn nothing_is_selected_before_the_first_sample() {
        assert_eq!(lc().current(), None);
    }

    #[test]
    fn option_count_change_resets() {
        let mut l = lc();
        l.update(1, &[255, 0]);
        // Same menu, different width: the lanes reset, so the new sample decides.
        assert_eq!(l.update(1, &[0, 255, 0]), Some(1));
    }

    #[test]
    fn state_hash_is_stable_and_sensitive() {
        let mut a = lc();
        a.update(1, &[10, 200, 30]);
        let mut b = lc();
        b.update(1, &[10, 200, 30]);
        assert_eq!(a.state_hash(), b.state_hash());
        b.update(1, &[11, 200, 30]);
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn a_menu_wider_than_the_index_is_refused() {
        let wide: Vec<u8> = vec![1; 257];
        let mut l = lc();
        assert_eq!(l.update(1, &wide), None);
        assert_eq!(l.current(), None);
    }

    #[test]
    fn the_widest_nameable_menu_still_works() {
        let wide: Vec<u8> = vec![0; 256];
        let mut l = lc();
        assert_eq!(l.update(1, &wide), Some(0));
    }
}

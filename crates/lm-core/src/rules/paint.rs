//! The paint's rule: what standing in the other team's paint does.
//!
//! The paint itself is the medium — it dries on its own, and it lives in
//! [`crate::world::paint`]. This module is the *rule* the paint plays on the
//! agents that stand in it: the slow it gives them and the push-back it
//! gives them. The rule is written here once, and the script that mirrors
//! the native rules (`assets/rules/beacons.rhai`) writes the same rule in
//! Rhai, so the oracle tests hold tick for tick:
//! `rhai_beacons_matches_native_with_the_paint_on` plays the round with the
//! knobs turned and the two rulesets have to agree, hash for hash.
//!
//! # The rule, exactly
//!
//! An agent that stands on wet paint owned by **another** squad is in
//! *contact* with the paint. Its own team's paint does nothing to it.
//!
//! 1. **The slow.** A contact agent's next step is
//!    `speed_upt * speed_left_q8(slow_q8) / 255` units long instead of
//!    `speed_upt`. The knob is Q8 and 255 is a full stop: `slow_q8 = 0` is
//!    no slow at all, `slow_q8 = 255` is no movement, `slow_q8 = 128` is
//!    half speed. The arithmetic is integer-only and floors: a slow never
//!    rounds a step up. (The knob is the codebase's Q8, whose one is 255 —
//!    the same "a full stop" the parameter's own doc comment has always
//!    said.)
//!
//! 2. **The bounce.** Every `N`-th tick of contact the paint pushes the
//!    agent back to the cell it came from. `N` comes from `bounce_q8`:
//!    `N = 256 / bounce_q8` ticks of contact, and never less than one.
//!    `bounce_q8 = 0` means the paint never pushes anybody back. So 255
//!    pushes on every tick of contact, 128 every second, 64 every fourth.
//!    The push-back is chance-free: it happens on the tick the count comes
//!    round, never on a roll.
//!
//! The count of contact ticks belongs to the agent: it rises by one on every
//! tick the agent is in contact and falls back to zero on the first tick it
//! is not. The push-back is one step backwards, to the centre of the cell
//! the agent came from — the cell it stood in before it last moved. An
//! agent that has not moved is pushed back to where it came from the last
//! time it did, and an agent that has never moved is not pushed at all.
//!
//! # Who decides, and when
//!
//! The rules decide; the motor obeys. The rules of a tick write the slow
//! into the agent (`Agent::paint_slow_q8`), and the motor of the *next* tick
//! moves the agent that much less — so an agent that steps into the paint
//! on tick T is slowed from tick T+1. The push-back is applied by the rules
//! of the tick itself, so an agent that steps into the paint is pushed out
//! at the end of that same tick.
//!
//! The rules read the paint at the *start* of their tick, before any splat
//! of that tick is written: the paint is a fact about the ground, and a
//! splat written by the rules of a tick is felt from the next tick on.

use crate::params::{Params, Q8_ONE};
use crate::world::{CellXY, Paint, PaintCell};

/// Whether the paint plays any rule at all.
///
/// A round with both knobs at zero is a round whose paint is only a mark on
/// the ground: no slow, no push-back, and nothing of the paint's own in the
/// state hash. The rule is off until a knob is turned.
pub fn paint_rules_on(p: &Params) -> bool {
    p.slow_q8 > 0 || p.bounce_q8 > 0
}

/// The speed an agent keeps when the paint slows it by `slow_q8`, in Q8.
///
/// The knob is the amount of speed the paint takes away: `slow_q8 = 0` leaves
/// the agent at full speed (255, all of it) and `slow_q8 = 255` stops it
/// dead (0, none of it). The answer is clamped, so a knob past the Q8 one
/// cannot make an agent move backwards.
pub fn speed_left_q8(slow_q8: u16) -> u16 {
    Q8_ONE.saturating_sub(slow_q8.min(Q8_ONE))
}

/// The number of ticks of contact between the paint's push-backs.
///
/// `bounce_q8 = 0` means the paint never pushes anybody back, and the period
/// is 0. Otherwise the period is `256 / bounce_q8` ticks of contact, and
/// never less than one: 255 pushes on every tick of contact, 128 every
/// second, 64 every fourth.
pub fn bounce_period(bounce_q8: u16) -> u16 {
    if bounce_q8 == 0 {
        return 0;
    }
    u16::try_from((256u32 / u32::from(bounce_q8)).max(1)).unwrap_or(u16::MAX)
}

/// The paint on `cell` that belongs to another squad, if there is any.
///
/// This is the fact the rule is about: a cell carries the paint of the last
/// squad to splat it, and the paint of another squad is what the rule acts
/// on. Own-team paint is not contact, and neither is bare ground.
pub fn enemy_paint_under(paint: &Paint, cell: CellXY, squad: u8) -> Option<&PaintCell> {
    let cell_paint = paint.get(cell)?;
    if cell_paint.owner == squad {
        return None;
    }
    Some(cell_paint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Q8_ONE;

    /// A cell in the middle of nowhere.
    fn cell() -> CellXY {
        CellXY { x: 3, y: 4 }
    }

    #[test]
    fn no_slow_leaves_the_whole_speed() {
        assert_eq!(speed_left_q8(0), Q8_ONE);
    }

    #[test]
    fn a_full_slow_stops_the_agent_dead() {
        assert_eq!(speed_left_q8(Q8_ONE), 0);
        // Past the Q8 one the answer is clamped, never negative.
        assert_eq!(speed_left_q8(300), 0);
    }

    #[test]
    fn half_a_slow_is_half_the_speed() {
        assert_eq!(speed_left_q8(128), 127);
    }

    #[test]
    fn no_bounce_means_no_period() {
        assert_eq!(bounce_period(0), 0);
    }

    #[test]
    fn the_bigger_the_bounce_the_often_the_push_back() {
        assert_eq!(bounce_period(255), 1);
        assert_eq!(bounce_period(128), 2);
        assert_eq!(bounce_period(64), 4);
        assert_eq!(bounce_period(32), 8);
    }

    #[test]
    fn the_period_is_never_below_one() {
        // A knob of 255 is the whole push-back; nothing may divide it away.
        assert!((1..=255).all(|knob| bounce_period(knob) >= 1));
    }

    #[test]
    fn the_own_teams_paint_is_not_contact() {
        let mut paint = Paint::new();
        paint.splat(cell(), 0);
        assert!(
            enemy_paint_under(&paint, cell(), 0).is_none(),
            "own-team paint does not act on its own squad"
        );
        assert!(
            enemy_paint_under(&paint, cell(), 1).is_some(),
            "the other team's paint is contact"
        );
    }

    #[test]
    fn bare_ground_is_not_contact() {
        let paint = Paint::new();
        assert!(enemy_paint_under(&paint, cell(), 0).is_none());
        assert!(enemy_paint_under(&paint, cell(), 1).is_none());
    }
}

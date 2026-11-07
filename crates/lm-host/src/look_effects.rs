//! The tag moment: what a hit looks like, and the path of the agent you
//! watch.
//!
//! Three things the renderer adds on top of the round, and none of them
//! change it:
//!
//! * the **splat** — a burst of paint around the agent that was hit, in
//!   the paint of the squad that hit them, and the stain it settles into
//!   on the ground beside them. It is short-lived: half a second of paint
//!   in the air, and then the board is clean again. The persistent layer
//!   of paint on the ground is another job's; this is the hit.
//! * the **stun pips** — a row of marks above an agent that is stunned, one
//!   for each tick of the stun that is left. A stunned agent that looks the
//!   same as a free one is a round nobody can read.
//! * the **trail** — the path the agent you are watching has walked, drawn
//!   behind it. The path is the host's own bookkeeping: it watches where
//!   the agent is, frame by frame, and never asks the round to remember.
//!
//! The effects live in one resource, and both views read it: the 2D map
//! and the 3D view are the same world drawn twice, and a hit has to look
//! like the same hit in both of them. So the resource *is* the truth, and
//! the views are only how it is shown.

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;

use lm_core::rules::Event;
use lm_render::look::HitLook;

use crate::look::Look;
use crate::sim_plugin::LiveEvents;

/// How long the trail is kept, in points.
pub const TRAIL_POINTS: usize = 48;

/// The most stun pips an agent is shown with.
///
/// The pips say "this agent is out of the round for a while"; past a dozen
/// they are a wall, and the number that matters is in the round's log.
pub const MAX_STUN_PIPS: u32 = 12;

/// One hit: who hit whom, and in what paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tag {
    /// The tick it happened on.
    pub tick: u64,
    /// The agent that made the tag.
    pub by: u16,
    /// The agent that was tagged.
    pub target: u16,
    /// The paint that hit: the tagger's own.
    pub paint: lm_render::look::Rgb,
}

/// A dab of paint: one piece of the burst, and of the stain it settles
/// into.
///
/// The paint of a hit is an event, not a state: it erupts around the piece
/// that was hit, hangs in the air for a moment, and falls onto the ground
/// around it. The dab knows its own age, so it can do all three on its
/// own, and both views read the same numbers, so a hit looks like the same
/// hit in the map and in the diorama.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splat {
    /// The piece that was hit: the centre of the burst, in metres.
    pub centre_m: (f32, f32),
    /// The dab's own place on the ring, in radians.
    pub angle_rad: f32,
    /// How big the dab is, in metres.
    pub radius_m: f32,
    /// How old the dab is, in seconds.
    pub age_s: f32,
    /// The paint of the hit.
    pub paint: lm_render::look::Rgb,
}

impl Splat {
    /// How far through the burst the dab is, from 0 to 1.
    fn burst(&self, hit: &HitLook) -> f32 {
        (self.age_s / hit.burst_s).clamp(0.0, 1.0)
    }

    /// How far through the fall the dab is, from 0 to 1.
    fn fall(&self, hit: &HitLook) -> f32 {
        let span = (hit.life_s - hit.burst_s).max(f32::EPSILON);
        ((self.age_s - hit.burst_s) / span).clamp(0.0, 1.0)
    }

    /// Whether the dab is still in the air.
    fn airborne(&self, hit: &HitLook) -> bool {
        self.age_s < hit.burst_s
    }

    /// Where the dab is now, in metres.
    ///
    /// The burst is a ring around the piece that was hit: it flies out
    /// from it, so the hit is seen from every angle the camera can be at
    /// and the piece that was hit is never covered by its own paint.
    /// Then the paint falls, and gathers into a ring of stains on the
    /// ground around it.
    pub fn pos(&self, hit: &HitLook) -> (f32, f32) {
        let (sin, cos) = self.angle_rad.sin_cos();
        let radius = if self.airborne(hit) {
            // The ring flies out as the burst grows.
            hit.burst_radius_m * (1.0 + 0.5 * self.burst(hit))
        } else {
            // And the paint gathers as it falls.
            let fall = self.fall(hit);
            let out = hit.burst_radius_m * 1.5;
            out + (hit.land_m - out) * fall
        };
        (
            self.centre_m.0 + cos * radius,
            self.centre_m.1 + sin * radius,
        )
    }

    /// How high off the ground the dab is, in metres.
    ///
    /// The paint rises as it flies out and falls as it settles: a burst
    /// that hangs around the piece's head is seen over the board, and a
    /// stain that lies on the ground is seen beside it.
    pub fn height(&self, hit: &HitLook) -> f32 {
        if self.airborne(hit) {
            hit.burst_height_m * (0.4 + 0.6 * self.burst(hit))
        } else {
            hit.burst_height_m * (1.0 - self.fall(hit)).powi(2)
        }
    }

    /// How big the dab is now, in metres.
    pub fn radius(&self, hit: &HitLook) -> f32 {
        let scale = if self.airborne(hit) {
            0.55 + 0.45 * self.burst(hit)
        } else {
            1.0 - 0.25 * self.fall(hit)
        };
        self.radius_m * scale
    }

    /// How much of the paint is left, from 1 when it lands to 0 when it is
    /// dry.
    pub fn opacity(&self, hit: &HitLook) -> f32 {
        if self.airborne(hit) {
            0.95
        } else {
            0.95 * (1.0 - self.fall(hit))
        }
    }

    /// How flat the dab is: a blob of paint in the air is round, and the
    /// stain it leaves on the ground is flat.
    pub fn flatten(&self, hit: &HitLook) -> f32 {
        if self.airborne(hit) {
            1.0
        } else {
            1.0 - 0.8 * self.fall(hit)
        }
    }
}

/// The effects of this round: the hits and the path of the watched agent.
#[derive(Resource, Default, Debug)]
pub struct Effects {
    /// The path of the watched agent, oldest first.
    pub trail: VecDeque<(f32, f32)>,
    /// The agent whose path is being kept.
    pub followed: Option<u16>,
    /// How many of the round's events have been read, so that a tag is not
    /// seen twice.
    ///
    /// The count, not the tick: a tick can carry more than one event, and a
    /// tag that is not the first event of its tick is a tag that happened.
    pub seen_events: usize,
    /// Who hit each stunned agent: the paint of the hit, so that the stun
    /// pips are in the colour of the paint that put them there.
    pub stun_paint: BTreeMap<u16, lm_render::look::Rgb>,
    /// The dabs of paint that are on the board right now.
    pub splats: Vec<Splat>,
}

impl Effects {
    /// Note the tags that have happened since the last frame.
    ///
    /// A tag is a moment, not a state: it happens once, and the splat is
    /// the only thing that remembers it. The event says who hit whom; the
    /// place is the target's own, because a stunned agent is stunned where
    /// it was hit.
    ///
    /// The paint of a hit is the paint of the **squad** that threw it, and
    /// a squad is not an agent: the round's roster says which squad an
    /// agent belongs to, and `squad_of` is how the host asks.
    pub fn note_tags(
        &mut self,
        events: &[(u64, Event)],
        look: &Look,
        squad_of: impl Fn(u16) -> Option<u8>,
    ) -> Vec<Tag> {
        let mut tags = Vec::new();
        // The events are kept for the whole round, so the ones already read
        // are skipped — by their place in the list, which is stable because
        // the list only grows. (A list that shrank is a new round, and is
        // read from the start.)
        let start = self.seen_events.min(events.len());
        for (tick, event) in &events[start..] {
            let Event::Tagged { by, target } = event else {
                continue;
            };
            let paint = look.squad_color(squad_of(*by).unwrap_or(0));
            self.stun_paint.insert(*target, paint);
            tags.push(Tag {
                tick: *tick,
                by: *by,
                target: *target,
                paint,
            });
        }
        self.seen_events = events.len();
        tags
    }

    /// Put a hit on the board: the burst of paint around the piece that was
    /// hit, and the stain it settles into.
    ///
    /// The paint is thrown in a ring around the piece, so the hit is seen
    /// from every angle the camera can be at, and the piece that was hit
    /// never stands on its own paint and hides it.
    ///
    /// The recipe is the look's, so the same hit always looks the same.
    pub fn splat(&mut self, centre_m: (f32, f32), paint: lm_render::look::Rgb, hit: &HitLook) {
        let dabs = hit.dabs.max(1);
        for index in 0..dabs {
            let angle = index as f32 / dabs as f32 * std::f32::consts::TAU;
            self.splats.push(Splat {
                centre_m,
                angle_rad: angle,
                radius_m: hit.dab_radius_m,
                age_s: 0.0,
                paint,
            });
        }
    }

    /// Age the dabs of paint, and forget the ones that have dried.
    ///
    /// The paint spreads as it dries, and it fades as it goes: the board is
    /// clean again by the time the next thing happens.
    pub fn age(&mut self, dt_s: f32, hit: &HitLook) {
        for splat in &mut self.splats {
            splat.age_s += dt_s;
        }
        self.splats.retain(|splat| splat.age_s < hit.life_s);
    }

    /// The path of the agent being watched.
    ///
    /// The trail is the host's own: it watches where the agent is, frame by
    /// frame, and the round is never asked to remember.
    pub fn follow(&mut self, agent: Option<u16>, pos_m: Option<(f32, f32)>) {
        if self.followed != agent {
            self.followed = agent;
            self.trail.clear();
        }
        let Some((x, y)) = pos_m else {
            return;
        };
        // A point that has not moved is not a step on the path.
        if self
            .trail
            .back()
            .is_some_and(|&(lx, ly)| (lx - x).abs() < 1e-3 && (ly - y).abs() < 1e-3)
        {
            return;
        }
        self.trail.push_back((x, y));
        while self.trail.len() > TRAIL_POINTS {
            self.trail.pop_front();
        }
    }

    /// The paint that put an agent out of the round, if it is known.
    pub fn stun_paint_of(&self, agent: u16) -> Option<lm_render::look::Rgb> {
        self.stun_paint.get(&agent).copied()
    }

    /// Forget the paint of an agent that is no longer stunned.
    pub fn forget(&mut self, agent: u16) {
        self.stun_paint.remove(&agent);
    }
}

/// The plugin that keeps the effects up to date.
///
/// It is added to every view, because the effects belong to the round and
/// not to a view: the 2D map and the 3D view read the same splats, and
/// only one of them may *note* a tag — a tag that was seen twice would
/// paint the board twice.
pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Effects>()
            .add_systems(Update, (note_tags, age_splats).chain());
    }
}

/// Note the hits of the round, and put them on the board.
fn note_tags(
    mut effects: ResMut<Effects>,
    events: Res<LiveEvents>,
    look: Res<Look>,
    round: Res<crate::sim_plugin::Round>,
) {
    let Some(sim) = round.sim() else {
        return;
    };
    // The paint of a hit is the paint of the squad that threw it: the
    // round's roster says which squad an agent belongs to.
    let tags = effects.note_tags(&events.0, &look, |id| {
        sim.agent(id).map(|agent| agent.squad)
    });
    for tag in tags {
        // The place the hit fell is the place the target was standing: a
        // stunned agent is stunned where it was hit. The paint is thrown
        // around it, not under it.
        let Some(pos) = lm_render::scene::agent_pos_m(sim, tag.target) else {
            continue;
        };
        effects.splat(pos, tag.paint, &look.0.hit);
    }
}

/// Age the dabs of paint, so they dry and go.
fn age_splats(mut effects: ResMut<Effects>, time: Res<Time>, look: Res<Look>) {
    effects.age(time.delta_secs(), &look.0.hit);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A look with the default tokens, for the squad colours.
    fn look() -> Look {
        Look::default()
    }

    #[test]
    fn a_tag_is_seen_even_when_it_is_not_the_first_event_of_its_tick() {
        // A tick can carry more than one event, and a tag that is not the
        // first of them is still a tag: it happened, and it is the moment
        // the board is painted.
        let mut effects = Effects::default();
        let events = vec![
            (7u64, Event::Respawned(3)),
            (7u64, Event::Tagged { by: 1, target: 2 }),
        ];
        let tags = effects.note_tags(&events, &look(), |_| Some(0));
        assert_eq!(tags.len(), 1, "the tag was seen");
        assert_eq!(tags[0].target, 2);
        assert_eq!(tags[0].by, 1);
    }

    #[test]
    fn a_tag_is_not_seen_twice() {
        let mut effects = Effects::default();
        let events = vec![(7u64, Event::Tagged { by: 1, target: 2 })];
        assert_eq!(effects.note_tags(&events, &look(), |_| Some(0)).len(), 1);
        // The next frame sees the same list, and the tag again.
        assert_eq!(effects.note_tags(&events, &look(), |_| Some(0)).len(), 0);
        // And a later tick's tag is seen.
        let later = vec![
            (7u64, Event::Tagged { by: 1, target: 2 }),
            (9u64, Event::Tagged { by: 0, target: 4 }),
        ];
        let tags = effects.note_tags(&later, &look(), |_| Some(0));
        assert_eq!(tags.len(), 1, "only the new tag");
        assert_eq!(tags[0].target, 4);
    }

    #[test]
    fn two_tags_on_one_tick_are_both_seen() {
        let mut effects = Effects::default();
        let events = vec![
            (7u64, Event::Tagged { by: 0, target: 4 }),
            (7u64, Event::Tagged { by: 1, target: 5 }),
        ];
        let tags = effects.note_tags(&events, &look(), |_| Some(0));
        assert_eq!(tags.len(), 2, "both hits happened");
    }

    #[test]
    fn the_paint_of_a_hit_is_the_paint_of_the_squad_that_threw_it() {
        let mut effects = Effects::default();
        // Agent 4 is in squad 1 — the roster numbers the agents by squad,
        // so an agent's number is not a squad. The paint has to come from
        // the roster, or a hit by the second squad is painted in the
        // first squad's colour.
        let events = vec![(7u64, Event::Tagged { by: 4, target: 2 })];
        effects.note_tags(&events, &look(), |id| match id {
            4 => Some(1),
            _ => Some(0),
        });
        let paint = effects.stun_paint_of(2).expect("the hit is remembered");
        assert_eq!(paint, look().squad_color(1));
        assert_ne!(
            paint,
            look().squad_color(0),
            "and not the paint of the squad the agent's number would name"
        );
    }

    #[test]
    fn a_splat_bursts_around_the_target_and_settles_beside_it() {
        let mut effects = Effects::default();
        let hit = look().0.hit;
        effects.splat((10.0, 20.0), look().squad_color(0), &hit);
        assert_eq!(
            effects.splats.len(),
            hit.dabs as usize,
            "a hit is a ring of dabs"
        );

        // The burst is around the piece that was hit — not on it: the paint
        // erupts at the burst's radius, so the piece is never covered by
        // its own paint.
        let fresh = effects.splats[0].pos(&hit);
        let distance = ((fresh.0 - 10.0).powi(2) + (fresh.1 - 20.0).powi(2)).sqrt();
        assert!(
            (distance - hit.burst_radius_m).abs() < 1e-3,
            "the paint erupts around the target, at {distance} m"
        );

        // It is in the air while it bursts, so it is seen over the board.
        assert!(
            effects.splats[0].height(&hit) > 0.3,
            "the burst hangs in the air"
        );

        // And it settles: the paint falls to the ground and gathers into a
        // ring of stains around the piece, not a puddle under it.
        effects.age(hit.life_s * 0.99, &hit);
        let settled = effects.splats[0].pos(&hit);
        let distance = ((settled.0 - 10.0).powi(2) + (settled.1 - 20.0).powi(2)).sqrt();
        assert!(
            (distance - hit.land_m).abs() < 0.02,
            "the paint settles beside the target, at {distance} m"
        );
        assert!(
            effects.splats[0].height(&hit) < 0.2,
            "and it is on the ground"
        );
        assert!(
            effects.splats[0].flatten(&hit) < 0.5,
            "and it is a stain, not a blob"
        );

        // The board is clean again by the time the next thing happens.
        effects.age(hit.life_s, &hit);
        assert!(effects.splats.is_empty(), "the board is clean");
    }
}

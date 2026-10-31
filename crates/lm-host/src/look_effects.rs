//! The tag moment: what a hit looks like, and the path of the agent you
//! watch.
//!
//! Three things the renderer adds on top of the round, and none of them
//! change it:
//!
//! * the **splat** — a burst of paint where an agent was hit, in the paint
//!   of the squad that hit them. It is short-lived: a quarter of a second
//!   of paint in the air, and then the board is clean again. The persistent
//!   layer of paint on the ground is another job's; this is the hit.
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

use crate::look::Look;
use crate::sim_plugin::LiveEvents;

/// How long a splat lives, in seconds.
///
/// Long enough to be seen at a glance, short enough that the board is
/// clean by the time the next thing happens.
pub const SPLAT_LIFE_S: f32 = 0.55;

/// How long the trail is kept, in points.
pub const TRAIL_POINTS: usize = 48;

/// The blobs a splat is made of: where each one sits and how big it is.
///
/// The recipe is fixed, so the same hit always looks the same: a dab in
/// the middle and four smaller ones around it, the way paint lands.
pub const SPLAT_BLOBS: [(f32, f32, f32); 5] = [
    (0.0, 0.0, 0.55),
    (0.42, 0.18, 0.26),
    (-0.35, 0.28, 0.22),
    (0.12, -0.44, 0.20),
    (-0.44, -0.22, 0.18),
];

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

/// A dab of paint: where a hit landed, and how old it is.
///
/// The dab knows its own age, so it can spread and fade on its own, and
/// it knows which paint it is, so the hit can be forgotten when it has
/// faded and the dab with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splat {
    /// Where the dab sits, in the world's metres.
    pub pos_m: (f32, f32),
    /// How big the dab is when it lands, in metres.
    pub radius_m: f32,
    /// How old the dab is, in seconds.
    pub age_s: f32,
    /// The paint of the hit.
    pub paint: lm_render::look::Rgb,
}

impl Splat {
    /// How big the dab is now, in metres.
    ///
    /// The paint spreads as it dries: a dab lands small and grows as it
    /// fades. Both views read this number, so the paint looks the same in
    /// the map and in the diorama.
    pub fn radius(&self) -> f32 {
        self.radius_m * (0.35 + 0.75 * self.progress())
    }

    /// How much of the paint is left, from 1 when it lands to 0 when it is
    /// dry.
    pub fn opacity(&self) -> f32 {
        (1.0 - self.progress()) * 0.85
    }

    /// How far through its life the dab is, from 0 to 1.
    fn progress(&self) -> f32 {
        (self.age_s / SPLAT_LIFE_S).clamp(0.0, 1.0)
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
    pub fn note_tags(&mut self, events: &[(u64, Event)], look: &Look) -> Vec<Tag> {
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
            let paint = look.squad_color(u8::try_from(*by).unwrap_or(0));
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

    /// Put a hit on the board: the dabs of paint where it landed.
    ///
    /// The dabs are spread around the place the hit fell, in the paint of
    /// the squad that threw it. The recipe is fixed, so the same hit always
    /// looks the same.
    pub fn splat(&mut self, pos_m: (f32, f32), paint: lm_render::look::Rgb) {
        for (dx, dy, radius) in SPLAT_BLOBS {
            self.splats.push(Splat {
                pos_m: (pos_m.0 + dx, pos_m.1 + dy),
                radius_m: radius,
                age_s: 0.0,
                paint,
            });
        }
    }

    /// Age the dabs of paint, and forget the ones that have dried.
    ///
    /// The paint spreads as it dries, and it fades as it goes: the board is
    /// clean again by the time the next thing happens.
    pub fn age(&mut self, dt_s: f32) {
        for splat in &mut self.splats {
            splat.age_s += dt_s;
        }
        self.splats.retain(|splat| splat.age_s < SPLAT_LIFE_S);
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
    let tags = effects.note_tags(&events.0, &look);
    let Some(sim) = round.sim() else {
        return;
    };
    for tag in tags {
        // The place the hit fell is the place the target was standing: a
        // stunned agent is stunned where it was hit.
        let Some(pos) = lm_render::scene::agent_pos_m(sim, tag.target) else {
            continue;
        };
        effects.splat(pos, tag.paint);
    }
}

/// Age the dabs of paint, so they dry and go.
fn age_splats(mut effects: ResMut<Effects>, time: Res<Time>) {
    effects.age(time.delta_secs());
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
        let tags = effects.note_tags(&events, &look());
        assert_eq!(tags.len(), 1, "the tag was seen");
        assert_eq!(tags[0].target, 2);
        assert_eq!(tags[0].by, 1);
    }

    #[test]
    fn a_tag_is_not_seen_twice() {
        let mut effects = Effects::default();
        let events = vec![(7u64, Event::Tagged { by: 1, target: 2 })];
        assert_eq!(effects.note_tags(&events, &look()).len(), 1);
        // The next frame sees the same list, and the tag again.
        assert_eq!(effects.note_tags(&events, &look()).len(), 0);
        // And a later tick's tag is seen.
        let later = vec![
            (7u64, Event::Tagged { by: 1, target: 2 }),
            (9u64, Event::Tagged { by: 0, target: 4 }),
        ];
        let tags = effects.note_tags(&later, &look());
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
        let tags = effects.note_tags(&events, &look());
        assert_eq!(tags.len(), 2, "both hits happened");
    }

    #[test]
    fn the_paint_of_a_hit_is_the_paint_of_the_squad_that_threw_it() {
        let mut effects = Effects::default();
        let events = vec![(7u64, Event::Tagged { by: 1, target: 2 })];
        effects.note_tags(&events, &look());
        let paint = effects.stun_paint_of(2).expect("the hit is remembered");
        assert_eq!(paint, look().squad_color(1));
    }

    #[test]
    fn a_splat_spreads_and_goes() {
        let mut effects = Effects::default();
        effects.splat((10.0, 20.0), look().squad_color(0));
        assert_eq!(effects.splats.len(), 5, "a hit is five dabs");
        // Fresh paint is small; it spreads as it dries.
        let fresh = effects.splats[0].radius();
        effects.age(SPLAT_LIFE_S / 2.0);
        assert!(effects.splats[0].radius() > fresh, "the paint spreads");
        assert!(effects.splats[0].opacity() < 0.85, "and it fades");
        assert_eq!(effects.splats.len(), 5, "and it is still on the board");
        effects.age(SPLAT_LIFE_S);
        assert!(effects.splats.is_empty(), "and then the board is clean");
    }
}

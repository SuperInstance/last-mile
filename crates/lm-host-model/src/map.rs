//! The map draw model: the world as the viewer is allowed to see it, in
//! metres, between ticks (phase 2 Task 4).
//!
//! The map model is everything the renderer knows about the round. It is
//! built once per frame from the sim as it stands after the last tick, and
//! it answers three questions for everything it draws: *where* it is, in
//! `f32` metres — the one place besides `lm-host` where positions become
//! floats, and they never travel back (D8); *what the viewer knows* — the
//! fog; and *where between two ticks* it appears — the interpolation.
//!
//! ## Fog
//!
//! Fog follows the viewer (spec §4, §6). A **spectator** sees the round as
//! the authority does: every cell [`FogState::Visible`], every agent drawn.
//! A **possessing** player sees it through the squad's eyes, exactly as
//! [`Sim::view`] reports it: teammates are always drawn (they report in),
//! and an enemy only while the squad's memory of it is a sighting from the
//! very tick of the view — `age_ticks == 0`. The cells the view vouches
//! for (a teammate standing in it, a beacon, a remembered sighting) read
//! [`FogState::Explored`]; the rest reads [`FogState::Unknown`]. No cell
//! under possession reads [`FogState::Visible`]: between ticks the map
//! only remembers, it does not see.
//!
//! The squad's explored-cell memory itself is the sim's private
//! bookkeeping — phase 1 hashes it into the state but hands out no
//! accessor — so `Explored` here marks the cells a view vouches for. The
//! beacons are pinned in the header, symmetric for both squads, and are
//! therefore the round's common knowledge: every viewer draws every
//! beacon. Note also what that freshest-sighting rule means in practice:
//! through `Sim::view`, taken between ticks, a memory recorded on the tick
//! that just ran reads age 1, so enemies surface to a possessing player
//! only in a view taken on the tick of the sighting itself, the way the
//! sim takes its own views inside a step. Live sight would need the core
//! to hand it out, which phase 2 does not ask of it.
//!
//! ## Interpolation
//!
//! The sim runs in `FixedUpdate` at `tick_hz` while frames render at their
//! own rate, so a frame usually falls between two ticks. [`MapModel::build`]
//! takes the positions as they were before the last tick (the host loop's
//! [`crate::host_loop::HostLoop::prev_positions`]) and the fixed-timestep
//! overstep fraction `alpha` — 0 draws the previous tick, 1 the last one —
//! and draws every agent lerped between the two. An agent with no previous
//! position (the first frame) draws where it stands.

use lm_core::decision::Tick;
use lm_core::fixed::{Angle, Pos, ANGLE_TURN, UNITS_PER_M};
use lm_core::header::Role;
use lm_core::sim::Sim;
use lm_core::utterance::{Channel, Heard};
use lm_core::world::{CellKind, CellXY};
// The viewer is Task 3's `input::Viewer` (the mirror used while the tasks ran
// in parallel was removed at merge, so the crate has exactly one Viewer).
pub use crate::input::Viewer;

/// How well the viewer knows one cell of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogState {
    /// In sight right now. Only a spectator's cells: a spectator sees the
    /// round as the authority does.
    Visible,
    /// Seen before and remembered: the terrain shows, the present does not.
    Explored,
    /// Never seen.
    Unknown,
}

/// One agent as the map draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentDraw {
    /// Which agent this is.
    pub id: u16,
    /// Which squad it belongs to.
    pub squad: u8,
    /// Where it stands, in metres: its position lerped between the previous
    /// tick and the last one by `alpha`.
    pub pos_m: (f32, f32),
    /// Which way it faces, in radians (a full turn is `2π`).
    pub facing_rad: f32,
    /// Whether it is stunned on the tick being drawn.
    pub stunned: bool,
    /// Whether it leads its squad.
    pub leader: bool,
    /// Whether the viewer sees it now. Everything the model draws is seen —
    /// teammates report in, a fresh sighting is in sight, a spectator sees
    /// the lot — so the renderer never has to re-derive it.
    pub visible: bool,
}

/// One beacon as the map draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeaconDraw {
    /// The cell the beacon stands in.
    pub cell: CellXY,
    /// The squad holding it, if any.
    pub holder: Option<u8>,
    /// The centre of that cell, in metres. A beacon never moves, so it does
    /// not interpolate.
    pub pos_m: (f32, f32),
}

/// One spoken word, as the map draws it: a tick at the speaker.
///
/// The map shows the talk as it is spoken: one tick per word, so the length
/// of a line is the length of its ticks and a squad that is talking is a
/// squad whose agents are ticking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeechTick {
    /// The agent who said the word.
    pub speaker: u16,
    /// The word's number in the line.
    pub index: u16,
    /// The tick of the round the word starts on.
    pub at: Tick,
    /// Where the speaker stands, in metres.
    pub pos_m: (f32, f32),
    /// The channel the line was spoken on.
    pub channel: Channel,
}

/// Everything one frame of the map draws.
#[derive(Clone, Debug, PartialEq)]
pub struct MapModel {
    /// Every cell of the grid, row-major, with its kind and its fog.
    pub cells: Vec<(CellXY, CellKind, FogState)>,
    /// The agents the viewer may see, in agent-id order.
    pub agents: Vec<AgentDraw>,
    /// The round's beacons, in world order.
    pub beacons: Vec<BeaconDraw>,
    /// The words being spoken right now, one tick per word.
    pub speech: Vec<SpeechTick>,
    /// The tick of the round the map is drawn at.
    ///
    /// It is what says whether a word has been said yet: a tick that is
    /// still in the future is a word the speaker has not reached.
    pub now_tick: u64,
    /// The world's extents in metres: (width, height).
    pub world_m: (f32, f32),
}

/// A position lerped from `prev` to `cur` by `alpha`, in metres.
///
/// The lerp runs in `f64` over the unit counts — the grid is powers of two,
/// and its exact midpoints must stay exact — and only the result becomes
/// `f32`: floats exist here to be drawn, never to be measured.
pub fn pos_m(prev: Pos, cur: Pos, alpha: f32) -> (f32, f32) {
    let (px, py) = prev.to_units();
    let (cx, cy) = cur.to_units();
    let a = f64::from(alpha);
    let m = |from: i64, to: i64| {
        let units = (from as f64) + ((to as f64) - (from as f64)) * a;
        (units / f64::from(UNITS_PER_M)) as f32
    };
    (m(px, cx), m(py, cy))
}

/// An angle as radians: core counts 1/65536 of a turn, the renderer wants
/// the turn itself.
fn facing_rad(facing: Angle) -> f32 {
    (f64::from(facing.0) * std::f64::consts::TAU / f64::from(ANGLE_TURN)) as f32
}

impl MapModel {
    /// Build the map one frame draws.
    ///
    /// `sim` is the round as it stands after its last tick; `prev` holds
    /// where every agent stood before that tick (the host loop's
    /// [`crate::host_loop::HostLoop::prev_positions`]); `alpha` is the
    /// fixed-timestep overstep fraction, with 0 drawing the previous tick
    /// and 1 the last one.
    pub fn build(sim: &Sim, prev: &[(u16, Pos)], viewer: &Viewer, alpha: f32) -> MapModel {
        let world = sim.world();
        let grid = &world.grid;
        let now = sim.tick();

        // What the viewer may know decides both lists: the agents on the
        // map and the fog over the cells. A spectator gets the authority's
        // round; a possessing player gets the squad's view of it, exactly
        // as `Sim::view` reports it.
        let view = match viewer {
            Viewer::Spectator { .. } => None,
            Viewer::Possessing(agent) => Some(sim.view(*agent)),
        };

        // A spectator sees everybody, in id order. A possessing player sees
        // the squad (it reports in, `me` included) and — of the enemies —
        // only a sighting from the very tick of the view.
        let mut drawn: Vec<u16> = match &view {
            None => sim.agent_ids(),
            Some(v) => {
                let mut ids: Vec<u16> = v.squad.iter().map(|s| s.id).collect();
                ids.extend(v.enemies.iter().filter(|e| e.age_ticks == 0).map(|e| e.id));
                ids
            }
        };
        drawn.sort_unstable();
        drawn.dedup();

        let mut cells: Vec<(CellXY, CellKind, FogState)> =
            Vec::with_capacity(usize::from(grid.w) * usize::from(grid.h));
        for y in 0..grid.h {
            for x in 0..grid.w {
                let c = CellXY { x, y };
                // Under possession a cell is Explored when the view vouches
                // for it: a teammate stands in it, it holds a beacon (the
                // beacons are common knowledge), or an enemy was sighted
                // in it. Everything else is Unknown; nothing is Visible.
                let fog = match &view {
                    None => FogState::Visible,
                    Some(v) => {
                        let vouched = v.squad.iter().any(|s| s.cell == c)
                            || v.beacons.iter().any(|b| b.cell == c)
                            || v.enemies.iter().any(|e| e.cell == c);
                        if vouched {
                            FogState::Explored
                        } else {
                            FogState::Unknown
                        }
                    }
                };
                cells.push((c, grid.get(c), fog));
            }
        }

        // The bodies come from the sim itself — the view only says who may
        // be drawn — and each is lerped from where the host last saw it.
        let agents: Vec<AgentDraw> = drawn
            .iter()
            .filter_map(|&id| {
                let agent = sim.agent(id)?;
                // An agent with no previous position (the first frame, or a
                // viewer whose memory predates the agent) draws at `cur`.
                let from = prev
                    .iter()
                    .find(|&&(p, _)| p == id)
                    .map_or(agent.pos, |&(_, p)| p);
                Some(AgentDraw {
                    id: agent.id,
                    squad: agent.squad,
                    pos_m: pos_m(from, agent.pos, alpha),
                    facing_rad: facing_rad(agent.facing),
                    stunned: agent.stunned_until.is_some_and(|t| now < t),
                    leader: agent.role == Role::Leader,
                    visible: true,
                })
            })
            .collect();

        // The beacons and their holders come from a view too — any view;
        // they are squad-level facts of the round, not of any one squad's
        // knowledge. A round with no agents has no view to ask and draws
        // no beacons.
        let snaps = match &view {
            Some(v) => v.beacons.clone(),
            None => sim
                .agent_ids()
                .first()
                .map_or_else(Vec::new, |&id| sim.view(id).beacons),
        };
        let beacons: Vec<BeaconDraw> = snaps
            .iter()
            .map(|b| {
                let centre = grid.center(b.cell, world.cell_units);
                BeaconDraw {
                    cell: b.cell,
                    holder: b.holder,
                    pos_m: pos_m(centre, centre, alpha),
                }
            })
            .collect();

        let metres =
            |cells_wide: u16| f32::from(cells_wide) * world.cell_units as f32 / UNITS_PER_M as f32;
        let speech = speech_ticks(sim, &view, &agents, alpha);
        MapModel {
            cells,
            agents,
            beacons,
            speech,
            now_tick: now,
            world_m: (metres(grid.w), metres(grid.h)),
        }
    }
}

/// The words being spoken right now, one tick per word.
///
/// A possessing player hears what their agent heard, so the map shows the
/// words that reached that agent. A spectator hears the round as the
/// authority does, so the map shows every line that is being spoken — once
/// per line, with the words that arrived.
fn speech_ticks(
    sim: &Sim,
    view: &Option<lm_core::view::ScoreView>,
    agents: &[AgentDraw],
    alpha: f32,
) -> Vec<SpeechTick> {
    let lines: Vec<Heard> = match view {
        Some(v) => v
            .heard
            .iter()
            .filter(|line| line.ongoing)
            .cloned()
            .collect(),
        None => sim.audible_lines(),
    };

    let mut ticks = Vec::new();
    for line in lines {
        // The speaker's place on the map: where it is drawn, so the ticks
        // are on the body that is talking.
        let pos_m = agents
            .iter()
            .find(|draw| draw.id == line.speaker)
            .map_or_else(
                || {
                    let pos = sim.agent(line.speaker).map_or_else(Pos::zero, |a| a.pos);
                    pos_m(pos, pos, alpha)
                },
                |draw| draw.pos_m,
            );
        for (index, word) in line.words.iter().enumerate() {
            ticks.push(SpeechTick {
                speaker: line.speaker,
                index: u16::try_from(index).unwrap_or(u16::MAX),
                // A heard word sits on the round's timeline: the degradation
                // has already moved it off the line's own, so the tick of
                // the round is the word's own `t_on`.
                at: word.t_on,
                pos_m,
                channel: line.channel,
            });
        }
    }
    ticks
}

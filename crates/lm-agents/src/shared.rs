//! The shared target: the one objective both squads are sent to.
//!
//! The doctrine's target is set by the facts builder, not by the harness
//! (crates/lm-agents/tests/engagement.md): the builder names "the nearest
//! beacon your squad does not hold", and on a point-symmetric map that
//! target is mirrored, so squad 0 walks to beacon 2 and squad 1 walks to
//! beacon 3 and the two never meet.
//!
//! This module is the deeper lever that research asks for: a **shared**
//! fact, one both squads read off the same field, so both walk to the same
//! beacon and meet there. It is a harness parameter (`facts.toml`, off by
//! default) and it changes only what the facts say.
//!
//! ## The rules
//!
//! | rule | the shared target | how it is stated |
//! |---|---|---|
//! | `off` | none | the shipped facts, unchanged (the default) |
//! | `free` | the lowest-numbered beacon **no squad holds** | a named target |
//! | `centre` | the **most central** beacon | a named target |
//! | `ghost` | the **hot beacon**: the one the attractor sits on now | a **field**: an attractor with a strength, a radius and a falloff |
//!
//! Every rule reads only what every agent can see — the beacons, who holds
//! them, and where they are — so the target is the same for both squads by
//! construction. The `ghost` rule adds one thing that is *not* shared: the
//! pull an agent feels where it stands, which is a field over the grid.
//!
//! ## The ghost field
//!
//! The field follows the ghost-tiles model (SuperInstance/ghost-tiles-1):
//! invisible tiles that influence the visible state. A tile has a position,
//! a kind (an attractor pulls, a repulsor pushes), a phase, a strength, a
//! radius and a falloff; the phase decides how the strength changes over
//! the round:
//!
//! - **permanent** — the strength is constant;
//! - **temporal** — it fades to nothing over `max_age_ticks`;
//! - **recurring** — it pulses, strengthening and weakening on a cycle.
//!
//! The field is a **hot zone**: the attractor sits on one beacon at a time
//! and walks to the next one every `rotation_ticks` ticks, so the place
//! worth fighting over moves as the round goes on. The hot beacon is a
//! function of the tick and of the beacon list, and both squads see the
//! same tick and the same list, so both are pulled toward the same place
//! at the same time.
//!
//! Every number here is an integer: the strength and the pull are Q14
//! (16384 is 1.0), the distance is Chebyshev (the grid's own metric), and
//! the falloff is integer arithmetic. The same situation always yields the
//! same field, so the facts are as deterministic as the rest of the
//! builder (D49).

use lm_core::view::ScoreView;
use lm_core::world::CellXY;
use serde::{Deserialize, Serialize};

/// Q14 scale: 16384 is 1.0.
pub const Q14_ONE: i64 = 16_384;

/// Which shared-target rule the facts builder states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SharedRule {
    /// The shipped facts: no shared target is stated (the default).
    #[default]
    Off,
    /// The lowest-numbered beacon no squad holds.
    Free,
    /// The most central beacon: the one nearest the middle of the
    /// beacons, which on the round's symmetric map is the one at the
    /// centre of the map.
    Centre,
    /// A ghost attractor field, hot on the beacon `free` names.
    Ghost,
}

impl SharedRule {
    /// The rule's name, as the harness file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            SharedRule::Off => "off",
            SharedRule::Free => "free",
            SharedRule::Centre => "centre",
            SharedRule::Ghost => "ghost",
        }
    }
}

/// The kind of a ghost tile: an attractor pulls, a repulsor pushes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GhostKind {
    /// The tile pulls towards itself.
    #[default]
    Attractor,
    /// The tile pushes away from itself.
    Repulsor,
}

/// The phase of a ghost tile: how its strength changes over the round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GhostPhase {
    /// The strength fades to nothing over `max_age_ticks`.
    Temporal,
    /// The strength never changes.
    #[default]
    Permanent,
    /// The strength pulses on a cycle.
    Recurring,
}

/// How a tile's influence falls off with distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Falloff {
    /// Linear: full strength at the centre, nothing at the radius.
    #[default]
    InverseLinear,
    /// Inverse square: full strength at the centre, fading fast.
    InverseSquare,
    /// Flat: the same strength everywhere inside the radius.
    Constant,
}

/// The ghost tile's own knobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GhostConfig {
    /// Whether the tile pulls or pushes.
    pub kind: GhostKind,
    /// The tile's strength in Q14 (16384 is 1.0).
    pub strength_q14: i64,
    /// How far the tile reaches, in cells.
    pub radius_cells: i32,
    /// How the influence falls off with distance.
    pub falloff: Falloff,
    /// How the strength changes over the round.
    pub phase: GhostPhase,
    /// The length of the recurring pulse, in ticks.
    pub cycle_ticks: u32,
    /// How long a temporal tile takes to fade, in ticks.
    pub max_age_ticks: u32,
    /// How long the hot zone stays on one beacon before it moves to the
    /// next, in ticks.
    ///
    /// Zero means it never moves: the hot beacon is the lowest-numbered
    /// one no squad holds, for the whole round.
    pub rotation_ticks: u32,
}

impl Default for GhostConfig {
    fn default() -> Self {
        Self {
            kind: GhostKind::Attractor,
            strength_q14: Q14_ONE,
            radius_cells: 96,
            falloff: Falloff::InverseLinear,
            phase: GhostPhase::Permanent,
            cycle_ticks: 900,
            max_age_ticks: 600,
            rotation_ticks: 600,
        }
    }
}

/// The harness parameter: which shared-target rule the facts state, and the
/// ghost tile's knobs.
///
/// It comes from the harness's `facts.toml`, which is optional: a harness
/// without one states no shared target, which is the shipped behaviour.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedConfig {
    /// Which rule the facts builder applies.
    pub rule: SharedRule,
    /// The ghost tile's knobs, read when the rule is `ghost`.
    pub ghost: GhostConfig,
}

/// The root of a `facts.toml`: the shared target lives under `[shared]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct FactsFile {
    shared: SharedConfig,
}

impl SharedConfig {
    /// Parse a `facts.toml`'s text. A missing file is the defaults (the
    /// rule is off), and a file that does not parse is an error the caller
    /// turns into a refusal — never a silent default (D26).
    ///
    /// The file's root is the facts file; the shared target lives under its
    /// `[shared]` table, so the file can carry other facts settings beside
    /// it later.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let file: FactsFile = toml::from_str(text).map_err(|err| err.to_string())?;
        Ok(file.shared)
    }

    /// Whether this configuration can be stated out loud.
    ///
    /// The ghost rule states an **attractor**: a shared target is a place
    /// both squads are sent to, and a repulsor is a place they are pushed
    /// away from. The field model knows both kinds — the ghost-tiles model
    /// has an attractor and a repulsor — but the target rule can only state
    /// one of them, and a repulsor under this rule is a configuration the
    /// facts cannot say out loud without contradicting themselves: the
    /// beacon would be named as the target and described as pushing away
    /// from it.
    ///
    /// So it is an error the round can see, not a silent agent that names a
    /// target it is pushed away from (D26).
    pub fn validate(&self) -> Result<(), String> {
        if self.rule == SharedRule::Ghost && self.ghost.kind == GhostKind::Repulsor {
            return Err(
                "the ghost's shared target is an attractor: a repulsor pushes both squads \
                 away from the beacon and names no target"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// The shared target for this situation, or `None` when the rule is off
    /// or the round has no beacon to name.
    ///
    /// The target is the same for every agent of every squad: it is read
    /// off the beacons and their holders, which every agent sees. The
    /// ghost rule adds the pull *at the agent's own cell*, which is the
    /// one part of the field that is not shared.
    pub fn resolve(&self, view: &ScoreView) -> Option<SharedTarget> {
        if self.rule == SharedRule::Off {
            return None;
        }
        let (beacon, reason) = match self.rule {
            SharedRule::Free => free_beacon(view),
            // The ghost's target is the beacon the attractor sits on, and
            // the attractor sits on the hot beacon. The facts state it the
            // way a field states it: a beacon the agent is pulled toward,
            // not a beacon nobody holds.
            SharedRule::Ghost => (
                hot_beacon(view, self.ghost.rotation_ticks),
                "the beacon you are pulled toward",
            ),
            SharedRule::Centre => central_beacon(view),
            SharedRule::Off => return None,
        };
        let cell = view.beacons.get(usize::from(beacon))?.cell;
        let distance = chebyshev(view.me.cell, cell);
        let (pull_q14, band) = match self.rule {
            SharedRule::Ghost => {
                let pull = self.field(cell).influence_at(view.me.cell, view.tick);
                (pull, PullBand::of(pull, self.ghost.strength_q14))
            }
            _ => (0, PullBand::None),
        };
        Some(SharedTarget {
            rule: self.rule,
            beacon,
            reason,
            cell,
            distance,
            pull_q14,
            band,
            radius_cells: self.ghost.radius_cells,
        })
    }

    /// The field of ghost tiles for this situation: one tile, on the shared
    /// beacon.
    pub fn field(&self, hot: CellXY) -> GhostField {
        GhostField {
            tiles: vec![GhostTile {
                position: hot,
                kind: self.ghost.kind,
                phase: self.ghost.phase,
                strength_q14: self.ghost.strength_q14,
                radius_cells: self.ghost.radius_cells,
                falloff: self.ghost.falloff,
                cycle_ticks: self.ghost.cycle_ticks,
                max_age: self.ghost.max_age_ticks,
            }],
        }
    }
}

/// One invisible tile that influences the visible state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostTile {
    /// Where the tile sits.
    pub position: CellXY,
    /// Whether it pulls or pushes.
    pub kind: GhostKind,
    /// How its strength changes over the round.
    pub phase: GhostPhase,
    /// The tile's strength, in Q14, before the phase is applied.
    pub strength_q14: i64,
    /// How far it reaches, in cells.
    pub radius_cells: i32,
    /// How the influence falls off with distance.
    pub falloff: Falloff,
    /// The length of the recurring pulse, in ticks.
    pub cycle_ticks: u32,
    /// How long a temporal tile takes to fade, in ticks.
    pub max_age: u32,
}

impl GhostTile {
    /// The tile's influence at `cell` on `tick`, in Q14: zero outside the
    /// radius.
    pub fn influence_at(&self, cell: CellXY, tick: u64) -> i64 {
        let distance = chebyshev(self.position, cell);
        if distance > self.radius_cells {
            return 0;
        }
        let strength = self.strength_at(tick);
        let magnitude = match self.falloff {
            Falloff::InverseLinear => {
                let radius = i64::from(self.radius_cells).max(1);
                strength * (radius - i64::from(distance)) / radius
            }
            Falloff::InverseSquare => {
                // The scale keeps the division in fixed point: 1024 is the
                // square of 32 cells, so the pull halves 32 cells out.
                const SCALE: i64 = 1_024;
                strength * SCALE / (SCALE + i64::from(distance) * i64::from(distance))
            }
            Falloff::Constant => strength,
        };
        match self.kind {
            GhostKind::Attractor => magnitude,
            GhostKind::Repulsor => -magnitude,
        }
    }

    /// The tile's strength on `tick`, in Q14, under its own phase.
    ///
    /// - permanent: constant;
    /// - temporal: fades linearly to nothing over `max_age` ticks;
    /// - recurring: a triangle between 0.5 and 1.0 over `cycle_ticks` ticks.
    pub fn strength_at(&self, tick: u64) -> i64 {
        match self.phase {
            GhostPhase::Permanent => self.strength_q14,
            GhostPhase::Temporal => {
                if self.max_age == 0 {
                    return 0;
                }
                let age = tick.min(u64::from(self.max_age));
                let left = u64::from(self.max_age) - age;
                self.strength_q14 * left as i64 / i64::from(self.max_age)
            }
            GhostPhase::Recurring => {
                let cycle = u64::from(self.cycle_ticks.max(1));
                let pos = tick % cycle;
                let half = cycle / 2;
                if half == 0 {
                    return self.strength_q14;
                }
                let multiplier = if pos <= half {
                    // Ramping up: 0.5 to 1.0.
                    Q14_ONE / 2 + (pos as i64 * (Q14_ONE / 2)) / half as i64
                } else {
                    // Ramping down: 1.0 to 0.5.
                    let down = cycle - half;
                    Q14_ONE - ((pos - half) as i64 * (Q14_ONE / 2)) / down as i64
                };
                self.strength_q14 * multiplier / Q14_ONE
            }
        }
    }
}

/// A field of ghost tiles: the influence of all of them, summed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhostField {
    /// The tiles of the field.
    pub tiles: Vec<GhostTile>,
}

impl GhostField {
    /// The field's influence at `cell` on `tick`, in Q14.
    pub fn influence_at(&self, cell: CellXY, tick: u64) -> i64 {
        self.tiles
            .iter()
            .map(|tile| tile.influence_at(cell, tick))
            .sum()
    }
}

/// How strongly a pull is felt where the agent stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullBand {
    /// No pull at all: the agent is outside the field's reach.
    None,
    /// A weak pull.
    Weak,
    /// A strong pull.
    Strong,
}

impl PullBand {
    /// The band a pull of `pull_q14` falls in, against a strength of
    /// `strength_q14`: a third of the strength is the line between weak and
    /// strong.
    fn of(pull_q14: i64, strength_q14: i64) -> PullBand {
        if pull_q14 <= 0 {
            PullBand::None
        } else if pull_q14 * 3 >= strength_q14 {
            PullBand::Strong
        } else {
            PullBand::Weak
        }
    }

    /// The word the facts use for the band.
    ///
    /// The band never says "no pull": the field names its target whatever
    /// the agent's distance to it, and the distance and the reach are
    /// stated as numbers beside it. A band that said "there is no pull"
    /// would read as a veto on the doctrine's third branch, which the
    /// field does not mean.
    pub fn as_str(self) -> &'static str {
        match self {
            PullBand::None => "faint",
            PullBand::Weak => "weak",
            PullBand::Strong => "strong",
        }
    }
}

/// The shared target, resolved for one situation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedTarget {
    /// Which rule named the target.
    pub rule: SharedRule,
    /// The beacon both squads are sent to.
    pub beacon: u8,
    /// Why that beacon is the shared target, in the facts' own words.
    pub reason: &'static str,
    /// Where that beacon is.
    pub cell: CellXY,
    /// How far the agent is from it, in cells.
    pub distance: i32,
    /// The pull at the agent's cell, in Q14 (zero unless the rule is ghost).
    pub pull_q14: i64,
    /// How strongly the pull is felt where the agent stands.
    pub band: PullBand,
    /// How far the ghost's field reaches, in cells.
    pub radius_cells: i32,
}

impl SharedTarget {
    /// The clause that marks a beacon as the shared target, in the facts'
    /// own words. The marker names the target for the beacon that is it,
    /// and says plainly that the others are not.
    pub fn marker(&self, beacon: u8) -> String {
        if beacon != self.beacon {
            return "; it is not the shared target".to_owned();
        }
        format!("; it is the shared target: {}", self.reason)
    }

    /// The sentences that state the target, once, above the beacons.
    ///
    /// The ghost rule adds the field's own reading: how hard the pull is
    /// where the agent stands, and how far away the target is.
    pub fn summary(&self) -> Vec<String> {
        let mut out = vec![format!(
            "The shared target is beacon {}: {}. Both squads are going for it.",
            self.beacon, self.reason
        )];
        if self.rule == SharedRule::Ghost {
            out.push(format!(
                "You are pulled toward beacon {}, and the pull is {} where you stand.",
                self.beacon,
                self.band.as_str()
            ));
            out.push(format!(
                "The pull reaches {} cells, and beacon {} is {} cells from you.",
                self.radius_cells, self.beacon, self.distance
            ));
        }
        out
    }
}

/// The lowest-numbered beacon no squad holds.
///
/// When every beacon is held there is no free one, and the shared target
/// falls back to the lowest-numbered beacon on the field: still the same
/// beacon for both squads, and still a beacon worth fighting over.
fn free_beacon(view: &ScoreView) -> (u8, &'static str) {
    match view
        .beacons
        .iter()
        .position(|beacon| beacon.holder.is_none())
    {
        Some(index) => (index as u8, "the beacon no squad holds"),
        None => (0, "the lowest-numbered beacon, since no beacon is free"),
    }
}

/// The hot beacon: the one the ghost's attractor sits on this tick.
///
/// The hot zone is on **one beacon at a time**. It walks the beacons no
/// squad holds, in beacon order, and it moves on every `rotation_ticks`
/// ticks; with `rotation_ticks` of zero it never moves and the hot beacon
/// is the lowest-numbered free one for the whole round.
///
/// The walk is a function of the tick and the beacon list, and both are
/// the same for both squads — the tick is the round's, and the beacon list
/// and its holders are what every agent sees. So both squads are pulled
/// toward the same hot beacon at the same time, which is what makes them
/// meet; and the hot beacon moves, so the meeting place moves too.
fn hot_beacon(view: &ScoreView, rotation_ticks: u32) -> u8 {
    if view.beacons.is_empty() {
        return 0;
    }
    // The beacons worth taking: the ones no squad holds. When they are all
    // held there is none left to walk, and the hot zone falls back to the
    // whole list, so it is never on nothing.
    let free: Vec<usize> = view
        .beacons
        .iter()
        .enumerate()
        .filter(|(_, beacon)| beacon.holder.is_none())
        .map(|(index, _)| index)
        .collect();
    let list = if free.is_empty() {
        (0..view.beacons.len()).collect()
    } else {
        free
    };
    if rotation_ticks == 0 {
        return list[0] as u8;
    }
    let step = (view.tick / u64::from(rotation_ticks)) as usize % list.len();
    list[step] as u8
}

/// The most central beacon: the one whose total distance to the others is
/// smallest, the lowest number breaking a tie.
///
/// It is the beacon nearest the middle of the beacons, which on the round's
/// symmetric map is the beacon at the centre of the map. It is computed
/// from the beacon list alone, so the fact builder and the harness's own
/// reflex — which sees the same list — name the same beacon.
fn central_beacon(view: &ScoreView) -> (u8, &'static str) {
    let mut best: Option<(u8, i64)> = None;
    for (index, beacon) in view.beacons.iter().enumerate() {
        let mut total = 0i64;
        for other in &view.beacons {
            total += i64::from(chebyshev(beacon.cell, other.cell));
        }
        if best.is_none_or(|(_, top)| total < top) {
            best = Some((index as u8, total));
        }
    }
    match best {
        Some((index, _)) => (index, "the most central of the beacons"),
        None => (0, "the most central of the beacons"),
    }
}

/// Chebyshev distance in cells — the metric of the 8-connected grid.
fn chebyshev(a: CellXY, b: CellXY) -> i32 {
    let dx = (i32::from(a.x) - i32::from(b.x)).abs();
    let dy = (i32::from(a.y) - i32::from(b.y)).abs();
    dx.max(dy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::header::Role;
    use lm_core::view::{AgentSnap, BeaconSnap};

    /// A view of two beacons: beacon 0 unheld, beacon 1 held by squad 1.
    fn view() -> ScoreView {
        ScoreView {
            paint: Vec::new(),
            heard: Vec::new(),
            tick: 100,
            me: AgentSnap {
                id: 0,
                cell: CellXY { x: 2, y: 2 },
                role: Role::Leader,
                stunned: false,
                airtime_ticks: 0,
            },
            squad: Vec::new(),
            enemies: Vec::new(),
            beacons: vec![
                BeaconSnap {
                    cell: CellXY { x: 10, y: 10 },
                    holder: None,
                },
                BeaconSnap {
                    cell: CellXY { x: 20, y: 20 },
                    holder: Some(1),
                },
            ],
            score: vec![0, 0],
            squad_id: 0,
        }
    }

    #[test]
    fn the_default_states_no_shared_target() {
        let cfg = SharedConfig::default();
        assert_eq!(cfg.rule, SharedRule::Off);
        assert!(cfg.resolve(&view()).is_none());
    }

    #[test]
    fn a_missing_file_is_the_defaults() {
        assert_eq!(
            SharedConfig::parse("").expect("empty loads"),
            SharedConfig::default()
        );
        assert_eq!(
            SharedConfig::parse("# nothing\n").expect("comments load"),
            SharedConfig::default()
        );
    }

    #[test]
    fn the_free_rule_names_the_lowest_unheld_beacon() {
        let cfg = SharedConfig {
            rule: SharedRule::Free,
            ..SharedConfig::default()
        };
        let target = cfg.resolve(&view()).expect("a target");
        assert_eq!(target.beacon, 0);
        assert!(target.marker(0).contains("the shared target"));
        assert!(target.marker(1).contains("not the shared target"));
    }

    #[test]
    fn the_target_is_the_same_for_both_squads() {
        let cfg = SharedConfig {
            rule: SharedRule::Free,
            ..SharedConfig::default()
        };
        let mut other = view();
        other.squad_id = 1;
        other.me.cell = CellXY { x: 38, y: 38 };
        let mine = cfg.resolve(&view()).expect("a target");
        let theirs = cfg.resolve(&other).expect("a target");
        assert_eq!(mine.beacon, theirs.beacon);
    }

    #[test]
    fn every_beacon_held_still_names_a_target() {
        let mut view = view();
        view.beacons[0].holder = Some(0);
        let cfg = SharedConfig {
            rule: SharedRule::Free,
            ..SharedConfig::default()
        };
        let target = cfg.resolve(&view).expect("a target");
        assert_eq!(target.beacon, 0);
        assert!(target.reason.contains("no beacon is free"));
    }

    #[test]
    fn the_centre_rule_names_the_most_central_beacon() {
        // Three beacons in a line: the middle one is nearest to both the
        // others, so it is the most central.
        let mut view = view();
        view.beacons[0].cell = CellXY { x: 1, y: 1 };
        view.beacons[1].cell = CellXY { x: 10, y: 10 };
        view.beacons.push(BeaconSnap {
            cell: CellXY { x: 20, y: 20 },
            holder: Some(1),
        });
        let cfg = SharedConfig {
            rule: SharedRule::Centre,
            ..SharedConfig::default()
        };
        let target = cfg.resolve(&view).expect("a target");
        assert_eq!(target.beacon, 1);
        assert_eq!(target.reason, "the most central of the beacons");
    }

    #[test]
    fn the_ghost_field_falls_off_with_distance() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ..SharedConfig::default()
        };
        let target = cfg.resolve(&view()).expect("a target");
        // The agent is 8 cells from beacon 0 and the radius is 96, so the
        // pull is there and it is strong.
        assert_eq!(target.distance, 8);
        assert_eq!(target.band, PullBand::Strong);
        assert!(target.pull_q14 > 0);
    }

    #[test]
    fn the_ghost_pull_is_zero_outside_the_radius() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                radius_cells: 4,
                ..GhostConfig::default()
            },
        };
        let target = cfg.resolve(&view()).expect("a target");
        assert_eq!(target.band, PullBand::None);
        assert_eq!(target.pull_q14, 0);
    }

    #[test]
    fn the_recurring_phase_pulses() {
        let tile = GhostTile {
            position: CellXY { x: 0, y: 0 },
            kind: GhostKind::Attractor,
            phase: GhostPhase::Recurring,
            strength_q14: Q14_ONE,
            radius_cells: 10,
            falloff: Falloff::InverseLinear,
            cycle_ticks: 100,
            max_age: 0,
        };
        assert_eq!(tile.strength_at(0), Q14_ONE / 2);
        assert_eq!(tile.strength_at(50), Q14_ONE);
        assert_eq!(tile.strength_at(100), Q14_ONE / 2);
    }

    #[test]
    fn the_temporal_phase_fades() {
        let tile = GhostTile {
            position: CellXY { x: 0, y: 0 },
            kind: GhostKind::Attractor,
            phase: GhostPhase::Temporal,
            strength_q14: Q14_ONE,
            radius_cells: 10,
            falloff: Falloff::InverseLinear,
            cycle_ticks: 0,
            max_age: 100,
        };
        assert_eq!(tile.strength_at(0), Q14_ONE);
        assert_eq!(tile.strength_at(50), Q14_ONE / 2);
        assert_eq!(tile.strength_at(100), 0);
        assert_eq!(tile.strength_at(500), 0);
    }

    #[test]
    fn a_repulser_pushes_the_other_way() {
        let tile = GhostTile {
            position: CellXY { x: 0, y: 0 },
            kind: GhostKind::Repulsor,
            phase: GhostPhase::Permanent,
            strength_q14: Q14_ONE,
            radius_cells: 10,
            falloff: Falloff::InverseLinear,
            cycle_ticks: 0,
            max_age: 0,
        };
        // A repulsor pushes: its influence is the other way round.
        assert!(tile.influence_at(CellXY { x: 0, y: 0 }, 0) < 0);
        assert_eq!(tile.influence_at(CellXY { x: 20, y: 0 }, 0), 0);
    }

    #[test]
    fn a_bad_file_is_an_error_not_a_default() {
        assert!(SharedConfig::parse("[shared]\nrule = \"nonsense\"").is_err());
    }

    #[test]
    fn a_repulser_cannot_be_the_shared_target() {
        // The field model knows a repulsor, but the target rule states an
        // attractor: a repulsor names no place to go to.
        let repulsor = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                kind: GhostKind::Repulsor,
                ..GhostConfig::default()
            },
        };
        assert!(repulsor.validate().is_err());
        // The attractor is fine, and so is every other rule.
        assert!(SharedConfig {
            rule: SharedRule::Ghost,
            ..SharedConfig::default()
        }
        .validate()
        .is_ok());
        assert!(SharedConfig::default().validate().is_ok());
    }

    #[test]
    fn the_file_names_the_rule_under_the_shared_table() {
        let cfg = SharedConfig::parse("[shared]\nrule = \"free\"").expect("the file loads");
        assert_eq!(cfg.rule, SharedRule::Free);
    }

    /// A view of four beacons, none of them held.
    fn four_beacons() -> ScoreView {
        let mut view = view();
        view.beacons = vec![
            BeaconSnap {
                cell: CellXY { x: 64, y: 63 },
                holder: None,
            },
            BeaconSnap {
                cell: CellXY { x: 63, y: 64 },
                holder: None,
            },
            BeaconSnap {
                cell: CellXY { x: 32, y: 63 },
                holder: None,
            },
            BeaconSnap {
                cell: CellXY { x: 95, y: 64 },
                holder: None,
            },
        ];
        view
    }

    #[test]
    fn the_hot_zone_walks_to_the_next_beacon() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                ..GhostConfig::default()
            },
        };
        let mut view = four_beacons();
        // The hot beacon is on beacon 0 until the zone turns over.
        view.tick = 0;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
        view.tick = 99;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
        // A hundred ticks on, it is on beacon 1.
        view.tick = 100;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 1);
        view.tick = 200;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 2);
        // And it comes back round.
        view.tick = 400;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
    }

    #[test]
    fn the_hot_zone_is_the_same_for_both_squads() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                ..GhostConfig::default()
            },
        };
        let mut mine = four_beacons();
        mine.me.cell = CellXY { x: 2, y: 2 };
        let mut theirs = four_beacons();
        theirs.squad_id = 1;
        theirs.me.cell = CellXY { x: 125, y: 125 };
        for tick in [0, 100, 250, 700] {
            mine.tick = tick;
            theirs.tick = tick;
            assert_eq!(
                cfg.resolve(&mine).expect("a target").beacon,
                cfg.resolve(&theirs).expect("a target").beacon,
                "both squads are pulled to the same hot beacon on tick {tick}"
            );
        }
    }

    #[test]
    fn a_hot_zone_that_never_turns_over_stays_on_the_free_beacon() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 0,
                ..GhostConfig::default()
            },
        };
        let mut view = four_beacons();
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
        view.tick = 5_000;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
    }

    #[test]
    fn the_hot_zone_skips_the_beacons_a_squad_holds() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                ..GhostConfig::default()
            },
        };
        let mut view = four_beacons();
        // Beacon 0 is taken, so the zone walks the free ones.
        view.beacons[0].holder = Some(1);
        view.tick = 0;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 1);
        view.tick = 100;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 2);
    }

    #[test]
    fn a_hot_zone_with_nothing_free_still_has_a_beacon() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                ..GhostConfig::default()
            },
        };
        let mut view = four_beacons();
        for beacon in &mut view.beacons {
            beacon.holder = Some(0);
        }
        view.tick = 0;
        assert_eq!(cfg.resolve(&view).expect("a target").beacon, 0);
    }
}

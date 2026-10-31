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

/// The order the hot zone walks the beacons in.
///
/// The map is point-symmetric (D61): the beacons come in 180°-rotated
/// pairs, and squad 1's side of the map is squad 0's side turned around.
/// The **beacon order** is not that symmetry — beacon 0 and beacon 1 are
/// each other's mirror, and beacon 2 and beacon 3 are, so "in beacon
/// order" visits the outer pair with squad 0's beacon first and squad 1's
/// second. Measured over 100 seeds, the numbered walk decided 64 rounds
/// to squad 0 and 36 to squad 1 — a gap of 28 over the fairness guard's
/// bound of 20 (crates/lm-agents/tests/engagement.md, "The hot zone,
/// judged").
///
/// | walk | the order | what it is for |
/// |---|---|---|
/// | `numbered` | the beacons no squad holds, in beacon order | the shipped walk |
/// | `around` | **every** beacon, in the order of its angle around the map's centre | the fair walk |
///
/// The `around` walk is fair **by construction**, on two counts:
///
/// 1. **The order commutes with the map's symmetry.** The beacons are
///    sorted by their angle about the centre, and the 180° rotation adds
///    half a turn to every angle — so the sorted list of the rotated map
///    is the rotated list of the map. The zone's schedule is therefore
///    the same schedule in the mirrored world, and no side of the map is
///    favoured by the order. The numbered walk does not have that
///    property: the mirrored world walks it in the other order.
/// 2. **The schedule does not depend on who holds what.** The `around`
///    walk visits every beacon, taken or not, so the zone's position is
///    a function of the tick and the beacons' places alone. The numbered
///    walk drops a beacon from its list the moment a squad takes it,
///    which shifts every later step: the round's own history feeds back
///    into the zone, and the side that got there first changes the walk
///    for everybody. That feedback is where the bias grew.
///
/// Both are integer-only and deterministic, and both are the same for
/// both squads, because both read only what every agent can see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Walk {
    /// The shipped walk: the beacons no squad holds, in beacon order.
    #[default]
    Numbered,
    /// The fair walk: every beacon, in the order of its angle around the
    /// map's centre.
    Around,
}

impl Walk {
    /// The walk's name, as the harness file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Walk::Numbered => "numbered",
            Walk::Around => "around",
        }
    }
}

/// The second objective a squad that has lost the shared target is given.
///
/// The shared target is worth fighting over, and it is the only one: a
/// squad that loses it has nothing to do, nothing to score and nowhere
/// to go, and the round ends with one squad on the board and the other
/// on zero points. The second objective gives the loser something to do.
///
/// | rule | the second objective |
/// |---|---|
/// | `off` | none: the shipped facts (the default) |
/// | `fallback` | the nearest beacon **no squad holds** |
/// | `next-stop` | the beacon the hot zone moves to next |
///
/// It is stated in one plain sentence, and only to the squad that has
/// lost the target: the squad that does not hold the shared target while
/// the other squad does. Until then both squads are sent to the target,
/// which is the whole point of the shared target.
///
/// The `fallback` sends the loser to a beacon of its own, where it can
/// hold and score, but it takes the squad out of the fight: measured over
/// 100 seeds, a fallback took the contact from 5.45 tags a round to 0.06.
///
/// The `next-stop` keeps the loser in the fight: it walks to where the
/// zone is going and is there when the zone arrives, so the fight
/// continues where the zone is and the loser is in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Second {
    /// No second objective: the shipped facts, unchanged.
    #[default]
    Off,
    /// The nearest beacon no squad holds.
    Fallback,
    /// The beacon the hot zone moves to next.
    #[serde(rename = "next-stop")]
    NextStop,
}

impl Second {
    /// The rule's name, as the harness file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Second::Off => "off",
            Second::Fallback => "fallback",
            Second::NextStop => "next-stop",
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
    /// The order the zone walks the beacons in.
    pub walk: Walk,
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
            walk: Walk::Numbered,
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
    /// The second objective a squad that has lost the shared target is
    /// given. Off by default.
    pub second: Second,
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
                hot_beacon(view, self.ghost.rotation_ticks, self.ghost.walk),
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

    /// The second objective for this squad, if it has lost the shared
    /// target.
    ///
    /// The squad that has lost it is the one that does not hold the shared
    /// target while the other squad does. Until then both squads are sent
    /// to the shared target, which is the whole point of it, so there is
    /// no second objective to state.
    ///
    /// `steps` is the length of the walk from the agent to each beacon, in
    /// the order of `view.beacons`; a beacon with no walk to it cannot be
    /// the objective, because there is no way to reach it.
    pub fn second_objective(
        &self,
        view: &ScoreView,
        target: &SharedTarget,
        steps: &[Option<i64>],
    ) -> Option<SecondObjective> {
        if self.second == Second::Off {
            return None;
        }
        let held = view.beacons.get(usize::from(target.beacon))?.holder?;
        if held == view.squad_id {
            return None;
        }
        match self.second {
            Second::Off => None,
            // The nearest beacon no squad holds: the one the loser can take
            // and hold, and so can score on.
            Second::Fallback => {
                let mut best: Option<(u8, i64)> = None;
                for (index, beacon) in view.beacons.iter().enumerate() {
                    if beacon.holder.is_some() {
                        continue;
                    }
                    let Some(walk) = steps.get(index).copied().flatten() else {
                        continue;
                    };
                    if best.is_none_or(|(_, top)| walk < top) {
                        best = Some((index as u8, walk));
                    }
                }
                best.map(|(beacon, _)| SecondObjective {
                    beacon,
                    reason: "the nearest unheld beacon",
                })
            }
            // Where the zone is going: the loser walks there and is there
            // when the zone arrives, so the fight carries on there.
            Second::NextStop => {
                let beacon = self.next_stop(view)?;
                Some(SecondObjective {
                    beacon,
                    reason: "the next stop of the zone",
                })
            }
        }
    }

    /// The beacon the hot zone moves to next, or `None` when there is no
    /// zone that walks.
    ///
    /// It is the beacon after the hot one in the same walk list, so it is
    /// the same for every agent of every squad: the tick and the beacon
    /// list are what every agent sees.
    pub fn next_stop(&self, view: &ScoreView) -> Option<u8> {
        if self.rule != SharedRule::Ghost || self.ghost.rotation_ticks == 0 {
            return None;
        }
        let list = walk_list(view, self.ghost.walk);
        if list.is_empty() {
            return None;
        }
        let step = ((view.tick / u64::from(self.ghost.rotation_ticks)) + 1) as usize % list.len();
        Some(list[step] as u8)
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

/// The second objective: the beacon a squad that has lost the shared
/// target is sent to instead.
///
/// The shared target is worth fighting over and it is the only target: a
/// squad that loses it has nothing to do, nothing to score and nowhere to
/// go, and the round ends with one squad on the board and the other on
/// zero points. The second objective is the loser's way to keep playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondObjective {
    /// The beacon the squad is sent to.
    pub beacon: u8,
    /// What the beacon is, in the facts' own words.
    pub reason: &'static str,
}

impl SecondObjective {
    /// The one sentence that states it.
    ///
    /// The condition is in the same sentence as the objective, so the
    /// model never has to guess whether the branch applies: the sentence
    /// is there or it is not.
    pub fn summary(&self) -> String {
        format!(
            "The other squad holds the shared target, so your second objective is beacon {}: {}.",
            self.beacon, self.reason
        )
    }

    /// The clause that marks the beacon, in the facts' own words.
    pub fn marker(&self, beacon: u8) -> String {
        if beacon != self.beacon {
            return "; it is not your second objective".to_owned();
        }
        format!("; it is your second objective: {}", self.reason)
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
/// The hot zone is on **one beacon at a time**. It walks the beacons and it
/// moves on every `rotation_ticks` ticks; with `rotation_ticks` of zero it
/// never moves and the hot beacon is the lowest-numbered free one for the
/// whole round.
///
/// Which beacons it walks, and in what order, is the [`Walk`]:
///
/// * `numbered` walks the beacons **no squad holds**, in beacon order. The
///   list depends on who holds what, so the round's own history feeds back
///   into the zone, and the beacon order does not commute with the map's
///   180-degree rotation. Measured over 100 seeds, that walk decided 64
///   rounds to squad 0 and 36 to squad 1.
/// * `around` walks **every** beacon, in the order of its angle about the
///   map's centre. The order commutes with the map's rotation, so the
///   schedule is the same schedule in the mirrored world, and the schedule
///   does not depend on who holds what at all.
///
/// The walk is a function of the tick and the beacon list, and both are
/// the same for both squads — the tick is the round's, and the beacon list
/// and its holders are what every agent sees. So both squads are pulled
/// toward the same hot beacon at the same time, which is what makes them
/// meet; and the hot beacon moves, so the meeting place moves too.
fn hot_beacon(view: &ScoreView, rotation_ticks: u32, walk: Walk) -> u8 {
    if view.beacons.is_empty() {
        return 0;
    }
    let list = walk_list(view, walk);
    if rotation_ticks == 0 {
        return list[0] as u8;
    }
    let step = (view.tick / u64::from(rotation_ticks)) as usize % list.len();
    list[step] as u8
}

/// The beacons the hot zone walks, in the order it walks them.
fn walk_list(view: &ScoreView, walk: Walk) -> Vec<usize> {
    match walk {
        Walk::Numbered => {
            // The beacons worth taking: the ones no squad holds. When they
            // are all held there is none left to walk, and the hot zone
            // falls back to the whole list, so it is never on nothing.
            let free: Vec<usize> = view
                .beacons
                .iter()
                .enumerate()
                .filter(|(_, beacon)| beacon.holder.is_none())
                .map(|(index, _)| index)
                .collect();
            if free.is_empty() {
                (0..view.beacons.len()).collect()
            } else {
                free
            }
        }
        Walk::Around => around_order(view),
    }
}

/// The beacons in the order of their angle about the map's centre.
///
/// The centre is the middle of the beacons themselves, which on the round's
/// point-symmetric map is the middle of the map: the beacons come in
/// 180-degree rotated pairs (D61), so their middle is the map's own centre
/// of symmetry. The offsets are scaled by the number of beacons so that the
/// middle of an even number of cells is exact: no rounding, no floats.
///
/// Sorting by angle is what makes the walk fair: the 180-degree rotation
/// adds half a turn to every angle, so the sorted list of a rotated map is
/// the rotated list of the map. The zone's schedule in the mirrored world
/// is therefore the mirror of its schedule here, and neither side of the
/// map is favoured by the order. The beacon order does not have that
/// property.
fn around_order(view: &ScoreView) -> Vec<usize> {
    let count = view.beacons.len();
    if count < 2 {
        return (0..count).collect();
    }
    let scale = count as i64;
    let sum_x: i64 = view.beacons.iter().map(|b| i64::from(b.cell.x)).sum();
    let sum_y: i64 = view.beacons.iter().map(|b| i64::from(b.cell.y)).sum();
    let offsets: Vec<(i64, i64)> = view
        .beacons
        .iter()
        .map(|beacon| offset(beacon.cell, scale, sum_x, sum_y))
        .collect();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| angle_order(offsets[a], offsets[b]));
    // The walk starts at the beacon nearest the middle of the beacons,
    // which on the round's symmetric map is the beacon at the middle of
    // the map: the one place neither squad is nearer to.
    //
    // It has to start there. A beacon is held by the squad that has an
    // agent inside it while no enemy is inside it, and a beacon two
    // squads stand in is held by nobody, so whoever reaches the zone
    // first holds it alone and scores alone until the other squad
    // arrives. That lead is a lead at the next beacon too, and the round
    // is decided by it. Starting the walk on the far side of the map
    // therefore hands the round to one squad: over 100 seeds, the walk
    // that started on the east side decided 88 rounds to squad 1 and 12
    // to squad 0.
    let middle = nearest_middle(&offsets);
    let start = order.iter().position(|&index| index == middle).unwrap_or(0);
    order.rotate_left(start);
    order
}

/// The beacon nearest the middle of the beacons, the lowest number breaking
/// a tie.
///
/// The lowest number is the same rule the facts builder uses for the
/// doctrine's own nearest beacon ("the lowest index breaks a tie, so the
/// same situation always names the same beacon"), so the walk's first stop
/// is named the same way the rest of the facts are.
fn nearest_middle(offsets: &[(i64, i64)]) -> usize {
    let mut best = 0usize;
    let mut best_distance = i64::MAX;
    for (index, (x, y)) in offsets.iter().enumerate() {
        let distance = x * x + y * y;
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// A beacon's offset from the middle of the beacons, scaled by `scale`.
///
/// The scaling keeps the middle of an even number of cells exact: the
/// offsets are whole numbers whatever the beacons' columns and rows are.
fn offset(cell: CellXY, scale: i64, sum_x: i64, sum_y: i64) -> (i64, i64) {
    (
        scale * i64::from(cell.x) - sum_x,
        scale * i64::from(cell.y) - sum_y,
    )
}

/// Which of two offsets comes first in the turn about the middle.
///
/// The turn starts at the east and runs anticlockwise: the offsets above
/// the middle come first, then those below. Within a half, the cross
/// product says which is anticlockwise of which. Two offsets on the same
/// ray are ordered by their distance from the middle, which the rotation
/// leaves alone; two on the same ray at the same distance are the same
/// cell, so the order is total and the rotation's own order survives it.
fn angle_order(left: (i64, i64), right: (i64, i64)) -> std::cmp::Ordering {
    let half_left = half(left);
    let half_right = half(right);
    if half_left != half_right {
        return half_left.cmp(&half_right);
    }
    let cross = left.0 * right.1 - left.1 * right.0;
    if cross != 0 {
        return cross.cmp(&0).reverse();
    }
    let left_d = left.0 * left.0 + left.1 * left.1;
    let right_d = right.0 * right.0 + right.1 * right.1;
    left_d.cmp(&right_d)
}

/// Which half of the turn an offset is in: above the middle, or below.
fn half(offset: (i64, i64)) -> u8 {
    if offset.1 > 0 || (offset.1 == 0 && offset.0 > 0) {
        0
    } else {
        1
    }
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
            second: Second::Off,
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
            second: Second::Off,
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                kind: GhostKind::Repulsor,
                ..GhostConfig::default()
            },
        };
        assert!(repulsor.validate().is_err());
        // The attractor is fine, and so is every other rule.
        assert!(SharedConfig {
            second: Second::Off,
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
            second: Second::Off,
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
            second: Second::Off,
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
            second: Second::Off,
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
            second: Second::Off,
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
            second: Second::Off,
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

    fn ghost(walk: Walk) -> SharedConfig {
        SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                walk,
                ..GhostConfig::default()
            },
            second: Second::Off,
        }
    }

    #[test]
    fn the_around_walk_does_not_depend_on_who_holds_a_beacon() {
        // The schedule is a function of the tick and the beacons' places,
        // so the round's own history cannot feed back into the zone.
        let cfg = ghost(Walk::Around);
        let mut view = four_beacons();
        view.beacons[0].holder = Some(1);
        let mut taken = view.clone();
        taken.beacons[0].holder = Some(0);
        for tick in (0..400).step_by(100) {
            view.tick = tick;
            taken.tick = tick;
            assert_eq!(
                cfg.resolve(&view).expect("a target").beacon,
                cfg.resolve(&taken).expect("a target").beacon,
                "the zone moved because a squad took a beacon"
            );
        }
    }

    #[test]
    fn the_numbered_walk_does_depend_on_who_holds_a_beacon() {
        // The shipped walk drops a taken beacon from its list, which
        // shifts every later step: the zone's place depends on what the
        // squads have already done. That is the feedback the around walk
        // does not have, and it is how the colour bias grew.
        let cfg = ghost(Walk::Numbered);
        let mut view = four_beacons();
        let mut taken = view.clone();
        taken.beacons[0].holder = Some(1);
        for tick in (0..400).step_by(100) {
            view.tick = tick;
            taken.tick = tick;
            assert_ne!(
                cfg.resolve(&view).expect("a target").beacon,
                cfg.resolve(&taken).expect("a target").beacon,
                "the numbered walk no longer moves because a beacon was taken"
            );
        }
    }

    #[test]
    fn the_around_walk_spends_half_its_time_on_each_side() {
        // The map's centre is between the middle columns, so a beacon
        // west of it is on squad 0's side of the map and one east of it is
        // on squad 1's. A walk that spends half its time on each side
        // gives neither squad the edge.
        let cfg = ghost(Walk::Around);
        let mut view = four_beacons();
        let mut west = 0;
        let mut east = 0;
        for tick in (0..400).step_by(100) {
            view.tick = tick;
            let beacon = cfg.resolve(&view).expect("a target").beacon;
            let cell = view.beacons[usize::from(beacon)].cell;
            if cell.x <= 63 {
                west += 1;
            } else {
                east += 1;
            }
        }
        assert_eq!((west, east), (2, 2));
    }

    #[test]
    fn the_around_walk_visits_every_beacon() {
        let cfg = ghost(Walk::Around);
        let mut view = four_beacons();
        let mut seen = Vec::new();
        for tick in (0..400).step_by(100) {
            view.tick = tick;
            seen.push(cfg.resolve(&view).expect("a target").beacon);
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_second_objective_is_off_by_default() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ..SharedConfig::default()
        };
        let mut view = four_beacons();
        view.beacons[0].holder = Some(1);
        let steps = vec![Some(1i64); 4];
        assert!(cfg
            .second_objective(&view, &cfg.resolve(&view).expect("a target"), &steps)
            .is_none());
    }

    #[test]
    fn the_second_objective_is_the_losers_only() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                walk: Walk::Around,
                ..GhostConfig::default()
            },
            second: Second::Fallback,
        };
        let mut view = four_beacons();
        view.tick = 0;
        let target = cfg.resolve(&view).expect("a target");
        // The other squad holds the shared target: the squad that lost it
        // is sent to the nearest beacon nobody holds.
        view.beacons[usize::from(target.beacon)].holder = Some(1);
        let steps = vec![Some(1i64), Some(2), Some(3), Some(4)];
        let second = cfg
            .second_objective(&view, &target, &steps)
            .expect("a second objective");
        // The nearest unheld beacon by the walk: beacon 0 is held by the
        // other squad now, so beacon 1 is the nearest one nobody holds.
        assert_eq!(second.beacon, 1);
        // The squad that holds it has no second objective: it is the one
        // that won the beacon.
        let mut winner = view.clone();
        winner.squad_id = 1;
        assert!(cfg.second_objective(&winner, &target, &steps).is_none());
    }

    #[test]
    fn the_next_stop_is_the_second_objective() {
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            ghost: GhostConfig {
                rotation_ticks: 100,
                walk: Walk::Around,
                ..GhostConfig::default()
            },
            second: Second::NextStop,
        };
        let mut view = four_beacons();
        view.tick = 0;
        let target = cfg.resolve(&view).expect("a target");
        // The zone is on the beacon nearest the middle of the beacons, and
        // it turns over to the next one in the walk.
        assert_eq!(target.beacon, 0);
        assert_eq!(cfg.next_stop(&view), Some(3));
        // The other squad holds the shared target, so the squad that lost
        // it is sent to where the zone is going.
        view.beacons[usize::from(target.beacon)].holder = Some(1);
        let steps = vec![Some(1i64); 4];
        let second = cfg
            .second_objective(&view, &target, &steps)
            .expect("a second objective");
        assert_eq!(second.beacon, 3);
        assert_eq!(second.reason, "the next stop of the zone");
    }

    #[test]
    fn a_second_objective_waits_for_a_lost_beacon() {
        // While nobody holds the shared target both squads are sent to
        // it: that is the whole point of a shared target, and a second
        // objective before the fight would split the squads again.
        let cfg = SharedConfig {
            rule: SharedRule::Ghost,
            second: Second::Fallback,
            ..SharedConfig::default()
        };
        let mut view = four_beacons();
        view.tick = 0;
        let target = cfg.resolve(&view).expect("a target");
        let steps = vec![Some(1i64); 4];
        assert!(cfg.second_objective(&view, &target, &steps).is_none());
    }
}

//! The plain-language facts a decision model is shown (D49).
//!
//! Code computes every number — positions, walking-path lengths, bearings,
//! distances, lines of sight — and states them as fixed-order sentences;
//! the model applies judgement to stated facts, never arithmetic to a grid
//! (E4c: facts take d1-3B from 0.45 to 0.97 on action). The same
//! computation fills the structured `fields` and the D25 situation key
//! hashed over them, so a decision's provenance names the exact situation
//! it answered, and a policy detects novelty as a change in the key.

use lm_core::hash::Fnv64;
use lm_core::header::Role;
use lm_core::view::{EnemySeen, ScoreView};
use lm_core::world::{jps, los, CellXY, World};
use std::collections::BTreeMap;

use crate::config::AgentsConfig;
use crate::shared::{PullBand, SharedConfig, SharedRule};

/// The compass words in the order their field indices run; row 0 is
/// north, so +y is south. "here" is a zero step.
const COMPASS: [&str; 5] = ["north", "east", "south", "west", "here"];

/// How close an enemy has to be to count as near, in cells (Chebyshev).
///
/// The squad doctrine of the default harness speaks of "near" in the same
/// cells: `assets/harness/default/gates.toml` says "within 6 cells" and
/// this is that 6. The rule and the facts have to agree on the number, or
/// the model is asked to apply a rule about a different world than the one
/// it is shown (E4c: facts beat grids; a rule over facts beats a guess).
pub const ENEMY_NEAR_CELLS: i32 = 6;

/// One agent's situation as code-computed facts (D49): what the model
/// reads, what the situation key addresses, both from one computation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Facts {
    /// The sentences, space-joined in the fixed order: my position; each
    /// beacon (holder, walking-path length, bearing); each fresh enemy
    /// (distance, line of sight); allies in range; my status. No tick
    /// appears: the same situation reads the same at any tick.
    pub text: String,
    /// The D25 situation key: FNV-1a 64 over the canonical `fields`.
    pub key: u64,
    /// Every stated number, by stable name (`me.x`, `beacon.0.steps`,
    /// `enemy.4.dist`, `ally.1.stunned`, …). Absences carry meaning too:
    /// a stale enemy or an out-of-range ally has no entry.
    pub fields: BTreeMap<String, i64>,
}

impl Facts {
    /// Compute the facts of `view` for an agent of squad `me_squad` on
    /// `world`, with the stale window and ally range of `cfg`.
    ///
    /// Deterministic by construction: fixed sentence order, integer math
    /// only, ordered maps only — the same view on the same world always
    /// yields the same text, fields and key.
    ///
    /// `shared` is the harness's shared-target parameter. It is off by
    /// default, and when it is off the facts are exactly the shipped ones:
    /// the doctrine's target is the nearest beacon the squad does not
    /// hold. When it is on, the shared target takes that place — the
    /// beacon both squads are sent to, read off the beacons every agent
    /// can see, so both squads get the same one (see [`crate::shared`]).
    pub fn from_view(
        view: &ScoreView,
        world: &World,
        me_squad: u8,
        cfg: &AgentsConfig,
        shared: &SharedConfig,
    ) -> Facts {
        let me = view.me.cell;
        let mut lines: Vec<String> = Vec::new();
        let mut fields: BTreeMap<String, i64> = BTreeMap::new();

        // My position.
        fields.insert("me.id".to_owned(), i64::from(view.me.id));
        fields.insert("me.x".to_owned(), i64::from(me.x));
        fields.insert("me.y".to_owned(), i64::from(me.y));
        lines.push(format!(
            "You are agent {}, the {}, of squad {} at column {}, row {} \
             (row 0 is north).",
            view.me.id,
            role_word(view.me.role),
            me_squad,
            me.x,
            me.y
        ));

        // The shared target: the one beacon both squads are sent to, so
        // that they meet instead of walking to mirrored beacons. It is
        // off by default; when it is on it takes the place of the
        // doctrine's own target, and the facts say so in the same
        // sentence as the beacon it names.
        let shared_target = shared.resolve(view);
        if let Some(target) = &shared_target {
            for line in target.summary() {
                lines.push(line);
            }
            fields.insert("shared.target".to_string(), i64::from(target.beacon));
            fields.insert("shared.distance".to_string(), i64::from(target.distance));
            if target.rule == SharedRule::Ghost {
                fields.insert("shared.pull".to_string(), target.pull_q14);
                fields.insert("shared.band".to_string(), pull_band_value(target.band));
                fields.insert("shared.radius".to_string(), i64::from(target.radius_cells));
            }
        }

        // Each beacon: holder, walking-path length and bearing, all
        // computed by code (the path in cells from JPS, the compass word
        // from the offsets).
        //
        // The paths come first: the doctrine's target is the nearest
        // beacon my squad does not hold, and "nearest" is the walk that
        // really costs, so the path decides — and a beacon with no path
        // cannot be walked to, so it cannot be the target.
        let mut paths: Vec<(Option<i64>, i64)> = Vec::with_capacity(view.beacons.len());
        for beacon in &view.beacons {
            match jps(&world.grid, me, beacon.cell) {
                Some(path) => {
                    let steps = path.len() as i64 - 1;
                    let bearing = bearing(
                        i32::from(beacon.cell.x) - i32::from(me.x),
                        i32::from(beacon.cell.y) - i32::from(me.y),
                    );
                    paths.push((Some(steps), bearing));
                }
                None => paths.push((None, -1)),
            }
        }

        // The beacon the doctrine would send me to: the nearest one my
        // squad does not hold, by the steps of the walk to it. The lowest
        // index breaks a tie, so the same situation always names the same
        // beacon.
        let mut nearest_unheld: Option<(u8, i64)> = None;
        for (i, beacon) in view.beacons.iter().enumerate() {
            if beacon.holder == Some(me_squad) {
                continue;
            }
            let (Some(steps), _) = paths[i] else {
                continue;
            };
            if nearest_unheld.is_none_or(|(_, best)| steps < best) {
                nearest_unheld = Some((i as u8, steps));
            }
        }
        let nearest_unheld = nearest_unheld.map(|(index, _)| index);
        // The fields state what the text states, so the nearest unheld
        // beacon is only a field while it is also what the text says. When
        // a shared target is stated instead, it takes the field's place.
        if shared_target.is_none() {
            fields.insert(
                "beacon.nearest_unheld".to_string(),
                nearest_unheld.map_or(-1, i64::from),
            );
        }

        // The second objective: the beacon the squad that has lost the
        // shared target is sent to instead. It is off by default; when it
        // is on it is stated in one plain sentence, with the condition
        // that makes it apply in the same sentence, so the model never
        // has to guess whether the branch is live.
        let steps_only: Vec<Option<i64>> = paths.iter().map(|(steps, _)| *steps).collect();
        let second = shared_target
            .as_ref()
            .and_then(|target| shared.second_objective(view, target, &steps_only));
        if let Some(second) = &second {
            lines.push(second.summary());
            fields.insert("shared.second".to_string(), i64::from(second.beacon));
        }

        for (i, beacon) in view.beacons.iter().enumerate() {
            let mine = beacon.holder == Some(me_squad);
            let holder_word = match beacon.holder {
                None => "unheld".to_owned(),
                Some(s) if s == me_squad => "held by your squad".to_owned(),
                Some(s) => format!("held by enemy squad {s}"),
            };
            fields.insert(format!("beacon.{i}.x"), i64::from(beacon.cell.x));
            fields.insert(format!("beacon.{i}.y"), i64::from(beacon.cell.y));
            fields.insert(
                format!("beacon.{i}.holder"),
                beacon.holder.map_or(-1, i64::from),
            );
            // The doctrine's wording, said of this beacon: it is the one
            // the rule would send me to, or it is not. With a shared
            // target the rule sends both squads to one beacon, so the
            // marker says which one that is and the other beacons say
            // plainly that they are not it.
            let nearest = if let Some(target) = &shared_target {
                target.marker(i as u8)
            } else if mine {
                String::new()
            } else if nearest_unheld == Some(i as u8) {
                "; it is the nearest beacon your squad does not hold".to_owned()
            } else {
                "; it is not the nearest beacon your squad does not hold".to_owned()
            };
            // The second objective is marked on its own beacon, so the
            // model can tell the two targets apart: the shared target is
            // the one both squads fight over, the second objective is the
            // loser's way to keep scoring.
            let nearest = match &second {
                Some(second) => format!("{nearest}{}", second.marker(i as u8)),
                None => nearest,
            };
            match paths[i] {
                (Some(steps), bearing) => {
                    fields.insert(format!("beacon.{i}.steps"), steps);
                    fields.insert(format!("beacon.{i}.bearing"), bearing);
                    if steps == 0 {
                        lines.push(format!(
                            "Beacon {i} at column {}, row {} is {holder_word}; \
                             you are standing on it{nearest}.",
                            beacon.cell.x, beacon.cell.y
                        ));
                    } else {
                        lines.push(format!(
                            "Beacon {i} at column {}, row {} is {holder_word}{nearest}; \
                             a walking path from you is {steps} steps long \
                             to the {}.",
                            beacon.cell.x, beacon.cell.y, COMPASS[bearing as usize]
                        ));
                    }
                }
                (None, _) => {
                    // Unreachable is a fact, not a failure to compute one.
                    fields.insert(format!("beacon.{i}.steps"), -1);
                    fields.insert(format!("beacon.{i}.bearing"), -1);
                    lines.push(format!(
                        "Beacon {i} at column {}, row {} is {holder_word}; \
                         you cannot go there, there is no walking path to it.",
                        beacon.cell.x, beacon.cell.y
                    ));
                }
            }
        }

        // Where I stand, and whose beacon it is: the doctrine's second
        // branch is about exactly this, so it is said in the doctrine's
        // words.
        let standing_on = view
            .beacons
            .iter()
            .position(|beacon| beacon.cell == me)
            .map(|i| i as u8);
        let standing_on_mine =
            standing_on.and_then(|i| view.beacons[usize::from(i)].holder) == Some(me_squad);
        fields.insert(
            "me.on_beacon".to_string(),
            standing_on.map_or(-1, i64::from),
        );
        fields.insert("me.on_held_beacon".to_string(), i64::from(standing_on_mine));
        match standing_on {
            Some(i) => {
                let whose = match view.beacons[usize::from(i)].holder {
                    Some(s) if s == me_squad => "which your squad holds".to_owned(),
                    Some(s) => format!("which enemy squad {s} holds"),
                    None => "which nobody holds".to_owned(),
                };
                lines.push(format!("You are standing on beacon {i}, {whose}."));
            }
            None => lines.push("You are not standing on a beacon.".to_owned()),
        }

        // The paint under my feet. Standing in the other team's paint is
        // contact: it slows me down, and it is why I am walking slowly. The
        // sentence says so in one line, and only when it is true — the
        // slowdown is the sim's own number, so the facts never promise a
        // slowdown the round is not applying.
        let paint_under = view.paint.iter().find(|p| p.cell == me).map(|p| p.owner);
        if let Some(owner) = paint_under {
            if owner != me_squad {
                fields.insert("me.in_enemy_paint".to_string(), 1);
                fields.insert(
                    "me.paint_slow_q8".to_string(),
                    i64::from(view.me.paint_slow_q8),
                );
                if view.me.paint_slow_q8 > 0 {
                    lines.push(
                        "You are standing in the other team's paint; it is slowing you down."
                            .to_owned(),
                    );
                } else {
                    lines.push("You are standing in the other team's paint.".to_owned());
                }
            }
        }

        // The doctrine's target, named. When there is none the rule says
        // what to do instead, so the fact has to say why.
        if nearest_unheld.is_none() {
            let unheld = view
                .beacons
                .iter()
                .filter(|beacon| beacon.holder != Some(me_squad))
                .count();
            lines.push(if unheld == 0 {
                "Your squad holds every beacon; there is no beacon your squad does not hold."
                    .to_owned()
            } else {
                "Every beacon your squad does not hold is out of reach; there is no walking path to it."
                    .to_owned()
            });
        }

        // Each fresh enemy: distance and line of sight from where I stand.
        // A stale memory is omitted entirely — better no fact than a
        // seven-tick-old position phrased as news.
        let mut any_enemy = false;
        let mut enemies_near = 0i64;
        let mut enemies_seeing = 0i64;
        for e in fresh_enemies(view, cfg.stale_enemy_ticks) {
            any_enemy = true;
            let dist = chebyshev(me, e.cell);
            let sighted = los::clear(&world.grid, me, e.cell);
            fields.insert(format!("enemy.{}.x", e.id), i64::from(e.cell.x));
            fields.insert(format!("enemy.{}.y", e.id), i64::from(e.cell.y));
            fields.insert(format!("enemy.{}.age", e.id), i64::from(e.age_ticks));
            fields.insert(format!("enemy.{}.dist", e.id), dist);
            fields.insert(format!("enemy.{}.los", e.id), i64::from(sighted));
            if dist <= i64::from(ENEMY_NEAR_CELLS) {
                enemies_near += 1;
                if sighted {
                    enemies_seeing += 1;
                }
            }
            let ticks = if e.age_ticks == 1 { "tick" } else { "ticks" };
            let sight = if sighted {
                "can see you (clear line of sight)"
            } else {
                "cannot see you (no clear line of sight)"
            };
            lines.push(format!(
                "Enemy {}, last seen {} {ticks} ago at column {}, row {}, \
                 is {} cells away and {sight}.",
                e.id, e.age_ticks, e.cell.x, e.cell.y, dist
            ));
        }
        if !any_enemy {
            lines.push("You know of no fresh enemies.".to_owned());
        }

        // How many enemies are near, and how many of them see me: the
        // doctrine's first branch is a comparison of these two numbers.
        fields.insert("enemy.near".to_string(), enemies_near);
        fields.insert("enemy.seeing".to_string(), enemies_seeing);
        lines.push(match (enemies_near, enemies_seeing) {
            (0, _) => format!("No enemy is within {ENEMY_NEAR_CELLS} cells of you."),
            (1, 1) => format!(
                "1 enemy is within {ENEMY_NEAR_CELLS} cells of you, and it can see you."
            ),
            (1, _) => format!(
                "1 enemy is within {ENEMY_NEAR_CELLS} cells of you, and it cannot see you."
            ),
            (n, 0) => format!(
                "{n} enemies are within {ENEMY_NEAR_CELLS} cells of you, and none of them can see you."
            ),
            (n, seen) if n == seen => format!(
                "{n} enemies are within {ENEMY_NEAR_CELLS} cells of you, and all of them can see you."
            ),
            (n, seen) => format!(
                "{n} enemies are within {ENEMY_NEAR_CELLS} cells of you, and {seen} of them can see you."
            ),
        });

        // Allies within range: who is near enough to matter, myself
        // excluded (I am the position sentence).
        let mut any_ally = false;
        let mut allies_near = 0i64;
        for ally in &view.squad {
            if ally.id == view.me.id {
                continue;
            }
            let dist = chebyshev(me, ally.cell);
            if dist > i64::from(cfg.ally_range_cells) {
                continue;
            }
            any_ally = true;
            allies_near += 1;
            fields.insert(format!("ally.{}.x", ally.id), i64::from(ally.cell.x));
            fields.insert(format!("ally.{}.y", ally.id), i64::from(ally.cell.y));
            fields.insert(format!("ally.{}.dist", ally.id), dist);
            fields.insert(format!("ally.{}.stunned", ally.id), i64::from(ally.stunned));
            let stunned = if ally.stunned { " and is stunned" } else { "" };
            lines.push(format!(
                "Ally {} ({}) is {} cells away at column {}, row {}{}.",
                ally.id,
                role_word(ally.role),
                dist,
                ally.cell.x,
                ally.cell.y,
                stunned
            ));
        }
        if !any_ally {
            lines.push(format!(
                "No allies are within {} cells.",
                cfg.ally_range_cells
            ));
        }

        // How many allies are near: the other number of the doctrine's
        // first branch.
        fields.insert("ally.near".to_string(), allies_near);
        if allies_near > 0 {
            let noun = if allies_near == 1 { "ally" } else { "allies" };
            lines.push(format!(
                "You have {allies_near} {noun} within {} cells of you.",
                cfg.ally_range_cells
            ));
        }

        // My status.
        fields.insert("me.stunned".to_owned(), i64::from(view.me.stunned));
        lines.push(if view.me.stunned {
            "You are stunned.".to_owned()
        } else {
            "You are not stunned.".to_owned()
        });

        let key = situation_key(&fields);
        Facts {
            text: lines.join(" "),
            key,
            fields,
        }
    }
}

/// The D25 situation key of a field set: FNV-1a 64 over the entry count
/// and, in `BTreeMap` key order, each name (NUL-terminated) and value
/// (little-endian i64).
pub fn situation_key(fields: &BTreeMap<String, i64>) -> u64 {
    let mut h = Fnv64::new();
    h.u32(fields.len() as u32);
    for (name, value) in fields {
        h.write(name.as_bytes());
        h.write(&[0]);
        h.i64(*value);
    }
    h.finish()
}

/// The fresh enemy sightings, in view order: aged at most `stale_after`
/// ticks.
fn fresh_enemies(view: &ScoreView, stale_after: u32) -> impl Iterator<Item = &EnemySeen> {
    view.enemies
        .iter()
        .filter(move |e| e.age_ticks <= stale_after)
}

/// Chebyshev distance in cells — the metric of the 8-connected grid, and
/// so of vision too.
fn chebyshev(a: CellXY, b: CellXY) -> i64 {
    let dx = (i32::from(a.x) - i32::from(b.x)).abs();
    let dy = (i32::from(a.y) - i32::from(b.y)).abs();
    i64::from(dx.max(dy))
}

/// The compass index of a step (dx, dy): the dominant axis, ties going
/// east/west; row 0 is north, so +y is south. A zero step is "here".
fn bearing(dx: i32, dy: i32) -> i64 {
    if dx == 0 && dy == 0 {
        4
    } else if dx.abs() >= dy.abs() {
        if dx > 0 {
            1
        } else {
            3
        }
    } else if dy > 0 {
        2
    } else {
        0
    }
}

/// The plain word for a role.
fn role_word(role: Role) -> &'static str {
    match role {
        Role::Leader => "leader",
        Role::Member => "member",
    }
}

/// The band a pull falls in, as a number for the fields: none, weak, strong.
fn pull_band_value(band: PullBand) -> i64 {
    match band {
        PullBand::None => 0,
        PullBand::Weak => 1,
        PullBand::Strong => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentsConfig;
    use lm_core::header::Role;
    use lm_core::view::{AgentSnap, BeaconSnap, EnemySeen, PaintSnap};
    use lm_core::world::{jps, CellKind, CellXY, Grid};

    /// The facts of a squad-0 view under the default config.
    fn facts(view: &ScoreView, world: &World) -> Facts {
        Facts::from_view(
            view,
            world,
            0,
            &AgentsConfig::default(),
            &SharedConfig::default(),
        )
    }

    /// An unstunned snapshot at a cell.
    fn snap(id: u16, x: u16, y: u16, role: Role) -> AgentSnap {
        AgentSnap {
            id,
            cell: CellXY { x, y },
            role,
            stunned: false,
            airtime_ticks: 0,
            paint_slow_q8: 0,
        }
    }

    /// A floor grid of `w × h` with walls at the listed cells (`Grid::set`
    /// is private to lm-core, so the cells are laid out directly).
    fn grid_with(w: u16, h: u16, walls: &[(u16, u16)]) -> Grid {
        let mut cells = vec![CellKind::Floor; usize::from(w) * usize::from(h)];
        for &(x, y) in walls {
            cells[usize::from(y) * usize::from(w) + usize::from(x)] = CellKind::Wall;
        }
        Grid { w, h, cells }
    }

    /// A 13×13 grid with a wall column at x = 6, open only at rows 10–12:
    /// the left half and the right half connect through the south corridor.
    fn wall_world() -> World {
        let walls: Vec<(u16, u16)> = (0..10).map(|y| (6, y)).collect();
        World {
            grid: grid_with(13, 13, &walls),
            beacons: vec![CellXY { x: 4, y: 2 }, CellXY { x: 10, y: 2 }],
            spawns: vec![CellXY { x: 2, y: 2 }, CellXY { x: 10, y: 10 }],
            cell_units: 1024,
            paint: lm_core::world::Paint::new(),
        }
    }

    /// A member's view over the wall world: an unheld beacon in front, an
    /// enemy-held beacon behind the wall, a fresh enemy diagonally ahead, a
    /// stale memory of another, one ally in range and the leader far away.
    fn base_view() -> ScoreView {
        ScoreView {
            paint: Vec::new(),
            heard: Vec::new(),
            tick: 100,
            me: snap(3, 2, 2, Role::Member),
            squad: vec![
                snap(0, 12, 12, Role::Leader),
                snap(1, 2, 4, Role::Member),
                snap(3, 2, 2, Role::Member),
            ],
            enemies: vec![
                EnemySeen {
                    id: 4,
                    cell: CellXY { x: 3, y: 3 },
                    age_ticks: 2,
                },
                EnemySeen {
                    id: 5,
                    cell: CellXY { x: 9, y: 9 },
                    age_ticks: 40,
                },
            ],
            beacons: vec![
                BeaconSnap {
                    cell: CellXY { x: 4, y: 2 },
                    holder: None,
                },
                BeaconSnap {
                    cell: CellXY { x: 10, y: 2 },
                    holder: Some(1),
                },
            ],
            score: vec![0, 0],
            squad_id: 0,
        }
    }

    #[test]
    fn facts_are_deterministic() {
        let a = facts(&base_view(), &wall_world());
        let b = facts(&base_view(), &wall_world());
        assert_eq!(a.text, b.text);
        assert_eq!(a.fields, b.fields);
        assert_eq!(a.key, b.key);
        assert_eq!(a.key, situation_key(&a.fields));
    }

    #[test]
    fn standing_in_the_other_teams_paint_is_stated() {
        // The agent stands in the enemy's paint, and the paint is slowing
        // it: the facts say so in one line.
        let mut view = base_view();
        view.paint = vec![PaintSnap {
            cell: CellXY { x: 2, y: 2 },
            owner: 1,
            wet_q8: 255,
            splats: 1,
        }];
        view.me.paint_slow_q8 = 128;
        let f = facts(&view, &wall_world());
        assert!(
            f.text
                .contains("You are standing in the other team's paint"),
            "the paint is stated: {}",
            f.text
        );
        assert!(
            f.text.contains("it is slowing you down"),
            "the slowdown is stated: {}",
            f.text
        );
        assert_eq!(f.fields.get("me.in_enemy_paint"), Some(&1));
        assert_eq!(f.fields.get("me.paint_slow_q8"), Some(&128));
    }

    #[test]
    fn the_paint_of_your_own_squad_is_not_stated_as_the_enemys() {
        let mut view = base_view();
        view.paint = vec![PaintSnap {
            cell: CellXY { x: 2, y: 2 },
            owner: 0,
            wet_q8: 255,
            splats: 1,
        }];
        let f = facts(&view, &wall_world());
        assert!(
            !f.text.contains("other team's paint"),
            "own paint is not the enemy's: {}",
            f.text
        );
        assert!(!f.fields.contains_key("me.in_enemy_paint"));
    }

    #[test]
    fn the_paint_is_stated_without_a_slowdown_when_there_is_none() {
        // The paint rules can be off: the agent is in the enemy's paint,
        // but nothing is slowing it, and the facts do not say it is.
        let mut view = base_view();
        view.paint = vec![PaintSnap {
            cell: CellXY { x: 2, y: 2 },
            owner: 1,
            wet_q8: 255,
            splats: 1,
        }];
        view.me.paint_slow_q8 = 0;
        let f = facts(&view, &wall_world());
        assert!(f
            .text
            .contains("You are standing in the other team's paint"));
        assert!(!f.text.contains("slowing you down"));
        assert_eq!(f.fields.get("me.paint_slow_q8"), Some(&0));
    }

    /// The facts under a shared-target rule.
    fn shared_facts(view: &ScoreView, world: &World, rule: SharedRule) -> Facts {
        Facts::from_view(
            view,
            world,
            0,
            &AgentsConfig::default(),
            &SharedConfig {
                rule,
                ..SharedConfig::default()
            },
        )
    }

    #[test]
    fn the_default_states_no_shared_target() {
        let f = facts(&base_view(), &wall_world());
        assert!(!f.text.contains("shared target"));
        assert!(!f.fields.contains_key("shared.target"));
        // The shipped target is still the nearest unheld beacon.
        assert!(f
            .text
            .contains("it is the nearest beacon your squad does not hold"));
    }

    #[test]
    fn a_shared_target_is_stated_for_both_squads() {
        let view = base_view();
        let f = shared_facts(&view, &wall_world(), SharedRule::Free);
        // Beacon 0 is unheld, so it is the beacon no squad holds.
        assert!(f.text.contains("The shared target is beacon 0"));
        assert!(f
            .text
            .contains("it is the shared target: the beacon no squad holds"));
        assert!(f.text.contains("Beacon 1"));
        assert!(f.text.contains("it is not the shared target"));
        assert_eq!(f.fields.get("shared.target"), Some(&0));
        // The doctrine's own target is not stated any more.
        assert!(!f.text.contains("nearest beacon your squad does not hold"));
    }

    #[test]
    fn the_shared_target_is_the_same_for_both_squads() {
        let mut other = base_view();
        other.squad_id = 1;
        other.me = snap(7, 12, 12, Role::Leader);
        let mine = shared_facts(&base_view(), &wall_world(), SharedRule::Free);
        let theirs = shared_facts(&other, &wall_world(), SharedRule::Free);
        assert_eq!(
            mine.fields.get("shared.target"),
            theirs.fields.get("shared.target")
        );
        assert!(theirs.text.contains("The shared target is beacon 0"));
    }

    #[test]
    fn the_centre_rule_names_the_centre_beacon() {
        let f = shared_facts(&base_view(), &wall_world(), SharedRule::Centre);
        // Beacon 0 at (4, 2) and beacon 1 at (10, 2) are both 6 cells from
        // each other, and the other pair is much further away, so beacon 0
        // is the most central of the two.
        assert_eq!(f.fields.get("shared.target"), Some(&0));
        assert!(f.text.contains("the most central of the beacons"));
    }

    #[test]
    fn the_ghost_rule_states_the_field() {
        let f = shared_facts(&base_view(), &wall_world(), SharedRule::Ghost);
        assert!(f.text.contains("You are pulled toward beacon 0"));
        assert!(f.text.contains("The pull reaches"));
        assert!(f.fields.contains_key("shared.pull"));
        assert!(f.fields.contains_key("shared.band"));
        // The field is in the key, so the situation moves with it.
        let plain = shared_facts(&base_view(), &wall_world(), SharedRule::Free);
        assert_ne!(f.key, plain.key);
    }

    #[test]
    fn a_shared_target_changes_the_key_but_not_the_shipped_one() {
        let plain = facts(&base_view(), &wall_world());
        let shared = shared_facts(&base_view(), &wall_world(), SharedRule::Free);
        assert_ne!(plain.key, shared.key);
    }

    #[test]
    fn facts_state_numbers_not_grids() {
        let f = facts(&base_view(), &wall_world());
        // Numbers, in cells, from code: positions and distances.
        assert!(f.text.contains("cells away"));
        assert!(f.text.contains("column"));
        assert!(f.text.contains("is 1 cells away"));
        // No grid glyphs: no wall character, no floor runs.
        assert!(!f.text.contains('#'), "the wall glyph renders a grid");
        assert!(!f.text.contains(".."), "a floor run renders a grid");
    }

    #[test]
    fn stale_enemies_omitted() {
        let view = ScoreView {
            paint: Vec::new(),
            enemies: vec![
                EnemySeen {
                    id: 7,
                    cell: CellXY { x: 3, y: 2 },
                    age_ticks: 6,
                },
                EnemySeen {
                    id: 9,
                    cell: CellXY { x: 4, y: 3 },
                    age_ticks: 7,
                },
            ],
            ..base_view()
        };
        let f = facts(&view, &wall_world());
        // Age 6 is the last fresh age; age 7 is stale and omitted.
        assert!(f.text.contains("Enemy 7"));
        assert!(f.fields.contains_key("enemy.7.dist"));
        assert!(!f.text.contains("Enemy 9"));
        assert!(!f.text.contains("column 4, row 3"));
        assert!(!f.fields.contains_key("enemy.9.dist"));
    }

    #[test]
    fn key_changes_with_situation() {
        let world = wall_world();
        let a = facts(&base_view(), &world);

        // One step south is a different situation.
        let moved = ScoreView {
            paint: Vec::new(),
            me: snap(3, 2, 3, Role::Member),
            squad: vec![
                snap(0, 12, 12, Role::Leader),
                snap(1, 2, 4, Role::Member),
                snap(3, 2, 3, Role::Member),
            ],
            ..base_view()
        };
        let b = facts(&moved, &world);
        assert_ne!(a.key, b.key);
        assert_ne!(a.text, b.text);

        // A later tick, the same situation: the key addresses the
        // situation, not the clock (Task 5 detects novelty through it).
        let later = ScoreView {
            paint: Vec::new(),
            tick: 200,
            ..base_view()
        };
        assert_eq!(a.key, facts(&later, &world).key);
    }

    #[test]
    fn stale_and_range_come_from_config() {
        // Task 5 follow-up: the stale window and the ally range are
        // parameters (defaults 6 and 4), not constants.
        let world = wall_world();
        let cfg = AgentsConfig {
            stale_enemy_ticks: 3,
            ally_range_cells: 1,
            ..AgentsConfig::default()
        };

        // Under the tighter config the age-2 enemy is fresh but the age-4
        // memory would be stale, and the ally 2 cells out is out of range.
        let mut view = base_view();
        view.enemies[1].age_ticks = 4;
        let f = Facts::from_view(&view, &world, 0, &cfg, &SharedConfig::default());
        assert!(f.text.contains("Enemy 4"));
        assert!(!f.text.contains("Enemy 5"), "age 4 is stale at window 3");
        assert!(!f.fields.contains_key("enemy.5.dist"));
        assert!(!f.text.contains("Ally 1"), "2 cells is out of range at 1");
        assert!(!f.fields.contains_key("ally.1.dist"));
        assert!(f.text.contains("No allies are within 1 cells."));

        // The defaults keep both.
        let f = Facts::from_view(
            &view,
            &world,
            0,
            &AgentsConfig::default(),
            &SharedConfig::default(),
        );
        assert!(f.text.contains("Enemy 5"), "age 4 is fresh at window 6");
        assert!(f.fields.contains_key("ally.1.dist"));
    }

    #[test]
    fn position_beacons_allies_and_status_are_stated() {
        let world = wall_world();
        let f = facts(&base_view(), &world);
        // The wall detour's exact step count comes from the same jps the
        // implementation calls (it is A*-verified in lm-core); the test
        // pins that the number lands in the sentence, correctly formatted.
        let detour = jps(&world.grid, CellXY { x: 2, y: 2 }, CellXY { x: 10, y: 2 })
            .map(|p| p.len() as i64 - 1)
            .expect("the corridor connects the halves");
        assert!(detour > 10, "the wall forces a detour, got {detour}");
        assert_eq!(
            f.text,
            format!(
                "You are agent 3, the member, of squad 0 at column 2, row 2 \
                 (row 0 is north). \
                 Beacon 0 at column 4, row 2 is unheld; it is the nearest \
                 beacon your squad does not hold; a walking path from \
                 you is 2 steps long to the east. \
                 Beacon 1 at column 10, row 2 is held by enemy squad 1; it \
                 is not the nearest beacon your squad does not hold; a \
                 walking path from you is {detour} steps long to the east. \
                 You are not standing on a beacon. \
                 Enemy 4, last seen 2 ticks ago at column 3, row 3, is 1 \
                 cells away and can see you (clear line of sight). \
                 1 enemy is within 6 cells of you, and it can see you. \
                 Ally 1 (member) is 2 cells away at column 2, row 4. \
                 You have 1 ally within 4 cells of you. \
                 You are not stunned."
            )
        );
        // The wall detour is computed, not estimated: beacon 1 costs more
        // than the open-ground beacon 0.
        assert_eq!(f.fields["beacon.0.steps"], 2);
        assert_eq!(f.fields["beacon.0.bearing"], 1); // east
        assert_eq!(f.fields["beacon.1.steps"], detour);
        assert_eq!(f.fields["beacon.1.holder"], 1);
        // The doctrine's numbers, as the facts state them.
        assert_eq!(f.fields["beacon.nearest_unheld"], 0);
        assert_eq!(f.fields["me.on_beacon"], -1);
        assert_eq!(f.fields["me.on_held_beacon"], 0);
        assert_eq!(f.fields["enemy.near"], 1);
        assert_eq!(f.fields["enemy.seeing"], 1);
        assert_eq!(f.fields["ally.near"], 1);
        // Allies: in range stated, out of range and self omitted.
        assert_eq!(f.fields["ally.1.dist"], 2);
        assert!(!f.text.contains("Ally 0"));
        assert!(!f.text.contains("Ally 3"));
        assert!(!f.fields.contains_key("ally.0.dist"));
        assert_eq!(f.fields["me.stunned"], 0);
    }

    #[test]
    fn the_doctrine_facts_are_stated_in_its_words() {
        // Every term of the doctrine in the default harness must be a
        // fact the builder states, in the words the rule uses (E4c).
        let world = wall_world();
        let mut view = base_view();
        // Standing on the beacon my squad holds, with one ally in range
        // and one enemy in sight: the doctrine's second branch.
        view.me.cell = CellXY { x: 4, y: 2 };
        view.squad[2].cell = CellXY { x: 4, y: 2 };
        view.beacons[0].holder = Some(0);
        let f = facts(&view, &world);
        assert!(f
            .text
            .contains("You are standing on beacon 0, which your squad holds."));
        assert_eq!(f.fields["me.on_beacon"], 0);
        assert_eq!(f.fields["me.on_held_beacon"], 1);
        // The beacon my squad holds is not the doctrine's target, and the
        // one it does not hold is named as the nearest.
        assert!(f.text.contains(
            "Beacon 0 at column 4, row 2 is held by your squad; you are standing on it."
        ));
        assert_eq!(f.fields["beacon.nearest_unheld"], 1);
        assert!(f
            .text
            .contains("it is the nearest beacon your squad does not hold"));

        // With every beacon held by my squad there is no target, and the
        // facts say so.
        view.beacons[1].holder = Some(0);
        let f = facts(&view, &world);
        assert_eq!(f.fields["beacon.nearest_unheld"], -1);
        assert!(f.text.contains(
            "Your squad holds every beacon; there is no beacon your squad does not hold."
        ));
    }

    #[test]
    fn holder_and_stun_wording() {
        let world = wall_world();
        let mut view = base_view();
        view.beacons[0].holder = Some(0);
        view.me.stunned = true;
        view.squad[1].stunned = true;
        let f = facts(&view, &world);
        assert!(f
            .text
            .contains("Beacon 0 at column 4, row 2 is held by your squad"));
        assert!(f
            .text
            .contains("Ally 1 (member) is 2 cells away at column 2, row 4 and is stunned"));
        assert!(f.text.contains("You are stunned."));
    }

    #[test]
    fn empty_situations_say_so() {
        let view = ScoreView {
            paint: Vec::new(),
            me: snap(0, 2, 2, Role::Leader),
            squad: vec![snap(0, 2, 2, Role::Leader)],
            enemies: vec![],
            beacons: vec![],
            ..base_view()
        };
        let f = facts(&view, &wall_world());
        assert!(f.text.contains("You know of no fresh enemies."));
        assert!(f.text.contains("No allies are within 4 cells."));
    }

    #[test]
    fn unreachable_beacon_states_no_path() {
        // A beacon sealed in a wall box: there for the menu, unreachable
        // under the strict movement model.
        let walls = [
            (3, 1),
            (4, 1),
            (5, 1),
            (3, 2),
            (5, 2),
            (3, 3),
            (4, 3),
            (5, 3),
        ];
        let world = World {
            grid: grid_with(13, 13, &walls),
            beacons: vec![CellXY { x: 4, y: 2 }],
            spawns: vec![CellXY { x: 0, y: 0 }],
            cell_units: 1024,
            paint: lm_core::world::Paint::new(),
        };
        let mut view = base_view();
        view.beacons = vec![BeaconSnap {
            cell: CellXY { x: 4, y: 2 },
            holder: None,
        }];
        let f = facts(&view, &world);
        assert!(f.text.contains(
            "Beacon 0 at column 4, row 2 is unheld; you cannot go there, there is no walking path to it."
        ));
        assert_eq!(f.fields["beacon.0.steps"], -1);
        assert_eq!(f.fields["beacon.0.bearing"], -1);
        // A beacon you cannot walk to is not the doctrine's target.
        assert_eq!(f.fields["beacon.nearest_unheld"], -1);
        assert!(f.text.contains(
            "Every beacon your squad does not hold is out of reach; there is no walking path to it."
        ));
    }
}

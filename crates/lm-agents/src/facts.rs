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

/// The compass words in the order their field indices run; row 0 is
/// north, so +y is south. "here" is a zero step.
const COMPASS: [&str; 5] = ["north", "east", "south", "west", "here"];

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
    pub fn from_view(view: &ScoreView, world: &World, me_squad: u8, cfg: &AgentsConfig) -> Facts {
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

        // Each beacon: holder, walking-path length and bearing, all
        // computed by code (the path in cells from JPS, the compass word
        // from the offsets).
        for (i, beacon) in view.beacons.iter().enumerate() {
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
            match jps(&world.grid, me, beacon.cell) {
                Some(path) => {
                    let steps = path.len() as i64 - 1;
                    let bearing = bearing(
                        i32::from(beacon.cell.x) - i32::from(me.x),
                        i32::from(beacon.cell.y) - i32::from(me.y),
                    );
                    fields.insert(format!("beacon.{i}.steps"), steps);
                    fields.insert(format!("beacon.{i}.bearing"), bearing);
                    if steps == 0 {
                        lines.push(format!(
                            "Beacon {i} at column {}, row {} is {holder_word}; \
                             you are standing on it.",
                            beacon.cell.x, beacon.cell.y
                        ));
                    } else {
                        lines.push(format!(
                            "Beacon {i} at column {}, row {} is {holder_word}; \
                             a walking path from you is {steps} steps long \
                             to the {}.",
                            beacon.cell.x, beacon.cell.y, COMPASS[bearing as usize]
                        ));
                    }
                }
                None => {
                    // Unreachable is a fact, not a failure to compute one.
                    fields.insert(format!("beacon.{i}.steps"), -1);
                    fields.insert(format!("beacon.{i}.bearing"), -1);
                    lines.push(format!(
                        "Beacon {i} at column {}, row {} is {holder_word}; \
                         there is no walking path to it.",
                        beacon.cell.x, beacon.cell.y
                    ));
                }
            }
        }

        // Each fresh enemy: distance and line of sight from where I stand.
        // A stale memory is omitted entirely — better no fact than a
        // seven-tick-old position phrased as news.
        let mut any_enemy = false;
        for e in fresh_enemies(view, cfg.stale_enemy_ticks) {
            any_enemy = true;
            let dist = chebyshev(me, e.cell);
            let sighted = los::clear(&world.grid, me, e.cell);
            fields.insert(format!("enemy.{}.x", e.id), i64::from(e.cell.x));
            fields.insert(format!("enemy.{}.y", e.id), i64::from(e.cell.y));
            fields.insert(format!("enemy.{}.age", e.id), i64::from(e.age_ticks));
            fields.insert(format!("enemy.{}.dist", e.id), dist);
            fields.insert(format!("enemy.{}.los", e.id), i64::from(sighted));
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

        // Allies within range: who is near enough to matter, myself
        // excluded (I am the position sentence).
        let mut any_ally = false;
        for ally in &view.squad {
            if ally.id == view.me.id {
                continue;
            }
            let dist = chebyshev(me, ally.cell);
            if dist > i64::from(cfg.ally_range_cells) {
                continue;
            }
            any_ally = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentsConfig;
    use lm_core::header::Role;
    use lm_core::view::{AgentSnap, BeaconSnap, EnemySeen};
    use lm_core::world::{jps, CellKind, CellXY, Grid};

    /// The facts of a squad-0 view under the default config.
    fn facts(view: &ScoreView, world: &World) -> Facts {
        Facts::from_view(view, world, 0, &AgentsConfig::default())
    }

    /// An unstunned snapshot at a cell.
    fn snap(id: u16, x: u16, y: u16, role: Role) -> AgentSnap {
        AgentSnap {
            id,
            cell: CellXY { x, y },
            role,
            stunned: false,
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
        }
    }

    /// A member's view over the wall world: an unheld beacon in front, an
    /// enemy-held beacon behind the wall, a fresh enemy diagonally ahead, a
    /// stale memory of another, one ally in range and the leader far away.
    fn base_view() -> ScoreView {
        ScoreView {
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
        let f = Facts::from_view(&view, &world, 0, &cfg);
        assert!(f.text.contains("Enemy 4"));
        assert!(!f.text.contains("Enemy 5"), "age 4 is stale at window 3");
        assert!(!f.fields.contains_key("enemy.5.dist"));
        assert!(!f.text.contains("Ally 1"), "2 cells is out of range at 1");
        assert!(!f.fields.contains_key("ally.1.dist"));
        assert!(f.text.contains("No allies are within 1 cells."));

        // The defaults keep both.
        let f = Facts::from_view(&view, &world, 0, &AgentsConfig::default());
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
                 Beacon 0 at column 4, row 2 is unheld; a walking path from \
                 you is 2 steps long to the east. \
                 Beacon 1 at column 10, row 2 is held by enemy squad 1; a \
                 walking path from you is {detour} steps long to the east. \
                 Enemy 4, last seen 2 ticks ago at column 3, row 3, is 1 \
                 cells away and can see you (clear line of sight). \
                 Ally 1 (member) is 2 cells away at column 2, row 4. \
                 You are not stunned."
            )
        );
        // The wall detour is computed, not estimated: beacon 1 costs more
        // than the open-ground beacon 0.
        assert_eq!(f.fields["beacon.0.steps"], 2);
        assert_eq!(f.fields["beacon.0.bearing"], 1); // east
        assert_eq!(f.fields["beacon.1.steps"], detour);
        assert_eq!(f.fields["beacon.1.holder"], 1);
        // Allies: in range stated, out of range and self omitted.
        assert_eq!(f.fields["ally.1.dist"], 2);
        assert!(!f.text.contains("Ally 0"));
        assert!(!f.text.contains("Ally 3"));
        assert!(!f.fields.contains_key("ally.0.dist"));
        assert_eq!(f.fields["me.stunned"], 0);
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
        };
        let mut view = base_view();
        view.beacons = vec![BeaconSnap {
            cell: CellXY { x: 4, y: 2 },
            holder: None,
        }];
        let f = facts(&view, &world);
        assert!(f
            .text
            .contains("Beacon 0 at column 4, row 2 is unheld; there is no walking path to it."));
        assert_eq!(f.fields["beacon.0.steps"], -1);
        assert_eq!(f.fields["beacon.0.bearing"], -1);
    }
}

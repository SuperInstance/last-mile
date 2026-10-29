//! What a squad knows and the menus its agents choose from.
//!
//! Fog of war (spec §4): an agent only knows what its squad can see or has
//! already seen. [`SquadKnowledge`] is that memory — one bit per explored
//! cell, plus the last place each enemy was sighted — and it only ever
//! grows: a revealed cell stays revealed, a sighted enemy stays remembered.
//! [`ScoreView`] projects the round through one agent's squad knowledge; it
//! is the summary every reflex and decision model is shown (spec §5).
//!
//! Menus are content-addressed: [`menu`] derives the option list from a
//! view and its id from the options alone, so the same options always share
//! an id, and a lane conditioned on that id (D43) only resets when the menu
//! truly changes. [`option_cmd`] maps a chosen option back to the motor
//! command it stands for.

use crate::agent::{Agent, MotorCmd};
use crate::decision::Tick;
use crate::hash::{Fnv64, StateHash};
use crate::header::Role;
use crate::params::Params;
use crate::world::los;
use crate::world::{CellXY, Grid, World};
use std::collections::BTreeMap;

/// Bits per word of the explored-cell bitset.
const BITS_PER_WORD: usize = 64;

/// One option of a squad menu (D23/D37).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MenuOption {
    GoToBeacon(u8),
    Hold,
    FollowLeader,
    Retreat,
}

impl StateHash for MenuOption {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            MenuOption::GoToBeacon(b) => {
                h.write(&[0]);
                h.write(&[*b]);
            }
            MenuOption::Hold => h.write(&[1]),
            MenuOption::FollowLeader => h.write(&[2]),
            MenuOption::Retreat => h.write(&[3]),
        }
    }
}

/// What one squad knows about the map and its enemies, fog of war included
/// (spec §4).
///
/// The knowledge is the squad's shared memory: [`SquadKnowledge::update`]
/// grows it every view update, and nothing shrinks it — an explored cell
/// stays explored, and a sighted enemy is remembered at the cell and tick
/// of its last sighting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SquadKnowledge {
    /// The explored cells as a bitset over the grid: bit `y * w + x`.
    pub explored: Vec<u64>,
    /// For each enemy sighted: the cell it was last seen in, and the tick
    /// of that sighting.
    pub last_seen: BTreeMap<u16, (CellXY, Tick)>,
    /// The grid width the bitset is laid out over.
    w: u16,
    /// The grid height the bitset is laid out over.
    h: u16,
}

impl SquadKnowledge {
    /// Empty knowledge for a round played on `grid`: nothing explored,
    /// nobody sighted. Updates are expected to come from that same grid.
    pub fn new(grid: &Grid) -> Self {
        let bits = usize::from(grid.w) * usize::from(grid.h);
        SquadKnowledge {
            explored: vec![0; bits.div_ceil(BITS_PER_WORD)],
            last_seen: BTreeMap::new(),
            w: grid.w,
            h: grid.h,
        }
    }

    /// Grow the knowledge from where the squad's members stand on tick
    /// `now`.
    ///
    /// A cell is revealed — and stays revealed — when it lies within
    /// `view_range_cells` of some member (Chebyshev distance, the metric of
    /// the 8-connected grid) and the line of sight to it is clear; stunned
    /// members still see. An enemy is (re)recorded in `last_seen` under the
    /// same test: within range and in sight of some member, at the cell it
    /// occupies now. A recorded enemy is remembered even after it slips
    /// back out of sight — the view carries the memory on, ageing it.
    pub fn update(&mut self, world: &World, agents: &[Agent], squad: u8, now: Tick, p: &Params) {
        let grid = &world.grid;
        // Sight past the grid's edge can reveal nothing, so clamping the
        // window to the grid keeps the walk bounded at any parameter.
        let range = i32::from(p.view_range_cells).min(i32::from(grid.w.max(grid.h)));
        let members: Vec<CellXY> = agents
            .iter()
            .filter(|a| a.squad == squad)
            .map(|a| grid.cell_of(a.pos, world.cell_units))
            .collect();

        for c in visible_cells(grid, &members, range) {
            self.set_explored(c);
        }
        for a in agents.iter().filter(|a| a.squad != squad) {
            let cell = grid.cell_of(a.pos, world.cell_units);
            if members.iter().any(|&eye| visible(grid, eye, cell, range)) {
                self.last_seen.insert(a.id, (cell, now));
            }
        }
    }

    /// Whether the squad has ever seen cell `c`. Cells outside the round's
    /// grid are never explored.
    pub fn is_explored(&self, c: CellXY) -> bool {
        let Some(i) = self.bit_index(c) else {
            return false;
        };
        self.explored[i / BITS_PER_WORD] >> (i % BITS_PER_WORD) & 1 == 1
    }

    /// The bitset index of `c`, or `None` outside the grid.
    fn bit_index(&self, c: CellXY) -> Option<usize> {
        if c.x >= self.w || c.y >= self.h {
            return None;
        }
        Some(usize::from(c.y) * usize::from(self.w) + usize::from(c.x))
    }

    /// Reveal one cell.
    fn set_explored(&mut self, c: CellXY) {
        if let Some(i) = self.bit_index(c) {
            self.explored[i / BITS_PER_WORD] |= 1 << (i % BITS_PER_WORD);
        }
    }
}

impl StateHash for SquadKnowledge {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.w);
        h.u16(self.h);
        h.u32(self.explored.len() as u32);
        for word in &self.explored {
            h.u64(*word);
        }
        h.u32(self.last_seen.len() as u32);
        for (id, (cell, tick)) in &self.last_seen {
            h.u16(*id);
            cell.hash_into(h);
            h.u64(*tick);
        }
    }
}

/// Whether `target` is in sight from `eye`: within Chebyshev `range` cells
/// and the line between them is clear.
fn visible(grid: &Grid, eye: CellXY, target: CellXY, range: i32) -> bool {
    let dx = i32::from(target.x) - i32::from(eye.x);
    let dy = i32::from(target.y) - i32::from(eye.y);
    dx.abs() <= range && dy.abs() <= range && los::clear(grid, eye, target)
}

/// Every in-bounds cell currently in sight of some `eye`.
fn visible_cells(grid: &Grid, eyes: &[CellXY], range: i32) -> Vec<CellXY> {
    let mut cells = Vec::new();
    for &eye in eyes {
        let (ex, ey) = (i32::from(eye.x), i32::from(eye.y));
        for dy in -range..=range {
            for dx in -range..=range {
                let (x, y) = (ex + dx, ey + dy);
                if !grid.in_bounds(x, y) {
                    continue;
                }
                let c = CellXY {
                    x: x as u16,
                    y: y as u16,
                };
                if los::clear(grid, eye, c) {
                    cells.push(c);
                }
            }
        }
    }
    cells
}

/// One agent as a view shows it: where it stands, its role, and whether it
/// is stunned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentSnap {
    pub id: u16,
    pub cell: CellXY,
    pub role: Role,
    pub stunned: bool,
}

/// An enemy as the squad last saw it: where, and how many ticks ago.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnemySeen {
    pub id: u16,
    pub cell: CellXY,
    pub age_ticks: u32,
}

/// A beacon of the round and, while a squad holds it, which one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeaconSnap {
    pub cell: CellXY,
    pub holder: Option<u8>,
}

/// The world as one agent's squad knows it on one tick (spec §5): the
/// summary every reflex and decision model is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreView {
    /// The tick the view was taken on.
    pub tick: Tick,
    /// The agent the view belongs to.
    pub me: AgentSnap,
    /// The whole squad in agent-id order, `me` included: squad-mates report
    /// in, so one's own squad is never fogged.
    pub squad: Vec<AgentSnap>,
    /// Every enemy the squad has sighted, in agent-id order, as last seen.
    /// A memory has no expiry (no parameter defines one), so each entry
    /// ages until the enemy is sighted again.
    pub enemies: Vec<EnemySeen>,
    /// The round's beacons in world order, with their holder if any.
    pub beacons: Vec<BeaconSnap>,
    /// One score per squad.
    pub score: Vec<i64>,
    /// The viewing agent's squad index. [`option_cmd`] needs it to find the
    /// squad's own spawn, and its pinned signature carries no `Params` to
    /// recompute it from the agent id.
    pub squad_id: u8,
}

impl ScoreView {
    /// Build the view of the round as `me`'s squad knows it at tick `now`.
    ///
    /// `agents` is the full roster in id order (it fills `squad`, in
    /// order); `knowledge` is `me`'s squad's knowledge (it fills `enemies`
    /// as aged memories); `scores` is one entry per squad; and
    /// `beacon_holders` mirrors `world.beacons` with the holding squad of
    /// each beacon, if any — the ruleset computes holders, this module only
    /// shows them.
    pub fn for_agent(
        me: &Agent,
        agents: &[Agent],
        world: &World,
        knowledge: &SquadKnowledge,
        scores: &[i64],
        beacon_holders: &[Option<u8>],
        now: Tick,
    ) -> ScoreView {
        let snap = |a: &Agent| AgentSnap {
            id: a.id,
            cell: world.grid.cell_of(a.pos, world.cell_units),
            role: a.role,
            stunned: a.stunned_until.is_some_and(|t| now < t),
        };
        ScoreView {
            tick: now,
            me: snap(me),
            squad: agents
                .iter()
                .filter(|a| a.squad == me.squad)
                .map(snap)
                .collect(),
            enemies: knowledge
                .last_seen
                .iter()
                .map(|(&id, &(cell, seen))| EnemySeen {
                    id,
                    cell,
                    age_ticks: u32::try_from(now.saturating_sub(seen)).unwrap_or(u32::MAX),
                })
                .collect(),
            beacons: world
                .beacons
                .iter()
                .enumerate()
                .map(|(i, &cell)| BeaconSnap {
                    cell,
                    holder: beacon_holders.get(i).copied().flatten(),
                })
                .collect(),
            score: scores.to_vec(),
            squad_id: me.squad,
        }
    }
}

/// A decision menu: the options, and the id that content-addresses them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The canonical hash of `options`; equal options always share it.
    pub id: u64,
    /// The options, in fixed order.
    pub options: Vec<MenuOption>,
}

/// The menu a decision is chosen from, built from `view`.
///
/// The options come in a fixed order — one [`MenuOption::GoToBeacon`] per
/// beacon of the view, in beacon order, then [`MenuOption::Hold`],
/// [`MenuOption::FollowLeader`] and [`MenuOption::Retreat`] — and the id is
/// the canonical hash of the options alone, so the same list carries the
/// same id at any tick, and a changed list (a beacon appearing or going)
/// gets a new one.
pub fn menu(view: &ScoreView) -> Menu {
    let mut options: Vec<MenuOption> = view
        .beacons
        .iter()
        .enumerate()
        .map(|(i, _)| MenuOption::GoToBeacon(i as u8))
        .collect();
    options.push(MenuOption::Hold);
    options.push(MenuOption::FollowLeader);
    options.push(MenuOption::Retreat);
    Menu {
        id: menu_id(&options),
        options,
    }
}

/// The content address of an option list: FNV-1a over the option count and
/// each option's canonical encoding.
fn menu_id(options: &[MenuOption]) -> u64 {
    let mut h = Fnv64::new();
    h.u32(options.len() as u32);
    for option in options {
        option.hash_into(&mut h);
    }
    h.finish()
}

/// The motor command a menu option stands for.
///
/// A beacon option addresses the view's beacon list — the very list the
/// menu was built from — `FollowLeader` walks to the squad leader's cell,
/// and `Retreat` walks to the squad's own spawn. An option whose target is
/// missing is refused as a hold; the motor never guesses (D26).
pub fn option_cmd(o: &MenuOption, view: &ScoreView, world: &World) -> MotorCmd {
    match o {
        MenuOption::GoToBeacon(b) => view
            .beacons
            .get(usize::from(*b))
            .map_or(MotorCmd::Hold, |b| MotorCmd::GoTo(b.cell)),
        MenuOption::Hold => MotorCmd::Hold,
        MenuOption::FollowLeader => view
            .squad
            .iter()
            .find(|s| s.role == Role::Leader)
            .map_or(MotorCmd::Hold, |leader| MotorCmd::GoTo(leader.cell)),
        MenuOption::Retreat => world
            .spawns
            .get(usize::from(view.squad_id))
            .map_or(MotorCmd::Hold, |&spawn| MotorCmd::GoTo(spawn)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Pos;
    use crate::rng::{Domain, Rng};
    use crate::world::gen::generate;
    use crate::world::CellKind;

    /// How many cells the squad has explored.
    fn popcount(k: &SquadKnowledge) -> u64 {
        k.explored.iter().map(|w| u64::from(w.count_ones())).sum()
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

    /// The centre position of a cell.
    fn center(world: &World, c: CellXY) -> Pos {
        world.grid.center(c, world.cell_units)
    }

    /// An 11×11 floor grid split by a wall column at x = 5, one spawn on
    /// each side, a beacon on the left.
    fn wall_world() -> World {
        let mut grid = Grid::new(11, 11);
        for y in 0..grid.h {
            grid.set(CellXY { x: 5, y }, CellKind::Wall);
        }
        World {
            grid,
            beacons: vec![CellXY { x: 2, y: 2 }],
            spawns: vec![CellXY { x: 2, y: 5 }, CellXY { x: 8, y: 5 }],
            cell_units: 1024,
        }
    }

    #[test]
    fn fog_hides_enemy_behind_wall() {
        let p = Params::default();
        let world = wall_world();
        let mut agents = [
            Agent::new(0, 0, Role::Member, center(&world, CellXY { x: 2, y: 5 })),
            Agent::new(4, 1, Role::Member, center(&world, CellXY { x: 8, y: 5 })),
        ];
        let mut k = SquadKnowledge::new(&world.grid);

        // The enemy is well within view range, but the wall column blocks
        // the line: it is in neither the knowledge nor the view, and its
        // cell is not explored.
        k.update(&world, &agents, 0, 0, &p);
        let view = ScoreView::for_agent(&agents[0], &agents, &world, &k, &[0, 0], &[], 0);
        assert!(view.enemies.is_empty());
        assert!(k.is_explored(CellXY { x: 2, y: 5 }));
        assert!(!k.is_explored(CellXY { x: 8, y: 5 }));

        // It steps around the wall into clear sight: present, at age 0.
        agents[1].pos = center(&world, CellXY { x: 4, y: 5 });
        k.update(&world, &agents, 0, 6, &p);
        let view = ScoreView::for_agent(&agents[0], &agents, &world, &k, &[0, 0], &[], 6);
        assert_eq!(
            view.enemies,
            vec![EnemySeen {
                id: 4,
                cell: CellXY { x: 4, y: 5 },
                age_ticks: 0,
            }]
        );
    }

    #[test]
    fn last_seen_ages() {
        let p = Params::default();
        let world = wall_world();
        let mut agents = [
            Agent::new(0, 0, Role::Member, center(&world, CellXY { x: 2, y: 5 })),
            Agent::new(4, 1, Role::Member, center(&world, CellXY { x: 4, y: 5 })),
        ];
        let mut k = SquadKnowledge::new(&world.grid);

        // Seen at t = 6, on the squad's side of the wall...
        k.update(&world, &agents, 0, 6, &p);
        assert_eq!(k.last_seen.get(&4), Some(&(CellXY { x: 4, y: 5 }, 6)));

        // ...hidden again from t = 12 on: the sight is remembered, and the
        // view at t = 30 reports it 24 ticks stale.
        agents[1].pos = center(&world, CellXY { x: 8, y: 5 });
        k.update(&world, &agents, 0, 12, &p);
        k.update(&world, &agents, 0, 30, &p);
        let view = ScoreView::for_agent(&agents[0], &agents, &world, &k, &[0, 0], &[], 30);
        assert_eq!(
            view.enemies,
            vec![EnemySeen {
                id: 4,
                cell: CellXY { x: 4, y: 5 },
                age_ticks: 24,
            }]
        );
    }

    #[test]
    fn explored_grows_monotonically() {
        let p = Params::default();
        let world = generate(11, &p);
        let spawn = world.grid.center(world.spawns[0], world.cell_units);
        let mut agents: Vec<Agent> = (0..4)
            .map(|i| {
                Agent::new(
                    i,
                    0,
                    if i == 0 { Role::Leader } else { Role::Member },
                    spawn,
                )
            })
            .collect();
        let mut goals = vec![world.beacons[0]; agents.len()];
        let mut k = SquadKnowledge::new(&world.grid);
        let mut rng = Rng::stream(11, Domain::Agent, 0);

        // 300 ticks of a round: the squad marches, and knowledge only ever
        // grows — never a bit lost between view updates.
        let mut explored: Option<u64> = None;
        let mut grew_after_first = false;
        for t in 0..300u64 {
            for (i, a) in agents.iter_mut().enumerate() {
                if t % 30 == 0 {
                    // Fresh marching orders: some random cell, reachable or
                    // not — the motor sorts out the walls.
                    goals[i] = CellXY {
                        x: rng.below(u32::from(world.grid.w)) as u16,
                        y: rng.below(u32::from(world.grid.h)) as u16,
                    };
                }
                a.move_step(MotorCmd::GoTo(goals[i]), &world, &p, t);
            }
            if t % u64::from(p.view_update_ticks) == 0 {
                k.update(&world, &agents, 0, t, &p);
                let now = popcount(&k);
                if let Some(prev) = explored {
                    assert!(now >= prev, "tick {t}: explored shrank from {prev}");
                    grew_after_first |= now > prev;
                }
                explored = Some(now);
            }
        }
        assert!(
            grew_after_first,
            "the squad learned nothing new after its first look"
        );
    }

    #[test]
    fn menu_id_stable_and_content_addressed() {
        fn view_with(n_beacons: usize, tick: Tick) -> ScoreView {
            ScoreView {
                tick,
                me: snap(0, 1, 1, Role::Leader),
                squad: vec![snap(0, 1, 1, Role::Leader)],
                enemies: vec![],
                beacons: (0..n_beacons)
                    .map(|i| BeaconSnap {
                        cell: CellXY {
                            x: 10 + i as u16,
                            y: 9,
                        },
                        holder: None,
                    })
                    .collect(),
                score: vec![0, 0],
                squad_id: 0,
            }
        }

        // The same options — read from views at different ticks — share
        // one id: the id addresses content, not the moment.
        let a = menu(&view_with(3, 0));
        let b = menu(&view_with(3, 100));
        assert_eq!(a.options, b.options);
        assert_eq!(a.id, b.id);
        assert_eq!(a.id, menu(&view_with(3, 7)).id);

        // A beacon more is an option more: a different menu, a new id.
        let c = menu(&view_with(4, 0));
        assert_ne!(a.options, c.options);
        assert_ne!(a.id, c.id);
    }

    #[test]
    fn retreat_goes_to_own_spawn() {
        let p = Params::default();
        let world = generate(3, &p);
        let roster = [
            Agent::new(0, 0, Role::Leader, center(&world, world.spawns[0])),
            Agent::new(4, 1, Role::Leader, center(&world, world.spawns[1])),
        ];
        let mut retreating = Vec::new();
        for a in &roster {
            let k = SquadKnowledge::new(&world.grid);
            let view = ScoreView::for_agent(a, &roster, &world, &k, &[0, 0], &[], 0);
            assert_eq!(
                option_cmd(&MenuOption::Retreat, &view, &world),
                MotorCmd::GoTo(world.spawns[usize::from(view.squad_id)])
            );
            retreating.push(option_cmd(&MenuOption::Retreat, &view, &world));
        }
        // Each squad retreats to its own spawn, not a shared one.
        assert_ne!(retreating[0], retreating[1]);
    }

    #[test]
    fn menu_options_are_in_fixed_order() {
        let view = ScoreView {
            tick: 0,
            me: snap(2, 1, 1, Role::Member),
            squad: vec![snap(0, 1, 1, Role::Leader), snap(2, 1, 1, Role::Member)],
            enemies: vec![],
            beacons: vec![
                BeaconSnap {
                    cell: CellXY { x: 10, y: 10 },
                    holder: None,
                },
                BeaconSnap {
                    cell: CellXY { x: 20, y: 10 },
                    holder: Some(0),
                },
            ],
            score: vec![],
            squad_id: 0,
        };
        assert_eq!(
            menu(&view).options,
            vec![
                MenuOption::GoToBeacon(0),
                MenuOption::GoToBeacon(1),
                MenuOption::Hold,
                MenuOption::FollowLeader,
                MenuOption::Retreat,
            ]
        );
    }

    #[test]
    fn option_cmd_maps_every_option() {
        let view = ScoreView {
            tick: 0,
            me: snap(2, 5, 5, Role::Member),
            squad: vec![snap(0, 1, 2, Role::Leader), snap(2, 5, 5, Role::Member)],
            enemies: vec![],
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
            score: vec![],
            squad_id: 1,
        };
        let world = generate(2, &Params::default());
        assert_eq!(option_cmd(&MenuOption::Hold, &view, &world), MotorCmd::Hold);
        assert_eq!(
            option_cmd(&MenuOption::GoToBeacon(1), &view, &world),
            MotorCmd::GoTo(CellXY { x: 20, y: 20 })
        );
        assert_eq!(
            option_cmd(&MenuOption::FollowLeader, &view, &world),
            MotorCmd::GoTo(CellXY { x: 1, y: 2 })
        );
        // An option with no target in the view is a hold, not a guess.
        assert_eq!(
            option_cmd(&MenuOption::GoToBeacon(9), &view, &world),
            MotorCmd::Hold
        );
    }

    #[test]
    fn reveal_needs_both_range_and_los() {
        let p = Params {
            view_range_cells: 5,
            ..Params::default()
        };
        let mut grid = Grid::new(12, 12);
        for y in 0..grid.h {
            grid.set(CellXY { x: 6, y }, CellKind::Wall);
        }
        let world = World {
            grid,
            beacons: vec![],
            spawns: vec![CellXY { x: 3, y: 3 }],
            cell_units: 1024,
        };
        let agents = [Agent::new(
            0,
            0,
            Role::Member,
            center(&world, CellXY { x: 3, y: 3 }),
        )];
        let mut k = SquadKnowledge::new(&world.grid);
        k.update(&world, &agents, 0, 0, &p);

        // In range with a clear line: explored, the eye's own cell too.
        assert!(k.is_explored(CellXY { x: 3, y: 3 }));
        assert!(k.is_explored(CellXY { x: 5, y: 3 }));
        assert!(k.is_explored(CellXY { x: 3, y: 8 }));
        // In range but behind the wall column: sight is blocked.
        assert!(!k.is_explored(CellXY { x: 8, y: 3 }));
        // A clear line, but one cell out of range.
        assert!(!k.is_explored(CellXY { x: 3, y: 9 }));
        // Outside the grid entirely.
        assert!(!k.is_explored(CellXY { x: 50, y: 50 }));
    }

    #[test]
    fn knowledge_hash_covers_what_was_seen() {
        let p = Params::default();
        let world = wall_world();
        let mut a = SquadKnowledge::new(&world.grid);
        let mut b = SquadKnowledge::new(&world.grid);
        let mut agents = [
            Agent::new(0, 0, Role::Member, center(&world, CellXY { x: 2, y: 5 })),
            Agent::new(4, 1, Role::Member, center(&world, CellXY { x: 4, y: 5 })),
        ];

        // Two squads that saw the same things hash the same.
        a.update(&world, &agents, 0, 6, &p);
        b.update(&world, &agents, 0, 6, &p);
        assert_eq!(a.state_hash(), b.state_hash());

        // Squad b's member then walks one cell east and sees a little more
        // of the map: the state, and so the hash, moves.
        agents[0].pos = center(&world, CellXY { x: 3, y: 5 });
        b.update(&world, &agents, 0, 12, &p);
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn for_agent_snapshots_the_squad() {
        let world = wall_world();
        let mut agents = [
            Agent::new(0, 0, Role::Leader, center(&world, CellXY { x: 2, y: 5 })),
            Agent::new(1, 0, Role::Member, center(&world, CellXY { x: 3, y: 5 })),
            Agent::new(4, 1, Role::Member, center(&world, CellXY { x: 8, y: 5 })),
        ];
        agents[0].stunned_until = Some(9);
        let k = SquadKnowledge::new(&world.grid);

        let view = ScoreView::for_agent(&agents[1], &agents, &world, &k, &[3, -7], &[Some(1)], 5);
        assert_eq!(view.tick, 5);
        assert_eq!(view.me, snap(1, 3, 5, Role::Member));
        // The whole squad in id order, me included; the leader's stun (until
        // tick 9) shows at t = 5.
        let mut leader = snap(0, 2, 5, Role::Leader);
        leader.stunned = true;
        assert_eq!(view.squad, vec![leader, snap(1, 3, 5, Role::Member)]);
        // The enemy is not a squad member: it can only appear as a sighted
        // memory in `enemies`.
        assert_eq!(view.enemies, vec![]);
        assert_eq!(
            view.beacons,
            vec![BeaconSnap {
                cell: CellXY { x: 2, y: 2 },
                holder: Some(1),
            }]
        );
        assert_eq!(view.score, vec![3, -7]);
        assert_eq!(view.squad_id, 0);
    }
}

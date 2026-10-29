//! Agents: the movable bodies of a round, their intents with safe branches
//! (D45) and the motor that walks them along the navigation grid.
//!
//! An agent is pure state. Each tick the sim asks it to [`Agent::choose_cmd`]
//! — stun first, then a held action, then the active intent (its safe branch
//! once expired or failed), then the reflex, then hold — and feeds the answer
//! to [`Agent::move_step`], which follows a cached Jump Point Search route
//! from cell centre to cell centre, axis-separated, and never enters a
//! non-walkable cell.
//!
//! The intent layering is FASTER's (D45): a committed [`Intent`] carries an
//! optimistic `plan` that may cross fog, plus a `safe` branch of known-free
//! cells. The plan plays until its TTL runs out or something refuses it;
//! from then on the agent takes its own safe branch, never a generic
//! default. Plans and safe branches are waypoint lists — the motor routes
//! between waypoints on the real grid, so an optimistic plan that crosses a
//! wall is walked around, not through.

use crate::decision::{Action, Intent, Tick};
use crate::fixed::{angle_to, Angle, Pos};
use crate::hash::{Fnv64, StateHash};
use crate::header::Role;
use crate::lane::LaneConditioner;
use crate::params::Params;
use crate::view::MenuOption;
use crate::world::jps;
use crate::world::{CellXY, World};
use std::collections::BTreeMap;

/// What the motor is asked to do this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotorCmd {
    /// Walk to a cell of the navigation grid.
    GoTo(CellXY),
    /// Stand still.
    Hold,
}

/// An [`Intent`] as the agent holds it: both branches, when the plan dies,
/// and how far along the active branch the agent has walked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveIntent {
    /// The optimistic plan: it may cross cells nobody has seen.
    pub plan: Vec<CellXY>,
    /// The safe branch: known-free cells only, ending in cover or a hold.
    pub safe: Vec<CellXY>,
    /// The tick the plan dies on; from then the safe branch plays.
    pub expires: Tick,
    /// Whether the agent is already on its safe branch.
    pub on_safe: bool,
    /// The index of the next waypoint of the active branch.
    pub idx: usize,
}

impl ActiveIntent {
    /// The cells the agent is currently committed to: the plan, or the safe
    /// branch once the intent has expired or failed.
    fn branch(&self) -> &[CellXY] {
        if self.on_safe {
            &self.safe
        } else {
            &self.plan
        }
    }
}

/// One agent of a round: identity, body, orders and everything the motor
/// caches between ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub id: u16,
    pub squad: u8,
    pub role: Role,
    pub pos: Pos,
    pub facing: Angle,
    /// The tick the stun ends on; the agent holds while `now < it`.
    pub stunned_until: Option<Tick>,
    /// The action pinned by an `ActionOn` decision, until an `ActionOff`.
    pub held: Option<Action>,
    /// The committed intent, if any.
    pub intent: Option<ActiveIntent>,
    /// One lane conditioner per gate (D38).
    pub lanes: BTreeMap<u8, LaneConditioner>,
    /// The menu option selected by the lanes and rolls (Task 8 wires it up).
    pub lane_cmd: Option<MenuOption>,
    /// The cached route: `(goal, path from jps, tick it was computed on)`.
    pub route: Option<(CellXY, Vec<CellXY>, Tick)>,
}

impl Agent {
    /// A fresh, unstunned agent standing at `pos` with no orders.
    pub fn new(id: u16, squad: u8, role: Role, pos: Pos) -> Agent {
        Agent {
            id,
            squad,
            role,
            pos,
            facing: Angle(0),
            stunned_until: None,
            held: None,
            intent: None,
            lanes: BTreeMap::new(),
            lane_cmd: None,
            route: None,
        }
    }

    /// Commit to an intent: its plan plays for `ttl_ticks` from `now`.
    ///
    /// The cached route served the previous commitment, so it is dropped;
    /// the motor re-plans on the next [`Agent::move_step`].
    pub fn set_intent(&mut self, i: &Intent, now: Tick) {
        self.intent = Some(ActiveIntent {
            plan: i.plan.clone(),
            safe: i.safe.clone(),
            expires: now + u64::from(i.ttl_ticks),
            on_safe: false,
            idx: 0,
        });
        self.route = None;
    }

    /// Refuse the plan and switch to the safe branch (D45).
    ///
    /// An intent that is already on its safe branch has nothing further to
    /// fall back to, so failing it again changes nothing.
    pub fn fail_intent(&mut self) {
        if let Some(act) = self.intent.as_mut() {
            if !act.on_safe {
                act.on_safe = true;
                act.idx = 0;
            }
        }
    }

    /// Decide this tick's motor command.
    ///
    /// Priority order:
    ///
    /// 1. a stun that has not ended yet holds;
    /// 2. a held action plays (a `Face` holds the body);
    /// 3. an active intent walks its plan — its safe branch once the TTL has
    ///    run out or the intent failed — and holds when the active branch is
    ///    exhausted, so a spent commitment still overrides the reflex;
    /// 4. the reflex, if it produced a command;
    /// 5. hold.
    pub fn choose_cmd(&mut self, now: Tick, reflex: Option<MotorCmd>) -> MotorCmd {
        if self.stunned_until.is_some_and(|t| now < t) {
            return MotorCmd::Hold;
        }
        if let Some(action) = &self.held {
            return match action {
                Action::GoTo(c) => MotorCmd::GoTo(*c),
                Action::Hold | Action::Face(_) => MotorCmd::Hold,
            };
        }
        self.sync_intent(now);
        if let Some(act) = &self.intent {
            let branch = if act.on_safe { &act.safe } else { &act.plan };
            return match branch.get(act.idx) {
                Some(&c) => MotorCmd::GoTo(c),
                None => MotorCmd::Hold,
            };
        }
        reflex.unwrap_or(MotorCmd::Hold)
    }

    /// Run one tick of the motor.
    ///
    /// `GoTo` follows a cached JPS route to the goal, recomputed when the
    /// goal changes, when the agent's cell has left the cached path, or
    /// every `repath_ticks`. Each tick moves at most `speed_upt` units
    /// toward the next cell centre, x first and the leftover budget along y,
    /// and a step whose destination cell is not walkable is refused
    /// outright — the agent never enters a wall, whatever it was told.
    pub fn move_step(&mut self, cmd: MotorCmd, world: &World, p: &Params, now: Tick) {
        self.sync_intent(now);
        self.advance_intent(world.grid.cell_of(self.pos, world.cell_units));

        let goal = match cmd {
            MotorCmd::GoTo(c) => c,
            MotorCmd::Hold => return,
        };
        let grid = &world.grid;
        let cell = grid.cell_of(self.pos, world.cell_units);
        if cell == goal {
            self.route = None;
            return;
        }
        let stale = match &self.route {
            None => true,
            Some((g, path, at)) => {
                *g != goal
                    || now.saturating_sub(*at) >= u64::from(p.repath_ticks)
                    || !path.contains(&cell)
            }
        };
        if stale {
            self.route = jps(grid, cell, goal).map(|path| (goal, path, now));
        }
        // An unreachable goal is refused: the agent stays put rather than
        // guess a direction (D26).
        let Some((_, path, _)) = &self.route else {
            return;
        };
        let target = match path.iter().position(|c| *c == cell) {
            Some(i) if i + 1 < path.len() => path[i + 1],
            _ => return,
        };

        let (tx, ty) = grid.center(target, world.cell_units).to_units();
        let (px, py) = self.pos.to_units();
        let mut budget = i64::from(p.speed_upt.max(0));
        let (mut nx, mut ny) = (px, py);
        let dx = tx - nx;
        if dx != 0 && budget > 0 {
            let step = dx.abs().min(budget);
            nx += if dx < 0 { -step } else { step };
            budget -= step;
        }
        let dy = ty - ny;
        if dy != 0 && budget > 0 {
            let step = dy.abs().min(budget);
            ny += if dy < 0 { -step } else { step };
        }
        let next = Pos::from_units(nx, ny);
        let next_cell = grid.cell_of(next, world.cell_units);
        if !grid.walkable(next_cell) {
            // Never enter a non-walkable cell; drop the route so the next
            // tick re-plans instead of butting against the same wall.
            self.route = None;
            return;
        }
        if nx != px || ny != py {
            self.facing = angle_to(nx - px, ny - py);
        }
        self.pos = next;
        self.advance_intent(next_cell);
    }

    /// Flip an expired intent onto its safe branch, restarting the walk.
    fn sync_intent(&mut self, now: Tick) {
        if let Some(act) = self.intent.as_mut() {
            if !act.on_safe && now >= act.expires {
                act.on_safe = true;
                act.idx = 0;
            }
        }
    }

    /// Move the intent's waypoint cursor past every waypoint the agent is
    /// already standing on.
    fn advance_intent(&mut self, cell: CellXY) {
        let Some(act) = self.intent.as_mut() else {
            return;
        };
        while act.branch().get(act.idx).is_some_and(|c| *c == cell) {
            act.idx += 1;
        }
    }
}

impl StateHash for ActiveIntent {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u32(self.plan.len() as u32);
        for c in &self.plan {
            c.hash_into(h);
        }
        h.u32(self.safe.len() as u32);
        for c in &self.safe {
            c.hash_into(h);
        }
        h.u64(self.expires);
        h.write(&[u8::from(self.on_safe)]);
        h.u64(self.idx as u64);
    }
}

impl StateHash for Agent {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.id);
        h.write(&[self.squad]);
        self.role.hash_into(h);
        for v in self.pos.chunk {
            h.i32(v);
        }
        for v in self.pos.off {
            h.i32(v);
        }
        h.u16(self.facing.0);
        match self.stunned_until {
            None => h.write(&[0]),
            Some(t) => {
                h.write(&[1]);
                h.u64(t);
            }
        }
        match &self.held {
            None => h.write(&[0]),
            Some(a) => {
                h.write(&[1]);
                a.hash_into(h);
            }
        }
        match &self.intent {
            None => h.write(&[0]),
            Some(act) => {
                h.write(&[1]);
                act.hash_into(h);
            }
        }
        h.u32(self.lanes.len() as u32);
        for (gate, lc) in &self.lanes {
            h.write(&[*gate]);
            lc.hash_into(h);
        }
        match &self.lane_cmd {
            None => h.write(&[0]),
            Some(o) => {
                h.write(&[1]);
                o.hash_into(h);
            }
        }
        match &self.route {
            None => h.write(&[0]),
            Some((goal, path, at)) => {
                h.write(&[1]);
                goal.hash_into(h);
                h.u32(path.len() as u32);
                for c in path {
                    c.hash_into(h);
                }
                h.u64(*at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Intent;
    use crate::header::Role;
    use crate::params::Params;
    use crate::rng::{Domain, Rng};
    use crate::world::gen::generate;
    use crate::world::{CellXY, Grid, World};

    /// An all-Floor 10x10 world with 1 m cells, for motor walks.
    fn open_world() -> World {
        World {
            grid: Grid::new(10, 10),
            beacons: Vec::new(),
            spawns: Vec::new(),
            cell_units: 1024,
        }
    }

    /// An agent standing at the centre of `c` in `world`.
    fn agent_at(world: &World, c: CellXY) -> Agent {
        Agent::new(0, 0, Role::Leader, world.grid.center(c, world.cell_units))
    }

    #[test]
    fn walks_route_and_arrives() {
        let world = open_world();
        let p = Params::default();
        let mut a = agent_at(&world, CellXY { x: 1, y: 1 });
        let goal = CellXY { x: 8, y: 8 };
        for t in 0..200u64 {
            a.move_step(MotorCmd::GoTo(goal), &world, &p, t);
            if world.grid.cell_of(a.pos, world.cell_units) == goal {
                return;
            }
        }
        panic!(
            "never reached {goal:?}; still at {:?}",
            world.grid.cell_of(a.pos, world.cell_units)
        );
    }

    #[test]
    fn never_enters_wall() {
        let p = Params::default();
        let world = generate(5, &p);
        let mut rng = Rng::stream(5, Domain::Agent, 0);
        let mut a = agent_at(&world, world.spawns[0]);
        for t in 0..1000u64 {
            let goal = CellXY {
                x: rng.below(u32::from(world.grid.w)) as u16,
                y: rng.below(u32::from(world.grid.h)) as u16,
            };
            a.move_step(MotorCmd::GoTo(goal), &world, &p, t);
            let cell = world.grid.cell_of(a.pos, world.cell_units);
            assert!(
                world.grid.walkable(cell),
                "t={t} goal={goal:?} cell={cell:?}"
            );
        }
    }

    #[test]
    fn intent_expiry_falls_to_safe_branch() {
        let mut a = agent_at(&open_world(), CellXY { x: 4, y: 4 });
        let plan = vec![CellXY { x: 5, y: 4 }, CellXY { x: 6, y: 4 }];
        let safe = vec![CellXY { x: 3, y: 4 }];
        a.set_intent(
            &Intent {
                ttl_ticks: 10,
                plan: plan.clone(),
                safe: safe.clone(),
            },
            0,
        );

        // t=9: the plan is still alive and the agent heads east along it.
        assert!(!a.intent.as_ref().unwrap().on_safe);
        assert_eq!(a.choose_cmd(9, None), MotorCmd::GoTo(plan[0]));

        // t=10: the intent has expired; the agent takes its safe branch west.
        assert_eq!(a.choose_cmd(10, None), MotorCmd::GoTo(safe[0]));
        let act = a.intent.as_ref().unwrap();
        assert!(act.on_safe);
        assert_eq!(act.idx, 0);
    }

    #[test]
    fn failed_intent_falls_to_safe_not_default() {
        let mut a = agent_at(&open_world(), CellXY { x: 4, y: 4 });
        let plan = vec![CellXY { x: 5, y: 4 }, CellXY { x: 6, y: 4 }];
        let safe = vec![CellXY { x: 9, y: 9 }];
        a.set_intent(
            &Intent {
                ttl_ticks: 300,
                plan: plan.clone(),
                safe: safe.clone(),
            },
            0,
        );
        let reflex = MotorCmd::GoTo(CellXY { x: 5, y: 5 });

        // Mid-plan the reflex is ignored; after fail_intent the safe branch,
        // not the reflex, is what plays (D45).
        assert_eq!(a.choose_cmd(1, Some(reflex)), MotorCmd::GoTo(plan[0]));
        a.fail_intent();
        assert_eq!(a.choose_cmd(2, Some(reflex)), MotorCmd::GoTo(safe[0]));
    }

    #[test]
    fn stunned_holds() {
        let mut a = agent_at(&open_world(), CellXY { x: 4, y: 4 });
        a.stunned_until = Some(50);
        assert_eq!(
            a.choose_cmd(10, Some(MotorCmd::GoTo(CellXY { x: 1, y: 1 }))),
            MotorCmd::Hold
        );
        // The stun ends at 50: the reflex plays again.
        assert_eq!(
            a.choose_cmd(50, Some(MotorCmd::GoTo(CellXY { x: 1, y: 1 }))),
            MotorCmd::GoTo(CellXY { x: 1, y: 1 })
        );
    }

    #[test]
    fn exhausted_safe_branch_holds_over_the_reflex() {
        let mut a = agent_at(&open_world(), CellXY { x: 4, y: 4 });
        let safe = vec![CellXY { x: 3, y: 4 }];
        a.set_intent(
            &Intent {
                ttl_ticks: 5,
                plan: vec![CellXY { x: 5, y: 4 }],
                safe,
            },
            0,
        );
        let act = a.intent.as_mut().unwrap();
        act.on_safe = true;
        act.idx = 1; // the single safe waypoint is behind us
        assert_eq!(
            a.choose_cmd(50, Some(MotorCmd::GoTo(CellXY { x: 2, y: 2 }))),
            MotorCmd::Hold
        );
    }

    #[test]
    fn completed_plan_holds_until_expiry_then_goes_safe() {
        let mut a = agent_at(&open_world(), CellXY { x: 4, y: 4 });
        let safe = vec![CellXY { x: 0, y: 4 }];
        a.set_intent(
            &Intent {
                ttl_ticks: 20,
                plan: vec![CellXY { x: 5, y: 4 }],
                safe: safe.clone(),
            },
            0,
        );
        // The whole plan is behind the agent (idx past its end) but the TTL
        // has not run out: the commitment holds, the reflex does not play.
        a.intent.as_mut().unwrap().idx = 1;
        assert_eq!(
            a.choose_cmd(5, Some(MotorCmd::GoTo(CellXY { x: 9, y: 9 }))),
            MotorCmd::Hold
        );
        assert_eq!(a.choose_cmd(20, None), MotorCmd::GoTo(safe[0]));
    }

    #[test]
    fn wall_goal_is_refused() {
        let p = Params::default();
        let mut world = open_world();
        world
            .grid
            .set(CellXY { x: 3, y: 3 }, crate::world::CellKind::Wall);
        let mut a = agent_at(&world, CellXY { x: 1, y: 1 });
        for t in 0..50u64 {
            a.move_step(MotorCmd::GoTo(CellXY { x: 3, y: 3 }), &world, &p, t);
        }
        assert_eq!(
            world.grid.cell_of(a.pos, world.cell_units),
            CellXY { x: 1, y: 1 }
        );
    }

    #[test]
    fn hold_does_not_move() {
        let world = open_world();
        let p = Params::default();
        let mut a = agent_at(&world, CellXY { x: 2, y: 2 });
        let before = a.pos;
        a.move_step(MotorCmd::Hold, &world, &p, 0);
        assert_eq!(a.pos, before);
    }

    #[test]
    fn arrival_clears_the_route_cache() {
        let world = open_world();
        let p = Params::default();
        let mut a = agent_at(&world, CellXY { x: 1, y: 1 });
        a.move_step(MotorCmd::GoTo(CellXY { x: 1, y: 1 }), &world, &p, 0);
        assert!(a.route.is_none());
    }

    #[test]
    fn state_hash_is_stable_and_sensitive() {
        let world = open_world();
        let a = agent_at(&world, CellXY { x: 2, y: 2 });
        let mut b = agent_at(&world, CellXY { x: 2, y: 2 });
        assert_eq!(a.state_hash(), b.state_hash());
        b.pos = b.pos.offset(1024, 0);
        assert_ne!(a.state_hash(), b.state_hash());
        let mut c = agent_at(&world, CellXY { x: 2, y: 2 });
        c.set_intent(
            &Intent {
                ttl_ticks: 10,
                plan: vec![CellXY { x: 3, y: 2 }],
                safe: vec![],
            },
            0,
        );
        assert_ne!(a.state_hash(), c.state_hash());
    }
}

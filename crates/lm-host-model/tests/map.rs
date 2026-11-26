//! The map draw model: fog and interpolation (phase 2 Task 4).
//!
//! These tests pin the four things the map model promises:
//!
//! * a **spectator** sees the round as the authority does: every cell
//!   `Visible`, every agent of both squads drawn;
//! * a **possessing** player sees it through the squad's eyes: teammates
//!   drawn, unseen enemies absent, and the cells the view vouches for
//!   `Explored` while the rest stays `Unknown` — never `Visible`;
//! * positions interpolate: halfway between two ticks, an agent that moved
//!   from (0, 0) to (2 m, 0) draws at (1.0, 0);
//! * the world's size in metres comes from the grid it was generated on:
//!   128 cells of 2 m are 256 m.

use lm_core::fixed::Pos;
use lm_core::header::Header;
use lm_core::sim::Sim;
use lm_core::world::CellXY;
use lm_host_model::map::{pos_m, FogState, MapModel, Viewer};
use lm_tools::runner::DEFAULT_REFLEX;

/// A one-chunk round: a 16 × 16 cell world with all eight agents and one
/// pair of beacons, so both squads stand inside each other's view range
/// from the start and the fog rules bite quickly.
///
/// The beacon count is even on purpose. The fairness fix (core commit
/// 5595db9) lays the beacons out in 180°-mirrored pairs — an unpaired
/// beacon would sit closer to one squad and bias the round — and
/// `Params::validate` refuses an odd count, so a one-beacon round no
/// longer starts at all. Two is the smallest legal count: one pair,
/// straddling the centre, and the world stays as small and as quick to
/// meet in as it was.
fn small_round(seed: u64) -> Header {
    let mut h = Header::default_round(seed, DEFAULT_REFLEX);
    h.params.world_chunks = (1, 1);
    h.params.beacons = 2;
    h
}

/// Step `sim` until the squad of agent 0 has sighted some enemy (its
/// `last_seen` is no longer empty).
fn step_until_seen(sim: &mut Sim) {
    for _ in 0..600 {
        sim.step(&[]);
        if !sim.view(0).enemies.is_empty() {
            return;
        }
    }
    panic!("the squads never sighted each other on the small round");
}

/// The map cell the agent stands in.
fn cell_of(sim: &Sim, id: u16) -> CellXY {
    let world = sim.world();
    world
        .grid
        .cell_of(sim.agent(id).expect("the agent").pos, world.cell_units)
}

/// The beacons of the small round: one 180°-mirrored pair, the smallest
/// layout the fairness fix allows.
///
/// The count is pinned so the beacon assertions below can never pass over an
/// empty list — and so the fixture stays honest about what the round is.
fn beacons_of(sim: &Sim) -> Vec<CellXY> {
    let beacons = sim.world().beacons.clone();
    assert_eq!(
        beacons.len(),
        2,
        "the small round is set up with one pair of beacons"
    );
    beacons
}

#[test]
fn spectator_sees_all_enemies() {
    let mut sim = Sim::new(&small_round(1)).expect("the round starts");
    step_until_seen(&mut sim);

    let model = MapModel::build(&sim, &[], &Viewer::Spectator { follow: None }, 1.0);

    // Every agent of the round is drawn — both squads, in id order — and
    // all of them visible: the spectator has no fog.
    let ids: Vec<u16> = model.agents.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    assert!(model.agents.iter().all(|a| a.visible));
    assert!(model.agents.iter().any(|a| a.squad == 1));

    // The whole map is Visible to the spectator, one entry per cell.
    let world = sim.world();
    assert_eq!(
        model.cells.len(),
        usize::from(world.grid.w) * usize::from(world.grid.h)
    );
    assert!(model
        .cells
        .iter()
        .all(|&(_, _, fog)| fog == FogState::Visible));

    // The beacons are drawn at their cell centres, unheld so far. The
    // round's beacons are common knowledge, so every viewer draws every
    // one of them, in world order.
    let beacons = beacons_of(&sim);
    assert_eq!(model.beacons.len(), beacons.len());
    for (draw, &cell) in model.beacons.iter().zip(beacons.iter()) {
        let centre = world.grid.center(cell, world.cell_units);
        assert_eq!(draw.cell, cell);
        assert_eq!(draw.holder, None);
        assert_eq!(draw.pos_m, pos_m(centre, centre, 0.0));
    }
}

#[test]
fn possessed_view_hides_unseen_enemy() {
    let mut sim = Sim::new(&small_round(2)).expect("the round starts");
    step_until_seen(&mut sim);

    // Precondition: the squad of agent 0 remembers enemies — but only as
    // aged memories. A sighting recorded on the tick that just ran reads
    // age 1 through `Sim::view` (the freshest a view can report), so
    // nothing in `last_seen` is a same-tick sighting.
    let view = sim.view(0);
    assert!(!view.enemies.is_empty());
    assert!(view.enemies.iter().all(|e| e.age_ticks > 0));

    let model = MapModel::build(&sim, &[], &Viewer::Possessing(0), 1.0);

    // Teammates are always drawn; the remembered-but-unseen enemies of
    // squad 1 are not drawn at all.
    let ids: Vec<u16> = model.agents.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![0, 1, 2, 3]);

    // The fog under possession is two-valued: no cell is Visible, the
    // cells the squad knows (its members' cells, the beacon) are
    // Explored, and the rest of the map is Unknown.
    assert!(model
        .cells
        .iter()
        .all(|&(_, _, fog)| fog != FogState::Visible));
    for id in 0..4u16 {
        let cell = cell_of(&sim, id);
        let (_, _, fog) = model
            .cells
            .iter()
            .find(|&&(c, _, _)| c == cell)
            .expect("the squad cell is on the map");
        assert_eq!(*fog, FogState::Explored);
    }
    // The beacons are common knowledge, so every one of them is Explored
    // for a possessing player — not only the one the squad is near.
    for &beacon in &beacons_of(&sim) {
        let (_, _, fog) = model
            .cells
            .iter()
            .find(|&&(c, _, _)| c == beacon)
            .expect("the beacon cell is on the map");
        assert_eq!(*fog, FogState::Explored);
    }
    assert!(model
        .cells
        .iter()
        .any(|&(_, _, fog)| fog == FogState::Unknown));
}

#[test]
fn interpolation_midpoint() {
    // Halfway between (0, 0) and (2 m, 0) is (1.0, 0).
    assert_eq!(
        pos_m(Pos::zero(), Pos::from_units(2_048, 0), 0.5),
        (1.0, 0.0)
    );
    // The ends are exact at alpha 0 and 1.
    assert_eq!(
        pos_m(Pos::zero(), Pos::from_units(2_048, 0), 0.0),
        (0.0, 0.0)
    );
    assert_eq!(
        pos_m(Pos::zero(), Pos::from_units(2_048, 0), 1.0),
        (2.0, 0.0)
    );

    // And through `build`: with every agent's previous position at the
    // origin and alpha 1/2, each drawn agent stands halfway between the
    // origin and where the sim has it, in metres.
    let mut sim = Sim::new(&small_round(3)).expect("the round starts");
    sim.step(&[]);
    let prev: Vec<(u16, Pos)> = sim
        .agent_ids()
        .into_iter()
        .map(|id| (id, Pos::zero()))
        .collect();
    let model = MapModel::build(&sim, &prev, &Viewer::Spectator { follow: None }, 0.5);
    for draw in &model.agents {
        let cur = sim.agent(draw.id).expect("the agent").pos;
        assert_eq!(draw.pos_m, pos_m(Pos::zero(), cur, 0.5));
    }
}

#[test]
fn world_size_matches_params() {
    // The default round is 8 × 8 chunks of 32 m at 2 m per cell:
    // 128 × 128 cells, a 256 m square.
    let h = Header::default_round(1, DEFAULT_REFLEX);
    let sim = Sim::new(&h).expect("the round starts");
    assert_eq!((sim.world().grid.w, sim.world().grid.h), (128, 128));

    let model = MapModel::build(&sim, &[], &Viewer::Spectator { follow: None }, 0.0);
    assert_eq!(model.world_m, (256.0, 256.0));
    assert_eq!(model.cells.len(), 128 * 128);
}

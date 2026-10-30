//! The shared-target variants: each harness's facts file states the target
//! that harness asks for, and each variant's reflex plays the same rule.
//!
//! The variants are the measurement's arms; this is the check that the
//! arms are what they claim to be. A variant whose facts file did not load
//! would state no shared target at all, and the round would silently
//! measure the shipped doctrine instead.

use std::path::{Path, PathBuf};

use lm_agents::facts::Facts;
use lm_agents::gates::GateSet;
use lm_agents::harness::Harness;
use lm_agents::shared::{SharedConfig, SharedRule};
use lm_core::header::Role;
use lm_core::view::{AgentSnap, BeaconSnap, ScoreView};
use lm_core::world::{CellKind, CellXY, Grid, World};

/// The variants directory, from the crate's manifest.
fn variants() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/variants")
}

/// A view of a four-beacon map: beacons 0 and 1 unheld, 2 and 3 held.
fn view() -> ScoreView {
    let beacon = |x: u16, y: u16, holder: Option<u8>| BeaconSnap {
        cell: CellXY { x, y },
        holder,
    };
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
            paint_slow_q8: 0,
        },
        squad: Vec::new(),
        enemies: Vec::new(),
        beacons: vec![
            beacon(64, 63, None),
            beacon(63, 64, None),
            beacon(32, 63, Some(0)),
            beacon(95, 64, Some(1)),
        ],
        score: vec![0, 0],
        squad_id: 0,
    }
}

/// A 128x128 open map.
fn world() -> World {
    World {
        paint: lm_core::world::Paint::new(),
        grid: Grid {
            w: 128,
            h: 128,
            cells: vec![CellKind::Floor; 128 * 128],
        },
        beacons: vec![
            CellXY { x: 64, y: 63 },
            CellXY { x: 63, y: 64 },
            CellXY { x: 32, y: 63 },
            CellXY { x: 95, y: 64 },
        ],
        spawns: vec![CellXY { x: 2, y: 2 }, CellXY { x: 125, y: 125 }],
        cell_units: 1024,
    }
}

/// The facts under a harness's own shared-target parameter.
fn facts_of(harness: &Harness) -> Facts {
    let shared = SharedConfig::parse(&harness.facts_toml).expect("the facts file loads");
    Facts::from_view(
        &view(),
        &world(),
        0,
        &lm_agents::config::AgentsConfig::default(),
        &shared,
    )
}

#[test]
fn the_shipped_harness_states_no_shared_target() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/default");
    let harness = Harness::open(&dir).expect("the shipped harness loads");
    assert_eq!(
        SharedConfig::parse(&harness.facts_toml)
            .expect("no facts file is the default")
            .rule,
        SharedRule::Off
    );
    let facts = facts_of(&harness);
    assert!(!facts.text.contains("shared target"));
    assert!(facts
        .text
        .contains("it is the nearest beacon your squad does not hold"));
}

#[test]
fn each_variant_states_its_own_shared_target() {
    for (name, rule, reason) in [
        ("shared-free", SharedRule::Free, "the beacon no squad holds"),
        (
            "shared-centre",
            SharedRule::Centre,
            "the most central of the beacons",
        ),
        (
            "shared-ghost",
            SharedRule::Ghost,
            "the beacon you are pulled toward",
        ),
    ] {
        let harness = Harness::open(&variants().join(name)).expect("the harness loads");
        let shared = SharedConfig::parse(&harness.facts_toml).expect("the facts file loads");
        assert_eq!(shared.rule, rule, "{name} states the rule it claims");

        let facts = facts_of(&harness);
        assert!(
            facts.text.contains("The shared target is beacon "),
            "{name} states the shared target: {}",
            facts.text
        );
        assert!(
            facts
                .text
                .contains(&format!("it is the shared target: {reason}")),
            "{name} marks the target with its own reason: {}",
            facts.text
        );
        // The doctrine's own target is not stated any more: one target only.
        assert!(
            !facts
                .text
                .contains("nearest beacon your squad does not hold"),
            "{name} states one target: {}",
            facts.text
        );
        // The gates load and the move gate names the shared target.
        let gates = GateSet::parse(&harness.gates_toml).expect("the gates load");
        let move_gate = gates.to_questions(&lm_core::view::menu(&view()));
        let instructions = move_gate["move"]["instructions"]
            .as_str()
            .expect("the instructions are text");
        assert!(
            instructions.contains("the shared target"),
            "{name}'s doctrine names the shared target: {instructions}"
        );
    }
}

#[test]
fn both_squads_are_sent_to_the_same_beacon() {
    for name in ["shared-free", "shared-centre", "shared-ghost"] {
        let harness = Harness::open(&variants().join(name)).expect("the harness loads");
        let shared = SharedConfig::parse(&harness.facts_toml).expect("the facts file loads");

        let mut other = view();
        other.squad_id = 1;
        other.me.cell = CellXY { x: 125, y: 125 };
        let mine = shared.resolve(&view()).expect("a target");
        let theirs = shared.resolve(&other).expect("a target");
        assert_eq!(
            mine.beacon, theirs.beacon,
            "{name} names one beacon for both squads"
        );
    }
}

#[test]
fn the_ghost_states_a_field_the_plain_rule_does_not() {
    let ghost = Harness::open(&variants().join("shared-ghost")).expect("the harness loads");
    let plain = Harness::open(&variants().join("shared-free")).expect("the harness loads");

    let ghost_facts = facts_of(&ghost);
    let plain_facts = facts_of(&plain);

    // The field says how hard the pull is where the agent stands, and the
    // plain target does not.
    assert!(ghost_facts.text.contains("You are pulled toward beacon"));
    assert!(ghost_facts.text.contains("The pull reaches"));
    assert!(ghost_facts.fields.contains_key("shared.pull"));
    assert!(!plain_facts.text.contains("pulled toward"));
    assert!(!plain_facts.fields.contains_key("shared.pull"));
}

#[test]
fn the_hot_zone_is_on_one_beacon_at_a_time_and_it_walks() {
    let harness = Harness::open(&variants().join("shared-ghost")).expect("the harness loads");
    let shared = SharedConfig::parse(&harness.facts_toml).expect("the facts file loads");
    assert_eq!(shared.ghost.rotation_ticks, 600);

    // On the first tick the hot zone is on the first free beacon.
    let mut zone = view();
    zone.tick = 0;
    assert_eq!(shared.resolve(&zone).expect("a target").beacon, 0);

    // Six hundred ticks on it has moved: the zone walks the free beacons.
    zone.tick = 600;
    assert_eq!(shared.resolve(&zone).expect("a target").beacon, 1);
    zone.tick = 1200;
    assert_eq!(shared.resolve(&zone).expect("a target").beacon, 0);

    // And it is the same beacon for both squads at every one of those
    // ticks: the tick and the beacon list are the same for both.
    for tick in [0, 600, 1200, 2400] {
        let mut theirs = view();
        theirs.squad_id = 1;
        theirs.me.cell = CellXY { x: 125, y: 125 };
        theirs.tick = tick;
        let mut mine = view();
        mine.tick = tick;
        assert_eq!(
            shared.resolve(&mine).expect("a target").beacon,
            shared.resolve(&theirs).expect("a target").beacon,
            "both squads are pulled to the same hot beacon on tick {tick}"
        );
    }
}

#[test]
fn the_pull_is_in_the_situation_key_so_the_field_moves_with_the_agent() {
    let ghost = Harness::open(&variants().join("shared-ghost")).expect("the harness loads");
    let shared = SharedConfig::parse(&ghost.facts_toml).expect("the facts file loads");

    // The same hot beacon, two agents standing in different places: the
    // pull at their cells differs, so the fields differ.
    let mut near = view();
    near.me.cell = CellXY { x: 60, y: 60 };
    let mut far = view();
    far.me.cell = CellXY { x: 2, y: 2 };
    let near_pull = shared.resolve(&near).expect("a target").pull_q14;
    let far_pull = shared.resolve(&far).expect("a target").pull_q14;
    assert_ne!(near_pull, far_pull);
}

#[test]
fn the_centre_target_is_the_centre_beacon_of_the_round_map() {
    let harness = Harness::open(&variants().join("shared-centre")).expect("the harness loads");
    let shared = SharedConfig::parse(&harness.facts_toml).expect("the facts file loads");
    let target = shared.resolve(&view()).expect("a target");
    // Beacon 0 at (64, 63) is the beacon at the centre of the round's map.
    assert_eq!(target.beacon, 0);
}

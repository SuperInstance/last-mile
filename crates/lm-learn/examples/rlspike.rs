//! The RL spike: a learned squad policy against the shipped reflex.
//!
//! Could reinforcement learning train a squad policy on the seed farm, and
//! could a directive between rounds reweight the reward so the trained
//! behaviour shifts as asked? This is the bench that answers it.
//!
//! Everything here is a measurement, not a change: the game's defaults are
//! untouched, the golden does not move, and nothing is written into a flow
//! record or a ledger.
//!
//! # The shape of the setup
//!
//! * **The policy is a table.** An observation is four buckets -- the fresh
//!   threats near me, the squadmates near me, whether my squad holds a
//!   beacon, and whether my squad remembers an enemy -- and the table says
//!   which of seven actions each observation takes. Every action is an answer
//!   the reflex contract already allows: a cell to walk to, or a hold.
//! * **The policy runs in the sim, not beside it.** The trained table is
//!   compiled into a Rhai reflex and played by the same sim, in the same
//!   sandbox, that plays the shipped reflex. The policy is a pure integer
//!   function of the score view, so a replay of the round is the same round,
//!   tick for tick: the rounds stay bit-exact (D69).
//! * **The reward is the round's own metrics.** A directive is a set of
//!   weights over `hold_ticks`, `tags_for` and `margin`; the reward of a
//!   policy on a seed is that weighted sum. Changing the weights between
//!   rounds is the coaching.
//! * **The search is a (1+lambda) evolution strategy.** The parameters are
//!   the table's entries; an offspring is the parent with one entry changed;
//!   the changes that each help are applied one at a time, and each is kept
//!   only if it still helps where the last one landed. The seeds are judged
//!   in parallel across the cores, the way the seed farm does -- and the
//!   parallelism is flat, so the bench holds eight worker threads and no
//!   more.
//!
//! The commands:
//!
//! ```text
//! cargo run --release -p lm-learn --example rlspike -- probe [seeds] [ticks] [world]
//! cargo run --release -p lm-learn --example rlspike -- candidates [seeds] [ticks]
//! cargo run --release -p lm-learn --example rlspike -- train [seeds] [generations] [directive] [ticks]
//! cargo run --release -p lm-learn --example rlspike -- judge [seeds]
//! cargo run --release -p lm-learn --example rlspike -- verify
//! cargo run --release -p lm-learn --example rlspike -- coach [seeds] [generations] [warm|cold]
//! cargo run --release -p lm-learn --example rlspike -- lever [seeds] [world]
//! cargo run --release -p lm-learn --example rlspike -- scrimmage [seeds] [generations]
//! cargo run --release -p lm-learn --example rlspike -- check [seeds]
//! cargo run --release -p lm-learn --example rlspike -- mirror [seeds]
//! cargo run --release -p lm-learn --example rlspike -- trace [seed] [ticks]
//! ```
//!
//! The directives are `hold`, `margin`, `deny`, `contact` and `base`. The
//! worlds are `shipped` and `scrimmage`; the scrimmage is a small one the
//! bench builds, where the squads meet and the tag metric moves.

use std::collections::BTreeMap;
use std::time::Instant;

use lm_core::header::Header;
use lm_core::rules::Event;
use lm_core::sim::Sim;
use lm_learn::eval::eval_seeds;
use lm_learn::flow::DEFAULT_REFLEX;
use lm_learn::ledger::{Direction, Prediction};
use lm_learn::round::play;

// ─────────────────────────────────────────────────────────────────────────────
// The policy
// ─────────────────────────────────────────────────────────────────────────────

/// The actions of the policy, in the order the tables lay them out.
///
/// Every one of them is an answer the reflex contract already allows: either
/// a cell to walk to or a hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Stand still.
    Hold,
    /// Walk to the nearest fresh enemy.
    Charge,
    /// Walk four cells away from the nearest fresh enemy.
    Flee,
    /// Walk to the free beacon nearest to me.
    Take,
    /// Walk to the beacon of my squad that a fresh enemy is nearest to.
    Defend,
    /// Walk to the nearest enemy my squad remembers, fresh or stale.
    Hunt,
    /// Walk to the free beacon *second* nearest to me: spread the squad.
    Spread,
}

impl Action {
    /// The action's index in a table.
    const fn index(self) -> usize {
        match self {
            Action::Hold => 0,
            Action::Charge => 1,
            Action::Flee => 2,
            Action::Take => 3,
            Action::Defend => 4,
            Action::Hunt => 5,
            Action::Spread => 6,
        }
    }

    /// The action of an index in a table.
    const fn from_index(index: usize) -> Action {
        match index {
            1 => Action::Charge,
            2 => Action::Flee,
            3 => Action::Take,
            4 => Action::Defend,
            5 => Action::Hunt,
            6 => Action::Spread,
            _ => Action::Hold,
        }
    }

    /// The name of the action, for the tables in the write-up.
    const fn name(self) -> &'static str {
        match self {
            Action::Hold => "hold",
            Action::Charge => "charge",
            Action::Flee => "flee",
            Action::Take => "take",
            Action::Defend => "defend",
            Action::Hunt => "hunt",
            Action::Spread => "spread",
        }
    }
}

/// How many actions there are.
const ACTIONS: usize = 7;

/// The buckets of the threats: none, one, two or more.
const T_BUCKETS: usize = 3;
/// The buckets of the allies near me: none, one, two or more.
const A_BUCKETS: usize = 3;
/// The buckets of "my squad holds a beacon": no, yes.
const H_BUCKETS: usize = 2;
/// The buckets of "my squad remembers an enemy": no, yes.
const E_BUCKETS: usize = 2;
/// How many observations the table has.
const STATES: usize = T_BUCKETS * A_BUCKETS * H_BUCKETS * E_BUCKETS;

/// The observation a state is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    /// The number of fresh threats, bucketed.
    threats: usize,
    /// The number of squadmates near me, bucketed.
    allies: usize,
    /// Whether my squad holds a beacon.
    holding: bool,
    /// Whether my squad remembers an enemy at all.
    remembering: bool,
}

impl Observation {
    /// The index of the observation in a table.
    const fn index(self) -> usize {
        let holding = if self.holding { 1 } else { 0 };
        let remembering = if self.remembering { 1 } else { 0 };
        (((self.threats * A_BUCKETS + self.allies) * H_BUCKETS + holding) * E_BUCKETS) + remembering
    }
}

/// A policy: one action per observation.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Policy {
    /// The table: the action of every observation, in index order.
    table: Vec<usize>,
}

impl Policy {
    /// A policy that always takes the same action.
    fn constant(action: Action) -> Policy {
        Policy {
            table: vec![action.index(); STATES],
        }
    }

    /// A policy drawn from a seeded generator.
    fn random(rng: &mut Rng) -> Policy {
        Policy {
            table: (0..STATES).map(|_| rng.below(ACTIONS)).collect(),
        }
    }

    /// The action of an observation.
    fn action(&self, observation: Observation) -> Action {
        Action::from_index(self.table[observation.index()])
    }

    /// The Rhai source of this policy: a reflex script that reads the
    /// observation out of the score view and answers with the table's action.
    fn source(&self) -> String {
        let mut source = String::with_capacity(8_000);
        source.push_str(PREAMBLE);
        source.push_str(OBSERVATION_HEAD);
        source.push_str(&self.table_block());
        source.push_str(OBSERVATION_BODY);
        source.push_str(ACTIONS_BLOCK);
        source
    }

    /// The table, as a Rhai array literal.
    ///
    /// It lives *inside* the reflex, not beside it: `lm-core` calls the
    /// reflex with a fresh scope, so nothing at the script's top level is
    /// visible to the function.
    fn table_block(&self) -> String {
        let cells: Vec<String> = self.table.iter().map(|a| a.to_string()).collect();
        format!(
            "    // The policy: the action of every observation, in index order.\n    let TABLE = [{}];\n",
            cells.join(", ")
        )
    }

    /// The table, as the rows of the write-up.
    fn rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for t in 0..T_BUCKETS {
            for a in 0..A_BUCKETS {
                for h in 0..H_BUCKETS {
                    for e in 0..E_BUCKETS {
                        let observation = Observation {
                            threats: t,
                            allies: a,
                            holding: h == 1,
                            remembering: e == 1,
                        };
                        rows.push(format!(
                            "{} {} {} {} {}",
                            t,
                            a,
                            h,
                            e,
                            self.action(observation).name()
                        ));
                    }
                }
            }
        }
        rows
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The policy script
// ─────────────────────────────────────────────────────────────────────────────

/// The helpers every policy script starts with.
const PREAMBLE: &str = r#"
// A learned squad policy.
//
// The observation is four buckets -- the fresh threats near me, the
// squadmates near me, whether my squad holds a beacon, and whether my squad
// remembers an enemy -- and the table says which action each observation
// takes. The script answers the reflex contract exactly: a cell to walk to,
// or a hold.
//
// Generated by crates/lm-learn/examples/rlspike.rs. Do not edit by hand.

fn cheb(a, b) {
    let dx = if a[0] > b[0] { a[0] - b[0] } else { b[0] - a[0] };
    let dy = if a[1] > b[1] { a[1] - b[1] } else { b[1] - a[1] };
    if dx > dy { dx } else { dy }
}

fn step_away(d) {
    if d > 0 { 4 } else if d < 0 { -4 } else { 0 }
}

fn clamp_min(v) {
    if v < 0 { 0 } else { v }
}
"#;

/// The opening of the reflex: the view's own cell.
const OBSERVATION_HEAD: &str = r#"
fn reflex(v) {
    let me = v.me.cell;
"#;

/// The observation, read out of the score view.
const OBSERVATION_BODY: &str = r#"
    // The fresh threats: an enemy seen within the last 6 ticks and within 6
    // cells of me. The nearest one is the one to charge or to flee from.
    let n_threats = 0;
    let d_threat = 9;
    let tx = -1;
    let ty = -1;
    // The enemy my squad remembers, fresh or stale: the one to hunt.
    let hx = -1;
    let hy = -1;
    let d_hunt = 999;
    for i in 0..v.enemies.len() {
        let e = v.enemies[i];
        let d = cheb(me, e.cell);
        if d < d_hunt {
            d_hunt = d;
            hx = e.cell[0];
            hy = e.cell[1];
        }
        if e.age_ticks <= 6 && d <= 6 {
            n_threats += 1;
            if d < d_threat {
                d_threat = d;
                tx = e.cell[0];
                ty = e.cell[1];
            }
        }
    }
    let remembering = if hx < 0 { 0 } else { 1 };

    // The squadmates near me: not me, not stunned, within 4 cells.
    let n_allies = 0;
    for mate in v.squad {
        if mate.id != v.me.id && !mate.stunned && cheb(me, mate.cell) <= 4 {
            n_allies += 1;
        }
    }

    // The beacons: the ones of my squad, and the ones that are free.
    let held = 0;
    let take_i = -1;
    let take_d = 999;
    let take2_i = -1;
    let take2_d = 999;
    let def_i = -1;
    let def_de = 9;
    let def_dm = 999;
    for i in 0..v.beacons.len() {
        let b = v.beacons[i];
        if b.holder == v.me.squad {
            held += 1;
            // The beacon of ours that a fresh enemy is nearest to; the one
            // nearest to me breaks the tie.
            let de = 9;
            for j in 0..v.enemies.len() {
                let e = v.enemies[j];
                if e.age_ticks <= 6 {
                    let d = cheb(b.cell, e.cell);
                    if d < de { de = d; }
                }
            }
            let dm = cheb(me, b.cell);
            if def_i < 0 || de < def_de || (de == def_de && dm < def_dm) {
                def_i = i;
                def_de = de;
                def_dm = dm;
            }
        } else {
            let d = cheb(me, b.cell);
            if take_i < 0 || d < take_d {
                take2_i = take_i;
                take2_d = take_d;
                take_i = i;
                take_d = d;
            } else if take2_i < 0 || d < take2_d {
                take2_i = i;
                take2_d = d;
            }
        }
    }

    // The buckets.
    let t = if n_threats >= 2 { 2 } else { n_threats };
    let a = if n_allies >= 2 { 2 } else { n_allies };
    let h = if held > 0 { 1 } else { 0 };

    // The state of the table.
    let state = ((t * 3 + a) * 2 + h) * 2 + remembering;
    let action = TABLE[state];
"#;

/// The answers: one per action of the table.
const ACTIONS_BLOCK: &str = r#"
    // 0: hold.
    if action == 0 {
        return #{ hold: true };
    }

    // 1: charge the nearest fresh enemy.
    if action == 1 {
        if tx < 0 { return #{ hold: true }; }
        return #{ "goto": [tx, ty] };
    }

    // 2: flee the nearest fresh enemy, four cells away.
    if action == 2 {
        if tx < 0 { return #{ hold: true }; }
        let x = clamp_min(me[0] + step_away(me[0] - tx));
        let y = clamp_min(me[1] + step_away(me[1] - ty));
        return #{ "goto": [x, y] };
    }

    // 3: walk to the free beacon nearest to me.
    if action == 3 {
        if take_i < 0 { return #{ hold: true }; }
        return #{ "goto": [v.beacons[take_i].cell[0], v.beacons[take_i].cell[1]] };
    }

    // 4: walk to the beacon of my squad that a fresh enemy is nearest to.
    if action == 4 {
        if def_i < 0 { return #{ hold: true }; }
        return #{ "goto": [v.beacons[def_i].cell[0], v.beacons[def_i].cell[1]] };
    }

    // 5: walk to the enemy my squad remembers.
    if action == 5 {
        if hx < 0 { return #{ hold: true }; }
        return #{ "goto": [hx, hy] };
    }

    // 6: walk to the free beacon second nearest to me, to spread the squad.
    if take2_i < 0 { return #{ hold: true }; }
    return #{ "goto": [v.beacons[take2_i].cell[0], v.beacons[take2_i].cell[1]] };
}
"#;

// ─────────────────────────────────────────────────────────────────────────────
// The harness
// ─────────────────────────────────────────────────────────────────────────────

/// How many cores the spike may use. The box is shared, so the spike does
/// not take all of it.
const CORES: usize = 8;

/// The horizon of a training round: the flow's default.
const DEFAULT_TICKS: u64 = 1_500;

/// The horizon of the coaching experiment.
///
/// The flow judges at 1_500 ticks, so the coaching experiment runs at the
/// flow's own horizon: the numbers it produces are the numbers the flow
/// would measure.
const COACH_TICKS: u64 = 1_500;

/// What a round was worth, as the flow measures it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    /// The beacon-ticks each squad held.
    hold: [i64; 2],
    /// The tags each squad landed.
    tags: [i64; 2],
    /// How the policy's reflex behaved: errors, refusals and fuel-outs.
    reflex_errors: u64,
    /// How many times the reflex refused to answer.
    reflex_refused: u64,
    /// How many times the reflex ran out of fuel.
    reflex_out_of_fuel: u64,
}

impl Counts {
    /// The score margin of the round: the first squad's beacon-ticks less
    /// the second's.
    fn margin(&self) -> i64 {
        self.hold[0] - self.hold[1]
    }
}

/// The shape of the world a round is played in.
///
/// The spike plays two. The **shipped** shape is the game's own default
/// round. The **scrimmage** shape is a small world where the two squads
/// meet, so the tag metric moves and a directive has something to reward.
/// It is a round the bench builds, not a change to the game: the shipped
/// defaults are untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scenario {
    /// The world's size in chunks.
    world_chunks: (u8, u8),
    /// How many beacons the world holds.
    beacons: u8,
    /// The name of the shape, for the write-up.
    name: &'static str,
}

impl Scenario {
    /// The game's own world: eight chunks a side, four beacons.
    const SHIPPED: Scenario = Scenario {
        world_chunks: (8, 8),
        beacons: 4,
        name: "the shipped world",
    };

    /// A small world: one chunk a side, two beacons.
    ///
    /// The shipped world is 256 m a side, and in it the two squads never
    /// come within tag range. This one is 32 m a side, and they meet.
    const SCRIMMAGE: Scenario = Scenario {
        world_chunks: (1, 1),
        beacons: 2,
        name: "the scrimmage world",
    };
}

/// A header where squad 0 runs `ours` and squad 1 runs `theirs`.
///
/// The header carries both scripts: the roster says which one each agent
/// runs, so the sim plays them side by side in one round, on one seed.
fn duel_header(seed: u64, ours: &str, theirs: &str, scenario: Scenario) -> Header {
    let mut header = Header::default_round(seed, theirs);
    header.params.world_chunks = scenario.world_chunks;
    header.params.beacons = scenario.beacons;
    header.scripts.insert("ours".to_owned(), ours.to_owned());
    for entry in &mut header.roster {
        if entry.squad == 0 {
            entry.reflex = "ours".to_owned();
        }
    }
    header
}

/// Play a round and count what it was worth.
///
/// This is the same count `RoundMetrics::from_log` makes, read from the
/// round itself instead of from a replay of its log: the beacons a squad
/// holds are worth a point a tick, and a tag is a tag.
fn measure(header: &Header, ticks: u64) -> Counts {
    let mut sim = Sim::new(header).expect("the round starts");
    let mut squad_of: BTreeMap<u16, u8> = BTreeMap::new();
    for entry in &header.roster {
        squad_of.insert(entry.agent, entry.squad);
    }
    let mut holders: BTreeMap<u8, u8> = BTreeMap::new();
    let mut counts = Counts::default();
    for _tick in 0..ticks {
        let out = sim.step(&[]);
        for event in &out.events {
            match event {
                Event::Tagged { by, .. } => {
                    if let Some(squad) = squad_of.get(by) {
                        counts.tags[usize::from(*squad)] += 1;
                    }
                }
                Event::BeaconHeld { beacon, squad } => {
                    holders.insert(*beacon, *squad);
                }
                Event::BeaconLost { beacon } => {
                    holders.remove(beacon);
                }
                _ => {}
            }
        }
        for squad in holders.values() {
            counts.hold[usize::from(*squad)] += 1;
        }
    }
    let stats = sim.stats();
    counts.reflex_errors = stats.reflex_error;
    counts.reflex_refused = stats.reflex_refused;
    counts.reflex_out_of_fuel = stats.reflex_out_of_fuel;
    counts
}

/// Play the seeds, across the cores, and hand back the counts of each, in
/// seed order.
fn measure_seeds(
    ours: &str,
    theirs: &str,
    seeds: &[u64],
    ticks: u64,
    scenario: Scenario,
) -> Vec<Counts> {
    let workers = CORES.min(seeds.len().max(1));
    if workers < 2 {
        return measure_seeds_serial(ours, theirs, seeds, ticks, scenario);
    }
    let mut parts: Vec<Vec<Counts>> = Vec::new();
    std::thread::scope(|scope| {
        let mut workers_out = Vec::new();
        for worker in 0..workers {
            let from = worker * seeds.len() / workers;
            let to = (worker + 1) * seeds.len() / workers;
            if from == to {
                continue;
            }
            let part = &seeds[from..to];
            workers_out.push(scope.spawn(move || {
                part.iter()
                    .map(|&seed| measure(&duel_header(seed, ours, theirs, scenario), ticks))
                    .collect::<Vec<Counts>>()
            }));
        }
        for worker in workers_out {
            parts.push(worker.join().expect("a worker finishes its seeds"));
        }
    });
    let mut all = Vec::with_capacity(seeds.len());
    for part in parts {
        all.extend(part);
    }
    all
}

/// Play the seeds, one after the other.
///
/// The serial path the parallel one is measured against, and the one the
/// offspring sweep uses: the sweep already spreads the work over the cores,
/// so it must not spread it again inside each worker.
fn measure_seeds_serial(
    ours: &str,
    theirs: &str,
    seeds: &[u64],
    ticks: u64,
    scenario: Scenario,
) -> Vec<Counts> {
    seeds
        .iter()
        .map(|&seed| measure(&duel_header(seed, ours, theirs, scenario), ticks))
        .collect()
}

/// The average of the counts over the seeds.
fn average(counts: &[Counts]) -> Counts {
    let mut total = Counts::default();
    for count in counts {
        for squad in 0..2 {
            total.hold[squad] += count.hold[squad];
            total.tags[squad] += count.tags[squad];
        }
    }
    let n = counts.len().max(1) as i64;
    for squad in 0..2 {
        total.hold[squad] /= n;
        total.tags[squad] /= n;
    }
    total
}

/// The seeds the policy is trained on. They are disjoint from the seeds it
/// is judged on.
fn train_seeds(count: usize) -> Vec<u64> {
    (0..count as u64).map(|i| 900_000 + i).collect()
}

/// The seeds the policy is judged on: the block the flow itself judges.
fn held_seeds(count: usize) -> Vec<u64> {
    eval_seeds(&[], count, 1)
}

// ─────────────────────────────────────────────────────────────────────────────
// The reward
// ─────────────────────────────────────────────────────────────────────────────

/// What a round is worth: the weights of the metrics.
///
/// This is the coaching surface. The base objective weights the objectives
/// the squad holds; a directive that "rewards contact" turns the tag weight
/// up and the trained behaviour moves with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Directive {
    /// What a beacon-tick my squad holds is worth.
    hold: i64,
    /// What a tag my squad lands is worth.
    tags: i64,
    /// What a point of score margin is worth.
    margin: i64,
}

impl Directive {
    /// The default objective: hold the objectives, and win the margin.
    const BASE: Directive = Directive {
        hold: 1,
        tags: 0,
        margin: 1,
    };

    /// The first coaching directive: hold the objectives, and nothing else.
    ///
    /// The margin is worth nothing here, so a policy that wins the margin by
    /// giving up an objective is worth exactly as much as one that does not.
    const HOLD: Directive = Directive {
        hold: 1,
        tags: 0,
        margin: 0,
    };

    /// The coaching directive: the same objective, but contact is worth more.
    ///
    /// The weight is the interesting part. A tag stuns its target for 90
    /// ticks, and a stunned agent holds nothing — so a tag is worth about
    /// 90 beacon-ticks of the objective it denies, and a weight of 90 would
    /// be the obvious choice. It is not enough. The bench measures the
    /// threshold (`lever`): below it the directive changes the reward and
    /// nothing else, because holding an objective for a whole round is
    /// worth far more than the one tag it costs to win. At 600 — contact
    /// worth 40% of an objective held end to end — the choice flips, and
    /// the search has a contact-seeking tactic to move towards.
    const CONTACT: Directive = Directive {
        hold: 1,
        tags: 600,
        margin: 1,
    };

    /// The second coaching directive: the score margin is the whole
    /// objective, and occupying is worth nothing on its own.
    const MARGIN: Directive = Directive {
        hold: 0,
        tags: 0,
        margin: 1,
    };

    /// The third coaching directive: hold what we have *and* take what they
    /// want.
    ///
    /// The margin is weighted a hundred times the holding, so the reward is
    /// 101·(what we hold) − 100·(what they hold). The weight has to be that
    /// big: in the shipped game the other squad's beacon-ticks barely move
    /// when we move, so until the margin outweighs the holding an order of
    /// magnitude over, the two objectives are the same objective.
    const DENY: Directive = Directive {
        hold: 1,
        tags: 0,
        margin: 100,
    };

    /// The reward of a round, from the counts.
    fn reward(&self, counts: &Counts) -> i64 {
        self.hold * counts.hold[0] + self.tags * counts.tags[0] + self.margin * counts.margin()
    }

    /// The name of the directive, for the write-up.
    fn name(&self) -> &'static str {
        match (self.hold, self.tags, self.margin) {
            (1, 0, 0) => "lock down the objectives",
            (1, 0, 1) => "hold the objectives and win the margin",
            (0, 0, 1) => "win the margin",
            (1, 0, 100) => "hold ours and take theirs",
            (1, 600, 1) => "reward contact and tags more",
            _ => "a custom directive",
        }
    }
}

/// The directives the bench can coach with, by name.
///
/// The `train` command takes one of these names, so the command line says
/// which objective is being trained instead of a number.
const DIRECTIVES: [(&str, Directive); 5] = [
    ("hold", Directive::HOLD),
    ("margin", Directive::MARGIN),
    ("deny", Directive::DENY),
    ("contact", Directive::CONTACT),
    ("base", Directive::BASE),
];

/// The scenario with this name, or the shipped one.
fn scenario_by_name(name: &str) -> Scenario {
    match name {
        "scrimmage" => Scenario::SCRIMMAGE,
        _ => Scenario::SHIPPED,
    }
}

/// The directive with this name, or the default one.
fn directive_by_name(name: &str) -> Directive {
    DIRECTIVES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
        .unwrap_or(Directive::BASE)
}

/// The reward of a policy over a set of seeds: the mean reward.
fn score(
    policy: &Policy,
    directive: &Directive,
    seeds: &[u64],
    ticks: u64,
    scenario: Scenario,
) -> i64 {
    let source = policy.source();
    let counts = measure_seeds(&source, DEFAULT_REFLEX, seeds, ticks, scenario);
    let total: i64 = counts.iter().map(|c| directive.reward(c)).sum();
    total / seeds.len().max(1) as i64
}

// ─────────────────────────────────────────────────────────────────────────────
// The search: a (1+lambda) evolution strategy over the table
// ─────────────────────────────────────────────────────────────────────────────

/// A small deterministic generator, so a run can be repeated exactly.
struct Rng(u64);

impl Rng {
    /// A generator seeded by `seed`.
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// The next number, a splitmix64 step.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Train a policy against the reflex, under a directive.
///
/// The strategy is an evolution strategy over the table's entries. Every
/// generation proposes one offspring per (observation, action) pair -- the
/// parent with that one entry changed -- and takes the best action for each
/// observation; the table that results is checked against the parent and
/// kept only if it is worth more. The seeds of each evaluation are judged in
/// parallel across the cores, the way the seed farm does.
fn train(
    directive: &Directive,
    seeds: &[u64],
    ticks: u64,
    generations: usize,
    seed: u64,
    warm: bool,
    scenario: Scenario,
) -> (Policy, Vec<i64>) {
    let mut rng = Rng::new(seed);
    // A warm start begins from the best of the hand-written tactics, as the
    // directive sees them; a cold start begins from a random table.
    let mut parent = if warm {
        best_catalogue(directive, seeds, ticks, scenario)
    } else {
        Policy::random(&mut rng)
    };
    let mut history = vec![score(&parent, directive, seeds, ticks, scenario)];
    let start = Instant::now();

    for generation in 1..=generations {
        // The offspring: the parent with one entry changed.
        let mut offspring: Vec<(usize, usize, Policy)> = Vec::with_capacity(STATES * ACTIONS);
        for state in 0..STATES {
            for action in 0..ACTIONS {
                if action == parent.table[state] {
                    continue;
                }
                let mut child = parent.clone();
                child.table[state] = action;
                offspring.push((state, action, child));
            }
        }
        // The order is shuffled so the evaluation is spread evenly over the
        // cores, and so no observation is favoured by the schedule.
        for i in (1..offspring.len()).rev() {
            offspring.swap(i, rng.below(i + 1));
        }

        // The fitness of every offspring, in parallel.
        let fitness = evaluate_offspring(&offspring, directive, seeds, ticks, scenario);
        let parent_fitness = history[history.len() - 1];

        // The changes that each improved the parent on their own, the best
        // one first. They are applied one at a time and each one is kept
        // only if it is still an improvement where the last one landed:
        // two changes that each help can hurt together.
        let mut changes: Vec<(usize, usize, i64)> = Vec::new();
        for (index, (state, action, _)) in offspring.iter().enumerate() {
            if fitness[index] > parent_fitness {
                changes.push((*state, *action, fitness[index] - parent_fitness));
            }
        }
        changes.sort_by_key(|change| std::cmp::Reverse(change.2));

        let mut chosen = parent.clone();
        let mut chosen_fitness = parent_fitness;
        for (state, action, _) in changes {
            let mut trial = chosen.clone();
            trial.table[state] = action;
            let fitness = score(&trial, directive, seeds, ticks, scenario);
            if fitness > chosen_fitness {
                chosen = trial;
                chosen_fitness = fitness;
            }
        }
        parent = chosen;
        history.push(chosen_fitness);
        println!("  generation {generation:>3}: reward {chosen_fitness}");
    }
    println!(
        "  trained in {:?} over {generations} generations",
        start.elapsed()
    );
    (parent, history)
}

/// The best of the hand-written tactics, under a directive.
fn best_catalogue(directive: &Directive, seeds: &[u64], ticks: u64, scenario: Scenario) -> Policy {
    let mut best = Policy::constant(Action::Take);
    let mut best_reward = i64::MIN;
    for (_, policy) in catalogue() {
        let reward = score(&policy, directive, seeds, ticks, scenario);
        if reward > best_reward {
            best_reward = reward;
            best = policy;
        }
    }
    best
}

/// The fitness of every offspring: the mean reward over the seeds.
///
/// The work is spread **flat** over the cores: the offspring are split
/// between the workers, and each worker plays its own offspring's seeds one
/// after another. The parallelism never nests, so the spike holds no more
/// threads than it has cores -- the box is shared with other jobs, and the
/// brief caps this one at eight.
fn evaluate_offspring(
    offspring: &[(usize, usize, Policy)],
    directive: &Directive,
    seeds: &[u64],
    ticks: u64,
    scenario: Scenario,
) -> Vec<i64> {
    let workers = CORES.min(offspring.len().max(1));
    let mut parts: Vec<Vec<(usize, i64)>> = Vec::new();
    std::thread::scope(|scope| {
        let mut workers_out = Vec::new();
        for worker in 0..workers {
            let from = worker * offspring.len() / workers;
            let to = (worker + 1) * offspring.len() / workers;
            if from == to {
                continue;
            }
            let part = &offspring[from..to];
            workers_out.push(scope.spawn(move || {
                part.iter()
                    .enumerate()
                    .map(|(offset, (_, _, policy))| {
                        let source = policy.source();
                        let counts =
                            measure_seeds_serial(&source, DEFAULT_REFLEX, seeds, ticks, scenario);
                        let total: i64 = counts.iter().map(|c| directive.reward(c)).sum();
                        (from + offset, total / seeds.len().max(1) as i64)
                    })
                    .collect::<Vec<(usize, i64)>>()
            }));
        }
        for worker in workers_out {
            parts.push(worker.join().expect("a worker finishes its offspring"));
        }
    });
    let mut all = vec![i64::MIN; offspring.len()];
    for part in parts {
        for (index, fitness) in part {
            all[index] = fitness;
        }
    }
    all
}

// ─────────────────────────────────────────────────────────────────────────────
// The judgement: the seed farm's rule
// ─────────────────────────────────────────────────────────────────────────────

/// What the judgement of a policy says.
struct Judgement {
    /// The per-seed deltas of the metric.
    deltas: Vec<i64>,
    /// How many seeds the delta met the bar on.
    qualified: usize,
    /// The sign test's p-value.
    p: f64,
    /// Whether the seed farm's rule adopts the change.
    adopted: bool,
}

/// The two rounds of every seed: the reflex's and the policy's.
///
/// The rounds are played once and read for every metric, so judging three
/// metrics costs three metrics' worth of *counting* and one round's worth
/// of *playing*.
fn duel_counts(policy: &Policy, seeds: &[u64], ticks: u64) -> Vec<(Counts, Counts)> {
    let source = policy.source();
    let baseline = measure_seeds(
        DEFAULT_REFLEX,
        DEFAULT_REFLEX,
        seeds,
        ticks,
        Scenario::SHIPPED,
    );
    let candidate = measure_seeds(&source, DEFAULT_REFLEX, seeds, ticks, Scenario::SHIPPED);
    baseline.into_iter().zip(candidate).collect()
}

/// Judge every metric at once, from one pair of rounds per seed.
fn judge_all(policy: &Policy, seeds: &[u64], ticks: u64) -> Vec<(Metric, Judgement)> {
    let pairs = duel_counts(policy, seeds, ticks);
    [Metric::Hold, Metric::Tags, Metric::Margin]
        .iter()
        .map(|metric| {
            let deltas: Vec<i64> = pairs
                .iter()
                .map(|(baseline, candidate)| metric.of(candidate) - metric.of(baseline))
                .collect();
            (*metric, summarise(&deltas))
        })
        .collect()
}

/// The seed farm's verdict on a set of per-seed deltas.
///
/// The rule is the seed farm's *proposed* rule (SEEDFARM.md): the delta
/// meets the sealed bar on at least three seeds in four, **and** the sign
/// test on the deltas' direction has p < 0.05. The flow's own evaluator
/// today applies the count alone; the sign test is the half of the rule
/// that is not shipped yet, so the bench reports both.
fn summarise(deltas: &[i64]) -> Judgement {
    let qualified = deltas.iter().filter(|&&d| d >= 1).count();
    let p = sign_test(deltas);
    let threshold = (deltas.len() * 3).div_ceil(4);
    Judgement {
        adopted: !deltas.is_empty() && qualified >= threshold && p < 0.05,
        deltas: deltas.to_vec(),
        qualified,
        p,
    }
}

/// The metrics a claim may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    /// The beacon-ticks the first squad held.
    Hold,
    /// The tags the first squad landed.
    Tags,
    /// The score margin.
    Margin,
}

impl Metric {
    /// The name the flow knows the metric by.
    fn name(self) -> &'static str {
        match self {
            Metric::Hold => "hold_ticks_squad0",
            Metric::Tags => "tags_for_squad0",
            Metric::Margin => "margin_squad0",
        }
    }

    /// The value of the metric in a round's counts.
    fn of(self, counts: &Counts) -> i64 {
        match self {
            Metric::Hold => counts.hold[0],
            Metric::Tags => counts.tags[0],
            Metric::Margin => counts.margin(),
        }
    }
}

/// The sign test on the per-seed deltas.
///
/// Under the null ("the change does nothing") each seed's delta is as likely
/// up as down, so the number of seeds that moved the way the claim says is a
/// coin toss count.
fn sign_test(deltas: &[i64]) -> f64 {
    let up = deltas.iter().filter(|&&d| d > 0).count();
    let down = deltas.iter().filter(|&&d| d < 0).count();
    let n = up + down;
    if n == 0 {
        return 1.0;
    }
    binomial_upper_tail(n, up)
}

/// P(X >= k) for X ~ Binomial(n, 1/2), summed from the far end so no term
/// overflows.
fn binomial_upper_tail(n: usize, k: usize) -> f64 {
    if k == 0 || n == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    if 2 * k <= n {
        return 1.0 - binomial_upper_tail(n, n - k + 1);
    }
    let mut p = 2f64.powi(-(n as i32));
    let mut mass = p;
    for j in 0..(n - k) {
        mass *= (n - j) as f64 / (j + 1) as f64;
        p += mass;
    }
    p.min(1.0)
}

/// A sealed prediction, for the flow's own evaluator.
fn prediction(seeds: &[u64], metric: &str) -> Prediction {
    Prediction {
        round_id: "rl-spike".into(),
        proposal_hash: "0000000000000000".into(),
        metric: metric.into(),
        direction: Direction::Up,
        min_delta: 1,
        eval_seeds: seeds.to_vec(),
        author: "rlspike".into(),
        text: "the learned policy holds more than the shipped reflex".into(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The commands
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("probe");
    let number = |at: usize, default: usize| -> usize {
        args.get(at).and_then(|a| a.parse().ok()).unwrap_or(default)
    };
    let text = |at: usize, default: &str| -> String {
        args.get(at).cloned().unwrap_or_else(|| default.to_owned())
    };
    match command {
        "probe" => probe(
            number(1, 16),
            number(2, DEFAULT_TICKS as usize) as u64,
            scenario_by_name(&text(3, "shipped")),
        ),
        "candidates" => candidates(number(1, 32), number(2, DEFAULT_TICKS as usize) as u64),
        "train" => train_command(
            number(1, 8),
            number(2, 8),
            text(3, "hold"),
            number(4, DEFAULT_TICKS as usize) as u64,
        ),
        "judge" => judge_command(number(1, 32)),
        "verify" => verify(),
        "coach" => coach(
            number(1, 8),
            number(2, 8),
            text(3, "cold"),
            Scenario::SHIPPED,
        ),
        "scrimmage" => scrimmage(number(1, 8), number(2, 8)),
        "lever" => lever(number(1, 8), scenario_by_name(&text(2, "scrimmage"))),
        "check" => check(number(1, 32)),
        "mirror" => mirror(number(1, 32)),
        "trace" => trace(number(1, 900_000) as u64, number(2, 3_000) as u64),
        other => {
            println!("unknown command {other:?}");
        }
    }
}

/// Trace where the agents of the shipped reflex go.
fn trace(seed: u64, ticks: u64) {
    // If a policy has been trained, squad 0 plays it; otherwise both squads
    // play the shipped reflex.
    let policy = std::fs::read_to_string("runs/rlspike/policy.rhai").ok();
    let header = match &policy {
        Some(source) => duel_header(seed, source, DEFAULT_REFLEX, Scenario::SHIPPED),
        None => Header::default_round(seed, DEFAULT_REFLEX),
    };
    let mut sim = Sim::new(&header).expect("the round starts");
    let cell_units = sim.world().cell_units;
    let beacons = sim.world().beacons.clone();
    println!("## trace: seed {seed}, {ticks} ticks");
    println!("  beacons {beacons:?}");
    for tick in 0..ticks {
        let out = sim.step(&[]);
        for event in &out.events {
            if let Event::Tagged { by, target } = event {
                println!("  tick {tick}: agent {by} tagged agent {target}");
            }
        }
        if tick % 250 == 0 || tick + 1 == ticks {
            let cells: Vec<Option<lm_core::world::CellXY>> = (0..8u16)
                .map(|id| {
                    sim.agent(id)
                        .map(|a| sim.world().grid.cell_of(a.pos, cell_units))
                })
                .collect();
            let mut tags = 0;
            for event in &out.events {
                if matches!(event, Event::Tagged { .. }) {
                    tags += 1;
                }
            }
            println!(
                "  tick {tick:>4}: {cells:?} scores {:?}{}",
                sim.scores(),
                if tags > 0 { " [TAG]" } else { "" }
            );
        }
    }
}

/// The baseline: the shipped reflex against itself.
fn probe(seed_count: usize, ticks: u64, scenario: Scenario) {
    let seeds = train_seeds(seed_count);
    let start = Instant::now();
    let counts = measure_seeds(DEFAULT_REFLEX, DEFAULT_REFLEX, &seeds, ticks, scenario);
    let elapsed = start.elapsed();
    let mean = average(&counts);
    println!("## probe: the shipped reflex against itself, {seed_count} seeds at {ticks} ticks");
    println!("  the world is {}", scenario.name);
    println!(
        "  wall time {:?} ({:.3} s per seed, {} cores)",
        elapsed,
        elapsed.as_secs_f64() / seed_count.max(1) as f64,
        CORES
    );
    println!(
        "  beacon-ticks  squad 0 {:>6}  squad 1 {:>6}",
        mean.hold[0], mean.hold[1]
    );
    println!(
        "  tags          squad 0 {:>6}  squad 1 {:>6}",
        mean.tags[0], mean.tags[1]
    );
    println!("  margin (average) {}", mean.margin());
    let mut margins: Vec<i64> = counts.iter().map(Counts::margin).collect();
    margins.sort_unstable();
    let draws = margins.iter().filter(|&&m| m == 0).count();
    let wins = margins.iter().filter(|&&m| m > 0).count();
    let losses = margins.iter().filter(|&&m| m < 0).count();
    println!("  rounds: squad 0 ahead {wins}, level {draws}, behind {losses}");
}

/// The hand-written policies, played against the shipped reflex.
///
/// The seeds are the **held-out** block, the one the flow judges on, so
/// every row of this table is a row the flow could have sealed.
fn candidates(seed_count: usize, ticks: u64) {
    let seeds = held_seeds(seed_count);
    println!("## candidates: hand-written tables against the shipped reflex");
    println!("  {seed_count} held-out seeds at {ticks} ticks");
    println!();
    println!("| policy | hold 0 | hold 1 | tags 0 | tags 1 | margin | vs itself: hold 0 | tags 0 | tags 1 |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    // The trained policy, if there is one, is measured on the same seeds as
    // the hand-written ones, so the two can be held against each other.
    let trained = std::fs::read_to_string("runs/rlspike/policy.rhai").ok();
    let mut rows: Vec<(String, String)> = catalogue()
        .into_iter()
        .map(|(name, policy)| (name, policy.source()))
        .collect();
    if let Some(source) = trained {
        rows.push(("the trained policy".to_owned(), source));
    }
    for (name, source) in rows {
        let counts = measure_seeds(&source, DEFAULT_REFLEX, &seeds, ticks, Scenario::SHIPPED);
        let mean = average(&counts);
        let errors: u64 = counts.iter().map(|c| c.reflex_errors).sum();
        let refused: u64 = counts.iter().map(|c| c.reflex_refused).sum();
        let fuel: u64 = counts.iter().map(|c| c.reflex_out_of_fuel).sum();
        let mirror = average(&measure_seeds(
            &source,
            &source,
            &seeds,
            ticks,
            Scenario::SHIPPED,
        ));
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} |",
            mean.hold[0],
            mean.hold[1],
            mean.tags[0],
            mean.tags[1],
            mean.margin(),
            mirror.hold[0],
            mirror.tags[0],
            mirror.tags[1]
        );
        if errors + refused + fuel > 0 {
            println!("  ({name}: {errors} errors, {refused} refused, {fuel} out of fuel)");
        }
    }
}

/// Train a policy under a directive, and judge it on held-out seeds.
fn train_command(seed_count: usize, generations: usize, which: String, ticks: u64) {
    let seeds = train_seeds(seed_count);
    let directive = directive_by_name(&which);
    println!("## train: the directive is '{}'", directive.name());
    println!("  {seed_count} training seeds, {generations} generations, {ticks} ticks");
    let (policy, history) = train(
        &directive,
        &seeds,
        ticks,
        generations,
        0x5EED_0001,
        false,
        Scenario::SHIPPED,
    );
    println!("  reward {} -> {}", history[0], history[history.len() - 1]);
    write_policy(&policy);
    println!();
    println!("| t | a | h | e | action |");
    println!("|---:|---:|---:|---:|---|");
    for row in policy.rows() {
        println!("| {row} |");
    }
    let held = held_seeds(32);
    println!("### on the 32 held-out seeds, against the shipped reflex");
    let counts = measure_seeds(
        &policy.source(),
        DEFAULT_REFLEX,
        &held,
        ticks,
        Scenario::SHIPPED,
    );
    let mean = average(&counts);
    let errors: u64 = counts.iter().map(|c| c.reflex_errors).sum();
    let refused: u64 = counts.iter().map(|c| c.reflex_refused).sum();
    let fuel: u64 = counts.iter().map(|c| c.reflex_out_of_fuel).sum();
    println!(
        "  beacon-ticks  squad 0 {:>6}  squad 1 {:>6}",
        mean.hold[0], mean.hold[1]
    );
    println!(
        "  tags          squad 0 {:>6}  squad 1 {:>6}",
        mean.tags[0], mean.tags[1]
    );
    println!("  margin (average) {}", mean.margin());
    println!("  the policy's reflex: {errors} errors, {refused} refused, {fuel} out of fuel");
    for (metric, judgement) in judge_all(&policy, &held, ticks) {
        println!(
            "  {}: {} of {} seeds up, sign p {:.3e}, {}",
            metric.name(),
            judgement.qualified,
            judgement.deltas.len(),
            judgement.p,
            if judgement.adopted {
                "ADOPTED"
            } else {
                "rejected"
            }
        );
    }
}

/// Write the trained policy out, as the script that plays it.
fn write_policy(policy: &Policy) {
    let path = std::path::Path::new("runs/rlspike/policy.rhai");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, policy.source()) {
        Ok(()) => println!("  the policy is written to {}", path.display()),
        Err(err) => println!("  the policy was not written: {err}"),
    }
}

/// Judge a trained policy with the flow's own evaluator.
///
/// The spike's own `judge` reads the rounds directly; this one goes through
/// `lm-learn`'s evaluator -- the same call the flow makes -- so the two
/// numbers can be held against each other.
fn judge_command(seed_count: usize) {
    let path = std::path::Path::new("runs/rlspike/policy.rhai");
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            println!("no trained policy at {}: {err}", path.display());
            println!("run the train command first");
            return;
        }
    };
    let seeds = held_seeds(seed_count);
    println!("## judge: the flow's own evaluator, {seed_count} held-out seeds");
    for metric in [Metric::Hold, Metric::Tags, Metric::Margin] {
        let baseline =
            |seed: u64| duel_header(seed, DEFAULT_REFLEX, DEFAULT_REFLEX, Scenario::SHIPPED);
        let candidate = |seed: u64| duel_header(seed, &source, DEFAULT_REFLEX, Scenario::SHIPPED);
        let pred = prediction(&seeds, metric.name());
        let verdict = lm_learn::eval::evaluate(&baseline, &candidate, DEFAULT_TICKS, &pred, 0)
            .expect("the evaluation runs");
        let deltas: Vec<i64> = verdict.deltas.iter().map(|(_, delta)| *delta).collect();
        let qualified = deltas.iter().filter(|&&d| d >= 1).count();
        let p = sign_test(&deltas);
        let threshold = (deltas.len() * 3).div_ceil(4);
        let adopted = qualified >= threshold && p < 0.05;
        println!(
            "  {}: the flow {}; {} of {} seeds up, sign p {:.3e} -> {}",
            metric.name(),
            if verdict.adopted {
                "adopted"
            } else {
                "rejected"
            },
            qualified,
            deltas.len(),
            p,
            if adopted { "ADOPTED" } else { "rejected" }
        );
        let moved: Vec<i64> = deltas.iter().copied().filter(|d| *d != 0).collect();
        println!("    the deltas that moved: {moved:?}");
    }
}

/// Verify that the trained policy's rounds are bit-exact.
///
/// Two claims are checked. First, the same seed plays the same round twice, segment
/// hash for segment hash: the policy is a pure integer function of the score view, so
/// the replay is the round. Second, the round the flow's own evaluator plays is the
/// round this bench plays -- the same hash chain, so the two paths are the same game.
fn verify() {
    let path = std::path::Path::new("runs/rlspike/policy.rhai");
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            println!("no trained policy at {}: {err}", path.display());
            return;
        }
    };
    let seeds = held_seeds(4);
    for seed in seeds {
        let header = duel_header(seed, &source, DEFAULT_REFLEX, Scenario::SHIPPED);
        let first = play(header.clone(), DEFAULT_TICKS);
        let second = play(header.clone(), DEFAULT_TICKS);
        let chain = |log: &lm_core::log::RoundLog| -> Vec<u64> {
            log.segments.iter().map(|segment| segment.hash).collect()
        };
        let a = chain(&first);
        let b = chain(&second);
        println!(
            "  seed {seed}: the same round twice: {}",
            if a == b { "identical" } else { "DIFFERENT" }
        );

        // The flow's own evaluator, on the same seed and the same header.
        let baseline = |s: u64| duel_header(s, DEFAULT_REFLEX, DEFAULT_REFLEX, Scenario::SHIPPED);
        let candidate = |s: u64| duel_header(s, &source, DEFAULT_REFLEX, Scenario::SHIPPED);
        let pred = prediction(&[seed], "hold_ticks_squad0");
        let verdict = lm_learn::eval::evaluate(&baseline, &candidate, DEFAULT_TICKS, &pred, 0)
            .expect("the evaluation runs");
        let judged = lm_learn::eval::judge_seeds(
            &baseline,
            &candidate,
            DEFAULT_TICKS,
            &[seed],
            "hold_ticks_squad0",
        )
        .expect("the seeds are judged");
        let flow_chain: Vec<u64> = judged
            .iter()
            .flat_map(|j| j.candidate_hashes.clone())
            .collect();
        let _ = &verdict;
        println!(
            "  seed {seed}: the flow's round is this bench's round: {}",
            if flow_chain == a {
                "identical"
            } else {
                "DIFFERENT"
            }
        );
    }
}

/// The coaching experiment: four directives, four policies, one difference.
///
/// The search, the seeds and the horizon are the same. The only thing that
/// changes between the runs is the weights of the reward, which is what a
/// directive between rounds would change.
///
/// The start matters, and both starts are measured:
///
/// * a **cold** start begins from a random table, so the search has to find
///   the tactic as well as choose it;
/// * a **warm** start begins from the best of the hand-written tactics, as
///   the directive sees them, so the directive has something to choose
///   between.
fn coach(seed_count: usize, generations: usize, start: String, scenario: Scenario) {
    let seeds = train_seeds(seed_count);
    let held = held_seeds(32);
    let ticks = COACH_TICKS;
    let warm = start != "cold";
    println!("## coach: the same search, four reward weights");
    println!("  {seed_count} training seeds, {generations} generations, {ticks} ticks");
    println!("  the world is {}", scenario.name);
    if warm {
        println!("  the search starts warm: from the best of the hand-written tactics, as the");
        println!("  directive sees them, so the directive has something to choose between");
    } else {
        println!("  the search starts cold: from a random table, so the directive has nothing");
        println!("  to choose between until the search finds a tactic of its own");
    }
    println!();

    let trained = coach_run(&seeds, ticks, generations, warm, scenario);

    println!("### what the policies do, against the shipped reflex");
    println!();
    println!("| directive | hold 0 | hold 1 | tags 0 | tags 1 | margin |");
    println!("|---|---:|---:|---:|---:|---:|");
    for (name, _, policy) in &trained {
        let counts = measure_seeds(&policy.source(), DEFAULT_REFLEX, &held, ticks, scenario);
        let mean = average(&counts);
        println!(
            "| {name} | {} | {} | {} | {} | {} |",
            mean.hold[0],
            mean.hold[1],
            mean.tags[0],
            mean.tags[1],
            mean.margin()
        );
    }
    println!();
    println!("### the tables");
    for (name, _, policy) in &trained {
        println!("**{name}**");
        println!();
        println!("| t | a | h | e | action |");
        println!("|---:|---:|---:|---:|---|");
        for row in policy.rows() {
            println!("| {row} |");
        }
        println!();
    }
}

/// The directives the coaching experiment runs, in the order it runs them.
const COACH_DIRECTIVES: [(&str, Directive); 4] = [
    ("lock down the objectives", Directive::HOLD),
    ("win the margin", Directive::MARGIN),
    ("hold ours and take theirs", Directive::DENY),
    ("reward contact and tags more", Directive::CONTACT),
];

/// Train one policy per directive, and hand back what each one plays.
///
/// Everything is held fixed except the reward's weights: the same search,
/// the same training seeds, the same horizon, the same generator seed.
fn coach_run(
    seeds: &[u64],
    ticks: u64,
    generations: usize,
    warm: bool,
    scenario: Scenario,
) -> Vec<(&str, Directive, Policy)> {
    let mut trained: Vec<(&str, Directive, Policy)> = Vec::new();
    for (name, directive) in COACH_DIRECTIVES {
        println!("### the directive: {name}");
        let (policy, history) = train(
            &directive,
            seeds,
            ticks,
            generations,
            0x5EED_0001,
            warm,
            scenario,
        );
        println!("  reward {} -> {}", history[0], history[history.len() - 1]);
        trained.push((name, directive, policy));
        println!();
    }
    trained
}

/// The scrimmage: the same coaching experiment, in a world where the squads
/// meet.
///
/// The shipped world is so large that the two squads never come within tag
/// range, so `tags_for_squad0` is identically zero and a directive that
/// rewards contact has nothing to reward. The scrimmage world is a small
/// world the bench builds -- the game's defaults are untouched -- where the
/// squads meet, so the tag metric moves and the directives have something to
/// disagree about.
fn scrimmage(seed_count: usize, generations: usize) {
    let scenario = Scenario::SCRIMMAGE;
    let seeds = train_seeds(seed_count);
    let held = held_seeds(32);
    let ticks = COACH_TICKS;

    println!("## scrimmage: the same experiment, in {}", scenario.name);
    println!("  {seed_count} training seeds, {generations} generations, {ticks} ticks");
    println!();

    // First: does contact happen at all here?
    let mirror = measure_seeds(DEFAULT_REFLEX, DEFAULT_REFLEX, &seeds, ticks, scenario);
    let mean = average(&mirror);
    println!(
        "### the shipped reflex against itself, in {}",
        scenario.name
    );
    println!(
        "  beacon-ticks  squad 0 {:>6}  squad 1 {:>6}",
        mean.hold[0], mean.hold[1]
    );
    println!(
        "  tags          squad 0 {:>6}  squad 1 {:>6}",
        mean.tags[0], mean.tags[1]
    );
    println!("  margin (average) {}", mean.margin());
    println!();

    for warm in [false, true] {
        println!(
            "### the search starts {}",
            if warm { "warm" } else { "cold" }
        );
        println!();
        let trained = coach_run(&seeds, ticks, generations, warm, scenario);
        println!("### what the policies do, against the shipped reflex");
        println!();
        println!("| directive | hold 0 | hold 1 | tags 0 | tags 1 | margin |");
        println!("|---|---:|---:|---:|---:|---:|");
        for (name, _, policy) in &trained {
            let counts = measure_seeds(&policy.source(), DEFAULT_REFLEX, &held, ticks, scenario);
            let mean = average(&counts);
            println!(
                "| {name} | {} | {} | {} | {} | {} |",
                mean.hold[0],
                mean.hold[1],
                mean.tags[0],
                mean.tags[1],
                mean.margin()
            );
        }
        println!();
        println!("### the tables");
        for (name, _, policy) in &trained {
            println!("**{name}**");
            println!();
            println!("| t | a | h | e | action |");
            println!("|---:|---:|---:|---:|---|");
            for row in policy.rows() {
                println!("| {row} |");
            }
            println!();
        }
    }
}

/// The lever: what a tag has to be worth before the directive moves the
/// choice.
///
/// The reward of a policy under a directive is a linear form in the round's
/// metrics, so the bench measures the metrics **once** and then reads the
/// reward off for every weight. The table says which tactic wins at each
/// weight, and where the winner changes: that is the threshold the
/// directive has to clear before it can pull on anything.
fn lever(seed_count: usize, scenario: Scenario) {
    let seeds = train_seeds(seed_count);
    println!("## lever: what a tag has to be worth to move the choice");
    println!("  the world is {}, {seed_count} seeds", scenario.name);
    println!();
    println!("| policy | hold 0 | tags 0 | margin |");
    println!("|---|---:|---:|---:|");
    let mut rows: Vec<(String, Counts)> = Vec::new();
    for (name, policy) in catalogue() {
        let counts = measure_seeds(
            &policy.source(),
            DEFAULT_REFLEX,
            &seeds,
            DEFAULT_TICKS,
            scenario,
        );
        let mean = average(&counts);
        rows.push((name.clone(), mean));
        println!(
            "| {name} | {} | {} | {} |",
            mean.hold[0],
            mean.tags[0],
            mean.margin()
        );
    }
    println!();
    println!("### the winner at each tag weight (reward = hold + w·tags + margin)");
    println!();
    println!("| tag weight | the best tactic | its reward | the runner-up | the gap |");
    println!("|---:|---|---:|---|---:|");
    for weight in [0i64, 60, 90, 150, 300, 600, 1200, 2400, 4800] {
        let mut scored: Vec<(String, i64)> = rows
            .iter()
            .map(|(name, mean)| {
                let reward = mean.hold[0] + weight * mean.tags[0] + mean.margin();
                (name.clone(), reward)
            })
            .collect();
        scored.sort_by_key(|(_, reward)| std::cmp::Reverse(*reward));
        let (best, best_reward) = scored[0].clone();
        let (second, second_reward) = scored[1].clone();
        println!(
            "| {weight} | {best} | {best_reward} | {second} | {} |",
            best_reward - second_reward
        );
    }
}

/// The hand-written tactics, judged by the flow's own rule.
///
/// This is the bench's own check on the compiler and the observation: the
/// `reflex-like` row is the shipped reflex's rule written as a table, so
/// if the table is right it plays the same game as the reflex and its
/// deltas are zero.
fn check(seed_count: usize) {
    let seeds = held_seeds(seed_count);
    println!("## check: the hand-written tactics, judged against the shipped reflex");
    println!("  {seed_count} held-out seeds, {DEFAULT_TICKS} ticks");
    println!();
    println!("| tactic | hold: seeds up | sign p | verdict | tags: seeds up | sign p |");
    println!("|---|---:|---:|---|---:|---:|");
    let mut moved: Vec<(String, Vec<i64>)> = Vec::new();
    for (name, policy) in catalogue() {
        let judged = judge_all(&policy, &seeds, DEFAULT_TICKS);
        let hold = judged
            .iter()
            .find(|(metric, _)| *metric == Metric::Hold)
            .map(|(_, judgement)| judgement);
        let tags = judged
            .iter()
            .find(|(metric, _)| *metric == Metric::Tags)
            .map(|(_, judgement)| judgement);
        let (hold, tags) = (
            hold.expect("the hold metric"),
            tags.expect("the tags metric"),
        );
        println!(
            "| {name} | {} of {} | {:.3e} | {} | {} of {} | {:.3e} |",
            hold.qualified,
            hold.deltas.len(),
            hold.p,
            if hold.adopted { "adopted" } else { "rejected" },
            tags.qualified,
            tags.deltas.len(),
            tags.p
        );
        let deltas: Vec<i64> = hold.deltas.iter().copied().filter(|d| *d != 0).collect();
        if !deltas.is_empty() {
            moved.push((name.clone(), deltas));
        }
    }
    if !moved.is_empty() {
        println!();
        println!("The hold-ticks deltas that moved, per tactic:");
        println!();
        for (name, deltas) in moved {
            println!("* `{name}`: {deltas:?}");
        }
    }
}

/// The mirror: both squads playing the trained policy.
///
/// D63 says the reflex match is exactly equal, so decisiveness has to come
/// from breaking the mirror. A learned policy is per-squad, so it can
/// break it. This command asks what actually happens when both sides play
/// the trained policy, and against the reflex.
fn mirror(seed_count: usize) {
    let path = std::path::Path::new("runs/rlspike/policy.rhai");
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            println!("no trained policy at {}: {err}", path.display());
            println!("run the train command first");
            return;
        }
    };
    let seeds = held_seeds(seed_count);
    let ticks = DEFAULT_TICKS;
    println!("## mirror: {seed_count} held-out seeds, {ticks} ticks");
    println!();
    println!("| match | hold 0 | hold 1 | tags 0 | tags 1 | margin | ahead | level | behind |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let matches = [
        ("reflex vs reflex", DEFAULT_REFLEX, DEFAULT_REFLEX),
        ("policy vs reflex", source.as_str(), DEFAULT_REFLEX),
        ("reflex vs policy", DEFAULT_REFLEX, source.as_str()),
        ("policy vs policy", source.as_str(), source.as_str()),
    ];
    for (name, ours, theirs) in matches {
        let counts = measure_seeds(ours, theirs, &seeds, ticks, Scenario::SHIPPED);
        let mean = average(&counts);
        let wins = counts.iter().filter(|c| c.margin() > 0).count();
        let draws = counts.iter().filter(|c| c.margin() == 0).count();
        let losses = counts.iter().filter(|c| c.margin() < 0).count();
        println!(
            "| {name} | {} | {} | {} | {} | {} | {wins} | {draws} | {losses} |",
            mean.hold[0],
            mean.hold[1],
            mean.tags[0],
            mean.tags[1],
            mean.margin()
        );
    }
}

/// The hand-written policies the spike starts from.
fn catalogue() -> Vec<(String, Policy)> {
    let mut out = Vec::new();

    // The reflex's own rule, in the table's buckets: no threat means take a
    // beacon; a threat means charge when we have the numbers and flee when we
    // do not.
    let mut reflex_like = Policy::constant(Action::Take);
    for t in 0..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 0..H_BUCKETS {
                for e in 0..E_BUCKETS {
                    let have_numbers = a + 1 >= t;
                    let action = if t == 0 {
                        Action::Take
                    } else if have_numbers {
                        Action::Charge
                    } else {
                        Action::Flee
                    };
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    reflex_like.table[observation.index()] = action.index();
                }
            }
        }
    }
    out.push(("reflex-like".to_owned(), reflex_like.clone()));

    // The reflex's rule, but a squad that holds a beacon keeps it.
    let mut defend = reflex_like.clone();
    for t in 1..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 1..H_BUCKETS {
                for e in 0..E_BUCKETS {
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    let index = observation.index();
                    if defend.table[index] == Action::Flee.index() {
                        defend.table[index] = Action::Defend.index();
                    }
                }
            }
        }
    }
    out.push(("defend".to_owned(), defend));

    // Hold the beacon we have, whatever else is going on.
    let mut hold = Policy::constant(Action::Take);
    for t in 0..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 1..H_BUCKETS {
                for e in 0..E_BUCKETS {
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    hold.table[observation.index()] = Action::Defend.index();
                }
            }
        }
    }
    out.push(("hold".to_owned(), hold.clone()));

    // Hunt: walk at the enemy the squad remembers, whatever else.
    out.push(("hunt".to_owned(), Policy::constant(Action::Hunt)));

    // Charge every fresh threat, whatever the numbers.
    let mut charge = Policy::constant(Action::Take);
    for t in 1..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 0..H_BUCKETS {
                for e in 0..E_BUCKETS {
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    charge.table[observation.index()] = Action::Charge.index();
                }
            }
        }
    }
    out.push(("charge".to_owned(), charge));

    // Hold the beacon, but hunt when the squad remembers an enemy.
    let mut hold_and_hunt = hold.clone();
    for t in 0..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 0..H_BUCKETS {
                for e in 1..E_BUCKETS {
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    hold_and_hunt.table[observation.index()] = Action::Hunt.index();
                }
            }
        }
    }
    out.push(("hold-and-hunt".to_owned(), hold_and_hunt));

    // Spread: walk to the second nearest free beacon, to reach across the
    // map and spread the squad over two objectives.
    out.push(("spread".to_owned(), Policy::constant(Action::Spread)));

    // Spread first, then hold what the squad has.
    let mut spread_and_hold = Policy::constant(Action::Spread);
    for t in 0..T_BUCKETS {
        for a in 0..A_BUCKETS {
            for h in 1..H_BUCKETS {
                for e in 0..E_BUCKETS {
                    let observation = Observation {
                        threats: t,
                        allies: a,
                        holding: h == 1,
                        remembering: e == 1,
                    };
                    spread_and_hold.table[observation.index()] = Action::Defend.index();
                }
            }
        }
    }
    out.push(("spread-and-hold".to_owned(), spread_and_hold));

    out
}

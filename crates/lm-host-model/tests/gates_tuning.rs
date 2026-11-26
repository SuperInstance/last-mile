//! The gates tuning measurement: the Thor's d1-3B on the default question
//! pack, over seeded mid-round situations.
//!
//! The finding this measures (phase 3 Task 7): against the real Thor
//! d1-3B the default pack — a "what should you do now?" question over the
//! bare menu — scored 0.20-0.47 confidence, below
//! `AgentsConfig::min_confidence["thor-d1-3b"] = 0.55`, so nearly every
//! thought is refused and the reflex plays the round.
//!
//! The evidence for the fix is E4c (docs/research/poc/thor/RESULTS.md):
//! d1-3B scored 0.97 when the question stated a rule over code-computed
//! plain-language facts. "Which action, given this rule?" works; "what
//! would you do?" does not. So the pack now states the squad's doctrine as
//! a rule over the facts, and this test measures whether that is enough.
//!
//! It is ignored by default: it needs the Thor on the LAN and it talks to
//! a real model. Run it with:
//!
//! ```text
//! cargo test -p lm-host-model --test gates_tuning -- --ignored --nocapture
//! ```
//!
//! What it prints, for the pack the harness shipped before the tuning and
//! for the pack it ships now, over the same situations:
//!
//!   * the confidence distribution (p10, p50, p90),
//!   * the share of answers above the 0.55 floor,
//!   * the agreement with a code oracle that applies the same rule, and
//!   * the latency of a call.
//!
//! The situations are real: a round is played on seeds 1..30 and one
//! agent's view is asked about, so the facts are the facts of a game in
//! progress and not a made-up scene. Two samples are taken from each
//! round, because the doctrine has three branches and a round has a
//! rhythm:
//!
//!   * **the march** (tick 600): both squads are still walking to the
//!     beacons and have never seen each other. Every situation here is
//!     the doctrine's last branch — "go to the nearest beacon your squad
//!     does not hold".
//!   * **the fight** (tick 1800): the squads have met at the beacons, so
//!     enemies are in sight and the first branch — retreat — fires, and
//!     the second — hold, because the beacon is contested — is live too.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::facts::Facts;
use lm_agents::gates::GateSet;
use lm_agents::shared::SharedConfig;
use lm_agents::sysone::{Answer, Refusal, SysOne, ThorBackend};
use lm_core::header::Header;
use lm_core::sim::Sim;
use lm_core::view::{menu, Menu, MenuOption};
use lm_tools::runner::DEFAULT_REFLEX;

/// The pack the harness shipped before the tuning: a "what now?" question
/// over the bare menu, with the options described as they are.
///
/// The file is written out here rather than read from the harness, because
/// the harness no longer holds it: this is the "before" of the comparison.
const OLD_GATES: &str = r#"
[[gates]]
id = "move"
type = "choice"
options_from = "menu"
instructions = "What should you do now?"

[[gates]]
id = "call_out"
type = "noul"
options_from = "fixed"
instructions = "Should you call out to your squad?"
"#;

/// The pack the harness ships now: the doctrine, stated as a rule.
const NEW_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");

/// The seeds of the situations: one round per seed.
const SEEDS: std::ops::RangeInclusive<u64> = 1..=30;

/// The tick of the march: the squads are walking to the beacons.
const MARCH_TICK: u64 = 600;

/// The tick of the fight: the squads have met at the beacons.
const FIGHT_TICK: u64 = 1800;

/// The confidence floor the Thor's answers have to clear to become a
/// decision: `AgentsConfig::min_confidence["thor-d1-3b"]`, which was 0.55
/// when this measurement ran (phase 3). The floor has since been calibrated
/// to the model's own scale — 0.15
/// (crates/lm-agents/tests/talk_on_thor.md) — and this measurement keeps
/// the 0.55 it was run with, because it is the record of what the tuning
/// was measured against.
const FLOOR: f32 = 0.55;

/// One situation: a real view of a real round, and everything asked about
/// it.
struct Situation {
    /// The seed of the round the situation comes from.
    seed: u64,
    /// The tick of the round the situation was taken on.
    tick: u64,
    /// The agent whose view it is.
    agent: u16,
    /// The facts the model is shown.
    facts: Facts,
    /// The legal menu of the moment.
    menu: Menu,
    /// What the doctrine says should happen: the code oracle.
    oracle: MenuOption,
}

/// Play a round to `FIGHT_TICK` and take the situations of both ticks.
///
/// The agent rotates through the squad, so the sample holds leaders and
/// members. The fight sample asks both squads, because the squad that is
/// outnumbered is the one the doctrine's first branch is about.
fn situations(seed: u64, cfg: &AgentsConfig) -> (Situation, Vec<Situation>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let mut sim = Sim::new(&header).expect("the round starts");
    let squad_size = u64::from(header.params.squad_size);
    let slot = u16::try_from((seed - 1) % squad_size).expect("the agent id fits");

    let mut march = None;
    let mut fight = Vec::new();
    for tick in 0..FIGHT_TICK {
        sim.step(&[]);
        let now = tick + 1;
        if now == MARCH_TICK {
            march = Some(situation_of(&sim, slot, seed, now, cfg));
        }
        if now == FIGHT_TICK {
            for agent in [slot, slot + u16::try_from(squad_size).expect("the id fits")] {
                fight.push(situation_of(&sim, agent, seed, now, cfg));
            }
        }
    }
    (march.expect("the march is inside the round"), fight)
}

/// One situation: the view of one agent, and what the doctrine says.
fn situation_of(sim: &Sim, agent: u16, seed: u64, tick: u64, cfg: &AgentsConfig) -> Situation {
    let view = sim.view(agent);
    let facts = Facts::from_view(
        &view,
        sim.world(),
        view.squad_id,
        cfg,
        &SharedConfig::default(),
    );
    let menu = menu(&view);
    let oracle = oracle(&facts);
    Situation {
        seed,
        tick,
        agent,
        facts,
        menu,
        oracle,
    }
}

/// The doctrine, applied by code: the oracle the model's answer is
/// measured against.
///
/// The rule is the one the default harness's gate states:
///
/// 1. an enemy can see you within 6 cells, and there are fewer allies
///    within 4 cells than there are enemies within 6 cells: retreat;
/// 2. standing on a beacon my squad holds, and an enemy is within 6 cells:
///    hold — the beacon is contested;
/// 3. otherwise: go to the nearest beacon my squad does not hold, if there
///    is one; if there is none, hold.
///
/// The second branch is the `advance` doctrine's (Casey's decision,
/// 2026-10-30): a squad holds a beacon only while it is contested, and
/// otherwise keeps advancing. The doctrine the harness shipped until then
/// held the beacon whenever no enemy was near, and that is what froze the
/// game; it is kept, comparable, as the `hold` variant.
///
/// The numbers come from the facts the builder computed (the same ones
/// the model is shown), so this measures whether the model applies the
/// rule, not whether the builder agrees with itself.
fn oracle(facts: &Facts) -> MenuOption {
    let enemies_near = facts.fields["enemy.near"];
    let enemies_seeing = facts.fields["enemy.seeing"];
    let allies_near = facts.fields["ally.near"];
    let standing_on = facts.fields["me.on_beacon"];
    let standing_on_held = facts.fields["me.on_held_beacon"] != 0;
    let nearest_unheld = facts.fields["beacon.nearest_unheld"];

    if enemies_seeing > 0 && allies_near < enemies_near {
        return MenuOption::Retreat;
    }
    if standing_on >= 0 && standing_on_held && enemies_near > 0 {
        return MenuOption::Hold;
    }
    if nearest_unheld >= 0 {
        return MenuOption::GoToBeacon(nearest_unheld as u8);
    }
    MenuOption::Hold
}

/// The branch the oracle picked, by name.
fn branch(option: &MenuOption) -> &'static str {
    match option {
        MenuOption::GoToBeacon(_) => "go to beacon",
        MenuOption::Hold => "hold",
        MenuOption::FollowLeader => "follow the leader",
        MenuOption::Retreat => "retreat",
    }
}

/// What the model chose: the option with the largest probability.
///
/// A tie goes to the first option, the way the lane conditioner reads the
/// samples.
fn chosen(probs: &BTreeMap<String, f32>, menu: &Menu) -> Option<MenuOption> {
    let mut best: Option<(usize, f32)> = None;
    for index in 0..menu.options.len() {
        let Some(probability) = probs.get(&format!("opt{index}")).copied() else {
            continue;
        };
        if best.is_none_or(|(_, highest)| probability > highest) {
            best = Some((index, probability));
        }
    }
    best.map(|(index, _)| menu.options[index])
}

/// The p-th percentile of the values, nearest rank.
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f32).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

/// What one pack measured over the situations.
#[derive(Clone, Default)]
struct Measurement {
    /// The confidences of the answers that came back.
    confidences: Vec<f32>,
    /// How many answers agreed with the oracle.
    agree: usize,
    /// How many answers came back at all.
    answered: usize,
    /// How many calls were refused for being too slow.
    timeouts: usize,
    /// How many calls were refused for any other reason, by the reason.
    refused: BTreeMap<String, usize>,
    /// The round trip of the calls, in milliseconds.
    latencies: Vec<u32>,
    /// How many times the oracle said each thing.
    branches: BTreeMap<&'static str, usize>,
    /// How many of the oracle's branches the model got right.
    right: BTreeMap<&'static str, (usize, usize)>,
    /// What the model picked, by branch: the shape of its mistakes.
    picked: BTreeMap<&'static str, usize>,
    /// The mistakes, by what the oracle said and what the model said.
    mistakes: BTreeMap<(&'static str, &'static str), usize>,
}

impl Measurement {
    /// Note one answer.
    fn note(&mut self, oracle: &MenuOption, right: bool) {
        let name = branch(oracle);
        *self.branches.entry(name).or_default() += 1;
        let tally = self.right.entry(name).or_default();
        tally.1 += 1;
        if right {
            tally.0 += 1;
        }
    }
}

/// Ask `situations` about `pack` and measure the answers.
///
/// The backend is permissive (the floor is zero, the deadline is long), so
/// the measurement is of what the model *says*, not of what the shipped
/// floor would accept; what would reach the game is derived from those
/// answers by [`decisions_under`].
fn measure(
    pack: &str,
    situations: &[Situation],
    backend: &ThorBackend,
    cfg: &AgentsConfig,
) -> Measurement {
    let gates = GateSet::parse(pack).expect("the pack parses");
    let mut measurement = Measurement::default();

    for situation in situations {
        let questions = gates.to_questions_limited(&situation.menu, cfg.max_questions);
        let deadline = Instant::now() + Duration::from_secs(30);
        let answers = match backend.ask(&situation.facts.text, &questions, deadline) {
            Ok(answers) => answers,
            Err(Refusal::Timeout) => {
                measurement.timeouts += 1;
                continue;
            }
            // A pack is not refused for being unsure any more: the rule is
            // per gate now, so an unsure gate is dropped and the pack that
            // carries it still comes back. The floor is applied below, to
            // the move gate's own confidence.
            Err(refusal) => {
                *measurement.refused.entry(refusal.to_string()).or_default() += 1;
                continue;
            }
        };
        let Some(Answer::Choice { probs, confidence }) = answers.per_gate.get("move") else {
            *measurement
                .refused
                .entry("the move gate was not answered".to_owned())
                .or_default() += 1;
            continue;
        };
        measurement.answered += 1;
        measurement.latencies.push(answers.latency_ms);
        measurement.confidences.push(*confidence);

        let Some(picked) = chosen(probs, &situation.menu) else {
            *measurement
                .refused
                .entry("the answer did not cover the menu".to_owned())
                .or_default() += 1;
            measurement.answered -= 1;
            measurement.latencies.pop();
            measurement.confidences.pop();
            continue;
        };
        let right = picked == situation.oracle;
        if right {
            measurement.agree += 1;
        }
        *measurement.picked.entry(branch(&picked)).or_default() += 1;
        if !right {
            *measurement
                .mistakes
                .entry((branch(&situation.oracle), branch(&picked)))
                .or_default() += 1;
        }
        measurement.note(&situation.oracle, right);
    }
    measurement
}

/// How many of the answers would become decisions under the shipped
/// floor and the shipped deadline.
///
/// The answer has to arrive inside the deadline and to clear the floor;
/// everything else is a refusal, and the reflex plays the moment (D26).
fn decisions_under(measurement: &Measurement, cfg: &AgentsConfig) -> (usize, usize, usize) {
    let deadline = u64::from(cfg.request_timeout_ms);
    let mut decided = 0;
    let mut too_slow = 0;
    let mut too_unsure = 0;
    for (confidence, latency) in measurement
        .confidences
        .iter()
        .zip(measurement.latencies.iter())
    {
        if u64::from(*latency) > deadline {
            too_slow += 1;
        } else if *confidence < FLOOR {
            too_unsure += 1;
        } else {
            decided += 1;
        }
    }
    (decided, too_slow, too_unsure)
}

/// Print what a pack measured.
fn report(name: &str, measurement: &Measurement) {
    let mut confidences = measurement.confidences.clone();
    confidences.sort_by(f32::total_cmp);
    let above = confidences.iter().filter(|c| **c >= FLOOR).count();
    let share = if confidences.is_empty() {
        0.0
    } else {
        above as f32 / confidences.len() as f32
    };
    let agreement = if measurement.answered == 0 {
        0.0
    } else {
        measurement.agree as f32 / measurement.answered as f32
    };
    let mut latencies: Vec<f32> = measurement.latencies.iter().map(|ms| *ms as f32).collect();
    latencies.sort_by(f32::total_cmp);

    println!("--- {name} ---");
    let refused = measurement.timeouts + measurement.refused.values().sum::<usize>();
    println!(
        "answered {} of {}, refused {} (too slow {}, other {})",
        measurement.answered,
        measurement.answered + refused,
        refused,
        measurement.timeouts,
        measurement.refused.values().sum::<usize>()
    );
    for (reason, count) in &measurement.refused {
        println!("  refused {count} times: {reason}");
    }
    println!(
        "confidence  p10 {:.3}  p50 {:.3}  p90 {:.3}",
        percentile(&confidences, 0.10),
        percentile(&confidences, 0.50),
        percentile(&confidences, 0.90)
    );
    println!(
        "above the {FLOOR} floor: {above} of {} ({:.0}%)",
        confidences.len(),
        share * 100.0
    );
    println!(
        "oracle agreement: {} of {} ({:.0}%)",
        measurement.agree,
        measurement.answered,
        agreement * 100.0
    );
    let branches: Vec<String> = measurement
        .branches
        .iter()
        .map(|(name, count)| {
            let (right, total) = measurement.right.get(name).copied().unwrap_or((0, 0));
            format!("{name} {count} (right {right}/{total})")
        })
        .collect();
    println!("the oracle says: {}", branches.join(", "));
    let picks: Vec<String> = measurement
        .picked
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect();
    println!("the model picked: {}", picks.join(", "));
    let mistakes: Vec<String> = measurement
        .mistakes
        .iter()
        .map(|((wanted, got), count)| format!("{wanted} -> {got} {count}"))
        .collect();
    if !mistakes.is_empty() {
        println!("the mistakes: {}", mistakes.join(", "));
    }
    println!(
        "latency  p50 {} ms  p90 {} ms",
        percentile(&latencies, 0.50),
        percentile(&latencies, 0.90)
    );
}

/// The measurement: both packs, over the same situations.
#[test]
#[ignore = "needs the Thor on the LAN (192.168.0.78:8093)"]
fn the_doctrine_pack_is_confident_enough_to_drive_play() {
    let cfg = AgentsConfig::default();

    let mut march = Vec::new();
    let mut fight = Vec::new();
    for seed in SEEDS {
        let (m, f) = situations(seed, &cfg);
        march.push(m);
        fight.extend(f);
    }

    // The floor is pinned at zero here: the measurement is of the
    // confidence the model reports, not of what the shipped floor would
    // accept.
    let permissive = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-3b"], "d1-3b")
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_secs(30));
    let mut after_march = None;
    let mut after_fight = None;
    for (label, sample) in [
        ("the march (tick 600)", &march),
        ("the fight (tick 1800)", &fight),
    ] {
        println!();
        println!("===== {label}: {} situations =====", sample.len());
        let before = measure(OLD_GATES, sample, &permissive, &cfg);
        let after = measure(NEW_GATES, sample, &permissive, &cfg);
        if label.starts_with("the march") {
            after_march = Some(after.clone());
        }
        if label.starts_with("the fight") {
            after_fight = Some(after.clone());
        }
        report("before: what should you do now?", &before);
        println!();
        report("after: the squad's doctrine, stated as a rule", &after);
        println!();
        // What would reach the game: the answers that arrive inside the
        // shipped deadline and clear the shipped floor.
        for (label, measurement) in [("before", &before), ("after", &after)] {
            let (decided, too_slow, too_unsure) = decisions_under(measurement, &cfg);
            println!(
                "{label}, with the shipped floor {FLOOR} and the shipped deadline {} ms: \
                 {} of {} thoughts become decisions ({} too slow, {} too unsure)",
                cfg.request_timeout_ms,
                decided,
                sample.len(),
                too_slow,
                too_unsure
            );
        }
    }

    // One situation, in full: the facts the model is shown and the pack it
    // is asked, so the numbers above can be read against a real case.
    let first = &fight[0];
    let gates = GateSet::parse(NEW_GATES).expect("the pack parses");
    println!();
    println!(
        "--- seed {} , agent {} , tick {} ---",
        first.seed, first.agent, first.tick
    );
    println!("facts: {}", first.facts.text);
    println!(
        "pack: {}",
        serde_json::to_string_pretty(&gates.to_questions_limited(&first.menu, cfg.max_questions))
            .unwrap_or_default()
    );
    println!("the oracle says: {:?}", first.oracle);

    // The brief's target, on the brief's sample: the pack the harness
    // ships now, over the 30 situations of seeds 1..30 at tick 600, has to
    // clear the floor it ships with and to follow the rule.
    let after = after_march.expect("the march was measured");
    let mut confidences = after.confidences.clone();
    confidences.sort_by(f32::total_cmp);
    assert!(
        percentile(&confidences, 0.50) >= FLOOR,
        "the median confidence is below the floor: p50 {:.3} of {FLOOR}",
        percentile(&confidences, 0.50)
    );
    assert!(
        after.answered == march.len(),
        "every situation got an answer: {} of {} (the endpoint may have gone away)",
        after.answered,
        march.len()
    );
    assert!(
        after.agree * 100 >= after.answered * 85,
        "the model follows the rule in {} of {} answers",
        after.agree,
        after.answered
    );

    // The fight sample is a harder one (the retreat branch is live), so
    // only the completeness of the measurement is asserted here: a sample
    // that lost its endpoint is not a measurement.
    let after = after_fight.expect("the fight was measured");
    assert!(
        after.answered == fight.len(),
        "every fight situation got an answer: {} of {} (the endpoint may have gone away)",
        after.answered,
        fight.len()
    );
}

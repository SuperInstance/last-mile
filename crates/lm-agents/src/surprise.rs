//! The surprise signal: where the minds struggled, by situation.
//!
//! The between-round learning flow (D64-D66) proposes a change and tests it
//! under a sealed prediction, but nothing in what it is shown says **where**
//! the agents struggled. The trainer's output is questions (D47), and a
//! missing gate, fact or lexicon entry is a permanent ceiling — so the cheap
//! signal that points the reviewer at the right question is a count, not a
//! model: per **situation key**, a coarse bucket of the state an ask was made
//! in, how often the minds were unsure ([`Answers::unsure`], D72), refused
//! (D26) or escalated (D71).
//!
//! A situation where the models keep failing is a situation the harness
//! cannot yet describe. The top of the table is the reviewer's reading list.
//!
//! Everything here is integer maths and ordered maps, and the counts are
//! taken from the answers that came home — the same answers the mind logs —
//! so the same round always produces the same table, and no wall-clock value
//! ever enters it.

use std::collections::BTreeMap;

use lm_core::params::Params;
use lm_core::view::ScoreView;
use serde::{Deserialize, Serialize};

use crate::facts::Facts;
use crate::sysone::{Answers, Refusal};

/// The fewest asks a key needs before it is ranked.
///
/// A key asked once and refused once is a coin, not a finding: below this
/// floor the key is counted (it is in the table) but not ranked, so rare
/// keys do not dominate the top of the list.
pub const MIN_ASKS: u64 = 5;

/// The score margin it takes to leave the `level` band, in points.
///
/// A beacon scores one point per tick held, so 180 points is six seconds of
/// holding one beacon — a tenth of the default win score (1,800), and about
/// the margin a squad opens before the other must answer it. Inside the band
/// the round is still a contest; outside it the round has a front-runner,
/// and that is a different situation to think in.
pub const MARGIN_BAND_POINTS: i64 = 180;

// ------------------------------------------------------------------ the key

/// Which beacon the agent stands on, and whose it is.
///
/// The beacon the agent stands on is the game's objective made local: it is
/// where holding happens and where the fights are, so the model's job is
/// different on it, next to it, or nowhere near it. The holder is said
/// rather than the index because the index is world-shape (four beacons
/// today, more in a bigger world) and would split the buckets without
/// naming a difference the reviewer can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    /// Standing on no beacon at all.
    Off,
    /// Standing on a beacon the agent's own squad holds.
    Own,
    /// Standing on a beacon an enemy squad holds.
    Enemy,
    /// Standing on a beacon nobody holds yet.
    Unheld,
}

impl Standing {
    /// The clause the description says.
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Off => "off the beacons",
            Standing::Own => "on your beacon",
            Standing::Enemy => "on an enemy beacon",
            Standing::Unheld => "on an unheld beacon",
        }
    }
}

/// What the agent knows of the enemy, at its closest.
///
/// The four states are the doctrine's own ladder — none, known (a fresh
/// sighting, none within reach), near (within the doctrine's
/// [`ENEMY_NEAR_CELLS`](crate::facts::ENEMY_NEAR_CELLS)), in sight (near,
/// with a clear line of sight) — because the model's questions change at
/// each rung: what to do, where to go, whether to run, whether to fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnemySpot {
    /// No fresh enemy is known.
    None,
    /// A fresh enemy is known, but none is within reach.
    Known,
    /// An enemy is within reach, without a clear line of sight.
    Near,
    /// An enemy is within reach and can see the agent.
    Seeing,
}

impl EnemySpot {
    /// The clause the description says.
    pub fn as_str(self) -> &'static str {
        match self {
            EnemySpot::None => "no fresh enemy",
            EnemySpot::Known => "a fresh enemy known",
            EnemySpot::Near => "an enemy near",
            EnemySpot::Seeing => "an enemy in sight",
        }
    }
}

/// The round's score margin, banded.
///
/// The margin is signed — the agent's own squad against the best other —
/// because "behind" and "ahead" are different situations to think in, and
/// banded at [`MARGIN_BAND_POINTS`] because the exact number is a fact the
/// model reads, not a situation; the band is the situation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Margin {
    /// The agent's squad is behind by more than the band.
    Behind,
    /// The squads are within the band of each other.
    Level,
    /// The agent's squad is ahead by more than the band.
    Ahead,
}

impl Margin {
    /// The clause the description says.
    pub fn as_str(self) -> &'static str {
        match self {
            Margin::Behind => "behind on score",
            Margin::Level => "level on score",
            Margin::Ahead => "ahead on score",
        }
    }
}

/// Where the round stands in its own time.
///
/// Thirds of the round's hard time limit: the same play means different
/// things early (the beacons are being taken), in the middle (they are being
/// kept) and late (the score has to be answered now). The tick the view
/// carries is the game's own clock, never a wall clock, so the band is as
/// deterministic as the round.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// The first third of the round's time limit.
    Early,
    /// The middle third.
    Mid,
    /// The last third.
    Late,
}

impl Phase {
    /// The clause the description says.
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Early => "early round",
            Phase::Mid => "mid round",
            Phase::Late => "late round",
        }
    }
}

/// The coarse bucket of the state an ask was made in.
///
/// The D25 situation key (a hash over every field of the facts) addresses a
/// situation exactly, which is right for the verdict cache; this key buckets
/// those situations into the handful a reviewer can read. Five fields, each
/// from facts the agents already get: at most 4 × 4 × 2 × 3 × 3 = 288
/// buckets exist, and a round occupies a few dozen of them.
///
/// The derive order (standing, enemy, stunned, margin, phase) is the
/// description order, so the keys sort the way they read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SituationKey {
    /// Which beacon the agent stands on, and whose it is.
    pub standing: Standing,
    /// What the agent knows of the enemy, at its closest.
    pub enemy: EnemySpot,
    /// Whether the agent itself is stunned.
    pub stunned: bool,
    /// The round's score margin, banded.
    pub margin: Margin,
    /// Where the round stands in its own time.
    pub phase: Phase,
}

impl Default for SituationKey {
    /// The empty bucket: no beacon, no enemy, not stunned, level, early.
    fn default() -> SituationKey {
        SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        }
    }
}

impl SituationKey {
    /// The bucket of the state an ask over `view`/`facts` was made in.
    ///
    /// The beacon, enemy and stun fields come from the facts the ask carried
    /// (so the bucket is the situation the model was *shown*); the margin
    /// band and the time band come from the view (they are facts the agents
    /// get, though the facts text does not state them, D49).
    pub fn of(view: &ScoreView, facts: &Facts, params: &Params) -> SituationKey {
        SituationKey {
            standing: standing_of(view, &facts.fields),
            enemy: enemy_of(&facts.fields),
            stunned: facts.fields.get("me.stunned").copied() == Some(1),
            margin: margin_of(view),
            phase: phase_of(view.tick, params),
        }
    }

    /// The situation in words, one clause per field, in a fixed order.
    pub fn describe(&self) -> String {
        format!(
            "{}, {}, {}, {}, {}",
            self.phase.as_str(),
            self.margin.as_str(),
            self.enemy.as_str(),
            if self.stunned {
                "stunned"
            } else {
                "not stunned"
            },
            self.standing.as_str(),
        )
    }
}

/// Which beacon the agent stands on, from the facts' own fields.
fn standing_of(view: &ScoreView, fields: &BTreeMap<String, i64>) -> Standing {
    match fields.get("me.on_beacon").copied() {
        Some(index) if index >= 0 => {
            match fields.get(&format!("beacon.{index}.holder")).copied() {
                Some(holder) if holder == i64::from(view.squad_id) => Standing::Own,
                Some(-1) => Standing::Unheld,
                Some(_) => Standing::Enemy,
                // The facts always state the holder of a beacon the agent
                // stands on; this is the defensive arm, and the safest read.
                None => Standing::Off,
            }
        }
        _ => Standing::Off,
    }
}

/// What the agent knows of the enemy, at its closest, from the facts.
fn enemy_of(fields: &BTreeMap<String, i64>) -> EnemySpot {
    let near = fields.get("enemy.near").copied().unwrap_or(0);
    let seeing = fields.get("enemy.seeing").copied().unwrap_or(0);
    if seeing > 0 {
        EnemySpot::Seeing
    } else if near > 0 {
        EnemySpot::Near
    } else if fields
        .keys()
        .any(|name| name.starts_with("enemy.") && name.ends_with(".dist"))
    {
        EnemySpot::Known
    } else {
        EnemySpot::None
    }
}

/// The margin band of the round, from the view's own scores.
fn margin_of(view: &ScoreView) -> Margin {
    let me = usize::from(view.squad_id);
    let mine = view.score.get(me).copied().unwrap_or(0);
    let best_other = view
        .score
        .iter()
        .enumerate()
        .filter(|(squad, _)| *squad != me)
        .map(|(_, score)| *score)
        .max()
        .unwrap_or(0);
    let diff = mine - best_other;
    if diff <= -MARGIN_BAND_POINTS {
        Margin::Behind
    } else if diff >= MARGIN_BAND_POINTS {
        Margin::Ahead
    } else {
        Margin::Level
    }
}

/// The time band of a tick, in thirds of the round's hard time limit.
fn phase_of(tick: u64, params: &Params) -> Phase {
    let limit = u64::from(params.time_limit_ticks).max(1);
    if tick * 3 < limit {
        Phase::Early
    } else if tick * 3 < limit.saturating_mul(2) {
        Phase::Mid
    } else {
        Phase::Late
    }
}

// ---------------------------------------------------------------- the counts

/// The name a refusal is counted under: its kind, not its message.
///
/// The message of a `Malformed` refusal names the field that broke; the kind
/// is what a reviewer can act on, so that is what the table keeps.
pub fn refusal_kind(refusal: &Refusal) -> String {
    match refusal {
        Refusal::Timeout => "timeout".to_owned(),
        Refusal::Http(0) => "unreachable".to_owned(),
        Refusal::Http(code) => format!("http {code}"),
        Refusal::Malformed(_) => "malformed".to_owned(),
        Refusal::InvalidPack(_) => "invalid pack".to_owned(),
        Refusal::LowConfidence(_) => "low confidence".to_owned(),
        Refusal::MenuChanged => "menu changed".to_owned(),
        Refusal::Budget => "budget".to_owned(),
        Refusal::OffPlan(_) => "off plan".to_owned(),
        Refusal::Superseded => "superseded".to_owned(),
        Refusal::Panicked(_) => "panicked".to_owned(),
        // The name N2 reserved for this rule, and the one the log and the
        // telemetry write: `escalation_late` (N3).
        Refusal::EscalationLate => "escalation_late".to_owned(),
    }
}

/// What one situation bucket did over a round, in numbers.
///
/// Every number comes from the answers that came home: an ask that was
/// never answered (dropped, expired in the queue) is not an outcome the
/// mind logged, so it is not counted here either.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SituationCounts {
    /// The bucket these counts belong to.
    pub key: SituationKey,
    /// The asks that came home in this bucket, answered or refused.
    pub asks: u64,
    /// The asks that came home with at least one gate the backend was
    /// unsure of (an unsure gate makes no decision of its own, D72).
    pub unsure_asks: u64,
    /// The unsure gates by gate id — an ask may hold several.
    pub unsure: BTreeMap<String, u64>,
    /// The refused asks, by the refusal's kind.
    pub refused: BTreeMap<String, u64>,
    /// The gates asked of a second model (D71), by gate id.
    pub escalated: BTreeMap<String, u64>,
}

impl SituationCounts {
    /// An empty tally for one bucket.
    pub fn new(key: SituationKey) -> SituationCounts {
        SituationCounts {
            key,
            ..SituationCounts::default()
        }
    }

    /// Count one ask's outcome.
    fn count(&mut self, result: &Result<Answers, Refusal>) {
        self.asks += 1;
        match result {
            Err(refusal) => {
                *self.refused.entry(refusal_kind(refusal)).or_default() += 1;
            }
            Ok(answers) => {
                if !answers.unsure.is_empty() {
                    self.unsure_asks += 1;
                }
                for gate in answers.unsure.keys() {
                    *self.unsure.entry(gate.clone()).or_default() += 1;
                }
                for gate in answers.escalated.keys() {
                    *self.escalated.entry(gate.clone()).or_default() += 1;
                }
            }
        }
    }

    /// The struggle events the bucket produced: unsure gates, refused asks
    /// and escalated gates, added together.
    ///
    /// A gate that was escalated and *stayed* unsure counts twice, on
    /// purpose: it was unsure, and it cost a second opinion — the worst
    /// outcome the pack has.
    pub fn events(&self) -> u64 {
        self.unsure.values().sum::<u64>()
            + self.refused.values().sum::<u64>()
            + self.escalated.values().sum::<u64>()
    }

    /// The surprise score: struggle events per ask, x1000.
    ///
    /// The unit is the per-mille share metrics.rs uses; `0` for a bucket
    /// with no asks, never a division by zero.
    pub fn surprise_x1000(&self) -> u64 {
        (self.events() * 1_000).checked_div(self.asks).unwrap_or(0)
    }

    /// Whether the bucket has enough asks to be ranked ([`MIN_ASKS`]).
    pub fn ranked(&self) -> bool {
        self.asks >= MIN_ASKS
    }

    /// The bucket as the screen shows it: one line, human-readable, the
    /// situation first and the dominant failure with it.
    ///
    /// `early round, level on score, an enemy in sight, stunned, on your
    /// beacon: 9 of 14 asks unsure (attention)`.
    pub fn line(&self) -> String {
        let unsure: u64 = self.unsure.values().sum();
        let refused: u64 = self.refused.values().sum();
        let escalated: u64 = self.escalated.values().sum();
        let what = if unsure >= refused && unsure >= escalated {
            format!(
                "{} of {} asks unsure{}",
                self.unsure_asks,
                self.asks,
                named(&self.unsure)
            )
        } else if refused >= escalated {
            format!(
                "{} of {} asks refused{}",
                refused,
                self.asks,
                named(&self.refused)
            )
        } else {
            format!(
                "{} gates escalated over {} asks{}",
                escalated,
                self.asks,
                named(&self.escalated)
            )
        };
        format!("{}: {what}", self.key.describe())
    }
}

/// The parenthesised name of a count's biggest contributor, with its count.
fn named(counts: &BTreeMap<String, u64>) -> String {
    match counts
        .iter()
        .max_by_key(|(name, count)| (**count, std::cmp::Reverse(name.as_str())))
    {
        Some((name, count)) => format!(" ({name}: {count})"),
        None => String::new(),
    }
}

// ------------------------------------------------------------------ the table

/// The surprise table of one round (or of several, merged).
///
/// Built from the answers the minds logged; merged across the minds of a
/// round (and across rounds, when a measurement wants one table for all its
/// seeds). Deterministic: ordered maps, integer counts, no clock.
///
/// The table itself is the round's working memory; what travels on — into
/// the flow record and the between-round screen — is its top entries
/// ([`RoundSurprise::top`]), a list of [`SituationCounts`] that each carry
/// their own key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoundSurprise {
    /// The counts by bucket; the mind records into its own table and the
    /// round merges them.
    pub counts: BTreeMap<SituationKey, SituationCounts>,
}

impl RoundSurprise {
    /// Count one ask's outcome under its bucket.
    pub fn record(&mut self, key: SituationKey, result: &Result<Answers, Refusal>) {
        let slot = self
            .counts
            .entry(key)
            .or_insert_with(|| SituationCounts::new(key));
        slot.count(result);
    }

    /// Merge another table into this one (the minds of a round, the seeds of
    /// a measurement).
    pub fn merge(&mut self, other: &RoundSurprise) {
        for (key, counts) in &other.counts {
            let slot = self
                .counts
                .entry(*key)
                .or_insert_with(|| SituationCounts::new(*key));
            slot.asks += counts.asks;
            slot.unsure_asks += counts.unsure_asks;
            for (gate, count) in &counts.unsure {
                *slot.unsure.entry(gate.clone()).or_default() += count;
            }
            for (kind, count) in &counts.refused {
                *slot.refused.entry(kind.clone()).or_default() += count;
            }
            for (gate, count) in &counts.escalated {
                *slot.escalated.entry(gate.clone()).or_default() += count;
            }
        }
    }

    /// The top `n` surprising situations: ranked, most surprising first.
    ///
    /// Only buckets with at least [`MIN_ASKS`] asks are ranked, and the sort
    /// is a total order — the score, then the events, then the asks, then
    /// the key itself — so the same table always produces the same top.
    pub fn top(&self, n: usize) -> Vec<SituationCounts> {
        let mut ranked: Vec<&SituationCounts> = self
            .counts
            .values()
            .filter(|counts| counts.ranked() && counts.events() > 0)
            .collect();
        ranked.sort_by(|a, b| {
            b.surprise_x1000()
                .cmp(&a.surprise_x1000())
                .then(b.events().cmp(&a.events()))
                .then(b.asks.cmp(&a.asks))
                .then(a.key.cmp(&b.key))
        });
        ranked.into_iter().take(n).cloned().collect()
    }

    /// Every ask the table counted.
    pub fn total_asks(&self) -> u64 {
        self.counts.values().map(|counts| counts.asks).sum()
    }

    /// Whether anything was counted at all.
    pub fn is_empty(&self) -> bool {
        self.counts.values().all(|counts| counts.asks == 0)
    }
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysone::{Escalation, EscalationOutcome};
    use lm_core::view::AgentSnap;

    /// A view with the given scores and squad.
    fn view(scores: &[i64], squad: u8) -> ScoreView {
        ScoreView {
            tick: 0,
            me: AgentSnap {
                id: u16::from(squad),
                cell: lm_core::world::CellXY { x: 0, y: 0 },
                role: lm_core::header::Role::Member,
                stunned: false,
                airtime_ticks: 0,
            },
            squad: vec![],
            enemies: vec![],
            beacons: vec![],
            score: scores.to_vec(),
            squad_id: squad,
            heard: vec![],
        }
    }

    /// A facts set from raw fields: the bucket reads the fields, so the
    /// tests build exactly the fields the facts builder would state.
    fn facts_of(fields: &[(&str, i64)]) -> Facts {
        Facts {
            text: String::new(),
            key: 0,
            fields: fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect(),
        }
    }

    #[test]
    fn the_key_is_small_and_bounded() {
        // 4 standings x 4 enemy states x 2 stun states x 3 margins x 3
        // phases = 288 buckets exist; that is the ceiling by construction.
        let standings = [
            Standing::Off,
            Standing::Own,
            Standing::Enemy,
            Standing::Unheld,
        ];
        let enemies = [
            EnemySpot::None,
            EnemySpot::Known,
            EnemySpot::Near,
            EnemySpot::Seeing,
        ];
        let margins = [Margin::Behind, Margin::Level, Margin::Ahead];
        let phases = [Phase::Early, Phase::Mid, Phase::Late];
        let mut keys = Vec::new();
        for standing in standings {
            for enemy in enemies {
                for stunned in [false, true] {
                    for margin in margins {
                        for phase in phases {
                            keys.push(SituationKey {
                                standing,
                                enemy,
                                stunned,
                                margin,
                                phase,
                            });
                        }
                    }
                }
            }
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 288);
    }

    #[test]
    fn the_key_reads_the_facts_the_ask_carried() {
        let params = Params::default();
        let mut view = view(&[100, 0], 0);

        // Standing on an enemy-held beacon, an enemy in sight, not stunned.
        let facts = facts_of(&[
            ("me.on_beacon", 2),
            ("beacon.2.holder", 1),
            ("enemy.near", 1),
            ("enemy.seeing", 1),
            ("me.stunned", 0),
        ]);
        let key = SituationKey::of(&view, &facts, &params);
        assert_eq!(key.standing, Standing::Enemy);
        assert_eq!(key.enemy, EnemySpot::Seeing);
        assert!(!key.stunned);
        // 100 - 0 = 100 points: inside the 180-point band.
        assert_eq!(key.margin, Margin::Level);
        assert_eq!(key.phase, Phase::Early);

        // The same situation one band apart: the margin band flips at 180.
        view.score = vec![180, 0];
        assert_eq!(
            SituationKey::of(&view, &facts, &params).margin,
            Margin::Ahead
        );
        view.score = vec![0, 181];
        assert_eq!(
            SituationKey::of(&view, &facts, &params).margin,
            Margin::Behind
        );

        // A known enemy that is not within reach.
        let far = facts_of(&[
            ("me.on_beacon", -1),
            ("enemy.3.dist", 40),
            ("enemy.near", 0),
            ("enemy.seeing", 0),
            ("me.stunned", 1),
        ]);
        let key = SituationKey::of(&view, &far, &params);
        assert_eq!(key.standing, Standing::Off);
        assert_eq!(key.enemy, EnemySpot::Known);
        assert!(key.stunned);
        assert_eq!(key.margin, Margin::Behind);

        // The same state, read again, is the same key.
        assert_eq!(key, SituationKey::of(&view, &far, &params));
    }

    #[test]
    fn the_time_band_is_thirds_of_the_limit() {
        let params = Params::default();
        assert_eq!(params.time_limit_ticks, 9_000);
        assert_eq!(phase_of(0, &params), Phase::Early);
        assert_eq!(phase_of(2_999, &params), Phase::Early);
        assert_eq!(phase_of(3_000, &params), Phase::Mid);
        assert_eq!(phase_of(5_999, &params), Phase::Mid);
        assert_eq!(phase_of(6_000, &params), Phase::Late);
        assert_eq!(phase_of(8_999, &params), Phase::Late);
    }

    #[test]
    fn the_description_names_every_field() {
        let key = SituationKey {
            standing: Standing::Own,
            enemy: EnemySpot::Seeing,
            stunned: true,
            margin: Margin::Ahead,
            phase: Phase::Late,
        };
        assert_eq!(
            key.describe(),
            "late round, ahead on score, an enemy in sight, stunned, on your beacon"
        );
    }

    /// An answer with the given unsure gates and escalated gates.
    fn answers(unsure: &[&str], escalated: &[&str]) -> Answers {
        Answers {
            per_gate: BTreeMap::new(),
            escalated: escalated
                .iter()
                .map(|gate| {
                    (
                        (*gate).to_owned(),
                        Escalation {
                            gate: (*gate).to_owned(),
                            primary_confidence: 0.1,
                            primary_choice: None,
                            second_confidence: Some(0.9),
                            second_choice: None,
                            latency_ms: 1,
                            outcome: EscalationOutcome::Taken,
                            refusal: None,
                        },
                    )
                })
                .collect(),
            unsure: unsure.iter().map(|g| ((*g).to_owned(), 0.2)).collect(),
            model: "test".to_owned(),
            input_tokens: 0,
            latency_ms: 1,
        }
    }

    #[test]
    fn the_table_counts_the_three_kinds_of_struggle() {
        let key = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        let mut table = RoundSurprise::default();
        table.record(key, &Ok(answers(&["attention"], &[])));
        table.record(key, &Ok(answers(&["attention", "subject"], &["attention"])));
        table.record(key, &Err(Refusal::Timeout));

        let counts = &table.counts[&key];
        assert_eq!(counts.asks, 3);
        assert_eq!(counts.unsure_asks, 2);
        assert_eq!(counts.unsure.get("attention"), Some(&2));
        assert_eq!(counts.unsure.get("subject"), Some(&1));
        assert_eq!(counts.refused.get("timeout"), Some(&1));
        assert_eq!(counts.escalated.get("attention"), Some(&1));
        // 3 unsure gates + 1 refused ask + 1 escalation = 5 events over 3
        // asks = 1666 per mille.
        assert_eq!(counts.events(), 5);
        assert_eq!(counts.surprise_x1000(), 5 * 1_000 / 3);
        assert_eq!(table.total_asks(), 3);
        assert!(!table.is_empty());
    }

    #[test]
    fn the_line_names_the_dominant_failure() {
        let key = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::Seeing,
            stunned: true,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        let mut counts = SituationCounts::new(key);
        counts.asks = 14;
        counts.unsure_asks = 9;
        *counts.unsure.entry("attention".to_owned()).or_default() = 9;
        *counts.unsure.entry("subject".to_owned()).or_default() = 2;
        assert_eq!(
            counts.line(),
            "early round, level on score, an enemy in sight, stunned, off the beacons: \
             9 of 14 asks unsure (attention: 9)"
        );

        // A refusal that dominates is named by its kind.
        let mut refused = SituationCounts::new(key);
        refused.asks = 7;
        *refused.refused.entry("timeout".to_owned()).or_default() = 6;
        assert!(refused.line().ends_with("6 of 7 asks refused (timeout: 6)"));
    }

    #[test]
    fn rare_keys_are_counted_but_not_ranked() {
        let quiet = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        let fight = SituationKey {
            standing: Standing::Enemy,
            enemy: EnemySpot::Seeing,
            stunned: true,
            margin: Margin::Behind,
            phase: Phase::Mid,
        };
        let mut table = RoundSurprise::default();
        // The quiet key: asked twice, refused once — a coin, not a finding.
        table.record(quiet, &Ok(answers(&[], &[])));
        table.record(quiet, &Err(Refusal::Timeout));
        // The fight key: asked MIN_ASKS times, unsure every time.
        for _ in 0..MIN_ASKS {
            table.record(fight, &Ok(answers(&["move"], &[])));
        }
        assert!(!table.counts[&quiet].ranked());
        assert!(table.counts[&fight].ranked());
        let top = table.top(5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].key, fight);
    }

    #[test]
    fn the_top_is_a_total_order() {
        let mut table = RoundSurprise::default();
        // Two buckets with the same score and the same events: the one with
        // more asks first, and a full tie falls to the key's own order.
        let a = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        let b = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::Known,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        for _ in 0..6 {
            table.record(a, &Ok(answers(&["move"], &[])));
        }
        for _ in 0..5 {
            table.record(b, &Ok(answers(&["move"], &[])));
        }
        let top = table.top(5);
        assert_eq!(top.len(), 2);
        // Same x1000 (1000), same events (6 vs 5 — a's events win).
        assert_eq!(top[0].key, a);
        assert_eq!(top[1].key, b);
        assert_eq!(
            table.top(5),
            table.top(5),
            "the same table gives the same top"
        );
    }

    #[test]
    fn tables_merge_by_bucket() {
        let key = SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        };
        let mut one = RoundSurprise::default();
        one.record(key, &Ok(answers(&["attention"], &[])));
        let mut two = RoundSurprise::default();
        two.record(key, &Err(Refusal::Budget));
        one.merge(&two);
        assert_eq!(one.counts[&key].asks, 2);
        assert_eq!(one.counts[&key].unsure.get("attention"), Some(&1));
        assert_eq!(one.counts[&key].refused.get("budget"), Some(&1));
    }

    #[test]
    fn refusals_are_counted_by_kind() {
        assert_eq!(refusal_kind(&Refusal::Timeout), "timeout");
        assert_eq!(refusal_kind(&Refusal::Http(0)), "unreachable");
        assert_eq!(refusal_kind(&Refusal::Http(503)), "http 503");
        assert_eq!(refusal_kind(&Refusal::Malformed("x".into())), "malformed");
        assert_eq!(refusal_kind(&Refusal::MenuChanged), "menu changed");
    }

    #[test]
    fn the_key_and_counts_round_trip_as_json() {
        // The flow record carries these: old records (without the field)
        // must keep loading, and new ones must keep their shape.
        let key = SituationKey {
            standing: Standing::Enemy,
            enemy: EnemySpot::Near,
            stunned: true,
            margin: Margin::Behind,
            phase: Phase::Mid,
        };
        let mut counts = SituationCounts::new(key);
        counts.asks = 9;
        counts.unsure_asks = 4;
        *counts.unsure.entry("attention".to_owned()).or_default() = 4;
        let text = serde_json::to_string(&counts).expect("serialises");
        let back: SituationCounts = serde_json::from_str(&text).expect("deserialises");
        assert_eq!(back, counts);
        assert_eq!(back.key.standing, Standing::Enemy);
    }

    #[test]
    fn an_empty_table_has_a_defined_shape() {
        let table = RoundSurprise::default();
        assert!(table.is_empty());
        assert_eq!(table.total_asks(), 0);
        assert!(table.top(5).is_empty());
        let counts = SituationCounts::new(SituationKey {
            standing: Standing::Off,
            enemy: EnemySpot::None,
            stunned: false,
            margin: Margin::Level,
            phase: Phase::Early,
        });
        assert_eq!(counts.surprise_x1000(), 0, "no asks, no division");
    }
}

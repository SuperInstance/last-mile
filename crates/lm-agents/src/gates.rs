//! Task 2: the gates — the typed questions an agent asks its decision model.
//!
//! A gate is one question with a type: `choice` (one of a set of options),
//! `noul` (yes or no) or `score` (a level). The set of gates is the agent's
//! harness's `gates.toml` — the part of the agent a human can edit between
//! rounds — and the harness commit pins it into the round log (D27), so a
//! replay asks exactly the questions the recorded round asked.
//!
//! Two things happen here:
//!
//! 1. **Asking.** [`GateSet::to_questions`] turns the gate set and the
//!    simulation's legal menu into the `questions` pack of a System One call
//!    (D37). A `choice` gate over the menu gets one criterion per option,
//!    keyed `opt0`, `opt1`, … — the same keys the answer comes back under, so
//!    an option and its probability never come apart.
//! 2. **Answering.** [`answers_to_decisions`] turns the model's answers into
//!    decisions: the menu's `choice` gate becomes a [`Lane`] — one quantised
//!    sample per option, in menu order — and every other gate is left to the
//!    log. A gate the model did not answer produces no decision, and a menu
//!    that changed under the question produces a refusal
//!    ([`Refusal::MenuChanged`]), never a neighbouring option (D26).
//!
//! Nothing here talks to a model, reads a clock or invents an answer: the
//! model's output reaches the simulation only as a logged decision, so a
//! round replays without any model at all.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use lm_core::{Decision, Kind, Lane, Menu, MenuOption, Provenance, Source, Tick};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::sysone::{Answer, Answers, Refusal};

/// The prefix of a menu option's key: `opt0`, `opt1`, …
///
/// The key is what the model answers with, so it has to be stable and short:
/// the option's *position* in the menu is its name, and the menu's own id
/// says which options those positions were.
const OPTION_PREFIX: &str = "opt";

/// The scale a probability is quantised to: 255 is 1.0.
///
/// It matches the lane conditioner's scale (`255 * 257 = 65 535` is one
/// below 1.0 in Q16), so a quantised sample and a Q16 lane mean the same
/// thing.
const PROB_SCALE: f32 = 255.0;

/// The most gates a set may hold: a lane names its gate with a `u8`.
const MAX_GATES: usize = 256;

// ---------------------------------------------------------------- the schema

/// The kind of question a gate asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateKind {
    /// One of a set of options: the model answers a probability per option.
    Choice,
    /// Yes or no: the model answers the probability of "yes".
    Noul,
    /// A level: the model answers the level's probabilities and a weighted
    /// score.
    Score,
}

impl GateKind {
    /// The name the schema and the System One pack use.
    pub fn as_str(self) -> &'static str {
        match self {
            GateKind::Choice => "choice",
            GateKind::Noul => "noul",
            GateKind::Score => "score",
        }
    }
}

/// Where a gate's options come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OptionsFrom {
    /// The gate's own criteria, written in `gates.toml`.
    #[default]
    Fixed,
    /// The simulation's legal menu at the moment of the question.
    Menu,
}

/// One gate: a typed question, its instructions and its options.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Gate {
    /// The gate's id: the key its answer comes back under.
    #[serde(rename = "id")]
    pub id: String,
    /// The kind of question.
    #[serde(rename = "type")]
    pub kind: GateKind,
    /// The question, in plain words. The model sees this and the criteria,
    /// never a grid (D49).
    #[serde(rename = "instructions")]
    pub instructions: String,
    /// The fixed options and what each one means.
    ///
    /// Empty for a gate whose options come from the menu.
    #[serde(default, rename = "criteria")]
    pub criteria: BTreeMap<String, String>,
    /// Where the options come from.
    #[serde(default, rename = "options_from")]
    pub options_from: OptionsFrom,
}

impl Gate {
    /// The key of the option at `index` in a menu.
    fn option_key(index: usize) -> String {
        format!("{OPTION_PREFIX}{index}")
    }

    /// The criteria of this gate over `menu`: one per option, in menu order.
    ///
    /// A menu gate takes its options from the menu, even when the file also
    /// lists criteria: the menu is the legal set, and a legal option is the
    /// only one the simulation can carry out.
    fn menu_criteria(&self, menu: &Menu) -> Map<String, Value> {
        let mut criteria = Map::new();
        for (index, option) in menu.options.iter().enumerate() {
            criteria.insert(Self::option_key(index), json!(describe_option(option)));
        }
        criteria
    }

    /// The question this gate asks, as the System One pack carries it.
    fn question(&self, menu: &Menu) -> Value {
        let mut question = Map::new();
        question.insert("type".to_string(), json!(self.kind.as_str()));
        question.insert("instructions".to_string(), json!(self.instructions));
        match self.kind {
            // A yes/no question has no options.
            GateKind::Noul => {}
            GateKind::Choice | GateKind::Score => {
                let criteria = match self.options_from {
                    OptionsFrom::Menu => self.menu_criteria(menu),
                    OptionsFrom::Fixed => self
                        .criteria
                        .iter()
                        .map(|(key, text)| (key.clone(), json!(text)))
                        .collect(),
                };
                question.insert("criteria".to_string(), Value::Object(criteria));
            }
        }
        Value::Object(question)
    }
}

/// What a menu option means, in words a model can read.
///
/// The beacon's index is the position of the beacon in the agent's facts, so
/// the facts and the option name agree.
fn describe_option(option: &MenuOption) -> String {
    match option {
        MenuOption::GoToBeacon(b) => format!("go to beacon {b}"),
        MenuOption::Hold => "hold position".to_string(),
        MenuOption::FollowLeader => "follow the squad leader".to_string(),
        MenuOption::Retreat => "retreat to the squad's spawn".to_string(),
    }
}

/// Why a gate set does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateError {
    /// The file is not TOML.
    Toml(String),
    /// The file has no `gates` list.
    MissingGates,
    /// The `gates` entry is not a list of gates.
    NotAList,
    /// One gate does not match the schema.
    Gate {
        /// Which gate of the list it was.
        index: usize,
        /// Why it did not match.
        why: String,
    },
    /// Two gates share an id.
    DuplicateId(String),
    /// A gate with fixed options lists none.
    MissingCriteria(String),
    /// The set holds more gates than a `u8` can number.
    TooManyGates,
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GateError::Toml(why) => write!(f, "the gates are not valid TOML: {why}"),
            GateError::MissingGates => f.write_str("the gates have no `gates` list"),
            GateError::NotAList => f.write_str("the `gates` entry is not a list"),
            GateError::Gate { index, why } => {
                write!(f, "gate {index} does not match the schema: {why}")
            }
            GateError::DuplicateId(id) => write!(f, "two gates share the id {id}"),
            GateError::MissingCriteria(id) => {
                write!(f, "the gate {id} has fixed options and lists none")
            }
            GateError::TooManyGates => {
                write!(f, "a gate set holds at most {MAX_GATES} gates")
            }
        }
    }
}

/// An agent's gates: the questions it asks its decision model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GateSet {
    gates: Vec<Gate>,
}

impl GateSet {
    /// Parse a gate set from `gates.toml`'s text.
    ///
    /// The file is a list of gates under the `gates` key:
    ///
    /// ```toml
    /// [[gates]]
    /// id = "move"
    /// type = "choice"
    /// options_from = "menu"
    /// instructions = "What should you do now?"
    /// ```
    ///
    /// The harness store hands this function the file's text verbatim
    /// ([`crate::harness::Harness::gates_toml`]), so the harness and the
    /// gate set never drift apart.
    pub fn parse(text: &str) -> Result<GateSet, GateError> {
        let value: toml::Value =
            toml::from_str(text).map_err(|e| GateError::Toml(e.to_string()))?;
        let Some(list) = value.as_table().and_then(|t| t.get("gates")) else {
            return Err(GateError::MissingGates);
        };
        let Some(list) = list.as_array() else {
            return Err(GateError::NotAList);
        };

        let mut gates = Vec::with_capacity(list.len());
        for (index, entry) in list.iter().enumerate() {
            let gate: Gate = entry.clone().try_into().map_err(|e| GateError::Gate {
                index,
                why: e.to_string(),
            })?;
            gates.push(gate);
        }

        if gates.len() > MAX_GATES {
            return Err(GateError::TooManyGates);
        }
        for (index, gate) in gates.iter().enumerate() {
            if gates[..index].iter().any(|g| g.id == gate.id) {
                return Err(GateError::DuplicateId(gate.id.clone()));
            }
            if gate.options_from == OptionsFrom::Fixed
                && matches!(gate.kind, GateKind::Choice | GateKind::Score)
                && gate.criteria.is_empty()
            {
                return Err(GateError::MissingCriteria(gate.id.clone()));
            }
        }

        Ok(GateSet { gates })
    }

    /// The gates, in the order the file lists them.
    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    /// The gate with this id, if the set has one.
    pub fn gate(&self, id: &str) -> Option<&Gate> {
        self.gates.iter().find(|g| g.id == id)
    }

    /// How many gates the set holds.
    pub fn len(&self) -> usize {
        self.gates.len()
    }

    /// Whether the set holds no gates at all.
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
    }

    /// The `questions` pack of a System One call (D37).
    ///
    /// One entry per gate, keyed by the gate's id. A `choice` or `score`
    /// gate over the menu carries one criterion per menu option, keyed
    /// `opt0`, `opt1`, … — the keys its answer comes back under.
    pub fn to_questions(&self, menu: &Menu) -> Value {
        let mut questions = Map::new();
        for gate in &self.gates {
            questions.insert(gate.id.clone(), gate.question(menu));
        }
        Value::Object(questions)
    }
}

/// Turn the model's answers into decisions.
///
/// The `choice` gate over the menu becomes a [`Lane`]: one quantised sample
/// per option, in menu order — exactly what the lane conditioner consumes.
/// Every other gate is left to the log: in phase 3 the `noul` gates are
/// asked and logged, and the utterances they call for arrive in phase 4.
///
/// - A gate the model named that the set does not have is a refusal
///   ([`Refusal::Malformed`]): the answer does not match what was asked.
/// - An answer whose keys are not the current menu's keys is a refusal
///   ([`Refusal::MenuChanged`]): the menu changed under the question, and
///   the answer is never mapped onto a neighbouring option.
/// - A gate the model did not answer produces no decision. Nothing is
///   invented to fill the gap (D26).
///
/// `now` is the tick the answer came back on and `lead` the number of ticks
/// the decision needs to arrive, so the decision is applied on
/// `now + lead`.
pub fn answers_to_decisions(
    set: &GateSet,
    answers: &Answers,
    menu: &Menu,
    agent: u16,
    now: Tick,
    lead: u16,
) -> Result<Vec<Decision>, Refusal> {
    // An answer that names a gate we never asked is not an answer to our
    // question: refuse the whole pack instead of taking the part that fits.
    for id in answers.per_gate.keys() {
        if set.gate(id).is_none() {
            return Err(Refusal::Malformed(format!(
                "the answer names the gate {id}, which is not one of ours"
            )));
        }
    }

    let mut decisions = Vec::new();
    for (index, gate) in set.gates().iter().enumerate() {
        // A gate with fixed options is not a lane in phase 3: it is asked,
        // answered and logged, and whatever it calls for arrives in phase 4.
        if gate.options_from != OptionsFrom::Menu {
            continue;
        }
        // A gate the model did not answer produces no decision: nothing is
        // invented to fill the gap (D26).
        let Some(answer) = answers.per_gate.get(&gate.id) else {
            continue;
        };
        let probs = match answer {
            Answer::Choice { probs, .. } | Answer::Score { probs, .. } => probs,
            Answer::Noul(_) => {
                return Err(Refusal::Malformed(format!(
                    "the gate {} answered a noul",
                    gate.id
                )))
            }
        };
        let values = lane_values(probs, menu)?;

        decisions.push(Decision {
            apply_tick: now.saturating_add(u64::from(lead)),
            source: Source::Agent(agent),
            seq: seq_for(now, decisions.len()),
            kind: Kind::Lane(Lane {
                gate: u8::try_from(index).unwrap_or(u8::MAX),
                menu_id: menu.id,
                values,
            }),
            provenance: Provenance::System1 {
                model: answers.model.clone(),
                tokens: answers.input_tokens,
                latency_ms: answers.latency_ms,
                // The menu's id is the hash of the situation the decision
                // was made over that this layer can see: the legal options.
                // The facts' own key (D25) travels with the think request.
                situation_key: menu.id,
            },
        });
    }
    Ok(decisions)
}

/// The lane sample of a menu answer: one value per option, in menu order.
///
/// The answer has to be over exactly this menu — the same number of
/// options and the same keys. An option that has gone is never mapped onto
/// its neighbour, and one that has appeared is never dropped (D26).
fn lane_values(probs: &BTreeMap<String, f32>, menu: &Menu) -> Result<Vec<u8>, Refusal> {
    if probs.len() != menu.options.len() {
        return Err(Refusal::MenuChanged);
    }
    let mut values = Vec::with_capacity(menu.options.len());
    for index in 0..menu.options.len() {
        let key = Gate::option_key(index);
        let Some(probability) = probs.get(&key) else {
            return Err(Refusal::MenuChanged);
        };
        values.push(quantise(*probability));
    }
    Ok(values)
}

/// The per-source sequence number of a decision.
///
/// The sequencer refuses a source that reuses a sequence number, and an
/// agent's decisions only ever move forward in the round, so the tick the
/// decision was made on is the agent's counter. The index keeps several
/// decisions made in the same tick strictly increasing.
fn seq_for(now: Tick, index: usize) -> u32 {
    let base = u32::try_from(now).unwrap_or(u32::MAX);
    base.saturating_add(u32::try_from(index).unwrap_or(u32::MAX))
}

/// Quantise a probability to the lane's scale: 255 is 1.0.
fn quantise(probability: f32) -> u8 {
    if !probability.is_finite() || probability <= 0.0 {
        return 0;
    }
    if probability >= 1.0 {
        return u8::MAX;
    }
    // The value is in 0.0..1.0, so the product is in 0.0..255.0 and the
    // rounded value fits in a u8.
    (probability * PROB_SCALE).round() as u8
}

// ---------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;

    /// The default harness's gates, straight from the file.
    const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");

    /// A gate set with the default harness's two gates.
    fn default_set() -> GateSet {
        GateSet::parse(DEFAULT_GATES).expect("the default gates parse")
    }

    /// A menu over the given options.
    fn menu_of(options: &[MenuOption]) -> Menu {
        Menu {
            id: 0x5eed_0001,
            options: options.to_vec(),
        }
    }

    /// The four-option menu of a squad with one beacon.
    fn menu() -> Menu {
        menu_of(&[
            MenuOption::GoToBeacon(0),
            MenuOption::Hold,
            MenuOption::FollowLeader,
            MenuOption::Retreat,
        ])
    }

    /// The answers of a model that answered one gate.
    fn answers_for(gate: &str, answer: Answer) -> Answers {
        Answers {
            per_gate: BTreeMap::from([(gate.to_string(), answer)]),
            model: "d1-3B".to_string(),
            input_tokens: 512,
            latency_ms: 120,
        }
    }

    /// A choice answer over the given option keys.
    fn choice(probs: &[(&str, f32)]) -> Answer {
        Answer::Choice {
            probs: probs.iter().map(|(key, p)| (key.to_string(), *p)).collect(),
            confidence: 0.7,
        }
    }

    #[test]
    fn menu_becomes_choice_criteria() {
        let set = default_set();
        let menu = menu();
        let questions = set.to_questions(&menu);

        let move_gate = &questions["move"];
        assert_eq!(move_gate["type"], "choice");
        assert_eq!(move_gate["instructions"], "What should you do now?");

        let criteria = move_gate["criteria"]
            .as_object()
            .expect("the criteria are a map");
        assert_eq!(criteria.len(), 4);
        assert_eq!(criteria["opt0"], "go to beacon 0");
        assert_eq!(criteria["opt1"], "hold position");
        assert_eq!(criteria["opt2"], "follow the squad leader");
        assert_eq!(criteria["opt3"], "retreat to the squad's spawn");

        // The noul gate is asked too, but it has no options.
        let call_out = &questions["call_out"];
        assert_eq!(call_out["type"], "noul");
        assert_eq!(
            call_out["instructions"],
            "Should you call out to your squad?"
        );
        assert!(call_out.get("criteria").is_none());
    }

    #[test]
    fn answers_quantise_in_menu_order() {
        let set = default_set();
        let menu = menu();
        let answers = answers_for(
            "move",
            choice(&[("opt0", 0.1), ("opt1", 0.6), ("opt2", 0.2), ("opt3", 0.1)]),
        );

        let decisions = answers_to_decisions(&set, &answers, &menu, 3, 100, 4)
            .expect("the answer becomes a decision");
        assert_eq!(decisions.len(), 1);

        let decision = &decisions[0];
        assert_eq!(decision.source, Source::Agent(3));
        assert_eq!(decision.apply_tick, 104);

        match &decision.kind {
            Kind::Lane(lane) => {
                assert_eq!(lane.menu_id, menu.id);
                // The samples are in menu order, whatever order the answer
                // listed its probabilities in.
                assert_eq!(lane.values, vec![26, 153, 51, 26]);
            }
            other => panic!("the move gate is a lane, got {other:?}"),
        }

        match &decision.provenance {
            Provenance::System1 {
                model,
                tokens,
                latency_ms,
                ..
            } => {
                assert_eq!(model, "d1-3B");
                assert_eq!(*tokens, 512);
                assert_eq!(*latency_ms, 120);
            }
            other => panic!("the decision is a System 1 decision, got {other:?}"),
        }
    }

    #[test]
    fn stale_menu_answer_refused() {
        let set = default_set();
        let menu = menu();
        // The answer was asked over a three-option menu; the menu now has
        // four options.
        let answers = answers_for(
            "move",
            choice(&[("opt0", 0.5), ("opt1", 0.3), ("opt2", 0.2)]),
        );

        let refusal = answers_to_decisions(&set, &answers, &menu, 0, 10, 2)
            .expect_err("the stale answer is refused");
        assert_eq!(refusal, Refusal::MenuChanged);
    }

    #[test]
    fn unknown_gate_refused() {
        let set = default_set();
        let menu = menu();
        let answers = answers_for("sneak", Answer::Noul(0.5));

        let refusal = answers_to_decisions(&set, &answers, &menu, 0, 10, 2)
            .expect_err("the unknown gate is refused");
        assert!(matches!(refusal, Refusal::Malformed(_)), "got {refusal:?}");
    }

    #[test]
    fn the_default_harness_gates_parse() {
        // The harness store hands the file's text over verbatim, so the
        // default harness and the gate set can never drift apart.
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/default");
        let harness = Harness::open(&dir).expect("the default harness opens");
        let set = GateSet::parse(&harness.gates_toml).expect("the harness gates parse");

        assert_eq!(set.len(), 2);
        let move_gate = set.gate("move").expect("the move gate is there");
        assert_eq!(move_gate.kind, GateKind::Choice);
        assert_eq!(move_gate.options_from, OptionsFrom::Menu);
        assert_eq!(move_gate.instructions, "What should you do now?");
        let call_out = set.gate("call_out").expect("the call-out gate is there");
        assert_eq!(call_out.kind, GateKind::Noul);
        assert_eq!(call_out.instructions, "Should you call out to your squad?");
    }

    #[test]
    fn a_noul_gate_makes_no_decision() {
        // The call-out is logged in phase 3; the utterance arrives in
        // phase 4. It never becomes a decision.
        let set = default_set();
        let menu = menu();
        let answers = answers_for("call_out", Answer::Noul(0.8));

        let decisions =
            answers_to_decisions(&set, &answers, &menu, 1, 5, 1).expect("the answer is accepted");
        assert!(decisions.is_empty(), "got {decisions:?}");
    }

    #[test]
    fn an_unanswered_gate_makes_no_decision() {
        // Two menu gates, one of them answered: the unanswered gate
        // contributes no lane and nothing is invented for it (D26).
        let text = r#"
            [[gates]]
            id = "move"
            type = "choice"
            options_from = "menu"
            instructions = "What should you do now?"

            [[gates]]
            id = "aim"
            type = "choice"
            options_from = "menu"
            instructions = "Which way should you face?"
        "#;
        let set = GateSet::parse(text).expect("the gates parse");
        let menu = menu();
        let answers = answers_for(
            "move",
            choice(&[("opt0", 0.5), ("opt1", 0.2), ("opt2", 0.2), ("opt3", 0.1)]),
        );

        let decisions =
            answers_to_decisions(&set, &answers, &menu, 2, 7, 3).expect("the answer is accepted");
        assert_eq!(decisions.len(), 1);
        match &decisions[0].kind {
            Kind::Lane(lane) => assert_eq!(lane.gate, 0),
            other => panic!("the move gate is a lane, got {other:?}"),
        }
    }

    #[test]
    fn every_menu_gate_gets_its_own_lane() {
        // Each menu gate is numbered by its place in the set, so the
        // conditioner never mixes two gates' samples.
        let text = r#"
            [[gates]]
            id = "move"
            type = "choice"
            options_from = "menu"
            instructions = "What should you do now?"

            [[gates]]
            id = "aim"
            type = "choice"
            options_from = "menu"
            instructions = "Which way should you face?"
        "#;
        let set = GateSet::parse(text).expect("the gates parse");
        let menu = menu();
        let probs = [("opt0", 0.5), ("opt1", 0.2), ("opt2", 0.2), ("opt3", 0.1)];
        let mut per_gate = BTreeMap::new();
        per_gate.insert("move".to_string(), choice(&probs));
        per_gate.insert("aim".to_string(), choice(&probs));
        let answers = Answers {
            per_gate,
            model: "d1-3B".to_string(),
            input_tokens: 512,
            latency_ms: 120,
        };

        let decisions =
            answers_to_decisions(&set, &answers, &menu, 2, 7, 3).expect("the answers are accepted");
        assert_eq!(decisions.len(), 2);
        let gates: Vec<u8> = decisions
            .iter()
            .map(|d| match &d.kind {
                Kind::Lane(lane) => lane.gate,
                other => panic!("the gate is a lane, got {other:?}"),
            })
            .collect();
        assert_eq!(gates, vec![0, 1]);
    }

    #[test]
    fn a_fixed_gate_asks_its_own_criteria() {
        let text = r#"
            [[gates]]
            id = "bearing"
            type = "choice"
            options_from = "fixed"
            instructions = "Which way is the beacon?"
            [gates.criteria]
            north = "the beacon lies north of you"
            south = "the beacon lies south of you"
        "#;
        let set = GateSet::parse(text).expect("the gates parse");
        let questions = set.to_questions(&menu());

        let criteria = questions["bearing"]["criteria"]
            .as_object()
            .expect("the criteria are a map");
        assert_eq!(criteria.len(), 2);
        assert_eq!(criteria["north"], "the beacon lies north of you");
        assert_eq!(criteria["south"], "the beacon lies south of you");
    }

    #[test]
    fn the_questions_are_stable() {
        let set = default_set();
        let menu = menu();
        assert_eq!(set.to_questions(&menu), set.to_questions(&menu));
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let text = r#"
            [[gates]]
            id = "move"
            type = "choice"
            options_from = "menu"
            instructions = "What should you do now?"

            [[gates]]
            id = "move"
            type = "noul"
            instructions = "Should you call out to your squad?"
        "#;
        let error = GateSet::parse(text).expect_err("the duplicate id is refused");
        assert_eq!(error, GateError::DuplicateId("move".to_string()));
    }

    #[test]
    fn a_fixed_gate_without_options_is_refused() {
        let text = r#"
            [[gates]]
            id = "bearing"
            type = "choice"
            options_from = "fixed"
            instructions = "Which way is the beacon?"
        "#;
        let error = GateSet::parse(text).expect_err("the gate without options is refused");
        assert_eq!(error, GateError::MissingCriteria("bearing".to_string()));
    }

    #[test]
    fn a_file_without_gates_is_refused() {
        let error = GateSet::parse("think_hz = 4\n").expect_err("the file is refused");
        assert_eq!(error, GateError::MissingGates);
    }
}

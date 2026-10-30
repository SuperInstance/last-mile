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
//!    an option and its probability never come apart. A `score` gate gets an
//!    **array** of level descriptions, lowest score first: the API refuses a
//!    score question whose criteria is a map (it wants 2 to 10 levels, in
//!    order), and it answers with the index of the level it picked.
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use lm_core::{Decision, Kind, Lane, Menu, MenuOption, Provenance, Source, Tick};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::sysone::{Answer, Answers, Refusal, MAX_SCORE_LEVELS, MIN_SCORE_LEVELS};
use crate::utter::{PRESENCE_SUFFIX, UTTERANCE_GATES};

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

/// One level of a score gate: its key, its score and what it means.
///
/// A score gate is an **ordered scale**, not a set of options: the API
/// takes its levels as an array of 2 to 10 descriptions, lowest score
/// first, and answers with the index of the level it picked. So a level
/// says its own score here, and the order of the array is decided by the
/// score and never by the order the file happens to be written in.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Level {
    /// The level's key: how the code names it (`idle`, `urgent`).
    #[serde(rename = "key")]
    pub key: String,
    /// The level's score: the lowest level is 0, the next is 1, and so on.
    ///
    /// The scores count from zero without a gap, because the index the
    /// model answers *is* the score: the answer's level 2 is the level
    /// whose score is 2.
    #[serde(rename = "score")]
    pub score: u32,
    /// What the level means, in the words the model reads.
    #[serde(rename = "description")]
    pub description: String,
}

/// Which pack a question belongs to (D70).
///
/// The move pack holds the questions whose answer is the agent's move, and
/// is asked on every think. The talk pack holds the questions a line is
/// chosen by, and is asked only when the agent has something to say. They
/// are two requests: the talk pack never waits behind the move pack, and
/// the move pack never waits behind the talk pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pack {
    /// The move pack: the gates that decide where the agent walks.
    Move,
    /// The talk pack: the gates that decide what the agent says.
    Talk,
}

impl Pack {
    /// The pack's name, for the log and the telemetry.
    pub fn as_str(self) -> &'static str {
        match self {
            Pack::Move => "move",
            Pack::Talk => "talk",
        }
    }

    /// The gates of this pack, in the order the file lists them.
    pub fn gates<'a>(&self, set: &'a GateSet) -> impl Iterator<Item = &'a Gate> {
        let pack = *self;
        set.gates
            .iter()
            .filter(move |gate| is_talk_gate(&gate.id) == (pack == Pack::Talk))
    }
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
    /// For a gate with `options_from = "fixed"` the keys are the option
    /// keys themselves (`opt0`, `opt1`, …).
    ///
    /// For a gate with `options_from = "menu"` the keys are the *kinds* of
    /// option — `beacon`, `hold`, `follow` and `retreat` — because the
    /// menu decides how many beacons there are and the file must not
    /// depend on that. The text of a beacon option may hold `{beacon}`,
    /// which is filled with that beacon's number.
    ///
    /// A `score` gate does not use this: its options are the ordered
    /// scale of [`Level`]s.
    #[serde(default, rename = "criteria")]
    pub criteria: BTreeMap<String, String>,
    /// The levels of a `score` gate: an ordered scale, lowest score first.
    ///
    /// The API wants a score question's criteria as an **array** of 2 to
    /// 10 level descriptions, so the levels are written out here with
    /// their scores instead of as a map: a map's key order is not a
    /// scale, and the API refuses the whole pack when a score question
    /// arrives as one.
    #[serde(default, rename = "levels")]
    pub levels: Vec<Level>,
    /// Where the options come from.
    #[serde(default, rename = "options_from")]
    pub options_from: OptionsFrom,
    /// Whether this gate's answer is the agent's **move**.
    ///
    /// A `choice` gate over the menu becomes a lane -- the agent's next
    /// move. A gate that asks about the menu *without* being a move (the
    /// subject of a line, for instance) sets `moves = false`: it is asked
    /// and answered like any other gate, and its answer says what the
    /// line is about, never where to walk.
    #[serde(default = "moves_by_default", rename = "moves")]
    pub moves: bool,
}

/// A gate is a move unless the file says otherwise.
fn moves_by_default() -> bool {
    true
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
    /// only one the simulation can carry out. What the file decides is how
    /// each option is *described* — see [`Gate::criterion_for`].
    fn menu_criteria(&self, menu: &Menu) -> Map<String, Value> {
        let mut criteria = Map::new();
        for (index, option) in menu.options.iter().enumerate() {
            criteria.insert(Self::option_key(index), json!(self.criterion_for(option)));
        }
        criteria
    }

    /// How this option is described to the model.
    ///
    /// The file's own words come first: a gate with `options_from = "menu"`
    /// writes its criteria by kind — `beacon`, `hold`, `follow`, `retreat`
    /// — because the number of beacons is the simulation's business, not the
    /// file's. A beacon's text may hold `{beacon}`, which is filled with the
    /// beacon's number.
    ///
    /// A kind the file is silent about falls back to the default wording,
    /// so a gate set never refuses to describe an option.
    fn criterion_for(&self, option: &MenuOption) -> String {
        let (kind, beacon) = match option {
            MenuOption::GoToBeacon(b) => ("beacon", Some(*b)),
            MenuOption::Hold => ("hold", None),
            MenuOption::FollowLeader => ("follow", None),
            MenuOption::Retreat => ("retreat", None),
        };
        match self.criteria.get(kind) {
            Some(text) => text.replace(
                "{beacon}",
                &beacon.map_or_else(String::new, |b| b.to_string()),
            ),
            None => describe_option(option),
        }
    }

    /// The question this gate asks, as the System One pack carries it.
    fn question(&self, menu: &Menu) -> Value {
        let mut question = Map::new();
        question.insert("type".to_string(), json!(self.kind.as_str()));
        question.insert("instructions".to_string(), json!(self.instructions));
        match self.kind {
            // A yes/no question has no options.
            GateKind::Noul => {}
            // A choice is a map of option key -> what the option means.
            GateKind::Choice => {
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
            // A score is an ordered scale: an **array** of level
            // descriptions, lowest score first. The API refuses a score
            // question whose criteria is a map ("criteria must be an array
            // of 2 to 10 levels"), and it answers with the index of the
            // level it picked — so the array's order is the scale.
            GateKind::Score => {
                question.insert(
                    "criteria".to_string(),
                    Value::Array(self.level_descriptions()),
                );
            }
        }
        Value::Object(question)
    }

    /// The levels of a score gate, lowest score first.
    ///
    /// The order is the score's and never the file's: a file may write
    /// its levels in any order, and the array the model reads is always
    /// the scale from the lowest level to the highest.
    fn levels_by_score(&self) -> Vec<&Level> {
        let mut levels: Vec<&Level> = self.levels.iter().collect();
        levels.sort_by_key(|level| level.score);
        levels
    }

    /// The descriptions of the levels, lowest score first: the array the
    /// API wants for a `score` question.
    fn level_descriptions(&self) -> Vec<Value> {
        self.levels_by_score()
            .into_iter()
            .map(|level| json!(level.description))
            .collect()
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
    /// A score gate's levels do not fit the API's scale.
    ScoreLevels {
        /// Which gate it was.
        id: String,
        /// Why the levels do not fit.
        why: String,
    },
    /// A score gate takes its options from the menu.
    ///
    /// The menu's options are named keys, not a scale: the API wants a
    /// score question's levels as an ordered array, and the menu cannot
    /// say which of its options is the lowest level.
    ScoreOverMenu(String),
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
            GateError::ScoreLevels { id, why } => {
                write!(f, "the score gate {id} does not fit the scale: {why}")
            }
            GateError::ScoreOverMenu(id) => write!(
                f,
                "the score gate {id} takes its levels from the menu, and the menu is not a scale"
            ),
            GateError::TooManyGates => {
                write!(f, "a gate set holds at most {MAX_GATES} gates")
            }
        }
    }
}

/// Check a score gate against the API's scale.
///
/// The API wants a `score` question's criteria as an **array of 2 to 10**
/// level descriptions, lowest score first, and it answers with the index
/// of the level it picked. So the levels have to exist, they have to
/// number 2 to 10, and their scores have to count 0, 1, 2, … with no gap
/// and no repeat: the index the model answers is the score, and the affect
/// the line gets is bucketed from it.
fn check_score_gate(gate: &Gate) -> Result<(), GateError> {
    let id = gate.id.clone();
    if gate.options_from == OptionsFrom::Menu {
        return Err(GateError::ScoreOverMenu(id));
    }
    let count = gate.levels.len();
    if !(MIN_SCORE_LEVELS..=MAX_SCORE_LEVELS).contains(&count) {
        return Err(GateError::ScoreLevels {
            id,
            why: format!(
                "it holds {count} levels and the API takes {MIN_SCORE_LEVELS} to {MAX_SCORE_LEVELS}"
            ),
        });
    }
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for level in &gate.levels {
        if level.key.is_empty() {
            return Err(GateError::ScoreLevels {
                id,
                why: "a level has no key".to_string(),
            });
        }
        if level.description.is_empty() {
            return Err(GateError::ScoreLevels {
                id,
                why: format!("the level {} has no description", level.key),
            });
        }
        if !keys.insert(level.key.as_str()) {
            return Err(GateError::ScoreLevels {
                id,
                why: format!("two levels are named {}", level.key),
            });
        }
    }
    let mut scores: Vec<u32> = gate.levels.iter().map(|level| level.score).collect();
    scores.sort_unstable();
    for (index, score) in scores.iter().enumerate() {
        if *score != index as u32 {
            return Err(GateError::ScoreLevels {
                id,
                why: format!("the scores must count from zero, but they are {scores:?}"),
            });
        }
    }
    Ok(())
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
            match gate.kind {
                // A choice with fixed options has to say what its options
                // mean; a choice over the menu takes them from the menu.
                GateKind::Choice => {
                    if gate.options_from == OptionsFrom::Fixed && gate.criteria.is_empty() {
                        return Err(GateError::MissingCriteria(gate.id.clone()));
                    }
                }
                // A score is a scale, and the scale has to fit the API's.
                GateKind::Score => check_score_gate(gate)?,
                // A noul has no options at all.
                GateKind::Noul => {}
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

    /// The level a score answer named.
    ///
    /// The API answers a score question with the **index** of the level it
    /// picked, counted from the lowest level of the array that was sent.
    /// This maps that index back to the level it names, so the code never
    /// has to guess what "level 2" meant: the index is the level's score.
    pub fn level_for(&self, gate: &str, index: usize) -> Option<&Level> {
        let gate = self.gate(gate)?;
        gate.levels
            .iter()
            .find(|level| usize::try_from(level.score).ok() == Some(index))
    }

    /// Where the gate with this id sits in the set, if the set has one.
    ///
    /// The pack is asked in file order and capped by `max_questions`, so
    /// the index says whether a gate was asked at all.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.gates.iter().position(|gate| gate.id == id)
    }

    /// Where the gate with this id sits in the pack it belongs to (D70).
    ///
    /// The talk pack holds the talk gates and the move pack the rest, so a
    /// gate's index in its own pack says whether that pack asked it.
    pub fn pack_index_of(&self, pack: Pack, id: &str) -> Option<usize> {
        pack.gates(self).position(|gate| gate.id == id)
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
    /// One entry per gate, keyed by the gate's id. A `choice` gate over
    /// the menu carries one criterion per menu option, keyed `opt0`,
    /// `opt1`, … — the keys its answer comes back under. A `score` gate
    /// carries an **array** of level descriptions, lowest score first:
    /// the API refuses a score question whose criteria is a map.
    pub fn to_questions(&self, menu: &Menu) -> Value {
        self.to_questions_limited(menu, u16::MAX)
    }

    /// The `questions` pack, capped at `limit` gates (the D56
    /// `max_questions` lever): a smaller pack is a faster call.
    ///
    /// The gates are taken in file order; a gate left unasked simply makes no
    /// decision (nothing is invented for it, D26).
    pub fn to_questions_limited(&self, menu: &Menu, limit: u16) -> Value {
        let mut questions = Map::new();
        for gate in self.gates.iter().take(usize::from(limit)) {
            questions.insert(gate.id.clone(), gate.question(menu));
        }
        Value::Object(questions)
    }

    /// The gates of the **move pack** (D70).
    ///
    /// Every gate that is not a talk gate: the menu choices whose answer is
    /// the agent's move, and any other gate the harness adds. The move pack
    /// is asked on every think, so it holds the questions that move the
    /// game.
    pub fn move_gates(&self) -> impl Iterator<Item = &Gate> {
        self.gates.iter().filter(|gate| !is_talk_gate(&gate.id))
    }

    /// The gates of the **talk pack** (D70): the gates a line needs — the
    /// five utterance gates and the presence gate of each.
    pub fn talk_gates(&self) -> impl Iterator<Item = &Gate> {
        self.gates.iter().filter(|gate| is_talk_gate(&gate.id))
    }

    /// How many gates the move pack holds.
    pub fn move_len(&self) -> u16 {
        u16::try_from(self.move_gates().count()).unwrap_or(u16::MAX)
    }

    /// How many gates the talk pack holds.
    pub fn talk_len(&self) -> u16 {
        u16::try_from(self.talk_gates().count()).unwrap_or(u16::MAX)
    }

    /// The `questions` pack of the move pack (D70), capped at `limit` gates.
    pub fn move_questions(&self, menu: &Menu, limit: u16) -> Value {
        self.questions_of(self.move_gates(), menu, limit)
    }

    /// The `questions` pack of the talk pack (D70), capped at `limit` gates.
    pub fn talk_questions(&self, menu: &Menu, limit: u16) -> Value {
        self.questions_of(self.talk_gates(), menu, limit)
    }

    /// The `questions` pack of a set of gates, capped at `limit`.
    fn questions_of<'a>(
        &self,
        gates: impl Iterator<Item = &'a Gate>,
        menu: &Menu,
        limit: u16,
    ) -> Value {
        let mut questions = Map::new();
        for gate in gates.take(usize::from(limit)) {
            questions.insert(gate.id.clone(), gate.question(menu));
        }
        Value::Object(questions)
    }
}

/// Whether a gate id is one of the talk gates (D70): a line gate, or the
/// presence gate of one.
///
/// The talk pack is the gates a line is chosen by, so this is what the pack
/// split turns on: the move pack holds every other gate.
pub fn is_talk_gate(id: &str) -> bool {
    UTTERANCE_GATES.contains(&id)
        || id
            .strip_suffix(PRESENCE_SUFFIX)
            .is_some_and(|gate| UTTERANCE_GATES.contains(&gate))
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
/// `now + lead`. `situation_key` is the D25 facts key the question was asked
/// over: it travels in the decision's provenance, so the log names the exact
/// situation the model answered (and the policy sees when a situation
/// repeats).
#[allow(clippy::too_many_arguments)] // a decision is a gate's answer over a menu
pub fn answers_to_decisions(
    set: &GateSet,
    pack: Pack,
    answers: &Answers,
    menu: &Menu,
    agent: u16,
    now: Tick,
    lead: u16,
    situation_key: u64,
) -> Result<Vec<Decision>, Refusal> {
    // An answer that names a gate we never asked is not an answer to our
    // question: refuse the whole pack instead of taking the part that fits.
    for id in answers.per_gate.keys() {
        if set.pack_index_of(pack, id).is_none() {
            return Err(Refusal::Malformed(format!(
                "the answer names the gate {id}, which is not one of this pack's"
            )));
        }
    }

    let mut decisions = Vec::new();
    for (index, gate) in set.gates().iter().enumerate() {
        // A gate of the other pack is not in this answer (D70): the move
        // pack decides where the agent walks, the talk pack what it says.
        if set.pack_index_of(pack, &gate.id).is_none() {
            continue;
        }
        // A gate with fixed options is not a lane: it is asked, answered
        // and logged, and whatever it calls for arrives in phase 4.
        if gate.options_from != OptionsFrom::Menu {
            continue;
        }
        // A gate that asks about the menu without being a move -- the
        // subject of a line -- is asked and answered like any other gate,
        // and never becomes a lane: the agent does not walk because of
        // what it says.
        if !gate.moves {
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
                // The situation the decision answered: the D25 key of the
                // facts the question was asked over. The menu's own id
                // travels in the lane.
                situation_key,
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
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "d1-3B".to_string(),
            input_tokens: 512,
            latency_ms: 120,
        }
    }

    /// The situation key the tests answer under: an arbitrary facts key.
    const SITUATION: u64 = 0x00c0_ffee_0000_0006;

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
        // The move gate states the squad's doctrine as a rule over the
        // facts, so the model applies the rule instead of guessing.
        let instructions = move_gate["instructions"].as_str().unwrap_or_default();
        assert!(instructions.contains("squad's doctrine"));
        assert!(instructions.contains("retreat"));
        assert!(instructions.contains("hold"));
        assert!(instructions.contains("the nearest beacon your squad does not hold"));

        // One criterion per menu option, keyed opt0..optN, and each one
        // says which branch of the doctrine it answers.
        let criteria = move_gate["criteria"]
            .as_object()
            .expect("the criteria are a map");
        assert_eq!(criteria.len(), 4);
        assert!(criteria["opt0"]
            .as_str()
            .unwrap_or_default()
            .contains("go to beacon 0"));
        assert!(criteria["opt0"]
            .as_str()
            .unwrap_or_default()
            .contains("the nearest beacon your squad does not hold"));
        assert!(criteria["opt1"]
            .as_str()
            .unwrap_or_default()
            .contains("you are standing on a beacon your squad holds"));
        assert!(criteria["opt2"]
            .as_str()
            .unwrap_or_default()
            .contains("no branch of the doctrine"));
        assert!(criteria["opt3"]
            .as_str()
            .unwrap_or_default()
            .contains("an enemy can see you within 6 cells"));

        // The presence gate is asked too, but it has no options.
        let presence = &questions["gesture_present"];
        assert_eq!(presence["type"], "noul");
        assert_eq!(
            presence["instructions"],
            "Is there a line worth saying at all right now?"
        );
        assert!(presence.get("criteria").is_none());
    }

    #[test]
    fn a_menu_gate_without_criteria_falls_back_to_the_default_wording() {
        // The file describes the options by kind; a kind it is silent
        // about still gets a description, so a gate never asks an
        // undescribed question.
        let text = r#"
[[gates]]
id = "move"
type = "choice"
options_from = "menu"
instructions = "What should you do now?"

[gates.criteria]
beacon = "walk to beacon {beacon}"
"#;
        let set = GateSet::parse(text).expect("the gates parse");
        let menu = menu();
        let questions = set.to_questions(&menu);
        let criteria = questions["move"]["criteria"]
            .as_object()
            .expect("the criteria are a map");
        assert_eq!(criteria["opt0"], "walk to beacon 0");
        assert_eq!(criteria["opt1"], "hold position");
        assert_eq!(criteria["opt2"], "follow the squad leader");
        assert_eq!(criteria["opt3"], "retreat to the squad's spawn");
    }

    #[test]
    fn answers_quantise_in_menu_order() {
        let set = default_set();
        let menu = menu();
        let answers = answers_for(
            "move",
            choice(&[("opt0", 0.1), ("opt1", 0.6), ("opt2", 0.2), ("opt3", 0.1)]),
        );

        let decisions =
            answers_to_decisions(&set, Pack::Move, &answers, &menu, 3, 100, 4, SITUATION)
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
                situation_key,
            } => {
                assert_eq!(model, "d1-3B");
                assert_eq!(*tokens, 512);
                assert_eq!(*latency_ms, 120);
                // The situation key is the facts key the decision answered,
                // not the menu's id (D25).
                assert_eq!(*situation_key, SITUATION);
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

        let refusal = answers_to_decisions(&set, Pack::Move, &answers, &menu, 0, 10, 2, SITUATION)
            .expect_err("the stale answer is refused");
        assert_eq!(refusal, Refusal::MenuChanged);
    }

    #[test]
    fn unknown_gate_refused() {
        let set = default_set();
        let menu = menu();
        let answers = answers_for("sneak", Answer::Noul(0.5));

        let refusal = answers_to_decisions(&set, Pack::Move, &answers, &menu, 0, 10, 2, SITUATION)
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

        // The move gate and the five talk gates, each with its presence
        // gate (D53).
        assert_eq!(set.len(), 11);
        let move_gate = set.gate("move").expect("the move gate is there");
        assert_eq!(move_gate.kind, GateKind::Choice);
        assert_eq!(move_gate.options_from, OptionsFrom::Menu);
        // The doctrine is a rule over the facts, not an open question.
        assert!(move_gate.instructions.contains("squad's doctrine"));
        assert!(move_gate.instructions.contains("retreat"));
        // And the file describes each kind of menu option.
        assert!(move_gate.criteria.contains_key("beacon"));
        assert!(move_gate.criteria.contains_key("hold"));
        assert!(move_gate.criteria.contains_key("follow"));
        assert!(move_gate.criteria.contains_key("retreat"));
        let presence = set
            .gate("gesture_present")
            .expect("the presence gate is there");
        assert_eq!(presence.kind, GateKind::Noul);
        assert_eq!(
            presence.instructions,
            "Is there a line worth saying at all right now?"
        );
    }

    #[test]
    fn the_doctrine_is_the_advance_one_and_the_hold_variant_keeps_the_old_one() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness");
        let default = Harness::open(&root.join("default")).expect("the default harness opens");
        let hold = Harness::open(&root.join("variants/hold")).expect("the hold variant opens");

        // The default holds the beacon only while it is contested: the
        // `advance` doctrine (Casey's decision, 2026-10-30).
        let default_set = GateSet::parse(&default.gates_toml).expect("the default gates parse");
        let instructions = &default_set
            .gate("move")
            .expect("the move gate is there")
            .instructions;
        assert!(
            instructions.contains("and an enemy is within 6 cells of you, hold"),
            "the default harness does not hold only while the beacon is contested: {instructions}"
        );

        // The `hold` variant keeps the doctrine the default harness shipped
        // until then: hold the beacon whenever no enemy is near. It is kept
        // so the two stay comparable.
        let hold_set = GateSet::parse(&hold.gates_toml).expect("the hold variant gates parse");
        let old = &hold_set
            .gate("move")
            .expect("the move gate is there")
            .instructions;
        assert!(
            old.contains("and no enemy is within 6 cells, hold"),
            "the hold variant does not hold the old doctrine: {old}"
        );

        // Every harness in the tree parses: a variant whose gates do not
        // parse is a variant nobody can play.
        let mut dirs = vec![root.join("default")];
        for entry in std::fs::read_dir(root.join("variants")).expect("the variants are there") {
            let path = entry.expect("the entry reads").path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        for dir in dirs {
            let harness = Harness::open(&dir).expect("the harness opens");
            GateSet::parse(&harness.gates_toml)
                .unwrap_or_else(|err| panic!("the gates of {} parse: {err}", dir.display()));
        }
    }

    #[test]
    fn the_urgency_gate_is_stated_over_the_facts_the_builder_states() {
        // The urgency gate is a score gate: the model picks a level and the
        // index of the level is the score the affect is bucketed from. The
        // levels are what the model reads, so they have to be phrased over
        // facts the facts builder actually states — otherwise the model
        // matches the level against a fact it was never given and answers
        // the wrong level.
        //
        // It did: the levels said "an enemy can see you **within 6 cells**"
        // and "a beacon your squad holds is **about to be lost**". The
        // builder states neither: its count line is "no enemy is within 6
        // cells of you" (the doctrine's own number, for the move gate),
        // and it says which squad holds a beacon, never that one is about
        // to be lost. On the Thor's d1-2B that made a tagged agent read as
        // calm (p(urgent) = 0.005) and an enemy watching the agent from
        // eleven cells read as ordinary — see
        // crates/lm-agents/tests/urgency_on_thor.md.
        let set = default_set();
        let gate = set.gate("intention").expect("the urgency gate is there");
        assert_eq!(gate.kind, GateKind::Score);

        // The scale is still three levels, lowest score first, and the
        // scores are the ones the affect buckets are read from.
        let levels = gate.levels_by_score();
        let keys: Vec<&str> = levels.iter().map(|level| level.key.as_str()).collect();
        assert_eq!(keys, vec!["idle", "ordinary", "urgent"]);
        let scores: Vec<u32> = levels.iter().map(|level| level.score).collect();
        assert_eq!(scores, vec![0, 1, 2]);

        // The two ends of the scale name the facts the builder states:
        // the enemy lines say "can see you", the stun says "You are
        // stunned." and an ally's line says "and is stunned". The middle
        // is the "otherwise", so it names nothing.
        for level in levels.iter().filter(|level| level.key != "ordinary") {
            assert!(
                level.description.contains("can see you"),
                "the level {} does not name the enemy's line: {}",
                level.key,
                level.description
            );
            assert!(
                level.description.contains("stunned"),
                "the level {} does not name the stun: {}",
                level.key,
                level.description
            );
        }

        // And no level asks for a fact the builder cannot state: it says
        // which squad holds a beacon, never that a beacon is about to be
        // lost, and it counts the enemies within six cells for the move
        // gate, not for the tone.
        for level in &levels {
            assert!(
                !level.description.contains("about to be lost"),
                "the level {} asks for a fact the builder never states: {}",
                level.key,
                level.description
            );
        }
        assert!(!gate.instructions.contains("about to be lost"));
        assert!(!gate.instructions.contains("not moving together"));

        // The instructions are the rule over the same facts.
        assert!(gate.instructions.contains("an enemy can see you"));
        assert!(gate.instructions.contains("you are stunned"));
        assert!(gate
            .instructions
            .contains("an ally within 4 cells of you is stunned"));
    }

    #[test]
    fn a_noul_gate_makes_no_decision() {
        // The call-out is logged in phase 3; the utterance arrives in
        // phase 4. It never becomes a decision.
        let set = default_set();
        let menu = menu();
        let answers = answers_for("gesture_present", Answer::Noul(0.8));

        let decisions = answers_to_decisions(&set, Pack::Talk, &answers, &menu, 1, 5, 1, SITUATION)
            .expect("the answer is accepted");
        assert!(decisions.is_empty(), "got {decisions:?}");
    }

    #[test]
    fn a_pack_answers_only_its_own_gates() {
        // D70: the move pack and the talk pack are separate requests, so
        // an answer over the other pack's gates is not an answer to this
        // question.
        let set = default_set();
        let menu = menu();
        let answers = answers_for(
            "move",
            Answer::Choice {
                probs: BTreeMap::new(),
                confidence: 0.9,
            },
        );
        let refusal = answers_to_decisions(&set, Pack::Talk, &answers, &menu, 1, 5, 1, SITUATION)
            .expect_err("the talk pack was not asked the move gate");
        assert!(matches!(refusal, Refusal::Malformed(_)), "got {refusal:?}");
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

        let decisions = answers_to_decisions(&set, Pack::Move, &answers, &menu, 2, 7, 3, SITUATION)
            .expect("the answer is accepted");
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
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "d1-3B".to_string(),
            input_tokens: 512,
            latency_ms: 120,
        };

        let decisions = answers_to_decisions(&set, Pack::Move, &answers, &menu, 2, 7, 3, SITUATION)
            .expect("the answers are accepted");
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
    fn score_criteria_are_an_ordered_array() {
        // The API wants a score question's criteria as an ARRAY of 2 to
        // 10 level descriptions, lowest score first. A map is refused —
        // and the whole pack with it, which is what stopped the agents
        // from deciding anything on the Thor.
        let set = default_set();
        let questions = set.to_questions(&menu());

        let criteria = questions["intention"]["criteria"]
            .as_array()
            .expect("the score criteria are an array");
        assert_eq!(criteria.len(), 3);
        assert_eq!(
            criteria[0],
            "idle: no enemy can see you, you are not stunned and no ally within 4 cells of you is stunned"
        );
        assert_eq!(
            criteria[1],
            "ordinary: nothing is happening that your squad has to know about now"
        );
        assert_eq!(
            criteria[2],
            "urgent: an enemy can see you, or you are stunned, or an ally within 4 cells of you is stunned"
        );

        // The order is the score's and not the file's: a file that writes
        // its levels out of order still sends the scale from the lowest
        // level to the highest.
        let text = r#"
            [[gates]]
            id = "intention"
            type = "score"
            options_from = "fixed"
            instructions = "How urgent is this moment?"

            [[gates.levels]]
            key = "urgent"
            score = 2
            description = "the moment is urgent"

            [[gates.levels]]
            key = "idle"
            score = 0
            description = "the moment is quiet"

            [[gates.levels]]
            key = "ordinary"
            score = 1
            description = "the moment is ordinary"
        "#;
        let set = GateSet::parse(text).expect("the gates parse");
        let questions = set.to_questions(&menu());
        let criteria = questions["intention"]["criteria"]
            .as_array()
            .expect("the score criteria are an array");
        assert_eq!(
            criteria,
            &vec![
                json!("the moment is quiet"),
                json!("the moment is ordinary"),
                json!("the moment is urgent"),
            ]
        );

        // And the index the model answers names the right level: the
        // index is the level's score, so "level 2" is the urgent one.
        assert_eq!(
            set.level_for("intention", 0)
                .map(|level| level.key.as_str()),
            Some("idle")
        );
        assert_eq!(
            set.level_for("intention", 1)
                .map(|level| level.key.as_str()),
            Some("ordinary")
        );
        assert_eq!(
            set.level_for("intention", 2)
                .map(|level| level.key.as_str()),
            Some("urgent")
        );
        assert!(
            set.level_for("intention", 3).is_none(),
            "the scale has three levels"
        );
    }

    #[test]
    fn a_score_gate_that_does_not_fit_the_scale_is_refused() {
        // Too few levels: the API takes 2 to 10, and the gates are read
        // before anything is sent, so the broken scale is a typed error
        // here and not a 400 on the Thor.
        let text = r#"
            [[gates]]
            id = "intention"
            type = "score"
            options_from = "fixed"
            instructions = "How urgent is this moment?"

            [[gates.levels]]
            key = "idle"
            score = 0
            description = "the moment is quiet"
        "#;
        let error = GateSet::parse(text).expect_err("the one-level scale is refused");
        match error {
            GateError::ScoreLevels { id, why } => {
                assert_eq!(id, "intention");
                assert!(why.contains("2 to 10"), "got {why}");
            }
            other => panic!("expected a score-level error, got {other:?}"),
        }

        // Too many levels.
        let mut text = String::from(
            "\n[[gates]]\nid = \"intention\"\ntype = \"score\"\noptions_from = \"fixed\"\ninstructions = \"How urgent is this moment?\"\n",
        );
        for level in 0..11 {
            text.push_str(&format!(
                "\n[[gates.levels]]\nkey = \"level{level}\"\nscore = {level}\ndescription = \"level {level}\"\n"
            ));
        }
        let error = GateSet::parse(&text).expect_err("the eleven-level scale is refused");
        assert!(
            matches!(error, GateError::ScoreLevels { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_score_gate_whose_scores_do_not_count_from_zero_is_refused() {
        // The index the model answers is the level's score, so the
        // scores have to count 0, 1, 2, … with no gap: a scale that
        // skips one would make the model's answer name the wrong level.
        let text = r#"
            [[gates]]
            id = "intention"
            type = "score"
            options_from = "fixed"
            instructions = "How urgent is this moment?"

            [[gates.levels]]
            key = "idle"
            score = 0
            description = "the moment is quiet"

            [[gates.levels]]
            key = "urgent"
            score = 2
            description = "the moment is urgent"
        "#;
        let error = GateSet::parse(text).expect_err("the scale with a gap is refused");
        match error {
            GateError::ScoreLevels { id, why } => {
                assert_eq!(id, "intention");
                assert!(why.contains("count from zero"), "got {why}");
            }
            other => panic!("expected a score-level error, got {other:?}"),
        }
    }

    #[test]
    fn a_score_gate_over_the_menu_is_refused() {
        // The menu's options are named keys, not a scale: nothing says
        // which of them is the lowest level.
        let text = r#"
            [[gates]]
            id = "intention"
            type = "score"
            options_from = "menu"
            instructions = "How urgent is this moment?"
        "#;
        let error = GateSet::parse(text).expect_err("the score gate over the menu is refused");
        assert_eq!(error, GateError::ScoreOverMenu("intention".to_string()));
    }

    #[test]
    fn the_questions_are_stable() {
        let set = default_set();
        let menu = menu();
        assert_eq!(set.to_questions(&menu), set.to_questions(&menu));
    }

    #[test]
    fn a_limited_pack_asks_fewer_gates() {
        // The max_questions lever (D56): a smaller pack takes the gates in
        // file order and leaves the rest unasked.
        let text = r#"
            [[gates]]
            id = "move"
            type = "choice"
            options_from = "menu"
            instructions = "What should you do now?"

            [[gates]]
            id = "call_out"
            type = "noul"
            instructions = "Should you call out to your squad?"
        "#;
        let set = GateSet::parse(text).expect("the gates parse");
        let full = set.to_questions(&menu());
        assert_eq!(full.as_object().expect("gates").len(), 2);

        let limited = set.to_questions_limited(&menu(), 1);
        let gates = limited.as_object().expect("gates");
        assert_eq!(gates.len(), 1);
        assert!(gates.contains_key("move"), "the first gate is asked");
        assert!(!gates.contains_key("call_out"), "the second is not");
        // A cap that covers everything asks everything.
        assert_eq!(set.to_questions_limited(&menu(), 3), full);
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

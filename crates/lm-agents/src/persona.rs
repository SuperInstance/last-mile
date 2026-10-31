//! Personality patches: a persona is a **System Two patch** (ROADMAP §2.4).
//!
//! A persona never asks a new question and never adds one to the pack
//! (D70): it patches the answers that came back, **before** the choice is
//! made. Three things are patched:
//!
//! * the **bias** on a closed choice — the option's log-odds are moved by a
//!   fixed amount, so the same answer leads somewhere else;
//! * the **voice** — the marks a line is written in, and the speaker and the
//!   style the harness's `voice.toml` carries;
//! * the **barks** — the short lines the persona says.
//!
//! ## The bias is arithmetic, not a thumb on the scale
//!
//! The bias is added to the option's **log-odds** in **fixed point** (Q16),
//! and the biased log-odds are turned back into a distribution by a softmax.
//! The arithmetic is integer-only, so the same answer gives the same choice
//! on every machine (D69): the last bit of a logarithm is a `libm`'s
//! business, and a `libm` is not a thing this repo may depend on.
//!
//! The unit is **tenths of a nat**. A bias of `10` is one nat, which
//! multiplies the option's odds by e (2.718); a bias of `8` is 0.8 nats,
//! which multiplies them by 2.2; a bias of `-10` is the other way.
//!
//! ## The names a persona uses
//!
//! A gate's options are named the way the **gate** names them, not the way
//! the answer keys them:
//!
//! * a gate over the **menu** (the move) names its options by kind —
//!   `beacon`, `hold`, `follow`, `retreat` — and a beacon may be named
//!   alone (`beacon2`), which wins over the kind;
//! * a gate with **fixed options** (the intent, the gesture) names them by
//!   their own key (`warn`, `report`, `callout`);
//! * a **score** gate (the urgency) names its levels by their key (`idle`,
//!   `ordinary`, `urgent`);
//! * a **noul** (the presence gate of each of those) has no options, so
//!   it is named `yes`: the persona tilts the belief that there is
//!   something to say, and that is how a persona talks more or less.
//!
//! A bias that names a gate or an option the harness does not have is
//! **not applied**: it is recorded as skipped, with a name. A persona never
//! invents an option (D26).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use lm_core::view::{Menu, MenuOption};
use serde::Deserialize;

use crate::gates::{Gate, GateSet};
use crate::sysone::{Answer, Answers};

/// The Q16 scale: 65536 is one.
const Q16: i64 = 65_536;

/// The natural log of two, in Q16.
const LN2_Q16: i64 = 45_426;

/// The lowest log-odds a probability of zero gets.
///
/// A zero has no logarithm; it gets the smallest one there is, so it stays
/// the least likely option after the softmax instead of poisoning it.
const LOG_ZERO_Q16: i64 = -30 * Q16;

/// How many options a persona may bias in one gate.
const MAX_BIASES_PER_GATE: usize = 64;

/// A personality: the patch one agent thinks with.
#[derive(Clone, Debug, PartialEq)]
pub struct Persona {
    /// The persona's id: the file stem it was read from.
    pub id: String,
    /// The name a reader gives it.
    pub name: String,
    /// One line on what the persona is.
    pub summary: String,
    /// The bias per gate, per option, in tenths of a nat.
    ///
    /// The gate is the gate's id (`move`, `intent`, `intention`, ...); the
    /// option is the name the gate gives it (see the module docs).
    pub gate_bias: BTreeMap<String, BTreeMap<String, i32>>,
    /// The marks of the voice: how the persona talks.
    pub marks: Vec<String>,
    /// The speaker the persona's lines are rendered in.
    pub voice: String,
    /// The emotion style the persona's lines are rendered in.
    pub style: String,
    /// The short lines the persona says.
    pub barks: Vec<String>,
}

/// The file a persona is read from.
///
/// The shape is checked when the file is read: a key the code does not know
/// is a refusal, never a guess.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonaFile {
    /// The persona's id.
    id: String,
    /// The persona's name.
    name: String,
    /// One line on what the persona is.
    #[serde(default)]
    summary: String,
    /// The bias per gate, per option, in tenths of a nat.
    #[serde(default)]
    gate_bias: BTreeMap<String, BTreeMap<String, i32>>,
    /// The marks of the voice.
    #[serde(default)]
    marks: Vec<String>,
    /// The speaker.
    #[serde(default)]
    voice: String,
    /// The emotion style.
    #[serde(default)]
    style: String,
    /// The short lines.
    #[serde(default)]
    barks: Vec<String>,
}

/// Why a persona did not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonaError {
    /// The file could not be read.
    Io(String),
    /// The file is not the TOML this build knows.
    Toml(String),
    /// The file says something the code does not know.
    Shape(String),
    /// The persona's id is not the file it was read from.
    IdMismatch {
        /// The id the file says.
        said: String,
        /// The file it was read from.
        file: String,
    },
    /// The persona's voice is not one `voice.toml` carries.
    UnknownVoice(String),
    /// The persona's style is not one `voice.toml` carries.
    UnknownStyle(String),
}

impl fmt::Display for PersonaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersonaError::Io(why) => write!(f, "the persona cannot be read: {why}"),
            PersonaError::Toml(why) => write!(f, "the persona is not valid TOML: {why}"),
            PersonaError::Shape(why) => write!(f, "the persona is not one this build knows: {why}"),
            PersonaError::IdMismatch { said, file } => write!(
                f,
                "the persona calls itself {said} and was read from {file}"
            ),
            PersonaError::UnknownVoice(voice) => {
                write!(f, "the voice {voice} is not one voice.toml carries")
            }
            PersonaError::UnknownStyle(style) => {
                write!(f, "the style {style} is not one voice.toml carries")
            }
        }
    }
}

impl std::error::Error for PersonaError {}

/// One bias a persona applied, and what it did to the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedBias {
    /// The gate the bias was applied to.
    pub gate: String,
    /// The answer's own key for the option (`opt2`, `warn`, `1`).
    pub option: String,
    /// The persona's name for the option (`beacon`, `warn`, `urgent`).
    pub kind: String,
    /// The bias, in tenths of a nat.
    pub bias: i32,
    /// The probability before the bias.
    pub before: f32,
    /// The probability after the bias.
    pub after: f32,
}

/// One bias a persona could not apply, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedBias {
    /// The gate the bias was for.
    pub gate: String,
    /// The option the bias was for, when it named one.
    pub option: String,
    /// Why the bias was not applied.
    pub why: String,
}

/// What a persona did to one answer set.
#[derive(Clone, Debug)]
pub struct BiasOutcome {
    /// The answers, patched.
    pub answers: Answers,
    /// The biases that were applied.
    pub applied: Vec<AppliedBias>,
    /// The biases that were not, and why.
    pub skipped: Vec<SkippedBias>,
}

impl Persona {
    /// Read a persona from its file.
    ///
    /// The file is `<dir>/<id>.toml`, and the id in the file must be the
    /// file's own name: a persona that calls itself something else is a
    /// refusal, because the id is what the round's log and the harness
    /// agree on.
    pub fn load(dir: &Path, id: &str) -> Result<Persona, PersonaError> {
        let path = dir.join(format!("{id}.toml"));
        let text = fs::read_to_string(&path)
            .map_err(|error| PersonaError::Io(format!("{}: {error}", path.display())))?;
        let persona = Persona::parse(&text)?;
        if persona.id != id {
            return Err(PersonaError::IdMismatch {
                said: persona.id,
                file: id.to_owned(),
            });
        }
        Ok(persona)
    }

    /// Parse a persona from its text.
    ///
    /// The shape is checked here: the id and the name must be there, a bias
    /// must be a number, and a bark may not carry a number of its own — the
    /// numbers of a line are the gates' (D37).
    pub fn parse(text: &str) -> Result<Persona, PersonaError> {
        let file: PersonaFile =
            toml::from_str(text).map_err(|error| PersonaError::Toml(error.to_string()))?;
        if file.id.trim().is_empty() {
            return Err(PersonaError::Shape("the persona has no id".to_string()));
        }
        if file.name.trim().is_empty() {
            return Err(PersonaError::Shape("the persona has no name".to_string()));
        }
        for (gate, options) in &file.gate_bias {
            if gate.trim().is_empty() {
                return Err(PersonaError::Shape(
                    "the persona has a bias for a gate with no name".to_string(),
                ));
            }
            if options.len() > MAX_BIASES_PER_GATE {
                return Err(PersonaError::Shape(format!(
                    "the persona biases {gate} on {} options, more than the {MAX_BIASES_PER_GATE} a gate may have",
                    options.len()
                )));
            }
            for (option, bias) in options {
                if option.trim().is_empty() {
                    return Err(PersonaError::Shape(format!(
                        "the persona has a bias on {gate} with no option"
                    )));
                }
                if !(-1_000..=1_000).contains(bias) {
                    return Err(PersonaError::Shape(format!(
                        "the persona's bias on {gate}.{option} is {bias}, outside the range a bias may take"
                    )));
                }
            }
        }
        for bark in &file.barks {
            if bark.trim().is_empty() {
                return Err(PersonaError::Shape(
                    "the persona has a bark that says nothing".to_string(),
                ));
            }
            if bark.chars().any(|c| c.is_ascii_digit()) {
                return Err(PersonaError::Shape(format!(
                    "the persona's bark {bark:?} carries a number: the numbers of a line are the gates'"
                )));
            }
        }
        for mark in &file.marks {
            if mark.trim().is_empty() {
                return Err(PersonaError::Shape(
                    "the persona has a mark that says nothing".to_string(),
                ));
            }
        }
        Ok(Persona {
            id: file.id,
            name: file.name,
            summary: file.summary,
            gate_bias: file.gate_bias,
            marks: file.marks,
            voice: file.voice,
            style: file.style,
            barks: file.barks,
        })
    }

    /// Check the persona's voice and style against the harness's voice map.
    ///
    /// The voice and the style are the ones `voice.toml` carries: a persona
    /// that names a speaker or a style the harness does not know is a
    /// refusal, because the line would be rendered in a voice that is not
    /// there (D26).
    ///
    /// A harness with no `voice.toml` has no voices to check against, and
    /// the persona is left alone.
    pub fn validate_voice(&self, voice_toml: &str) -> Result<(), PersonaError> {
        if voice_toml.trim().is_empty() {
            return Ok(());
        }
        let map: toml::Value = toml::from_str(voice_toml)
            .map_err(|error| PersonaError::Toml(format!("voice.toml: {error}")))?;
        if !self.voice.is_empty() {
            let speakers = speakers_of(&map);
            if !speakers.is_empty() && !speakers.contains(&self.voice) {
                return Err(PersonaError::UnknownVoice(self.voice.clone()));
            }
        }
        if !self.style.is_empty() {
            let styles = styles_of(&map);
            if !styles.is_empty() && !styles.contains(&self.style) {
                return Err(PersonaError::UnknownStyle(self.style.clone()));
            }
        }
        Ok(())
    }

    /// The bias this persona carries for a gate and an option, if any.
    pub fn bias_of(&self, gate: &str, option: &str) -> Option<i32> {
        self.gate_bias.get(gate)?.get(option).copied()
    }

    /// The gates this persona says something about.
    pub fn gates(&self) -> impl Iterator<Item = &str> {
        self.gate_bias.keys().map(String::as_str)
    }

    /// Patch an answer set: the biased log-odds become the new answer.
    ///
    /// The patch is applied per gate, and only to the gates the persona
    /// names. The distribution that comes out is a softmax over the biased
    /// log-odds, so it is a distribution again: the lane conditioner and
    /// the choices that read it see probabilities that add up to one.
    ///
    /// A gate the persona does not name is left exactly as it was: a
    /// persona is a patch, not a replacement.
    pub fn apply(&self, answers: &Answers, gates: &GateSet, menu: &Menu) -> BiasOutcome {
        let mut out = answers.clone();
        let mut applied = Vec::new();
        let mut skipped = Vec::new();

        for (gate_id, answer) in answers.per_gate.iter() {
            if !self.gate_bias.contains_key(gate_id) {
                continue;
            }
            let Some(gate) = gates.gate(gate_id) else {
                skipped.push(SkippedBias {
                    gate: gate_id.clone(),
                    option: String::new(),
                    why: "the harness has no gate of this name".to_string(),
                });
                continue;
            };
            let (probs, is_score) = match answer {
                Answer::Choice { probs, .. } => (probs, false),
                Answer::Score { probs, .. } => (probs, true),
                // A noul is a belief, not a choice of options: the persona
                // tilts its "yes" side, and the belief is what the presence
                // gate is read from (D53).
                Answer::Noul(value) => {
                    if let Some(bias) = self.bias_of(gate_id, "yes") {
                        let patched = noul_q16(to_q16(*value), bias_q16(bias));
                        out.per_gate
                            .insert(gate_id.clone(), Answer::Noul(from_q16(patched)));
                        applied.push(AppliedBias {
                            gate: gate_id.clone(),
                            option: "yes".to_string(),
                            kind: "yes".to_string(),
                            bias,
                            before: *value,
                            after: from_q16(patched),
                        });
                    }
                    continue;
                }
            };
            if probs.is_empty() {
                continue;
            }

            // The persona's name for each option of the gate, and the bias
            // it carries. An option the persona says nothing about keeps
            // its own log-odds.
            let mut kinds: BTreeMap<&str, (String, i32)> = BTreeMap::new();
            for key in probs.keys() {
                match option_kind(gate, key, menu) {
                    Some(kind) => {
                        // A beacon named alone (`beacon2`) wins over the kind
                        // it belongs to (`beacon`): the more specific name is
                        // the one the persona meant.
                        if let Some((specific, bias)) = self.specific_bias(gate_id, gate, key, menu)
                        {
                            kinds.insert(key.as_str(), (specific, bias));
                        } else if let Some(bias) = self.bias_of(gate_id, &kind) {
                            kinds.insert(key.as_str(), (kind, bias));
                        }
                    }
                    None => skipped.push(SkippedBias {
                        gate: gate_id.clone(),
                        option: key.clone(),
                        why: "the gate does not have this option".to_string(),
                    }),
                }
            }
            if kinds.is_empty() {
                continue;
            }

            // The log-odds, the bias, and the softmax back to a
            // distribution.
            let mut logits: Vec<i64> = Vec::with_capacity(probs.len());
            for (key, probability) in probs {
                let bias = kinds.get(key.as_str()).map_or(0, |(_, bias)| *bias);
                logits.push(ln_q16(to_q16(*probability)) + bias_q16(bias));
            }
            let patched = softmax_q16(&logits);

            let mut new_probs: BTreeMap<String, f32> = BTreeMap::new();
            for ((key, probability), q) in probs.iter().zip(patched.iter()) {
                let after = from_q16(*q);
                new_probs.insert(key.clone(), after);
                if let Some((kind, bias)) = kinds.get(key.as_str()) {
                    applied.push(AppliedBias {
                        gate: gate_id.clone(),
                        option: key.clone(),
                        kind: kind.clone(),
                        bias: *bias,
                        before: *probability,
                        after,
                    });
                }
            }

            // The answer of a score gate is the weighted level, so the
            // score is recomputed from the patched levels: a persona that
            // makes a moment more urgent changes the score, not only the
            // distribution behind it.
            let patched_answer = if is_score {
                let score = weighted_score(gate, &new_probs);
                Answer::Score {
                    probs: new_probs,
                    score,
                    confidence: match answer {
                        Answer::Score { confidence, .. } => *confidence,
                        _ => 0.0,
                    },
                }
            } else {
                Answer::Choice {
                    probs: new_probs,
                    confidence: match answer {
                        Answer::Choice { confidence, .. } => *confidence,
                        _ => 0.0,
                    },
                }
            };
            out.per_gate.insert(gate_id.clone(), patched_answer);
        }

        BiasOutcome {
            answers: out,
            applied,
            skipped,
        }
    }

    /// The bias on one specific option of a menu gate — a beacon of its own.
    ///
    /// A beacon may be named alone (`beacon2`), and that name wins over the
    /// kind it belongs to.
    fn specific_bias(
        &self,
        gate_id: &str,
        gate: &Gate,
        key: &str,
        menu: &Menu,
    ) -> Option<(String, i32)> {
        let option = menu_option(gate, key, menu)?;
        let MenuOption::GoToBeacon(beacon) = option else {
            return None;
        };
        let name = format!("beacon{beacon}");
        self.bias_of(gate_id, &name).map(|bias| (name, bias))
    }
}

// ------------------------------------------------------------------ the gates

/// The persona's name for an option of a gate.
///
/// A gate over the menu names its options by kind; a gate with fixed
/// options names them by their own key; a score gate names its levels by
/// their key.
fn option_kind(gate: &Gate, key: &str, menu: &Menu) -> Option<String> {
    if gate.options_from == crate::gates::OptionsFrom::Menu {
        let option = menu_option(gate, key, menu)?;
        return Some(match option {
            MenuOption::GoToBeacon(_) => "beacon".to_string(),
            MenuOption::Hold => "hold".to_string(),
            MenuOption::FollowLeader => "follow".to_string(),
            MenuOption::Retreat => "retreat".to_string(),
        });
    }
    if gate.kind == crate::gates::GateKind::Score {
        // A score gate's options are the levels of the scale, and the
        // answer names them by their index.
        let index: usize = key.parse().ok()?;
        let level = gate
            .levels
            .iter()
            .find(|level| usize::try_from(level.score).ok() == Some(index))?;
        return Some(level.key.clone());
    }
    Some(key.to_string())
}

/// The menu option an answer's key names, if the gate is over the menu.
fn menu_option<'a>(gate: &Gate, key: &str, menu: &'a Menu) -> Option<&'a MenuOption> {
    if gate.options_from != crate::gates::OptionsFrom::Menu {
        return None;
    }
    let index: usize = key.strip_prefix("opt")?.parse().ok()?;
    menu.options.get(index)
}

/// The weighted level of a score gate, from its patched level probabilities.
///
/// The index a model answers *is* the level's score, so the weighted score
/// is the sum of the index times its probability.
fn weighted_score(gate: &Gate, probs: &BTreeMap<String, f32>) -> f32 {
    let mut score = 0.0f32;
    for (key, probability) in probs {
        let Ok(index) = key.parse::<f32>() else {
            continue;
        };
        let level_score = gate
            .levels
            .iter()
            .find(|level| usize::try_from(level.score).ok() == Some(index as usize))
            .map_or(index, |level| {
                f32::from(u16::try_from(level.score).unwrap_or(0))
            });
        score += level_score * probability;
    }
    score
}

// ------------------------------------------------------------- fixed point

/// A probability as a Q16 integer.
///
/// The scale is a power of two, so the multiplication is exact and the
/// truncation is the only rounding: the same probability always gives the
/// same integer, on every machine.
fn to_q16(probability: f32) -> i64 {
    if !probability.is_finite() || probability <= 0.0 {
        return 0;
    }
    if probability >= 1.0 {
        return Q16;
    }
    i64::from((probability * Q16 as f32) as i32)
}

/// A Q16 integer as a probability.
fn from_q16(q: i64) -> f32 {
    q as f32 / Q16 as f32
}

/// A bias in tenths of a nat, in Q16.
fn bias_q16(tenths: i32) -> i64 {
    let scaled = i64::from(tenths) * Q16;
    if scaled >= 0 {
        (scaled + 5) / 10
    } else {
        (scaled - 5) / 10
    }
}

/// The natural log of a Q16 value, in Q16.
///
/// The value is split into a power of two and a mantissa in `[1, 2)`: the
/// power of two is exact, and the mantissa's logarithm is the `atanh`
/// series, which converges quickly over that range.
///
/// The value is the **integer**, so the split is the integer's: a Q16 value
/// of `65536` is one, and it is `2⁰` times a mantissa of one.
fn ln_q16(value: i64) -> i64 {
    if value <= 0 {
        return LOG_ZERO_Q16;
    }
    let mut exponent = 0i32;
    let mut mantissa = value;
    while mantissa >= 2 * Q16 {
        mantissa /= 2;
        exponent += 1;
    }
    while mantissa < Q16 {
        mantissa *= 2;
        exponent -= 1;
    }
    // ln(value / 65536) = exponent * ln2 + ln(mantissa / 65536)
    let z = ((mantissa - Q16) * Q16) / (mantissa + Q16);
    let log_mantissa = 2 * atanh_q16(z);
    i64::from(exponent) * LN2_Q16 + log_mantissa
}

/// The `atanh` series: `z + z³/3 + z⁵/5 + ...`, in Q16.
///
/// The series converges for `|z| < 1`, and the mantissa's `z` is at most a
/// third, so a handful of terms is the whole answer.
fn atanh_q16(z: i64) -> i64 {
    let z_squared = (z * z) >> 16;
    let mut term = z;
    let mut sum = z;
    let mut divisor = 3i64;
    while divisor <= 31 && term != 0 {
        term = ((term * z_squared) >> 16) / divisor;
        sum += term;
        divisor += 2;
    }
    sum
}

/// The exponential of a Q16 value, in Q16.
///
/// The value is split into a power of two and a remainder under `ln 2`, and
/// the remainder is the Taylor series, which converges in a few terms over
/// that range.
fn exp_q16(value: i64) -> i64 {
    if value >= 0 {
        // Only the negative side is used by the softmax, but the positive
        // side is the same arithmetic and must not overflow.
        if value > 20 * Q16 {
            return i64::MAX / 4;
        }
        let exponent = div_floor(value, LN2_Q16);
        let remainder = value - exponent * LN2_Q16;
        let sum = exp_series_q16(remainder);
        sum.saturating_mul(1i64 << exponent.clamp(0, 40))
    } else {
        let exponent = div_floor(value, LN2_Q16);
        let remainder = value - exponent * LN2_Q16;
        let sum = exp_series_q16(remainder);
        if exponent >= -40 {
            sum >> (-exponent)
        } else {
            0
        }
    }
}

/// The Taylor series of the exponential, in Q16, over `[0, ln 2)`.
fn exp_series_q16(remainder: i64) -> i64 {
    let mut sum = Q16;
    let mut term = Q16;
    let mut n = 1i64;
    while n < 24 {
        term = ((term * remainder) >> 16) / n;
        if term == 0 {
            break;
        }
        sum += term;
        n += 1;
    }
    sum
}

/// The floor of a division, for negative dividends.
fn div_floor(dividend: i64, divisor: i64) -> i64 {
    let quotient = dividend / divisor;
    let rest = dividend % divisor;
    if rest != 0 && (rest < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

/// A softmax over log-odds, in Q16: the probabilities that add up to one.
///
/// The largest log-odds is taken out first: the softmax does not care where
/// its input starts, and the exponent of a number under one is the only one
/// this arithmetic can hold.
fn softmax_q16(logits: &[i64]) -> Vec<i64> {
    let largest = logits.iter().copied().max().unwrap_or(0);
    let exponents: Vec<i64> = logits
        .iter()
        .map(|logit| exp_q16(logit - largest))
        .collect();
    let sum: i64 = exponents.iter().sum();
    if sum <= 0 {
        // Nothing survived: every option is as likely as the next.
        let each = Q16 / i64::try_from(logits.len().max(1)).unwrap_or(1);
        return vec![each; logits.len()];
    }
    exponents
        .iter()
        .map(|exponent| (exponent * Q16) / sum)
        .collect()
}

/// A noul's "yes" side, moved by a bias in log-odds.
///
/// A noul is a belief between zero and one, so the bias moves it the same
/// way it moves an option: the log-odds of "yes" are moved, and the belief
/// is what comes out.
fn noul_q16(value: i64, bias: i64) -> i64 {
    let yes = value.clamp(0, Q16);
    let no = Q16 - yes;
    // The log-odds of "yes", with the two sides kept apart: a belief of
    // zero or one has no log-odds, and it stays where it is.
    let log_odds = if yes == 0 {
        -30 * Q16
    } else if no == 0 {
        30 * Q16
    } else {
        ln_q16(yes) - ln_q16(no)
    };
    let moved = log_odds + bias;
    // The sigmoid: one over one plus the exponential of the other side.
    let other = exp_q16(-moved);
    (Q16 * Q16) / (Q16 + other)
}

// ------------------------------------------------------------------ voices

/// The speakers a voice map carries.
fn speakers_of(map: &toml::Value) -> Vec<String> {
    map.get("agents")
        .and_then(toml::Value::as_table)
        .map(|agents| {
            agents
                .values()
                .filter_map(|value| value.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// The emotion styles a voice map carries.
///
/// The styles the file itself uses are the ones it carries, and the
/// palette every shipped speaker carries is added to them: `voice.toml`
/// says that the styles are Default, Neutral, Calm, Angry, Happy and Sad,
/// and that the shipped table sticks to the four every speaker (mia,
/// jason, aria) carries — Neutral, Calm, Angry and Happy. Sad is left out
/// on purpose: Jason does not carry it.
fn styles_of(map: &toml::Value) -> Vec<String> {
    let mut styles: Vec<String> = SKALD_PALETTE
        .iter()
        .map(|style| (*style).to_string())
        .collect();
    if let Some(table) = map.get("styles").and_then(toml::Value::as_table) {
        for values in table.values().filter_map(toml::Value::as_table) {
            for style in values.values().filter_map(|value| value.as_str()) {
                if !styles.iter().any(|known| known == style) {
                    styles.push(style.to_string());
                }
            }
        }
    }
    styles
}

/// The styles every shipped speaker carries, as `voice.toml` says.
const SKALD_PALETTE: [&str; 4] = ["Neutral", "Calm", "Angry", "Happy"];

// ------------------------------------------------------------------ the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;

    /// A harness with the shipped gates, for the tests that need one.
    fn harness() -> Harness {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/default");
        Harness::open(&dir).expect("the shipped harness opens")
    }

    #[test]
    fn a_persona_is_read_from_its_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/personas");
        let persona = Persona::load(&dir, "anxious-grunt").expect("the persona loads");
        assert_eq!(persona.id, "anxious-grunt");
        assert_eq!(persona.barks.len(), 6);
        assert!(!persona.gate_bias.is_empty());
    }

    #[test]
    fn a_persona_that_names_another_is_refused() {
        let text = "id = \"one\"\nname = \"One\"\n";
        let _persona = Persona::parse(text).expect("the persona parses");
        let dir = std::env::temp_dir().join("lm-persona-mismatch");
        std::fs::create_dir_all(&dir).expect("the directory is made");
        std::fs::write(dir.join("two.toml"), text).expect("the file is written");
        let error = Persona::load(&dir, "two").expect_err("the mismatch is refused");
        assert_eq!(
            error,
            PersonaError::IdMismatch {
                said: "one".to_string(),
                file: "two".to_string(),
            }
        );
    }

    #[test]
    fn a_persona_with_a_key_the_code_does_not_know_is_refused() {
        let text = "id = \"one\"\nname = \"One\"\nnonsense = 3\n";
        assert!(Persona::parse(text).is_err());
    }

    #[test]
    fn a_bark_with_a_number_in_it_is_refused() {
        let text = "id = \"one\"\nname = \"One\"\nbarks = [\"take beacon 2 now\"]\n";
        let error = Persona::parse(text).expect_err("the numbered bark is refused");
        assert!(matches!(error, PersonaError::Shape(_)));
    }

    #[test]
    fn the_logarithm_is_close_enough() {
        // ln(1) = 0, ln(2) = 0.693, ln(0.5) = -0.693.
        assert_eq!(ln_q16(Q16), 0);
        assert!((ln_q16(2 * Q16) - LN2_Q16).abs() <= 2);
        assert!((ln_q16(Q16 / 2) + LN2_Q16).abs() <= 2);
        // The exponent of the logarithm of x is x, over the range that
        // matters: a probability.
        for x in [1i64, 64, 1_000, 16_384, 32_768, 49_152, 65_536] {
            let round_trip = exp_q16(ln_q16(x));
            assert!(
                (round_trip - x).abs() <= 64,
                "exp(ln({x})) = {round_trip}, wanted {x}"
            );
        }
    }

    #[test]
    fn the_softmax_adds_up_to_one() {
        let logits = [0i64, -1000, 2000, 65_536];
        let patched = softmax_q16(&logits);
        let sum: i64 = patched.iter().sum();
        assert!((sum - Q16).abs() <= 4, "the softmax sums to {sum}");
        // The largest log-odds is the largest probability.
        assert_eq!(patched.iter().max(), Some(&patched[3]));
        // And the smallest is the smallest.
        assert_eq!(patched.iter().min(), Some(&patched[1]));
    }

    #[test]
    fn the_bias_moves_the_option_it_names() {
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![
                MenuOption::GoToBeacon(0),
                MenuOption::Hold,
                MenuOption::FollowLeader,
                MenuOption::Retreat,
            ],
        };
        let mut probs = BTreeMap::new();
        probs.insert("opt0".to_string(), 0.4f32);
        probs.insert("opt1".to_string(), 0.3f32);
        probs.insert("opt2".to_string(), 0.2f32);
        probs.insert("opt3".to_string(), 0.1f32);
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "move".to_string(),
                Answer::Choice {
                    probs: probs.clone(),
                    confidence: 0.8,
                },
            )]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let persona = Persona::parse(
            "id = \"test\"\nname = \"Test\"\n\
             [gate_bias.move]\nretreat = 30\n",
        )
        .expect("the persona parses");
        let outcome = persona.apply(&answers, &gates, &menu);
        let Answer::Choice { probs: patched, .. } = &outcome.answers.per_gate["move"] else {
            panic!("the move gate is still a choice");
        };
        // The retreat option is the one the persona named, and it is now
        // the most likely one.
        assert!(
            patched["opt3"] > patched["opt0"],
            "retreat {} should beat beacon {}",
            patched["opt3"],
            patched["opt0"]
        );
        assert_eq!(outcome.applied.len(), 1);
        assert_eq!(outcome.applied[0].kind, "retreat");
        assert_eq!(outcome.applied[0].bias, 30);
    }

    #[test]
    fn the_patch_is_the_same_every_time() {
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![MenuOption::GoToBeacon(0), MenuOption::Hold],
        };
        let mut probs = BTreeMap::new();
        probs.insert("opt0".to_string(), 0.6f32);
        probs.insert("opt1".to_string(), 0.4f32);
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "move".to_string(),
                Answer::Choice {
                    probs,
                    confidence: 0.8,
                },
            )]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let persona =
            Persona::parse("id = \"test\"\nname = \"Test\"\n[gate_bias.move]\nhold = 12\n")
                .expect("the persona parses");
        let first = persona.apply(&answers, &gates, &menu);
        let second = persona.apply(&answers, &gates, &menu);
        assert_eq!(first.answers, second.answers);
    }

    #[test]
    fn a_gate_the_persona_does_not_name_is_left_alone() {
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![MenuOption::GoToBeacon(0), MenuOption::Hold],
        };
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "intent".to_string(),
                Answer::Choice {
                    probs: BTreeMap::from([
                        ("order".to_string(), 0.5f32),
                        ("warn".to_string(), 0.5f32),
                    ]),
                    confidence: 0.8,
                },
            )]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let persona =
            Persona::parse("id = \"test\"\nname = \"Test\"\n[gate_bias.move]\nhold = 12\n")
                .expect("the persona parses");
        let outcome = persona.apply(&answers, &gates, &menu);
        assert_eq!(outcome.answers, answers);
        assert!(outcome.applied.is_empty());
    }

    #[test]
    fn a_bias_on_a_gate_the_harness_does_not_have_is_skipped() {
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![MenuOption::GoToBeacon(0)],
        };
        let answers = Answers {
            per_gate: BTreeMap::new(),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let persona =
            Persona::parse("id = \"test\"\nname = \"Test\"\n[gate_bias.nonsense]\nhold = 12\n")
                .expect("the persona parses");
        let outcome = persona.apply(&answers, &gates, &menu);
        assert!(outcome.applied.is_empty());
    }

    #[test]
    fn the_voice_is_the_one_the_voice_map_carries() {
        let voice = r#"
[agents]
default = "jason"
"0" = "mia"

[styles.tone]
calm = "Neutral"
"#;
        let persona =
            Persona::parse("id = \"test\"\nname = \"Test\"\nvoice = \"aria\"\nstyle = \"Calm\"\n")
                .expect("the persona parses");
        assert!(persona.validate_voice(voice).is_err());
        let persona = Persona::parse(
            "id = \"test\"\nname = \"Test\"\nvoice = \"mia\"\nstyle = \"Neutral\"\n",
        )
        .expect("the persona parses");
        assert!(persona.validate_voice(voice).is_ok());
    }

    #[test]
    fn every_shipped_persona_loads_and_is_well_formed() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/personas");
        let ids = [
            "anxious-grunt",
            "chatterbox",
            "literal-minded-rule-follower",
            "middle-manager-leader",
            "overconfident-sprinter",
            "quiet-sharpshooter",
        ];
        let voice = include_str!("../../../assets/harness/default/voice.toml");
        for id in ids {
            let persona = Persona::load(&dir, id).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!(persona.id, id);
            assert_eq!(persona.barks.len(), 6, "{id} has six barks");
            assert!(!persona.marks.is_empty(), "{id} has marks");
            assert!(!persona.voice.is_empty(), "{id} has a voice");
            assert!(!persona.style.is_empty(), "{id} has a style");
            assert!(!persona.gate_bias.is_empty(), "{id} has a bias");
            persona
                .validate_voice(voice)
                .unwrap_or_else(|error| panic!("{id}: {error}"));
        }
    }

    #[test]
    fn a_persona_moves_the_answer_it_names() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/personas");
        let persona = Persona::load(&dir, "anxious-grunt").expect("the persona loads");
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![
                MenuOption::GoToBeacon(0),
                MenuOption::Hold,
                MenuOption::FollowLeader,
                MenuOption::Retreat,
            ],
        };
        let mut probs = BTreeMap::new();
        probs.insert("opt0".to_string(), 0.4f32);
        probs.insert("opt1".to_string(), 0.3f32);
        probs.insert("opt2".to_string(), 0.2f32);
        probs.insert("opt3".to_string(), 0.1f32);
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "move".to_string(),
                Answer::Choice {
                    probs,
                    confidence: 0.8,
                },
            )]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let outcome = persona.apply(&answers, &gates, &menu);
        let Answer::Choice { probs: patched, .. } = &outcome.answers.per_gate["move"] else {
            panic!("the move gate is still a choice");
        };
        // The anxious grunt retreats: the retreat option was the least
        // likely one and is now the most likely one.
        assert!(
            patched["opt3"] > patched["opt0"],
            "retreat {} should beat beacon {}",
            patched["opt3"],
            patched["opt0"]
        );
        // The patch is recorded.
        assert!(outcome
            .applied
            .iter()
            .any(|applied| applied.kind == "retreat" && applied.bias == 18));
    }

    #[test]
    fn a_beacon_named_alone_wins_over_its_kind() {
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![
                MenuOption::GoToBeacon(0),
                MenuOption::GoToBeacon(1),
                MenuOption::Hold,
            ],
        };
        let mut probs = BTreeMap::new();
        probs.insert("opt0".to_string(), 0.4f32);
        probs.insert("opt1".to_string(), 0.3f32);
        probs.insert("opt2".to_string(), 0.3f32);
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "move".to_string(),
                Answer::Choice {
                    probs,
                    confidence: 0.8,
                },
            )]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        // The persona names the kind and one beacon of it: the beacon's own
        // name wins.
        let persona = Persona::parse(
            "id = \"test\"\nname = \"Test\"\n[gate_bias.move]\nbeacon = 10\nbeacon1 = 30\n",
        )
        .expect("the persona parses");
        let outcome = persona.apply(&answers, &gates, &menu);
        let by_option: BTreeMap<&str, (String, i32)> = outcome
            .applied
            .iter()
            .map(|applied| {
                (
                    applied.option.as_str(),
                    (applied.kind.clone(), applied.bias),
                )
            })
            .collect();
        assert_eq!(by_option.get("opt0").map(|(_, bias)| *bias), Some(10));
        assert_eq!(by_option.get("opt1").map(|(_, bias)| *bias), Some(30));
        assert_eq!(by_option.get("opt2"), None);
    }

    #[test]
    fn a_presence_gate_is_tilted_by_its_yes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/personas");
        let persona = Persona::load(&dir, "quiet-sharpshooter").expect("the persona loads");
        let harness = harness();
        let gates = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates load");
        let menu = Menu {
            id: 7,
            options: vec![MenuOption::GoToBeacon(0)],
        };
        let answers = Answers {
            per_gate: BTreeMap::from([("gesture_present".to_string(), Answer::Noul(0.9))]),
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let outcome = persona.apply(&answers, &gates, &menu);
        let Answer::Noul(after) = outcome.answers.per_gate["gesture_present"] else {
            panic!("the presence gate is still a noul");
        };
        assert!(after < 0.9, "the quiet one says less: {after}");
    }
}

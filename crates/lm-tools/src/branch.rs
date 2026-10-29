//! `lm branch`: rewind a round to a tick, change one thing, re-run it.
//!
//! This is the permanent test of the whole design (spec: "watch a stretch
//! of play, change one decision rule, re-run, and see the intended
//! difference"). Deterministic replay is what makes it cheap: the parent
//! round is replayed to the branch tick with every hash verified, the
//! changes go in, and the round runs on with the parent's decisions — the
//! same round, one rule different.
//!
//! The child log is a round log like any other. Its header is the parent's
//! round plus a [`Lineage`] that names the parent (header hash and head
//! segment hash), the fork tick and the changes; the sim applies those
//! changes when their tick begins, during the first run and during every
//! replay alike, so a branch replays from its own header, bit for bit.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::runner::{self, ReplayError};
use lm_core::decision::{Decision, Kind, Provenance, Source, Stamped, Tick};
use lm_core::hash::Fnv64;
use lm_core::header::{Header, Lineage, LineageChange};
use lm_core::log::{Recorder, RoundLog};
use lm_core::params::{Params, BRANCH_FIELDS};
use lm_core::rules::{Event, SimError};
use lm_core::sim::Sim;

/// One change a branch makes at its tick.
///
/// The changes of one branch fire in the order they are given, on the tick
/// the branch forks on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Replace the reflex script `name` with the source in `src`, for every
    /// roster entry that names it.
    Reflex { name: String, src: String },
    /// Set one parameter field, from the mid-round whitelist
    /// ([`BRANCH_FIELDS`]). Anything else is refused: a branch may retune
    /// how the round plays, never change what the round is.
    Param { field: String, value: i64 },
    /// Stop applying the decisions of `source` from the branch tick on.
    DropDecisions { source: Source },
}

/// What a branch did, compared with its parent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchReport {
    /// The first tick whose state hash differs from the parent's; `None`
    /// when the branch played the parent's round to the end.
    pub first_divergent_tick: Option<Tick>,
    /// The parent's final scores, in squad order.
    pub parent_scores: Vec<i64>,
    /// The branch's final scores, in squad order.
    pub branch_scores: Vec<i64>,
    /// How many more (or fewer) events of each kind the branch produced
    /// than the parent: counts of event kinds, branch minus parent.
    pub events_delta: BTreeMap<String, i64>,
}

impl fmt::Display for BranchReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.first_divergent_tick {
            Some(tick) => writeln!(f, "first divergence at tick {tick}")?,
            None => writeln!(f, "no divergence: the branch played the parent's round")?,
        }
        writeln!(
            f,
            "scores parent {:?} -> branch {:?}",
            self.parent_scores, self.branch_scores
        )?;
        if self.events_delta.is_empty() {
            return write!(f, "events: none differ");
        }
        write!(f, "events (branch - parent): ")?;
        for (index, (kind, delta)) in self.events_delta.iter().enumerate() {
            if index > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{kind} {delta:+}")?;
        }
        Ok(())
    }
}

/// Why a branch could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchError {
    /// The param field is not on the mid-round whitelist: it shapes the
    /// round instead of tuning it, and the log the branch forked already
    /// recorded that shape.
    ParamNotBranchable(String),
    /// The param value does not fit the field, or leaves the parameters
    /// invalid.
    ParamRefused { field: String, reason: String },
    /// The branch tick is not inside the parent round.
    AtOutsideParent { at: Tick, first: Tick, last: Tick },
    /// The parent log does not describe the round it claims to: a hash the
    /// replay saw disagrees with the hash the log recorded.
    Parent(ReplayError),
    /// The branch round could not start (a replacement script that does not
    /// compile, a change the parameters refuse, ...).
    Setup(SimError),
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BranchError::ParamNotBranchable(field) => write!(
                f,
                "the field {field:?} is not branchable mid-round; the branch fields are {}",
                BRANCH_FIELDS.join(", ")
            ),
            BranchError::ParamRefused { field, reason } => {
                write!(f, "the field {field:?} refuses the value: {reason}")
            }
            BranchError::AtOutsideParent { at, first, last } => write!(
                f,
                "the branch tick {at} is outside the parent round ({first}..={last})"
            ),
            BranchError::Parent(err) => write!(f, "the parent round does not replay: {err}"),
            BranchError::Setup(err) => write!(f, "the branch round does not start: {err}"),
        }
    }
}

impl std::error::Error for BranchError {}

/// Branch a round: replay the parent to `at`, apply the changes, and play on
/// with the parent's decisions minus any dropped sources.
///
/// The parent is verified on the way — every state hash the parent log
/// recorded must come out of the replay again — so a branch of a damaged
/// log is refused, never based on a round that never happened. The child
/// log that comes back replays from its own header like any round log, and
/// the report says what the change did: where the two rounds parted company,
/// how each squad scored, and which events happened more or less often.
pub fn branch(
    parent: &RoundLog,
    at: Tick,
    changes: &[Change],
) -> Result<(RoundLog, BranchReport), BranchError> {
    // Everything a branch will do is checked before it does anything: a
    // refused branch costs nothing and half-exists nowhere.
    let changes = to_lineage(&parent.header.params, changes)?;

    // The branch tick must fall inside the parent round: the branch replays
    // the parent up to `at` and plays on from there.
    let first = parent.header.start_tick;
    let last = match parent.segments.last() {
        Some(segment) => segment.last_tick,
        None => {
            return Err(BranchError::AtOutsideParent {
                at,
                first,
                last: first,
            })
        }
    };
    if at < first || at > last {
        return Err(BranchError::AtOutsideParent { at, first, last });
    }

    // The child header is the parent's round plus the lineage. The params
    // and the scripts stay the parent's — the sim applies the lineage's
    // changes when their tick begins — so every tick before `at` replays
    // bit-exact, and the log that comes out carries its own ancestry: a
    // branch of a branch chains to its parent's lineage in `prior`.
    let mut header = parent.header.clone();
    header.lineage = Some(Lineage {
        parent_header: parent.header.hash(),
        parent_head: parent
            .segments
            .last()
            .map(|segment| segment.hash)
            .unwrap_or(0),
        at,
        changes,
        prior: parent.header.lineage.clone().map(Box::new),
    });

    // Two rounds, side by side: the parent, verified against its log, and
    // the branch, recorded into a log of its own.
    let mut parent_sim = Sim::new(&parent.header).map_err(BranchError::Setup)?;
    let mut branch_sim = Sim::new(&header).map_err(BranchError::Setup)?;
    let mut recorder = Recorder::new(header.clone(), header.params.segment_ticks);

    // The marker decisions of the script swaps, stamped for the branch tick
    // so the log itself says where the round changed.
    let markers = marker_decisions(&header, at);

    // The parent's hashes, to verify the replay against, in tick order.
    let expected = parent.hashes();
    let mut hash_index = 0usize;
    let mut parent_events: BTreeMap<&'static str, i64> = BTreeMap::new();
    let mut branch_events: BTreeMap<&'static str, i64> = BTreeMap::new();

    for tick in first..=last {
        let recorded = parent.decisions_for(tick);

        // The parent replays with exactly the decisions it logged.
        let parent_out = parent_sim.step(recorded);
        for event in &parent_out.events {
            *parent_events.entry(kind_name(event)).or_default() += 1;
        }
        if let Some(hash) = parent_out.hash {
            match expected.get(hash_index) {
                Some((want_tick, want)) if *want_tick == tick && *want == hash => hash_index += 1,
                Some((want_tick, want)) => {
                    return Err(BranchError::Parent(ReplayError::Diverged {
                        tick: (*want_tick).min(tick),
                        expected: *want,
                        got: hash,
                    }));
                }
                None => {
                    // The replay produced a hash the log never recorded.
                    return Err(BranchError::Parent(ReplayError::Diverged {
                        tick,
                        expected: 0,
                        got: hash,
                    }));
                }
            }
        }

        // The branch plays the same decisions, its swaps marked, until its
        // own round ends; its log ends there, like any round's.
        if branch_sim.outcome().is_some() {
            continue;
        }
        let mut applied: Vec<Stamped> = Vec::new();
        if tick == at {
            applied.extend(markers.iter().cloned());
        }
        applied.extend(recorded.iter().cloned());
        let branch_out = branch_sim.step(&applied);
        for event in &branch_out.events {
            *branch_events.entry(kind_name(event)).or_default() += 1;
        }
        recorder.record(tick, &applied, branch_out.hash);
    }

    // A parent that produced hashes past the ones it logged, or a log that
    // ended before the round did, is not the round it claims to be.
    if hash_index < expected.len() {
        let (tick, hash) = expected[hash_index];
        return Err(BranchError::Parent(ReplayError::Diverged {
            tick,
            expected: hash,
            got: 0,
        }));
    }

    let log = recorder.finish();

    let mut events_delta = BTreeMap::new();
    let kinds: BTreeSet<&'static str> = parent_events
        .keys()
        .chain(branch_events.keys())
        .copied()
        .collect();
    for kind in kinds {
        let delta = branch_events.get(kind).copied().unwrap_or(0)
            - parent_events.get(kind).copied().unwrap_or(0);
        if delta != 0 {
            events_delta.insert(kind.to_owned(), delta);
        }
    }

    let report = BranchReport {
        first_divergent_tick: runner::diff(parent, &log).map(|(tick, _, _)| tick),
        parent_scores: parent_sim.scores().to_vec(),
        branch_scores: branch_sim.scores().to_vec(),
        events_delta,
    };
    Ok((log, report))
}

/// Check the changes and put them in the header's canonical form.
///
/// The params are checked on a scratch copy, in the order the changes will
/// fire, so the exact sequence that will run is the sequence that was
/// checked.
fn to_lineage(params: &Params, changes: &[Change]) -> Result<Vec<LineageChange>, BranchError> {
    let mut checked = Vec::with_capacity(changes.len());
    let mut scratch = *params;
    for change in changes {
        match change {
            Change::Reflex { name, src } => checked.push(LineageChange::Reflex {
                name: name.clone(),
                src: src.clone(),
            }),
            Change::Param { field, value } => match scratch.set_branchable(field, *value) {
                Ok(true) => checked.push(LineageChange::Param {
                    field: field.clone(),
                    value: *value,
                }),
                Ok(false) => return Err(BranchError::ParamNotBranchable(field.clone())),
                Err(err) => {
                    return Err(BranchError::ParamRefused {
                        field: field.clone(),
                        reason: err.reason.clone(),
                    })
                }
            },
            Change::DropDecisions { source } => {
                checked.push(LineageChange::DropDecisions { source: *source })
            }
        }
    }
    Ok(checked)
}

/// The marker decisions that record the branch's script swaps in its log.
///
/// The sim writes no logs; the branch records each swap itself, as a marker
/// of the system on the branch tick. The payload is the FNV-1a hash of the
/// script's name, truncated — enough to tell the swaps apart in a dump.
fn marker_decisions(header: &Header, at: Tick) -> Vec<Stamped> {
    let Some(lineage) = &header.lineage else {
        return Vec::new();
    };
    lineage
        .changes
        .iter()
        .enumerate()
        .filter_map(|(index, change)| match change {
            LineageChange::Reflex { name, .. } => Some(marker(
                name,
                at,
                u32::try_from(index).unwrap_or(u32::MAX) + 1,
            )),
            _ => None,
        })
        .collect()
}

/// One swap's marker decision, stamped for the branch tick.
fn marker(name: &str, at: Tick, seq: u32) -> Stamped {
    let mut h = Fnv64::new();
    h.write(name.as_bytes());
    Stamped {
        d: Decision {
            apply_tick: at,
            source: Source::System,
            seq,
            kind: Kind::Marker(h.finish() as u32),
            provenance: Provenance::Test,
        },
        arrived: at,
        tick: at,
        late_by: 0,
    }
}

/// The name of an event's kind, for counting.
fn kind_name(event: &Event) -> &'static str {
    match event {
        Event::Tagged { .. } => "Tagged",
        Event::Respawned(_) => "Respawned",
        Event::BeaconHeld { .. } => "BeaconHeld",
        Event::BeaconLost { .. } => "BeaconLost",
        Event::RoundOver(_) => "RoundOver",
        Event::Marker(_) => "Marker",
    }
}

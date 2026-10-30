//! The names of the refusals on the model path, and the record the log keeps.
//!
//! A refusal is an outcome, not a bug (D26): the reflex keeps playing and no
//! default answer is ever invented. What D26 did not say is *which* refusal
//! happened, and "8 refusals" hides four different fixes. A model that timed
//! out, a model that answered 503, a pack that does not fit the API's schema
//! and a gate the backend was unsure about are four different problems, and a
//! counter that adds them up cannot tell them apart.
//!
//! So every refusal on the model path carries a name, and the name is in the
//! round log: the log is what a replay reads, and a replay that cannot say
//! *why* a mind said nothing cannot teach anything about it (D64). The score
//! view (E7) draws the same names as hollow marks, in a live round and in a
//! replay, because both read the same record.
//!
//! This is the **model path's** vocabulary. The rules layer has its own
//! ([`crate::rules::RefusalKind`]), which names what a *script* refused. The
//! two are different things, and neither is re-exported at the crate root, so
//! neither can be mistaken for the other.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::decision::Tick;

/// The name of the rule that failed on the model path.
///
/// The order of the variants is part of the log format: a new kind is
/// **appended**, never inserted, so a log written by an older build still
/// decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RefusalKind {
    /// The call did not come back before the deadline.
    Timeout,
    /// The call never reached the model: DNS, connect, TLS or protocol
    /// failure. The model may well be up — nothing answered.
    Unreachable,
    /// The endpoint answered, and the status was not a success.
    ///
    /// The status is in the record's `detail`.
    Http,
    /// The answer did not match the schema.
    Malformed,
    /// The pack does not fit the API's schema.
    ///
    /// This one is a bug in the pack and not in the model: the pack is built
    /// by the gates and checked before it is sent.
    InvalidPack,
    /// The answer was below the backend's confidence floor.
    ///
    /// The rule is per gate (D72): the gate that was unsure makes no
    /// decision of its own, and every unsure gate is a refusal of its own.
    /// The confidence is in the record's `detail`, in Q16.
    LowConfidence,
    /// The menu changed between the question and the answer.
    MenuChanged,
    /// The agent's budget for this round is spent.
    Budget,
    /// The wording did not follow the plan (D37).
    ///
    /// The model wrote something that is not the line the gates chose: the
    /// notation did not compile, or the reply was not a line at all.
    OffPlan,
    /// The request's deadline passed while it was queued.
    ///
    /// The request was dropped instead of being sent: it would only have
    /// produced a stale decision.
    Expired,
    /// A newer question replaced this one before it was sent.
    ///
    /// The queue keeps one request per agent and pack, and the newest facts
    /// are the only ones worth asking about.
    Superseded,
    /// The second opinion did not answer before the decision's tick.
    ///
    /// The name is **reserved**: the escalation deadline (N3) has not landed,
    /// so nothing produces this kind yet. It is here so that the name is
    /// taken before the feature is, and so the vocabulary does not have to
    /// grow a variant in the middle later.
    EscalationLate,
    /// The backend crashed answering the question.
    ///
    /// A panic is a bug in the backend and not a decision by the model, but
    /// the question still ends here, with a name: a worker that catches the
    /// crash answers with this refusal instead of dying with the question
    /// still in its hands (N4).
    Panicked,
}

impl RefusalKind {
    /// Every kind, in the order of the enum.
    ///
    /// The telemetry walks this list, so a column exists for every kind even
    /// when the kind never happened.
    pub const ALL: [RefusalKind; 13] = [
        RefusalKind::Timeout,
        RefusalKind::Unreachable,
        RefusalKind::Http,
        RefusalKind::Malformed,
        RefusalKind::InvalidPack,
        RefusalKind::LowConfidence,
        RefusalKind::MenuChanged,
        RefusalKind::Budget,
        RefusalKind::OffPlan,
        RefusalKind::Expired,
        RefusalKind::Superseded,
        RefusalKind::EscalationLate,
        RefusalKind::Panicked,
    ];

    /// The name of the kind, as the telemetry writes it.
    ///
    /// The name is the kind's identity: it is the column of the soak table,
    /// and the word a reader of the log looks for.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalKind::Timeout => "timeout",
            RefusalKind::Unreachable => "unreachable",
            RefusalKind::Http => "http",
            RefusalKind::Malformed => "malformed",
            RefusalKind::InvalidPack => "invalid_pack",
            RefusalKind::LowConfidence => "low_confidence",
            RefusalKind::MenuChanged => "menu_changed",
            RefusalKind::Budget => "budget",
            RefusalKind::OffPlan => "off_plan",
            RefusalKind::Expired => "expired",
            RefusalKind::Superseded => "superseded",
            RefusalKind::EscalationLate => "escalation_late",
            RefusalKind::Panicked => "panicked",
        }
    }
}

impl fmt::Display for RefusalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            RefusalKind::Timeout => "the model did not answer before the deadline",
            RefusalKind::Unreachable => "the call never reached the model",
            RefusalKind::Http => "the endpoint refused the call",
            RefusalKind::Malformed => "the answer did not match the schema",
            RefusalKind::InvalidPack => "the pack does not fit the API's schema",
            RefusalKind::LowConfidence => "the answer was below the confidence floor",
            RefusalKind::MenuChanged => "the menu changed under the question",
            RefusalKind::Budget => "the budget is spent",
            RefusalKind::OffPlan => "the wording did not follow the plan",
            RefusalKind::Expired => "the request expired while it was queued",
            RefusalKind::Superseded => "a newer question replaced this one",
            RefusalKind::EscalationLate => "the second opinion did not answer in time",
            RefusalKind::Panicked => "the backend crashed answering the question",
        };
        f.write_str(text)
    }
}

/// One refusal, as the round log keeps it.
///
/// The record is whole numbers only: the tick, the agent whose ask it was,
/// the name of the rule that failed, and the number that rule cares about —
/// the HTTP status, the confidence in Q16, the ticks late. A record that
/// carried text or a decimal would not be byte-exact across architectures
/// (D69), and the log has to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Refused {
    /// The tick the refusal happened on.
    pub tick: Tick,
    /// The agent whose ask it was.
    pub agent: u16,
    /// Which rule failed.
    pub kind: RefusalKind,
    /// The number the rule cares about: the HTTP status, the confidence in
    /// Q16, the ticks late. Zero when the rule has no number.
    pub detail: u32,
}

impl Refused {
    /// A refusal of `kind` on `tick`, for `agent`.
    pub fn new(tick: Tick, agent: u16, kind: RefusalKind) -> Refused {
        Refused {
            tick,
            agent,
            kind,
            detail: 0,
        }
    }

    /// The same refusal, with the number its rule cares about.
    pub fn with_detail(mut self, detail: u32) -> Refused {
        self.detail = detail;
        self
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "agent {} on tick {}: {}",
            self.agent, self.tick, self.kind
        )
    }
}

/// How many times each kind of refusal happened.
///
/// The telemetry splits the refusals by kind, so a reader can see *which*
/// rule failed without adding the kinds up first: "8 refusals" hides four
/// different fixes, and one column cannot tell them apart.
///
/// The counts are whole numbers. The log is byte-exact across architectures
/// (D69) and a float never is, so nothing here is one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusalCounts {
    /// One counter per kind, in the order of [`RefusalKind`].
    counts: [u64; RefusalKind::ALL.len()],
}

impl RefusalCounts {
    /// No refusals at all.
    pub fn new() -> RefusalCounts {
        RefusalCounts::default()
    }

    /// Count one refusal of `kind`.
    pub fn count(&mut self, kind: RefusalKind) {
        self.add(kind, 1);
    }

    /// Count `n` refusals of `kind`.
    pub fn add(&mut self, kind: RefusalKind, n: u64) {
        self.counts[kind as usize] = self.counts[kind as usize].saturating_add(n);
    }

    /// How many refusals of `kind` were counted.
    pub fn get(&self, kind: RefusalKind) -> u64 {
        self.counts[kind as usize]
    }

    /// How many refusals were counted, of every kind together.
    pub fn total(&self) -> u64 {
        self.counts.iter().copied().sum()
    }

    /// Whether no refusal was counted at all.
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Every kind with its count, in the order of the enum.
    ///
    /// A kind that never happened is still in the list: the telemetry writes
    /// a column for every kind, so a table's columns do not move when a kind
    /// starts happening.
    pub fn iter(&self) -> impl Iterator<Item = (RefusalKind, u64)> + '_ {
        RefusalKind::ALL
            .into_iter()
            .map(|kind| (kind, self.counts[kind as usize]))
    }

    /// Add the counts of `other` to this one.
    pub fn merge(&mut self, other: &RefusalCounts) {
        for (kind, count) in other.iter() {
            self.add(kind, count);
        }
    }
}

impl fmt::Display for RefusalCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (kind, count) in self.iter() {
            if count == 0 {
                continue;
            }
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{} {}", count, kind.as_str())?;
        }
        if first {
            f.write_str("none")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_name_of_its_own() {
        let mut names: Vec<&str> = RefusalKind::ALL.iter().map(|k| k.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two kinds share a name");
    }

    #[test]
    fn every_kind_is_in_the_list() {
        for kind in [
            RefusalKind::Timeout,
            RefusalKind::Unreachable,
            RefusalKind::Http,
            RefusalKind::Malformed,
            RefusalKind::InvalidPack,
            RefusalKind::LowConfidence,
            RefusalKind::MenuChanged,
            RefusalKind::Budget,
            RefusalKind::OffPlan,
            RefusalKind::Expired,
            RefusalKind::Superseded,
            RefusalKind::EscalationLate,
            RefusalKind::Panicked,
        ] {
            assert!(RefusalKind::ALL.contains(&kind), "{kind:?} is not in ALL");
        }
        assert_eq!(RefusalKind::ALL.len(), 13);
    }

    #[test]
    fn a_refusal_says_who_and_where() {
        let refused = Refused::new(40, 3, RefusalKind::Timeout);
        assert_eq!(
            refused.to_string(),
            "agent 3 on tick 40: the model did not answer before the deadline"
        );
        assert_eq!(refused.with_detail(503).detail, 503);
    }

    #[test]
    fn the_counts_keep_the_kinds_apart() {
        let mut counts = RefusalCounts::new();
        assert!(counts.is_empty());
        counts.count(RefusalKind::Timeout);
        counts.count(RefusalKind::Timeout);
        counts.count(RefusalKind::LowConfidence);
        assert_eq!(counts.get(RefusalKind::Timeout), 2);
        assert_eq!(counts.get(RefusalKind::LowConfidence), 1);
        assert_eq!(counts.get(RefusalKind::Budget), 0);
        assert_eq!(counts.total(), 3);
        assert_eq!(counts.iter().count(), RefusalKind::ALL.len());
    }

    #[test]
    fn the_counts_merge() {
        let mut a = RefusalCounts::new();
        a.count(RefusalKind::Timeout);
        let mut b = RefusalCounts::new();
        b.count(RefusalKind::Timeout);
        b.count(RefusalKind::OffPlan);
        a.merge(&b);
        assert_eq!(a.get(RefusalKind::Timeout), 2);
        assert_eq!(a.get(RefusalKind::OffPlan), 1);
        assert_eq!(a.total(), 3);
    }

    #[test]
    fn the_counts_write_the_kinds_that_happened() {
        let mut counts = RefusalCounts::new();
        assert_eq!(counts.to_string(), "none");
        counts.count(RefusalKind::Timeout);
        counts.add(RefusalKind::Budget, 2);
        assert_eq!(counts.to_string(), "1 timeout, 2 budget");
    }
}

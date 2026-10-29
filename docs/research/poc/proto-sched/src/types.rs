//! Decision vocabulary shared by the sequencer, the world and the log.

/// What a decision asks the world to do.
///
/// `ActionOn` is a one-shot "move one cell now" command, `Lane` feeds a
/// per-gate probability vector into the lane conditioner, `Intent` installs a
/// plan with a mandatory safe branch, and `Marker` is an opaque timestamp
/// beacon that only shows up in the log and in the world hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    ActionOn { action: u8 },
    Lane { gate: u8, p: [u8; 4] },
    Intent { ttl: u16, plan: Vec<(i16, i16)>, safe: Vec<(i16, i16)> },
    Marker(u32),
}

impl Kind {
    /// Wire tag used by the binary log encoding.
    pub fn tag(&self) -> u8 {
        match self {
            Kind::ActionOn { .. } => 0,
            Kind::Lane { .. } => 1,
            Kind::Intent { .. } => 2,
            Kind::Marker(_) => 3,
        }
    }

    /// Size in bytes of the tag-specific payload (header excluded).
    pub fn payload_len(&self) -> usize {
        match self {
            Kind::ActionOn { .. } => 1,
            Kind::Lane { .. } => 5,
            Kind::Intent { plan, safe, .. } => 6 + 4 * (plan.len() + safe.len()),
            Kind::Marker(_) => 4,
        }
    }
}

/// A decision as submitted by an agent, before the sequencer stamps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Tick the agent wants the decision to take effect on.
    pub apply_tick: u64,
    /// Agent that produced the decision.
    pub source: u16,
    /// Per-source monotonically increasing counter.
    pub seq: u32,
    pub kind: Kind,
}

impl Decision {
    pub fn action_on(apply_tick: u64, source: u16, seq: u32, action: u8) -> Self {
        Decision { apply_tick, source, seq, kind: Kind::ActionOn { action } }
    }

    pub fn lane(apply_tick: u64, source: u16, seq: u32, gate: u8, p: [u8; 4]) -> Self {
        Decision { apply_tick, source, seq, kind: Kind::Lane { gate, p } }
    }

    pub fn intent(
        apply_tick: u64,
        source: u16,
        seq: u32,
        ttl: u16,
        plan: Vec<(i16, i16)>,
        safe: Vec<(i16, i16)>,
    ) -> Self {
        Decision { apply_tick, source, seq, kind: Kind::Intent { ttl, plan, safe } }
    }

    pub fn marker(apply_tick: u64, source: u16, seq: u32, value: u32) -> Self {
        Decision { apply_tick, source, seq, kind: Kind::Marker(value) }
    }
}

/// A decision after the sequencer has stamped it: the effective apply tick
/// plus the lateness bookkeeping the sequencer kept for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamped {
    /// Effective tick the decision is applied on (rewritten when late).
    pub apply_tick: u64,
    pub source: u16,
    pub seq: u32,
    pub kind: Kind,
    /// Tick the decision arrived at the sequencer.
    pub arrival_tick: u64,
    /// True when the decision missed its requested tick.
    pub late: bool,
    /// Ticks the decision was late by (0 when on time).
    pub lateness: u32,
}

impl Stamped {
    /// The decision that will actually be applied on `apply_tick`.
    pub fn to_decision(&self) -> Decision {
        Decision {
            apply_tick: self.apply_tick,
            source: self.source,
            seq: self.seq,
            kind: self.kind.clone(),
        }
    }
}

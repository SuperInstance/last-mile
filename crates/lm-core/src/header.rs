//! The round header: the complete, canonical description of a round.
//!
//! Two nodes that agree on the header bytes agree on everything the
//! simulation will ever do — same seed, same parameters, same ruleset, same
//! roster and same reflex sources — so the header hash is the round's
//! identity.

use crate::decision::{Source, Tick};
use crate::hash::{Fnv64, StateHash};
use crate::params::{Params, ParamsV3};
use crate::talk::TalkParams;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The wire and log format version carried by every header.
///
/// Version 2 added the talk params to the header and the utterance to the
/// decision kinds. Both were **appended**, so every phase-1 header and every
/// phase-1 decision still decodes, and the phase-1 state hashes still hold.
///
/// Version 3 added the refusals to the log's segments. That one is not an
/// append: a postcard blob is read positionally, so a version-2 segment
/// cannot be read as a version-3 one. The old shape has its own type
/// ([`crate::log::SegmentV2`]) and is migrated on the way in, so an old log
/// still reads and still replays to the state hashes it recorded.
///
/// Version 4 added the paint's knobs to the params. That is an append too,
/// but a postcard blob is read positionally, so a params blob written
/// before the paint is short by four fields and cannot be read as the
/// current params. The old shape has its own type ([`crate::params::
/// ParamsV3`]) and is migrated on the way in, the same way: an old log
/// still reads, and the paint takes the defaults the current build ships.
///
/// Version 5 added the Vibe records a round carries to the header. The
/// values themselves were always there — a Vibe change writes a branchable
/// param, and the params travel in the header — so the new field is the
/// *provenance*: which sealed ledger lines put those values in force. The
/// field is hashed only when it holds a record, so a round with no Vibe
/// records in force keeps the hash it always had. The postcard shape moved,
/// so a version-4 header is read through [`HeaderV4`] and migrated, the
/// same way every older shape is.
pub const PROTO_VERSION: u16 = 5;

/// An agent's role inside its squad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Leader,
    Member,
}

impl Role {
    /// Canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Role::Leader => 0,
            Role::Member => 1,
        }
    }
}

impl StateHash for Role {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

/// One agent of the round: which squad it belongs to, its role, the voice it
/// speaks with, the harness commit its behaviour was frozen at (D27), and the
/// name of its reflex script in `Header::scripts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub agent: u16,
    pub squad: u8,
    pub role: Role,
    pub voice: String,
    pub harness_commit: String,
    pub reflex: String,
}

impl StateHash for RosterEntry {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.agent);
        h.write(&[self.squad]);
        self.role.hash_into(h);
        write_str(h, &self.voice);
        write_str(h, &self.harness_commit);
        write_str(h, &self.reflex);
    }
}

/// One change a branch makes when its tick comes (see [`Lineage`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineageChange {
    /// Run the reflex source `src` under the name `name` from the change's
    /// tick on, for every roster entry that names it.
    Reflex { name: String, src: String },
    /// Set one parameter field, from the mid-round whitelist
    /// ([`Params::set_branchable`](crate::params::Params::set_branchable)).
    Param { field: String, value: i64 },
    /// Stop applying the decisions of `source` from the change's tick on.
    DropDecisions { source: Source },
}

impl StateHash for LineageChange {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            LineageChange::Reflex { name, src } => {
                h.write(&[0]);
                write_str(h, name);
                write_str(h, src);
            }
            LineageChange::Param { field, value } => {
                h.write(&[1]);
                write_str(h, field);
                h.i64(*value);
            }
            LineageChange::DropDecisions { source } => {
                h.write(&[2]);
                source.hash_into(h);
            }
        }
    }
}

/// Where a round came from: a branch of an earlier round.
///
/// A branch is the permanent test of the design (`lm branch`): watch a
/// stretch of play, change one rule, re-run, and see the intended
/// difference. The lineage is how a branch's header says which round it
/// forked and what it changed, so a branch log replays from its own header
/// alone: the changes fire on their ticks during the replay, exactly as
/// they fired the first time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    /// The hash of the parent round's header.
    pub parent_header: u64,
    /// The hash of the parent round's last segment; 0 when the parent log
    /// recorded no segments.
    pub parent_head: u64,
    /// The tick the branch forks on: the changes below fire when it begins.
    pub at: Tick,
    /// The changes of this fork, applied in order.
    pub changes: Vec<LineageChange>,
    /// The parent's own lineage, when the parent was itself a branch: the
    /// chain records every ancestor back to the root round.
    #[serde(default)]
    pub prior: Option<Box<Lineage>>,
}

impl StateHash for Lineage {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.parent_header);
        h.u64(self.parent_head);
        h.u64(self.at);
        h.u32(self.changes.len() as u32);
        for change in &self.changes {
            change.hash_into(h);
        }
        match &self.prior {
            None => h.write(&[0]),
            Some(prior) => {
                h.write(&[1]);
                prior.hash_into(h);
            }
        }
    }
}

/// One Vibe record a round carries: a change adopted out of the Vibe
/// Dictionary, in force for this round (the sandbox, M2b).
///
/// The value itself rides in [`Header::params`] — the field the dictionary
/// entry names — so this record is the provenance: the ledger line (the
/// seal) the value was adopted by, and the entry's stable id. A replay
/// that finds a value it cannot explain can name the line that put it
/// there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VibeCarried {
    /// The ledger line (the seal) of the adopted proposal.
    pub seal: u64,
    /// The dictionary entry (`unit.vision`): what Bench names out loud.
    pub block: String,
    /// The value in force (`32`).
    pub value: i64,
}

impl StateHash for VibeCarried {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.seal);
        write_str(h, &self.block);
        h.i64(self.value);
    }
}

/// Everything a round is decided by, in declaration order for the hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub proto_version: u16,
    pub seed: u64,
    pub params: Params,
    pub ruleset: String,
    pub roster: Vec<RosterEntry>,
    /// Reflex scripts by name; `RosterEntry::reflex` indexes into this map.
    pub scripts: BTreeMap<String, String>,
    pub start_tick: u64,
    /// Where this round came from: `None` for a round that started at
    /// `start_tick`, `Some` for a branch of an earlier round. The field is
    /// hashed — and joined the round's identity — only when it is present,
    /// so every header hashed before it existed keeps its hash.
    #[serde(default)]
    pub lineage: Option<Lineage>,
    /// The round's talk rules: how far a voice carries, what a wall costs,
    /// how much airtime an agent has.
    ///
    /// The field was **appended** after `lineage`, so a header blob written
    /// before phase 4 still decodes through [`HeaderV1`] and migrates here.
    /// The talk rules are not branchable: they change *what the round is* —
    /// who hears what — so they join the header hash and stay out of
    /// `Params::BRANCH_FIELDS`.
    pub talk: TalkParams,
    /// The Vibe records this round carries: the changes adopted out of the
    /// Vibe Dictionary that are in force (M2b). The values live in `params`;
    /// these name the ledger lines that put them there, so a replay knows
    /// where each value came from.
    ///
    /// The field is hashed — and joins the round's identity — only when it
    /// holds a record, so every header hashed before it existed keeps its
    /// hash: a round with no Vibe records in force is exactly the round it
    /// always was.
    #[serde(default)]
    pub vibe: Vec<VibeCarried>,
}

impl Header {
    /// A standard round: default params, the `beacons` ruleset, 2 squads ×
    /// 4 agents (ids `squad_size * s` lead squad `s`), every agent running
    /// the `default` reflex from `reflex_src`.
    pub fn default_round(seed: u64, reflex_src: &str) -> Header {
        let params = Params::default();
        let size = u16::from(params.squad_size);
        let mut roster = Vec::new();
        for s in 0..u16::from(params.squads) {
            for i in 0..size {
                roster.push(RosterEntry {
                    agent: s * size + i,
                    squad: s as u8,
                    role: if i == 0 { Role::Leader } else { Role::Member },
                    voice: "default".to_owned(),
                    harness_commit: "none".to_owned(),
                    reflex: "default".to_owned(),
                });
            }
        }
        let mut scripts = BTreeMap::new();
        scripts.insert("default".to_owned(), reflex_src.to_owned());
        Header {
            proto_version: PROTO_VERSION,
            seed,
            params,
            ruleset: "beacons".to_owned(),
            roster,
            scripts,
            start_tick: 0,
            lineage: None,
            talk: TalkParams::default(),
            vibe: Vec::new(),
        }
    }

    /// The canonical hash of the header: the round's identity.
    pub fn hash(&self) -> u64 {
        self.state_hash()
    }
}

impl StateHash for Header {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.proto_version);
        h.u64(self.seed);
        self.params.hash_into(h);
        write_str(h, &self.ruleset);
        h.u32(self.roster.len() as u32);
        for r in &self.roster {
            r.hash_into(h);
        }
        h.u32(self.scripts.len() as u32);
        for (name, src) in &self.scripts {
            write_str(h, name);
            write_str(h, src);
        }
        h.u64(self.start_tick);
        // A branch's lineage joins the identity only when it is there: a
        // `None` adds no bytes, so the hash of every header serialized
        // before the field existed is unchanged.
        if let Some(lineage) = &self.lineage {
            h.write(&[1]);
            lineage.hash_into(h);
        }
        // The talk rules are the last field, and they are always there: the
        // talk params change what the round is, so they are part of its
        // identity.
        self.talk.hash_into(h);
        // The Vibe records join the identity only when there are any: an
        // empty list adds no bytes, so the hash of every header serialized
        // before the field existed is unchanged — a round with no Vibe
        // records in force is exactly the round it always was.
        if !self.vibe.is_empty() {
            h.u32(self.vibe.len() as u32);
            for record in &self.vibe {
                record.hash_into(h);
            }
        }
    }
}

/// The phase-1 header: the shape a version-1 log carries.
///
/// The fields are the phase-1 header's fields, in the same order and with the
/// same types, so a version-1 header blob decodes through this shim exactly
/// as it decoded through the phase-1 `Header` — and hashes exactly as it
/// hashed then. The params are the phase-1 shape too
/// ([`crate::params::ParamsV3`]): the paint's knobs were not there to decode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderV1 {
    pub proto_version: u16,
    pub seed: u64,
    pub params: ParamsV3,
    pub ruleset: String,
    pub roster: Vec<RosterEntry>,
    pub scripts: BTreeMap<String, String>,
    pub start_tick: u64,
    #[serde(default)]
    pub lineage: Option<Lineage>,
}

impl HeaderV1 {
    /// Migrate the header to the current version.
    ///
    /// The talk params take their defaults — a phase-1 round had no talk
    /// rules, and the default talk rules are the ones the plan pins — the
    /// paint takes its defaults, the Vibe records are none — a round from
    /// before the Vibe Dictionary carried none — and the version becomes the
    /// current one. Nothing else moves: the seed, the parameters, the
    /// roster, the scripts and the lineage are the round that was played.
    pub fn migrate(self) -> Header {
        Header {
            proto_version: PROTO_VERSION,
            seed: self.seed,
            params: self.params.migrate(),
            ruleset: self.ruleset,
            roster: self.roster,
            scripts: self.scripts,
            start_tick: self.start_tick,
            lineage: self.lineage,
            talk: TalkParams::default(),
            vibe: Vec::new(),
        }
    }
}

/// The version-3 header: the shape a version-3 log carries.
///
/// It is the current header — the talk rules are in it — with the params of
/// version 3 ([`crate::params::ParamsV3`]): the paint's knobs were appended
/// to the params in version 4, and a postcard blob is read positionally, so
/// a version-3 params blob is short by four fields and cannot be read as
/// the current params.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderV3 {
    pub proto_version: u16,
    pub seed: u64,
    pub params: ParamsV3,
    pub ruleset: String,
    pub roster: Vec<RosterEntry>,
    pub scripts: BTreeMap<String, String>,
    pub start_tick: u64,
    #[serde(default)]
    pub lineage: Option<Lineage>,
    pub talk: TalkParams,
}

impl HeaderV3 {
    /// Migrate the header to the current version.
    ///
    /// The paint takes its defaults — a version-3 round had no paint, and the
    /// defaults are the ones this build ships — the Vibe records are none,
    /// and the version becomes the current one. Nothing else moves: the
    /// seed, the parameters, the roster, the scripts, the lineage and the
    /// talk rules are the round that was played.
    pub fn migrate(self) -> Header {
        Header {
            proto_version: PROTO_VERSION,
            seed: self.seed,
            params: self.params.migrate(),
            ruleset: self.ruleset,
            roster: self.roster,
            scripts: self.scripts,
            start_tick: self.start_tick,
            lineage: self.lineage,
            talk: self.talk,
            vibe: Vec::new(),
        }
    }
}

/// The version-4 header: the shape a version-4 log carries.
///
/// It is the current header with the Vibe records missing: the records were
/// appended in version 5, and a postcard blob is read positionally, so a
/// version-4 header blob is short by the list and cannot be read as the
/// current header. This is the shape those logs carry, so they decode
/// through it and migrate into the current header — with no records in
/// force, because a round from before the dictionary carried none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderV4 {
    pub proto_version: u16,
    pub seed: u64,
    pub params: Params,
    pub ruleset: String,
    pub roster: Vec<RosterEntry>,
    pub scripts: BTreeMap<String, String>,
    pub start_tick: u64,
    #[serde(default)]
    pub lineage: Option<Lineage>,
    pub talk: TalkParams,
}

impl From<&Header> for HeaderV4 {
    /// The version-4 view of a header: every field as it is, with the Vibe
    /// records dropped, because they were not there.
    fn from(h: &Header) -> HeaderV4 {
        HeaderV4 {
            proto_version: 4,
            seed: h.seed,
            params: h.params,
            ruleset: h.ruleset.clone(),
            roster: h.roster.clone(),
            scripts: h.scripts.clone(),
            start_tick: h.start_tick,
            lineage: h.lineage.clone(),
            talk: h.talk.clone(),
        }
    }
}

impl HeaderV4 {
    /// Migrate the header to the current version.
    ///
    /// Nothing moves but the version: a version-4 round had the paint's
    /// knobs and the talk rules, and it carried no Vibe records. The
    /// parameters, the roster, the scripts, the lineage and the talk rules
    /// are the round that was played.
    pub fn migrate(self) -> Header {
        Header {
            proto_version: PROTO_VERSION,
            seed: self.seed,
            params: self.params,
            ruleset: self.ruleset,
            roster: self.roster,
            scripts: self.scripts,
            start_tick: self.start_tick,
            lineage: self.lineage,
            talk: self.talk,
            vibe: Vec::new(),
        }
    }
}

impl StateHash for HeaderV4 {
    fn hash_into(&self, h: &mut Fnv64) {
        // The version-4 header hashes as it hashed then: the Vibe records
        // were not there to hash.
        h.u16(self.proto_version);
        h.u64(self.seed);
        self.params.hash_into(h);
        write_str(h, &self.ruleset);
        h.u32(self.roster.len() as u32);
        for r in &self.roster {
            r.hash_into(h);
        }
        h.u32(self.scripts.len() as u32);
        for (name, src) in &self.scripts {
            write_str(h, name);
            write_str(h, src);
        }
        h.u64(self.start_tick);
        if let Some(lineage) = &self.lineage {
            h.write(&[1]);
            lineage.hash_into(h);
        }
        self.talk.hash_into(h);
    }
}

impl StateHash for HeaderV3 {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.proto_version);
        h.u64(self.seed);
        self.params.hash_into(h);
        write_str(h, &self.ruleset);
        h.u32(self.roster.len() as u32);
        for r in &self.roster {
            r.hash_into(h);
        }
        h.u32(self.scripts.len() as u32);
        for (name, src) in &self.scripts {
            write_str(h, name);
            write_str(h, src);
        }
        h.u64(self.start_tick);
        if let Some(lineage) = &self.lineage {
            h.write(&[1]);
            lineage.hash_into(h);
        }
        self.talk.hash_into(h);
    }
}

impl StateHash for HeaderV1 {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.proto_version);
        h.u64(self.seed);
        self.params.hash_into(h);
        write_str(h, &self.ruleset);
        h.u32(self.roster.len() as u32);
        for r in &self.roster {
            r.hash_into(h);
        }
        h.u32(self.scripts.len() as u32);
        for (name, src) in &self.scripts {
            write_str(h, name);
            write_str(h, src);
        }
        h.u64(self.start_tick);
        if let Some(lineage) = &self.lineage {
            h.write(&[1]);
            lineage.hash_into(h);
        }
    }
}

/// Absorb a string as a u32-LE length prefix followed by its bytes.
fn write_str(h: &mut Fnv64, s: &str) {
    h.u32(s.len() as u32);
    h.write(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::talk::AIRTIME_ON_EXHAUST_FREEZE;

    #[test]
    fn header_hash_stable_and_sensitive() {
        let a = Header::default_round(1, "fn reflex(v){}");
        assert_eq!(a.hash(), a.clone().hash());
        let mut b = a.clone();
        b.seed = 2;
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn default_round_roster() {
        let h = Header::default_round(1, "");
        assert_eq!(h.roster.len(), 8);
        assert_eq!(
            h.roster.iter().filter(|r| r.role == Role::Leader).count(),
            2
        );
    }

    #[test]
    fn default_round_shape() {
        let h = Header::default_round(7, "fn reflex(v){}");
        assert_eq!(h.proto_version, PROTO_VERSION);
        assert_eq!(h.seed, 7);
        assert_eq!(h.ruleset, "beacons");
        assert_eq!(h.start_tick, 0);
        assert_eq!(
            h.scripts,
            BTreeMap::from([("default".to_owned(), "fn reflex(v){}".to_owned())])
        );
        // Agent ids run 0..8 in squad order; ids 4*s are the leaders.
        for (i, r) in h.roster.iter().enumerate() {
            assert_eq!(r.agent, i as u16);
            assert_eq!(u16::from(r.squad), (i / 4) as u16);
            assert_eq!(r.role == Role::Leader, i % 4 == 0);
            assert_eq!(r.reflex, "default");
        }
    }

    #[test]
    fn hash_covers_every_field() {
        // Each field, mutated alone, must change the header hash.
        let base = Header::default_round(1, "src");
        let mut variants: Vec<Header> = Vec::new();
        let mut v = base.clone();
        v.proto_version = PROTO_VERSION + 1;
        variants.push(v);
        let mut v = base.clone();
        v.ruleset = "other".to_owned();
        variants.push(v);
        let mut v = base.clone();
        v.start_tick = 1;
        variants.push(v);
        let mut v = base.clone();
        v.scripts.insert("extra".to_owned(), "x".to_owned());
        variants.push(v);
        let mut v = base.clone();
        v.roster[0].harness_commit = "abc123".to_owned();
        variants.push(v);
        let mut v = base.clone();
        v.roster[0].role = Role::Member;
        variants.push(v);
        let mut v = base.clone();
        v.params.win_score = 1;
        variants.push(v);
        let mut v = base.clone();
        v.lineage = Some(Lineage {
            parent_header: 7,
            parent_head: 9,
            at: 5,
            changes: Vec::new(),
            prior: None,
        });
        variants.push(v);
        let mut v = base.clone();
        v.vibe = vec![VibeCarried {
            seal: 3,
            block: "unit.vision".to_owned(),
            value: 32,
        }];
        variants.push(v);
        for v in &variants {
            assert_ne!(base.hash(), v.hash());
        }
    }

    #[test]
    fn header_talk_is_hashed() {
        let base = Header::default_round(1, "src");
        let original = base.hash();

        // The talk rules change what the round *is* — who hears what — so
        // they are part of the round's identity.
        let mut changed = base.clone();
        changed.talk.range_voice_units = 51_200;
        assert_ne!(base.hash(), changed.hash());

        // And changing them twice returns to the original hash: the hash
        // follows the values, not the number of edits.
        let mut back = changed.clone();
        back.talk.range_voice_units = base.talk.range_voice_units;
        assert_eq!(original, back.hash());

        // Every talk field is covered.
        let mut variants: Vec<TalkParams> = Vec::new();
        let mut v = base.talk.clone();
        v.range_shout_units += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.range_whisper_units += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.radio_is_squad = !v.radio_is_squad;
        variants.push(v);
        let mut v = base.talk.clone();
        v.wall_loss_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.min_clarity_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.clarity_pitch_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.clarity_rate_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.act_clarity_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.intent_clarity_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.word_drop_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.radio_pitch_steps += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.radio_drop_q16 += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.base_wpm += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.beat_ticks += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.min_word_ticks += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.max_utterance_ticks += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.airtime_s += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.airtime_on_exhaust = AIRTIME_ON_EXHAUST_FREEZE.to_owned();
        variants.push(v);
        let mut v = base.talk.clone();
        v.order_uptake_window_ticks += 1;
        variants.push(v);
        let mut v = base.talk.clone();
        v.enemy_overhears_voice = !v.enemy_overhears_voice;
        variants.push(v);
        for talk in variants {
            let mut header = base.clone();
            header.talk = talk;
            assert_ne!(base.hash(), header.hash());
        }
    }

    #[test]
    fn the_default_round_carries_the_default_talk_rules() {
        let h = Header::default_round(1, "src");
        assert_eq!(h.talk, TalkParams::default());
        assert_eq!(h.proto_version, PROTO_VERSION);
        // Version 3 is the one that put the refusals in the log's segments,
        // version 4 the one that put the paint's knobs in the params, and
        // version 5 the one that put the Vibe records in the header. A
        // postcard blob is read positionally, so a blob written in the
        // older shape cannot be read in the newer one — the version had to
        // move with the shape, and the older shapes are read through their
        // own shims and migrated.
        assert_eq!(PROTO_VERSION, 5);
    }

    /// A round with no Vibe records in force hashes exactly as it did
    /// before the field existed: this is the promise that keeps the golden
    /// and every pinned header hash honest.
    #[test]
    fn vibe_records_join_the_hash_only_when_there_are_any() {
        let base = Header::default_round(1, "src");
        let original = base.hash();

        // No records: the same hash, whether the list is empty by default
        // or emptied by hand.
        let mut emptied = base.clone();
        emptied.vibe.clear();
        assert_eq!(original, emptied.hash());

        // One record: a new identity, covered field by field.
        let mut carried = base.clone();
        carried.vibe = vec![VibeCarried {
            seal: 0,
            block: "unit.vision".to_owned(),
            value: 32,
        }];
        assert_ne!(original, carried.hash());
        let mut other_seal = carried.clone();
        other_seal.vibe[0].seal = 1;
        assert_ne!(carried.hash(), other_seal.hash());
        let mut other_value = carried.clone();
        other_value.vibe[0].value = 48;
        assert_ne!(carried.hash(), other_value.hash());
        let mut other_block = carried.clone();
        other_block.vibe[0].block = "unit.speed".to_owned();
        assert_ne!(carried.hash(), other_block.hash());

        // And the header survives the log's codec, both with records and
        // without.
        for header in [&base, &carried] {
            let bytes = postcard::to_allocvec(header).expect("the header encodes");
            let back: Header = postcard::from_bytes(&bytes).expect("the header decodes");
            assert_eq!(&back, header);
        }
    }

    /// A version-4 header — the shape before the Vibe records — migrates
    /// into the current one carrying none, and its own hash is the hash it
    /// hashed then.
    #[test]
    fn a_v4_header_migrates_into_the_current_one() {
        let v4 = HeaderV4 {
            proto_version: 4,
            ..HeaderV4::from(&Header::default_round(7, "src"))
        };
        let migrated = v4.clone().migrate();
        assert_eq!(migrated.proto_version, PROTO_VERSION);
        assert_eq!(migrated.seed, 7);
        assert!(migrated.vibe.is_empty());
        assert_eq!(migrated.params, Header::default_round(7, "src").params);
        assert_ne!(v4.state_hash(), migrated.state_hash());

        // A version-4 blob is short by the Vibe list: it is refused as the
        // current shape, loudly, never read as a round that carried none.
        let old_bytes = postcard::to_allocvec(&v4).expect("the v4 header encodes");
        assert!(postcard::from_bytes::<Header>(&old_bytes).is_err());
    }

    #[test]
    fn a_v1_header_migrates_into_the_current_one() {
        let v1 = HeaderV1 {
            proto_version: 1,
            seed: 7,
            params: ParamsV3::from(&Params::default()),
            ruleset: "beacons".to_owned(),
            roster: Vec::new(),
            scripts: BTreeMap::new(),
            start_tick: 0,
            lineage: None,
        };
        let migrated = v1.clone().migrate();
        assert_eq!(migrated.proto_version, PROTO_VERSION);
        assert_eq!(migrated.talk, TalkParams::default());
        assert_eq!(migrated.seed, 7);
        // The migration is a new identity: the version and the talk rules
        // joined the header hash.
        assert_ne!(v1.state_hash(), migrated.state_hash());
    }

    #[test]
    fn lineage_joins_the_hash_only_when_present() {
        let a = Header::default_round(1, "fn reflex(v){}");
        let mut b = a.clone();
        // Two headers with no lineage hash alike...
        assert_eq!(a.hash(), b.hash());
        // ...and the lineage, when it appears, is the round's new identity.
        b.lineage = Some(Lineage {
            parent_header: 7,
            parent_head: 9,
            at: 5,
            changes: vec![LineageChange::Reflex {
                name: "default".to_owned(),
                src: "fn reflex(v){ #{ hold: true } }".to_owned(),
            }],
            prior: None,
        });
        assert_ne!(a.hash(), b.hash());
        // The lineage itself is covered field by field.
        let mut c = b.clone();
        c.lineage.as_mut().expect("just set").at = 6;
        assert_ne!(b.hash(), c.hash());

        // The header survives the log's codec, both with a lineage and
        // without one.
        let bytes = postcard::to_allocvec(&b).expect("the header encodes");
        let back: Header = postcard::from_bytes(&bytes).expect("the header decodes");
        assert_eq!(back, b);
        let bytes = postcard::to_allocvec(&a).expect("the header encodes");
        let back: Header = postcard::from_bytes(&bytes).expect("the header decodes");
        assert_eq!(back, a);

        // A header blob written before the field existed is refused, loudly
        // and by name when it is read as part of a log — postcard is not
        // self-describing, so the absent trailing `Option` is a short buffer,
        // never a silently missing lineage.
        #[derive(Serialize)]
        struct OldHeader<'a> {
            proto_version: u16,
            seed: u64,
            params: &'a Params,
            ruleset: &'a str,
            roster: &'a [RosterEntry],
            scripts: &'a BTreeMap<String, String>,
            start_tick: u64,
        }
        let old_bytes = postcard::to_allocvec(&OldHeader {
            proto_version: a.proto_version,
            seed: a.seed,
            params: &a.params,
            ruleset: &a.ruleset,
            roster: &a.roster,
            scripts: &a.scripts,
            start_tick: a.start_tick,
        })
        .expect("the old header encodes");
        assert!(postcard::from_bytes::<Header>(&old_bytes).is_err());
    }
}

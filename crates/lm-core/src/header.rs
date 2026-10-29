//! The round header: the complete, canonical description of a round.
//!
//! Two nodes that agree on the header bytes agree on everything the
//! simulation will ever do — same seed, same parameters, same ruleset, same
//! roster and same reflex sources — so the header hash is the round's
//! identity.

use crate::hash::{Fnv64, StateHash};
use crate::params::Params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The wire and log format version carried by every header.
pub const PROTO_VERSION: u16 = 1;

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
        v.proto_version = 2;
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
        for v in &variants {
            assert_ne!(base.hash(), v.hash());
        }
    }
}

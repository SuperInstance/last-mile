//! `lm-proto`: the messages that travel between last-mile nodes.
//!
//! Phase 1 ships the vocabulary and the codec only. The QUIC transport that
//! carries these bytes is phase 3 (D34: the ProArt dials out to the Thor and
//! the Orin), so nothing here opens a socket: a message goes in as bytes and
//! comes out as bytes.
//!
//! The encoding is postcard: compact, canonical and deterministic. Two nodes
//! that speak the same `PROTO_VERSION` agree on the exact bytes of every
//! message, because the round log and the state hashes are built from them.
//!
//! A message decodes only if it consumes the whole buffer, so a truncated,
//! padded or doubled frame is rejected instead of being half-believed.

#![deny(
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
#![forbid(unsafe_code)]

use lm_core::{Decision, Header, Refused, Stamped};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use lm_core::PROTO_VERSION;

/// What kind of node is on the other end of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// The host that owns the round (the ProArt): the authority for it.
    Host,
    /// An agent host (the Thor): runs a mirror of the simulation.
    AgentHost,
    /// The log archive (the Orin): ingests and verifies round logs.
    Archive,
    /// A helper that speaks the protocol but holds no round state.
    Tool,
}

/// One message on the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Msg {
    /// The first message of a connection: version, node kind, capabilities.
    Hello {
        /// The wire format version the node speaks.
        proto_version: u16,
        /// What kind of node this is.
        node: NodeKind,
        /// The optional features the node offers, by name.
        caps: Vec<String>,
    },
    /// The round header: the identity of the round.
    Header(Header),
    /// A decision as it was submitted.
    Decision(Decision),
    /// A decision as the sequencer stamped it.
    Stamped(Stamped),
    /// A state hash, for a mirror to compare itself against.
    StateHash {
        /// The tick the hash was taken on.
        tick: u64,
        /// The hash itself.
        hash: u64,
    },
    /// The last message of a connection.
    Bye {
        /// Why the connection is closing.
        reason: String,
    },
    /// A refusal: the model path's own record of why there was no answer.
    ///
    /// The refusals are part of the round's record (N2): a round that is
    /// pushed without them loses the reason for every silence, and a peer
    /// that cannot say *why* a mind said nothing cannot teach anything about
    /// it (D64).
    Refused(Refused),
}

/// Encode a message into its canonical wire bytes.
///
/// The encoding is postcard: the variant tag, then the fields in declaration
/// order, with varints for the integers. Two nodes that build the same
/// message build the same bytes.
pub fn encode(m: &Msg) -> Vec<u8> {
    postcard::to_allocvec(m).expect("encoding a message into a Vec cannot fail")
}

/// Decode a message from wire bytes.
///
/// The whole buffer must be the message: bytes left over are an error, so a
/// padded, doubled or truncated frame never decodes into a half-believed
/// message. Anything that is not a well-formed message is an error, never a
/// panic: the bytes come off a socket.
pub fn decode(b: &[u8]) -> Result<Msg, ProtoError> {
    let (m, rest) = postcard::take_from_bytes(b).map_err(ProtoError::Malformed)?;
    if !rest.is_empty() {
        return Err(ProtoError::TrailingBytes { len: rest.len() });
    }
    Ok(m)
}

/// Check the peer's opening message against the local protocol version.
///
/// A connection starts with a hello: anything else is refused, and a peer
/// that speaks another version is refused with both versions named, so the
/// log says which side is behind (spec §8).
pub fn check_hello(local: u16, remote: &Msg) -> Result<(), HandshakeError> {
    match remote {
        Msg::Hello { proto_version, .. } if *proto_version == local => Ok(()),
        Msg::Hello { proto_version, .. } => Err(HandshakeError::VersionMismatch {
            local,
            remote: *proto_version,
        }),
        _ => Err(HandshakeError::NotHello),
    }
}

/// Why a message could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ProtoError {
    /// The bytes are not a valid message.
    #[error("malformed message: {0}")]
    Malformed(postcard::Error),
    /// The message decoded, but the buffer had bytes left over.
    #[error("{len} trailing bytes after the message")]
    TrailingBytes {
        /// How many bytes were left over.
        len: usize,
    },
}

/// Why a handshake was refused.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum HandshakeError {
    /// The peer speaks a different protocol version.
    #[error("protocol version mismatch: local {local}, remote {remote}")]
    VersionMismatch {
        /// The version this node speaks.
        local: u16,
        /// The version the peer speaks.
        remote: u16,
    },
    /// The peer did not open with a hello.
    #[error("expected a hello message")]
    NotHello,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::{Action, Domain, Kind, Provenance, Rng, Source};

    /// A decision of every shape the codec has to carry.
    fn sample_decision() -> Decision {
        Decision {
            apply_tick: 7,
            source: Source::Agent(3),
            seq: 2,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Reflex,
        }
    }

    /// One message of every variant.
    fn every_variant() -> Vec<Msg> {
        vec![
            Msg::Hello {
                proto_version: PROTO_VERSION,
                node: NodeKind::Host,
                caps: vec!["beacons".to_owned(), "replay".to_owned()],
            },
            Msg::Header(Header::default_round(1, "fn reflex(v){}")),
            Msg::Decision(sample_decision()),
            Msg::Stamped(Stamped {
                d: sample_decision(),
                arrived: 5,
                tick: 7,
                late_by: 0,
            }),
            Msg::StateHash {
                tick: 42,
                hash: 0xdead_beef_1234_5678,
            },
            Msg::Bye {
                reason: "round over".to_owned(),
            },
        ]
    }

    #[test]
    fn roundtrip_every_variant() {
        for m in every_variant() {
            let bytes = encode(&m);
            let back = decode(&bytes).expect("a message decodes back to itself");
            assert_eq!(back, m);
        }
    }

    #[test]
    fn version_mismatch_named() {
        let remote = Msg::Hello {
            proto_version: 2,
            node: NodeKind::AgentHost,
            caps: Vec::new(),
        };
        let err = check_hello(1, &remote).expect_err("a version mismatch is refused");
        assert_eq!(
            err,
            HandshakeError::VersionMismatch {
                local: 1,
                remote: 2
            }
        );
        match err {
            HandshakeError::VersionMismatch { local, remote } => {
                assert_eq!(local, 1);
                assert_eq!(remote, 2);
            }
            other => panic!("expected a version mismatch, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_error_not_panic() {
        // Strings shorter than four bytes can be legitimate encodings (an
        // empty `Bye`, a zero `StateHash`), so the garbage starts where a real
        // message needs more bytes than that.
        let mut rng = Rng::stream(0x5EED_1234, Domain::Agent, 13);
        for i in 0..1_000u32 {
            let len = 4 + usize::try_from(rng.below(61)).expect("a small length");
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                bytes.push(u8::try_from(rng.next_u64() & 0xFF).expect("eight bits"));
            }
            let decoded = decode(&bytes);
            assert!(decoded.is_err(), "byte string {i} decoded as {decoded:?}");
        }
    }

    #[test]
    fn decision_bytes_small() {
        let d = Decision {
            apply_tick: 1234,
            source: Source::Agent(7),
            seq: 42,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Test,
        };
        let bytes = encode(&Msg::Decision(d.clone()));
        assert!(
            bytes.len() <= 24,
            "a decision encoded to {} bytes",
            bytes.len()
        );
        let bare = postcard::to_allocvec(&d).expect("a decision encodes");
        assert!(
            bare.len() <= 24,
            "a bare decision encoded to {} bytes",
            bare.len()
        );
    }

    #[test]
    fn hello_with_the_local_version_is_accepted() {
        let hello = Msg::Hello {
            proto_version: PROTO_VERSION,
            node: NodeKind::Host,
            caps: Vec::new(),
        };
        assert_eq!(check_hello(PROTO_VERSION, &hello), Ok(()));
    }

    #[test]
    fn a_non_hello_is_not_hello() {
        let err = check_hello(
            PROTO_VERSION,
            &Msg::Bye {
                reason: "bye".to_owned(),
            },
        )
        .expect_err("a bye is not a hello");
        assert_eq!(err, HandshakeError::NotHello);
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = encode(&Msg::Bye {
            reason: "bye".to_owned(),
        });
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(ProtoError::TrailingBytes { len: 1 }));
    }

    #[test]
    fn an_unknown_variant_is_rejected() {
        // 6 is not a `Msg` variant.
        assert!(decode(&[6, 0]).is_err());
    }
}

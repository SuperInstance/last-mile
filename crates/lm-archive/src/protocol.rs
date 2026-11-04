//! The wire: framed `lm_proto::Msg` over a TCP stream.
//!
//! The framing is the one `lm-core` already uses for its log blobs: a
//! little-endian `u32` length and the postcard bytes of the message
//! (`crates/lm-core/src/log.rs:281`). The archive adds **no codec of its
//! own**: `lm_proto::encode` and `lm_proto::decode` are the only things that
//! turn bytes into messages, and `decode` refuses a frame that carries bytes
//! after the message — so a padded, doubled or truncated frame is rejected
//! instead of being half-believed.
//!
//! ## What a push carries
//!
//! A round arrives as its header, the decisions the sequencer stamped and the
//! state hashes the ProArt recorded, and it ends with a `Msg::Bye`:
//!
//! - `Msg::Header` — once, first: the round's identity.
//! - `Msg::Stamped` — the decisions, as the sequencer stamped them.
//! - `Msg::StateHash` — the state hashes the ProArt's own round recorded.
//!   These are what the archive checks its replay against: the replay runs on
//!   the Orin, the hashes come from the ProArt, and a difference means the
//!   two machines did not play the same game.
//! - `Msg::Bye` — once, last.
//!
//! A `Msg::Hello` is accepted — it is the handshake the phase-3 transport
//! will use — and a `Msg::Decision`, a decision the sequencer never stamped,
//! is **refused by name**: this build does not know what to do with it, and
//! it will not guess.
//!
//! ## The ingest
//!
//! [`handle_connection`] reads the frames until the `Bye`, rebuilds the round
//! log, stores it atomically and (when the config says so) verifies it. The
//! verdict goes back over the same socket: a `Msg::StateHash` when the round
//! verified clean, a `Msg::Bye` with the reason when it did not. A mismatch
//! is reported, never swallowed.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use lm_core::decision::{Stamped, Tick};
use lm_core::header::Header;
use lm_core::log::{RoundLog, Segment};
use lm_proto::{Msg, ProtoError};

use crate::store::{self, Store};
use crate::verify::{self, VerifyReport};
use crate::ArchiveConfig;

/// A frame that was read: the message, and how many bytes its body had.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The message the frame carried.
    pub msg: Msg,
    /// The size of the frame's body, in bytes.
    pub len: u64,
}

/// Why a frame could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The peer closed the connection between two frames.
    Closed,
    /// The frame is longer than the cap.
    TooLarge {
        /// The size the frame claimed.
        len: u64,
        /// The cap.
        max: u64,
    },
    /// The frame ended in the middle.
    Truncated {
        /// How many bytes the frame wanted.
        want: usize,
        /// How many bytes it got.
        got: usize,
    },
    /// The frame's bytes are not a message.
    Decode(ProtoError),
    /// The peer said nothing for the whole read timeout.
    ///
    /// A peer that opens a socket and then goes quiet must not pin a thread
    /// of the service forever, so the connection is dropped when the timeout
    /// is up — and the reason is named.
    TimedOut,
    /// The socket failed.
    Io(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Closed => write!(f, "the peer closed the connection"),
            FrameError::TooLarge { len, max } => {
                write!(f, "a frame of {len} bytes is over the {max} byte cap")
            }
            FrameError::Truncated { want, got } => {
                write!(f, "the frame ended early: {got} of {want} bytes")
            }
            FrameError::Decode(ProtoError::TrailingBytes { len }) => {
                write!(f, "the frame has {len} trailing bytes after the message")
            }
            FrameError::Decode(ProtoError::Malformed(err)) => {
                write!(f, "the frame is not a message: {err}")
            }
            FrameError::TimedOut => {
                write!(f, "the peer said nothing for the whole read timeout")
            }
            FrameError::Io(message) => write!(f, "the socket failed: {message}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Why a push was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestError {
    /// A frame could not be read.
    Frame(FrameError),
    /// The push never carried a header.
    NoHeader,
    /// The push never said goodbye.
    NoBye,
    /// This build does not understand this message.
    Unknown {
        /// What the message was.
        what: &'static str,
    },
    /// The peer speaks another protocol version.
    VersionMismatch {
        /// The version this build speaks.
        local: u16,
        /// The version the peer claims.
        remote: u16,
    },
    /// The message arrived in the wrong order.
    OutOfOrder {
        /// What the message was.
        what: &'static str,
    },
    /// The round is bigger than the cap.
    TooLarge {
        /// The size of the round.
        len: u64,
        /// The cap.
        max: u64,
    },
    /// The round could not be rebuilt from what the wire carried.
    Reconstruct(ReconstructError),
    /// The store could not be written.
    Store(String),
    /// The socket failed.
    Io(String),
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngestError::Frame(err) => write!(f, "{err}"),
            IngestError::NoHeader => write!(f, "the push carried no header"),
            IngestError::NoBye => write!(f, "the push ended without a goodbye"),
            IngestError::Unknown { what } => {
                write!(f, "this build does not understand {what}")
            }
            IngestError::VersionMismatch { local, remote } => {
                write!(
                    f,
                    "protocol version mismatch: local {local}, remote {remote}"
                )
            }
            IngestError::OutOfOrder { what } => write!(f, "{what} arrived out of order"),
            IngestError::TooLarge { len, max } => {
                write!(f, "the round is {len} bytes, over the {max} byte cap")
            }
            IngestError::Reconstruct(err) => write!(f, "{err}"),
            IngestError::Store(message) => write!(f, "the store: {message}"),
            IngestError::Io(message) => write!(f, "the socket failed: {message}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<FrameError> for IngestError {
    fn from(err: FrameError) -> Self {
        IngestError::Frame(err)
    }
}

/// What an ingest did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestReport {
    /// The name the round was stored under.
    pub name: String,
    /// The file the round was stored in.
    pub path: PathBuf,
    /// How many decisions the round carried.
    pub decisions: usize,
    /// How many state hashes the round carried.
    pub hashes: usize,
    /// The verification of the round, when the service verifies on arrival.
    pub verified: Option<VerifyReport>,
}

/// Write a frame: a little-endian `u32` length and the postcard bytes.
pub fn write_frame(w: &mut impl Write, msg: &Msg) -> io::Result<()> {
    let body = lm_proto::encode(msg);
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the frame is too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    Ok(())
}

/// Read one frame.
///
/// The length prefix is read first and checked against `max_bytes`, so a
/// frame that claims to be enormous is refused **before** its body is
/// buffered: a push cannot make the archive allocate what it says it is.
pub fn read_frame(r: &mut impl Read, max_bytes: u64) -> Result<Frame, FrameError> {
    let mut prefix = [0u8; 4];
    let got = read_up_to(r, &mut prefix)?;
    if got == 0 {
        return Err(FrameError::Closed);
    }
    if got < prefix.len() {
        return Err(FrameError::Truncated {
            want: prefix.len(),
            got,
        });
    }
    let len = u64::from(u32::from_le_bytes(prefix));
    if len > max_bytes {
        return Err(FrameError::TooLarge {
            len,
            max: max_bytes,
        });
    }

    let mut body = vec![0u8; usize::try_from(len).unwrap_or(usize::MAX)];
    let got = read_up_to(r, &mut body)?;
    if got < body.len() {
        return Err(FrameError::Truncated {
            want: body.len(),
            got,
        });
    }
    let msg = lm_proto::decode(&body).map_err(FrameError::Decode)?;
    Ok(Frame { msg, len })
}

/// Read as much as the buffer holds, and say how much it got.
fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> Result<usize, FrameError> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(read) => got += read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            // The read timeout is up: the peer is not talking. It is named,
            // so the log says a silent peer was dropped rather than a
            // mysterious socket failure.
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                return Err(FrameError::TimedOut)
            }
            Err(err) => return Err(FrameError::Io(err.to_string())),
        }
    }
    Ok(got)
}

/// The longest round the archive will rebuild.
///
/// A round's tick count is bounded by its own time limit, and the longest
/// round anyone plays is a few thousand ticks. A peer that claims a round of
/// a million ticks or more is refused by name rather than believed: the
/// archive must not spend a minute of CPU on one push.
pub const MAX_ROUND_TICKS: u64 = 1 << 20;

/// Why a round could not be rebuilt from what the wire carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconstructError {
    /// The round claims more ticks than the archive will rebuild.
    TooLong {
        /// How many ticks the round claims.
        ticks: u64,
        /// The cap.
        max: u64,
    },
}

impl fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReconstructError::TooLong { ticks, max } => {
                write!(
                    f,
                    "the round claims {ticks} ticks, over the {max} the archive rebuilds"
                )
            }
        }
    }
}

impl std::error::Error for ReconstructError {}

/// Rebuild the round log from what the wire carried.
///
/// The segments are built the way `lm_core::log::Recorder` builds them: the
/// ticks of the round in order, grouped into runs of `segment_ticks` ticks,
/// each segment sealed with the hash of the one before it and the first one
/// sealed with the header's hash. A push of a round therefore rebuilds into
/// exactly the log the ProArt recorded — byte for byte — whenever the push
/// carried the round whole.
///
/// The round's ticks run from the header's `start_tick` to the last tick the
/// push mentioned: the last decision or the last state hash. The state hash
/// of a round is taken every `hash_interval_ticks` ticks, so the last hash of
/// a round that ran a whole number of intervals is the round's last tick, and
/// the rebuilt log covers the round exactly.
///
/// A round that claims more than [`MAX_ROUND_TICKS`] ticks is refused: the
/// archive does not rebuild a round it cannot play, and it does not spend a
/// minute of the Orin's CPU on a peer that says so.
pub fn reconstruct(
    header: &Header,
    decisions: &[Stamped],
    hashes: &[(Tick, u64)],
) -> Result<RoundLog, ReconstructError> {
    let mut by_tick: BTreeMap<Tick, Vec<Stamped>> = BTreeMap::new();
    for stamped in decisions {
        by_tick
            .entry(stamped.tick)
            .or_default()
            .push(stamped.clone());
    }
    let mut hash_by_tick: BTreeMap<Tick, Vec<u64>> = BTreeMap::new();
    for (tick, hash) in hashes {
        hash_by_tick.entry(*tick).or_default().push(*hash);
    }

    let last = by_tick
        .keys()
        .chain(hash_by_tick.keys())
        .copied()
        .max()
        .unwrap_or(header.start_tick);
    if last < header.start_tick {
        return Ok(RoundLog {
            header: header.clone(),
            segments: Vec::new(),
        });
    }
    let ticks = last.saturating_sub(header.start_tick).saturating_add(1);
    if ticks > MAX_ROUND_TICKS {
        return Err(ReconstructError::TooLong {
            ticks,
            max: MAX_ROUND_TICKS,
        });
    }

    let per_segment = header.params.segment_ticks;
    let mut segments: Vec<Segment> = Vec::new();
    let mut current: Option<Segment> = None;
    let mut ticks_in_segment: u64 = 0;

    for tick in header.start_tick..=last {
        if current.is_none() {
            current = Some(Segment {
                first_tick: tick,
                last_tick: tick,
                decisions: Vec::new(),
                hashes: Vec::new(),
                prev: 0,
                hash: 0,
            });
            ticks_in_segment = 0;
        }
        let segment = current.as_mut().expect("the segment is open");
        segment.last_tick = tick;
        if let Some(applied) = by_tick.get(&tick) {
            segment.decisions.extend(applied.iter().cloned());
        }
        if let Some(hashes) = hash_by_tick.get(&tick) {
            for hash in hashes {
                segment.hashes.push((tick, *hash));
            }
        }
        ticks_in_segment += 1;
        if per_segment > 0 && ticks_in_segment >= u64::from(per_segment) {
            close_segment(&mut segments, &mut current, header);
        }
    }
    close_segment(&mut segments, &mut current, header);

    Ok(RoundLog {
        header: header.clone(),
        segments,
    })
}

/// Seal the segment being filled and start the next one.
fn close_segment(segments: &mut Vec<Segment>, current: &mut Option<Segment>, header: &Header) {
    let Some(mut segment) = current.take() else {
        return;
    };
    segment.prev = segments.last().map_or(header.hash(), |s| s.hash);
    segment.hash = segment.compute_hash();
    segments.push(segment);
}

/// Store a round and count it in the service's state.
///
/// The round is written atomically, so a half-written round is never visible
/// to the verifier, and the state file counts the round as taken in.
pub fn store_round(store: &Store, log: &RoundLog, name: &str) -> Result<PathBuf, IngestError> {
    let path = store
        .save_round(name, log)
        .map_err(|err| IngestError::Store(err.to_string()))?;

    let mut state = store.read_state();
    state.rounds_ingested += 1;
    store
        .write_state(&state)
        .map_err(|err| IngestError::Store(err.to_string()))?;

    Ok(path)
}

/// Record a verdict in the service's state.
///
/// A round that verified clean is counted, and named; a round that did not
/// is counted as a failure. The state is the service's own bookkeeping, and
/// it is written the same way the rounds are.
pub fn record_verdict(store: &Store, name: &str, report: &VerifyReport) -> Result<(), IngestError> {
    let mut state = store.read_state();
    if report.ok {
        state.rounds_verified += 1;
        state.last_verified_round = Some(name.to_owned());
    } else {
        state.rounds_failed += 1;
    }
    store
        .write_state(&state)
        .map_err(|err| IngestError::Store(err.to_string()))?;
    Ok(())
}

/// Store a round, verify it and write the report.
///
/// This is the whole ingest, minus the wire: the round is written atomically,
/// it is replayed when the service verifies on arrival, and the report and
/// the service's state are written the same way the round was.
pub fn ingest_round(
    store: &Store,
    log: &RoundLog,
    name: &str,
    verify_on_arrival: bool,
) -> Result<IngestReport, IngestError> {
    let path = store_round(store, log, name)?;

    let decisions: usize = log.segments.iter().map(|s| s.decisions.len()).sum();
    let hashes: usize = log.segments.iter().map(|s| s.hashes.len()).sum();

    let verified = if verify_on_arrival {
        // The chain of the log we just built is checked before the replay:
        // a log that does not chain is not replayed at all.
        let report = match log.verify() {
            Ok(()) => verify::verify_log(log),
            Err(err) => VerifyReport::failed(verify::VerifyError::Log(err), log.header.hash()),
        };
        store
            .save_report(name, &report)
            .map_err(|err| IngestError::Store(err.to_string()))?;
        record_verdict(store, name, &report)?;
        Some(report)
    } else {
        None
    };

    Ok(IngestReport {
        name: name.to_owned(),
        path,
        decisions,
        hashes,
        verified,
    })
}

/// Read a push off the wire: the header, the decisions, the hashes and the
/// goodbye.
///
/// The round is rebuilt but **not** stored: what happens to it is the
/// caller's business. The service stores it and hands the replay to a
/// worker; the command-line ingest stores it and replays it right here.
///
/// Every way a push can be wrong is refused **by name**: a second header, a
/// decision before the header, a decision from before the round started, a
/// message this build does not understand, a peer that speaks another
/// version, a round that claims more ticks than the archive will rebuild.
pub fn read_push(reader: &mut impl Read, cfg: &ArchiveConfig) -> Result<RoundLog, IngestError> {
    let mut header: Option<Header> = None;
    let mut decisions: Vec<Stamped> = Vec::new();
    let mut hashes: Vec<(Tick, u64)> = Vec::new();
    let mut total: u64 = 0;

    loop {
        let frame = match read_frame(reader, cfg.max_log_bytes) {
            Ok(frame) => frame,
            // The peer hung up between the frames: the round is incomplete,
            // and the push is refused for exactly that reason.
            Err(FrameError::Closed) => return Err(IngestError::NoBye),
            Err(err) => return Err(err.into()),
        };
        total = total.saturating_add(frame.len);
        if total > cfg.max_log_bytes {
            return Err(IngestError::TooLarge {
                len: total,
                max: cfg.max_log_bytes,
            });
        }

        match frame.msg {
            // The handshake of the phase-3 transport: accepted, and its
            // version checked against the one this build speaks.
            Msg::Hello { proto_version, .. } => {
                if proto_version != lm_proto::PROTO_VERSION {
                    return Err(IngestError::VersionMismatch {
                        local: lm_proto::PROTO_VERSION,
                        remote: proto_version,
                    });
                }
            }
            Msg::Header(next) => {
                if header.is_some() {
                    return Err(IngestError::OutOfOrder {
                        what: "a second header",
                    });
                }
                header = Some(next);
            }
            Msg::Stamped(stamped) => {
                if header.is_none() {
                    return Err(IngestError::OutOfOrder {
                        what: "a stamped decision before the header",
                    });
                }
                decisions.push(stamped);
            }
            Msg::StateHash { tick, hash } => {
                if header.is_none() {
                    return Err(IngestError::OutOfOrder {
                        what: "a state hash before the header",
                    });
                }
                hashes.push((tick, hash));
            }
            // A decision the sequencer never stamped: this build does not
            // know what to do with it, and it will not guess.
            Msg::Decision(_) => {
                return Err(IngestError::Unknown {
                    what: "an unstamped decision",
                });
            }
            Msg::Bye { .. } => {
                // The only way out of the loop: the push is over.
                break;
            }
        }
    }

    let Some(header) = header else {
        return Err(IngestError::NoHeader);
    };
    // A decision that lands before the round started cannot be replayed: the
    // sequencer never stamps one, and the archive will not quietly drop it.
    if decisions
        .iter()
        .any(|stamped| stamped.tick < header.start_tick)
    {
        return Err(IngestError::OutOfOrder {
            what: "a decision from before the round started",
        });
    }

    reconstruct(&header, &decisions, &hashes).map_err(IngestError::Reconstruct)
}

/// Write the verdict of a push back to the peer.
///
/// A round that verified clean is answered with a `Msg::StateHash` and the
/// round's last hash — the ProArt's HUD shows that. A round that did not is
/// answered with a `Msg::Bye` that says why: a mismatch is reported, never
/// swallowed.
pub fn write_verdict(
    writer: &mut impl Write,
    verified: Option<&VerifyReport>,
    log: &RoundLog,
    name: &str,
) -> io::Result<()> {
    let answer = match verified {
        Some(report) if report.ok => match log.hashes().last() {
            Some((tick, hash)) => Msg::StateHash {
                tick: *tick,
                hash: *hash,
            },
            None => Msg::Bye {
                reason: "the round carried no state hash".to_owned(),
            },
        },
        Some(report) => Msg::Bye {
            reason: report.summary(),
        },
        None => Msg::Bye {
            reason: format!("stored as {name}"),
        },
    };
    write_frame(writer, &answer)
}

/// Take a pushed round in: read it, store it, verify it and answer.
///
/// The connection is read until the peer's `Msg::Bye`. The round is rebuilt,
/// stored atomically and verified; the verdict goes back over the same
/// socket: a `Msg::StateHash` with the round's last hash when it verified
/// clean, a `Msg::Bye` with the reason when it did not.
///
/// This is the whole ingest on one thread. The service does it in two: the
/// round is stored here and the replay is handed to a worker, so a slow
/// round never holds the accept loop up.
pub fn handle_connection(
    stream: TcpStream,
    cfg: &ArchiveConfig,
) -> Result<IngestReport, IngestError> {
    let mut writer = stream
        .try_clone()
        .map_err(|err| IngestError::Io(err.to_string()))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|err| IngestError::Io(err.to_string()))?,
    );

    let log = match read_push(&mut reader, cfg) {
        Ok(log) => log,
        Err(err) => return Err(refuse(&mut writer, err)),
    };

    let store = cfg.store();
    let name = store.free_round_name(log.header.seed, store::now_unix_secs());
    let report = match ingest_round(&store, &log, &name, cfg.verify_on_arrival) {
        Ok(report) => report,
        // The peer is told why the round did not go in.
        Err(err) => return Err(refuse(&mut writer, err)),
    };
    // The store keeps as many rounds as the config says; 0 keeps them all.
    if cfg.retain_rounds > 0 {
        let _ = store.prune(cfg.retain_rounds);
    }

    // The verdict, back over the wire.
    let _ = write_verdict(&mut writer, report.verified.as_ref(), &log, &report.name);

    Ok(report)
}

/// Tell the peer why the push was refused, and hand the reason back.
///
/// The reason goes over the wire as a `Msg::Bye`, so the peer knows *why* it
/// was turned away and not merely that it was.
pub fn refuse(writer: &mut impl Write, err: IngestError) -> IngestError {
    let _ = write_frame(
        writer,
        &Msg::Bye {
            reason: err.to_string(),
        },
    );
    err
}

/// Why a push did not get through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushError {
    /// The archive could not be reached at all.
    Connect(String),
    /// The socket failed.
    Io(String),
    /// The archive refused the push, and said why.
    Refused(String),
}

impl fmt::Display for PushError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PushError::Connect(message) => write!(f, "the archive could not be reached: {message}"),
            PushError::Io(message) => write!(f, "the socket failed: {message}"),
            PushError::Refused(reason) => write!(f, "the archive refused the push: {reason}"),
        }
    }
}

impl std::error::Error for PushError {}

/// The biggest frame the push client will read back.
///
/// The verdict is one small message, but the cap keeps a confused peer from
/// making the client allocate whatever it says.
const PUSH_FRAME_CAP: u64 = 64 * 1024 * 1024;

/// How long the push client waits for the verdict, in seconds.
///
/// The verdict comes back after the replay, so this is generous: the point
/// is not to hang forever, not to cut a slow replay short.
const PUSH_TIMEOUT_S: u64 = 300;

/// Push a round to an archive, and read the verdict.
///
/// This is the ProArt's side of the wire: the same frames the service reads,
/// in the same order, with the same goodbye. The verdict comes back over the
/// same socket — a `Msg::StateHash` when the round verified clean, a
/// `Msg::Bye` that says why not.
///
/// The archive answers with a greeting first, so the push waits for it: a
/// connection to something that is not an archive is caught before a round
/// is half sent.
pub fn push_round(addr: &str, log: &RoundLog) -> Result<Msg, PushError> {
    let mut stream = TcpStream::connect(addr).map_err(|err| PushError::Connect(err.to_string()))?;
    stream
        .set_nodelay(true)
        .map_err(|err| PushError::Io(err.to_string()))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(PUSH_TIMEOUT_S)))
        .map_err(|err| PushError::Io(err.to_string()))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|err| PushError::Io(err.to_string()))?,
    );

    // The handshake, then the round itself.
    let mut send = |msg: &Msg| -> Result<(), PushError> {
        write_frame(&mut stream, msg).map_err(|err| PushError::Io(err.to_string()))
    };
    send(&Msg::Hello {
        proto_version: lm_proto::PROTO_VERSION,
        node: lm_proto::NodeKind::Host,
        caps: Vec::new(),
    })?;
    send(&Msg::Header(log.header.clone()))?;
    for segment in &log.segments {
        for stamped in &segment.decisions {
            send(&Msg::Stamped(stamped.clone()))?;
        }
    }
    for (tick, hash) in log.hashes() {
        send(&Msg::StateHash { tick, hash })?;
    }
    send(&Msg::Bye {
        reason: "round over".to_owned(),
    })?;
    stream
        .flush()
        .map_err(|err| PushError::Io(err.to_string()))?;

    // The greeting, then the verdict.
    let greeting =
        read_frame(&mut reader, PUSH_FRAME_CAP).map_err(|err| PushError::Io(err.to_string()))?;
    if !matches!(greeting.msg, Msg::Hello { .. }) {
        return Err(PushError::Refused(
            "the archive did not greet the push".to_owned(),
        ));
    }
    let verdict =
        read_frame(&mut reader, PUSH_FRAME_CAP).map_err(|err| PushError::Io(err.to_string()))?;
    match verdict.msg {
        Msg::Bye { reason } => Err(PushError::Refused(reason)),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{round, Scratch};
    use lm_core::header::Header;
    use std::net::TcpListener;

    /// A config that keeps everything in a scratch directory.
    fn config(scratch: &Scratch) -> ArchiveConfig {
        ArchiveConfig {
            store_dir: scratch.path().to_path_buf(),
            ..ArchiveConfig::default()
        }
    }

    /// The far end of a connection: the test writes the frames and reads the
    /// answer, while the archive does its work on its own thread.
    fn peer(
        cfg: &ArchiveConfig,
    ) -> (
        TcpStream,
        std::thread::JoinHandle<Result<IngestReport, IngestError>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let addr = listener.local_addr().expect("the listener's address");
        let cfg = cfg.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("a connection");
            handle_connection(stream, &cfg)
        });
        let client = TcpStream::connect(addr).expect("the client connects");
        (client, handle)
    }

    /// Push a round: the header, the decisions, the hashes, the goodbye.
    fn push(client: &mut TcpStream, log: &RoundLog) {
        write_frame(client, &Msg::Header(log.header.clone())).expect("the header goes out");
        for segment in &log.segments {
            for stamped in &segment.decisions {
                write_frame(client, &Msg::Stamped(stamped.clone())).expect("the decision goes out");
            }
        }
        for (tick, hash) in log.hashes() {
            write_frame(client, &Msg::StateHash { tick, hash }).expect("the hash goes out");
        }
        write_frame(
            client,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        client.flush().expect("the frames are flushed");
    }

    #[test]
    fn framed_roundtrip() {
        let scratch = Scratch::new("framed-roundtrip");
        let cfg = config(&scratch);
        let log = round(1, 300);

        let (mut client, handle) = peer(&cfg);
        push(&mut client, &log);

        let answer = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
        let report = handle
            .join()
            .expect("the thread")
            .expect("the round was taken in");

        // The stored round is the round that was pushed, byte for byte.
        let mut file = std::fs::File::open(&report.path).expect("the round is on disk");
        let stored = RoundLog::read(&mut file).expect("the round reads back");
        assert_eq!(stored, log);

        // And the verdict says the round verified clean.
        let verified = report.verified.expect("the round was verified");
        assert!(verified.ok, "{}", verified.summary());
        assert_eq!(verified.hashes_checked, log.hashes().len());
        assert!(matches!(answer.msg, Msg::StateHash { .. }));
    }

    #[test]
    fn the_rebuild_is_the_log_the_round_recorded() {
        let log = round(1, 300);
        let rebuilt = reconstruct(&log.header, &[], &log.hashes()).expect("the round rebuilds");
        assert_eq!(rebuilt, log);
    }

    #[test]
    fn trailing_bytes_rejected() {
        let scratch = Scratch::new("trailing");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        // A message with a byte glued onto the end of it.
        let mut body = lm_proto::encode(&Msg::Bye {
            reason: "bye".to_owned(),
        });
        body.push(0);
        let len = u32::try_from(body.len()).expect("the length fits");
        client
            .write_all(&len.to_le_bytes())
            .expect("the length goes out");
        client.write_all(&body).expect("the body goes out");
        client.flush().expect("the frame is flushed");

        let answer = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::Frame(FrameError::Decode(ProtoError::TrailingBytes { .. }))
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("trailing bytes"));
        // The peer is told why.
        match answer.msg {
            Msg::Bye { reason } => assert!(reason.contains("trailing bytes"), "{reason}"),
            other => panic!("expected a goodbye, got {other:?}"),
        }
    }

    #[test]
    fn max_log_bytes_is_enforced() {
        let scratch = Scratch::new("too-large");
        let mut cfg = config(&scratch);
        cfg.max_log_bytes = 1024;

        let (mut client, handle) = peer(&cfg);

        // A frame that claims to be a hundred megabytes. The archive has to
        // refuse it on the length alone: it must not wait for the body.
        let claimed = 100 * 1024 * 1024u32;
        client
            .write_all(&claimed.to_le_bytes())
            .expect("the length goes out");
        client.flush().expect("the length is flushed");
        // The body never arrives.
        let _ = client.shutdown(std::net::Shutdown::Write);

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        match err {
            IngestError::Frame(FrameError::TooLarge { len, max }) => {
                assert_eq!(len, u64::from(claimed));
                assert_eq!(max, 1024);
            }
            other => panic!("expected a refusal for the size, got {other:?}"),
        }
    }

    #[test]
    fn unknown_message_rejected() {
        let scratch = Scratch::new("unknown");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        // A decision the sequencer never stamped.
        let decision = lm_core::Decision {
            apply_tick: 3,
            source: lm_core::Source::Agent(0),
            seq: 1,
            kind: lm_core::Kind::ActionOn(lm_core::Action::Hold),
            provenance: lm_core::Provenance::Test,
        };
        write_frame(&mut client, &Msg::Decision(decision)).expect("the frame goes out");
        client.flush().expect("the frame is flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::Unknown {
                    what: "an unstamped decision"
                }
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("does not understand"));
    }

    #[test]
    fn a_frame_that_is_not_a_message_is_refused() {
        let scratch = Scratch::new("not-a-message");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        // A `Msg` variant this build does not have.
        let body = [6u8, 0];
        client
            .write_all(&2u32.to_le_bytes())
            .expect("the length goes out");
        client.write_all(&body).expect("the body goes out");
        client.flush().expect("the frame is flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::Frame(FrameError::Decode(ProtoError::Malformed(_)))
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_push_without_a_header_is_refused() {
        let scratch = Scratch::new("no-header");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        write_frame(
            &mut client,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        client.flush().expect("the frame is flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(matches!(err, IngestError::NoHeader), "{err:?}");
    }

    #[test]
    fn a_push_without_a_goodbye_is_refused() {
        let scratch = Scratch::new("no-bye");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        write_frame(
            &mut client,
            &Msg::Header(Header::default_round(1, "fn reflex(v) {}")),
        )
        .expect("the header goes out");
        client.flush().expect("the frame is flushed");
        let _ = client.shutdown(std::net::Shutdown::Write);

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(matches!(err, IngestError::NoBye), "{err:?}");
    }

    #[test]
    fn a_tampered_round_is_stored_and_reported() {
        let scratch = Scratch::new("tampered");
        let cfg = config(&scratch);
        let log = round(1, 300);
        // The push carries a hash the round never produced.
        let mut hashes = log.hashes();
        let (tick, hash) = hashes[3];
        hashes[3] = (tick, hash ^ 0x00ff_ffff_ffff_ffff);

        let (mut client, handle) = peer(&cfg);
        write_frame(&mut client, &Msg::Header(log.header.clone())).expect("the header goes out");
        for (tick, hash) in hashes {
            write_frame(&mut client, &Msg::StateHash { tick, hash }).expect("the hash goes out");
        }
        write_frame(
            &mut client,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        client.flush().expect("the frames are flushed");

        let answer = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
        let report = handle
            .join()
            .expect("the thread")
            .expect("the round was taken in");

        // The round was stored, and the report says it did not verify.
        assert!(report.path.exists());
        let verified = report.verified.expect("the round was verified");
        assert!(!verified.ok);
        assert_eq!(
            verified.first_divergence.map(|(tick, _, _)| tick),
            Some(tick)
        );
        // And the peer is told, by name.
        match answer.msg {
            Msg::Bye { reason } => assert!(reason.contains("diverged"), "{reason}"),
            other => panic!("expected a goodbye, got {other:?}"),
        }
        // The report is on disk, and the state says the round failed.
        let report_path = cfg.store().report_path(&report.name);
        assert!(report_path.exists());
        let state = cfg.store().read_state();
        assert_eq!(state.rounds_ingested, 1);
        assert_eq!(state.rounds_failed, 1);
        assert_eq!(state.rounds_verified, 0);
        assert_eq!(state.last_verified_round, None);
    }

    #[test]
    fn a_clean_round_is_reported_as_verified() {
        let scratch = Scratch::new("clean");
        let cfg = config(&scratch);
        let log = round(1, 300);

        let (mut client, handle) = peer(&cfg);
        push(&mut client, &log);
        let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
        let report = handle
            .join()
            .expect("the thread")
            .expect("the round was taken in");

        let state = cfg.store().read_state();
        assert_eq!(state.rounds_verified, 1);
        assert_eq!(state.last_verified_round, Some(report.name.clone()));
    }

    #[test]
    fn a_hello_of_this_version_is_accepted() {
        let scratch = Scratch::new("hello");
        let cfg = config(&scratch);
        let log = round(1, 300);

        let (mut client, handle) = peer(&cfg);
        write_frame(
            &mut client,
            &Msg::Hello {
                proto_version: lm_proto::PROTO_VERSION,
                node: lm_proto::NodeKind::Host,
                caps: Vec::new(),
            },
        )
        .expect("the hello goes out");
        push(&mut client, &log);
        let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");

        let report = handle
            .join()
            .expect("the thread")
            .expect("the round was taken in");
        assert!(report.verified.expect("verified").ok);
    }

    #[test]
    fn a_hello_of_another_version_is_refused() {
        let scratch = Scratch::new("hello-version");
        let cfg = config(&scratch);

        let (mut client, handle) = peer(&cfg);
        write_frame(
            &mut client,
            &Msg::Hello {
                proto_version: 99,
                node: lm_proto::NodeKind::Host,
                caps: Vec::new(),
            },
        )
        .expect("the hello goes out");
        client.flush().expect("the frame is flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::VersionMismatch {
                    local: lm_proto::PROTO_VERSION,
                    remote: 99
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_round_that_claims_too_many_ticks_is_refused() {
        let scratch = Scratch::new("too-long");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        // A peer that claims a round of a billion ticks: the archive must
        // refuse it by name, not spend a minute of CPU on it.
        let header = Header::default_round(1, crate::testing::REFLEX);
        write_frame(&mut client, &Msg::Header(header.clone())).expect("the header goes out");
        write_frame(
            &mut client,
            &Msg::Stamped(lm_core::decision::Stamped {
                d: lm_core::Decision {
                    apply_tick: 1_000_000_000,
                    source: lm_core::Source::Agent(0),
                    seq: 1,
                    kind: lm_core::Kind::ActionOn(lm_core::Action::Hold),
                    provenance: lm_core::Provenance::Test,
                },
                arrived: 1_000_000_000,
                tick: 1_000_000_000,
                late_by: 0,
            }),
        )
        .expect("the decision goes out");
        write_frame(
            &mut client,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        client.flush().expect("the frames are flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::Reconstruct(ReconstructError::TooLong { .. })
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("ticks"));
    }

    #[test]
    fn a_decision_from_before_the_round_started_is_refused() {
        let scratch = Scratch::new("early-decision");
        let cfg = config(&scratch);
        let (mut client, handle) = peer(&cfg);

        let mut header = Header::default_round(1, crate::testing::REFLEX);
        // A round that starts at tick 10: a decision on tick 9 is from
        // before the round began.
        header.start_tick = 10;
        let start = header.start_tick;
        write_frame(&mut client, &Msg::Header(header)).expect("the header goes out");
        write_frame(
            &mut client,
            &Msg::Stamped(lm_core::decision::Stamped {
                d: lm_core::Decision {
                    apply_tick: start.saturating_sub(1),
                    source: lm_core::Source::Agent(0),
                    seq: 1,
                    kind: lm_core::Kind::ActionOn(lm_core::Action::Hold),
                    provenance: lm_core::Provenance::Test,
                },
                arrived: start.saturating_sub(1),
                tick: start.saturating_sub(1),
                late_by: 0,
            }),
        )
        .expect("the decision goes out");
        write_frame(
            &mut client,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        client.flush().expect("the frames are flushed");

        let err = handle
            .join()
            .expect("the thread")
            .expect_err("the push was refused");
        assert!(
            matches!(
                err,
                IngestError::OutOfOrder {
                    what: "a decision from before the round started"
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn the_store_keeps_only_as_many_rounds_as_it_is_told() {
        let scratch = Scratch::new("retain");
        let mut cfg = config(&scratch);
        cfg.retain_rounds = 1;

        for seed in [1u64, 2] {
            let log = round(seed, 300);
            let (mut client, handle) = peer(&cfg);
            push(&mut client, &log);
            let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
            handle
                .join()
                .expect("the thread")
                .expect("the round was taken in");
        }

        let rounds = cfg.store().rounds().expect("the rounds are listed");
        assert_eq!(rounds.len(), 1, "only the newest round is kept");
    }
}

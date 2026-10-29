//! The round log: a hash-chained, byte-exact record of a round.
//!
//! A log is everything a replay needs: the header says which round this is,
//! and the segments say, tick by tick, which decisions were applied. The state
//! hashes inside the segments let a replay prove that it saw the same world,
//! and the hash chain proves that nobody edited the file on the way.
//!
//! The file format is deliberately boring:
//!
//! ```text
//! file    := MAGIC version:u16 blob*
//! MAGIC   := "LMLOG1\0\0"
//! version := PROTO_VERSION, little-endian u16
//! blob    := len:u32 body:len bytes
//! body    := postcard(header) | postcard(segment)
//! ```
//!
//! Blob 0 is the header; blob `i` (for `i >= 1`) is `segments[i - 1]`. The
//! length prefix makes a blob skippable without decoding it, and the magic
//! makes a wrong file obvious instead of a wrong replay.
//!
//! Each segment carries the hash of the segment before it, and the first
//! segment carries the hash of the header: the whole file is one chain, so a
//! single flipped byte is caught and named, never replayed as a prefix.

use std::fmt;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::decision::{Stamped, Tick};
use crate::hash::Fnv64;
use crate::header::{Header, PROTO_VERSION};

/// The bytes that start every log file.
pub const MAGIC: [u8; 8] = *b"LMLOG1\0\0";

/// The size of the chunks a blob body is read in.
const READ_CHUNK: usize = 64 * 1024;

/// Why a log could not be read, or did not verify.
///
/// The `segment` field names the blob the damage is in: 0 is the header and
/// blob `i` (for `i >= 1`) is `segments[i - 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogError {
    /// The file does not start with [`MAGIC`].
    BadMagic,
    /// The file's version is not the one this build speaks.
    Version {
        /// The version the file claims.
        found: u16,
    },
    /// The file ended in the middle of a blob.
    Truncated {
        /// The blob that was cut short.
        segment: usize,
    },
    /// The hash chain does not hold.
    ChainBroken {
        /// The segment whose hash does not match.
        segment: usize,
    },
    /// A blob could not be decoded.
    Decode {
        /// The blob that could not be decoded.
        segment: usize,
    },
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::BadMagic => write!(f, "not a round log: bad magic"),
            LogError::Version { found } => {
                write!(f, "log version {found}, this build speaks {PROTO_VERSION}")
            }
            LogError::Truncated { segment } => {
                write!(f, "log truncated inside blob {segment}")
            }
            LogError::ChainBroken { segment } => {
                write!(f, "hash chain broken at segment {segment}")
            }
            LogError::Decode { segment } => {
                write!(f, "blob {segment} could not be decoded")
            }
        }
    }
}

impl std::error::Error for LogError {}

/// One segment of the log: a run of ticks and the hash that seals them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// The first tick in the segment.
    pub first_tick: Tick,
    /// The last tick in the segment.
    pub last_tick: Tick,
    /// The decisions applied in the segment, in tick order.
    pub decisions: Vec<Stamped>,
    /// The state hashes reported in the segment, with their ticks.
    pub hashes: Vec<(Tick, u64)>,
    /// The hash of the previous segment; the header's hash for the first one.
    pub prev: u64,
    /// The hash of this segment.
    pub hash: u64,
}

/// The part of a segment the hash covers.
///
/// It has the same four fields, in the same order and with the same types, as
/// the front of the segment's own encoding, so the bytes the hash covers are
/// the bytes the segment starts with.
#[derive(Serialize)]
struct SegmentBody<'a> {
    first_tick: Tick,
    last_tick: Tick,
    decisions: &'a [Stamped],
    hashes: &'a [(Tick, u64)],
}

impl Segment {
    /// An empty segment: the starting point of a recorder.
    fn empty() -> Self {
        Segment {
            first_tick: 0,
            last_tick: 0,
            decisions: Vec::new(),
            hashes: Vec::new(),
            prev: 0,
            hash: 0,
        }
    }

    /// The hash of this segment: FNV-1a 64 over the previous hash
    /// (little-endian) followed by the canonical bytes of the segment body.
    pub fn compute_hash(&self) -> u64 {
        let body = postcard::to_allocvec(&SegmentBody {
            first_tick: self.first_tick,
            last_tick: self.last_tick,
            decisions: &self.decisions,
            hashes: &self.hashes,
        })
        .expect("postcard encoding of a segment cannot fail");
        let mut h = Fnv64::new();
        h.u64(self.prev);
        h.write(&body);
        h.finish()
    }
}

/// A whole round: the header and the hash-chained segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundLog {
    /// The round header: the round's identity.
    pub header: Header,
    /// The segments, in tick order.
    pub segments: Vec<Segment>,
}

impl RoundLog {
    /// Write the log: magic, version, then the header and every segment as
    /// length-prefixed postcard blobs.
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&MAGIC)?;
        w.write_all(&PROTO_VERSION.to_le_bytes())?;
        write_blob(w, &self.header)?;
        for segment in &self.segments {
            write_blob(w, segment)?;
        }
        Ok(())
    }

    /// Read a log back.
    ///
    /// The file is decoded strictly: a wrong magic, a wrong version, a
    /// truncated blob or a blob that does not decode is an error that names
    /// the blob it happened in. The hash chain is verified before the log is
    /// handed back, so a damaged log never replays as a prefix.
    pub fn read(r: &mut impl Read) -> Result<RoundLog, LogError> {
        let mut magic = [0u8; MAGIC.len()];
        // A file too short to hold the magic is not a log file.
        r.read_exact(&mut magic).map_err(|_| LogError::BadMagic)?;
        if magic != MAGIC {
            return Err(LogError::BadMagic);
        }
        let mut version = [0u8; 2];
        r.read_exact(&mut version)
            .map_err(|_| LogError::Truncated { segment: 0 })?;
        let found = u16::from_le_bytes(version);
        if found != PROTO_VERSION {
            return Err(LogError::Version { found });
        }

        // Blob 0 is the header; blob `i` (for `i >= 1`) is `segments[i - 1]`.
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        loop {
            let index = blobs.len();
            let len = match read_len(r, index)? {
                Some(len) => len,
                None => break,
            };
            let body = read_body(r, len as usize).ok_or(LogError::Truncated { segment: index })?;
            blobs.push(body);
        }

        let header = blobs
            .first()
            .ok_or(LogError::Truncated { segment: 0 })
            .and_then(|blob| {
                postcard::from_bytes(blob).map_err(|_| LogError::Decode { segment: 0 })
            })?;
        let mut segments = Vec::with_capacity(blobs.len().saturating_sub(1));
        for (index, blob) in blobs.iter().enumerate().skip(1) {
            let segment: Segment =
                postcard::from_bytes(blob).map_err(|_| LogError::Decode { segment: index })?;
            segments.push(segment);
        }

        let log = RoundLog { header, segments };
        log.verify()?;
        Ok(log)
    }

    /// Check the log: the version and the hash chain.
    ///
    /// Every segment must carry the hash of the one before it, and the first
    /// one must carry the header's hash, and every segment's hash must be the
    /// hash of its own contents.
    pub fn verify(&self) -> Result<(), LogError> {
        if self.header.proto_version != PROTO_VERSION {
            return Err(LogError::Version {
                found: self.header.proto_version,
            });
        }
        let mut prev = self.header.hash();
        for (index, segment) in self.segments.iter().enumerate() {
            // The blobs are numbered from 1: blob 0 is the header.
            let number = index + 1;
            if segment.prev != prev {
                return Err(LogError::ChainBroken { segment: number });
            }
            if segment.hash != segment.compute_hash() {
                return Err(LogError::ChainBroken { segment: number });
            }
            prev = segment.hash;
        }
        Ok(())
    }

    /// The decisions applied on `tick`.
    ///
    /// The decisions of one tick are recorded together, so they are one
    /// contiguous run; a tick with no decisions has none.
    pub fn decisions_for(&self, tick: Tick) -> &[Stamped] {
        for segment in &self.segments {
            let Some(start) = segment.decisions.iter().position(|s| s.tick == tick) else {
                continue;
            };
            let end = segment.decisions[start..]
                .iter()
                .position(|s| s.tick != tick)
                .map_or(segment.decisions.len(), |run| start + run);
            return &segment.decisions[start..end];
        }
        &[]
    }

    /// Every state hash in the log, with its tick, in tick order.
    pub fn hashes(&self) -> Vec<(Tick, u64)> {
        let mut out = Vec::new();
        for segment in &self.segments {
            out.extend_from_slice(&segment.hashes);
        }
        out
    }
}

/// Write one blob: a little-endian `u32` length and the postcard bytes.
fn write_blob<T: Serialize>(w: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = postcard::to_allocvec(value).expect("postcard encoding of a log blob cannot fail");
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "log blob is too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    Ok(())
}

/// Read the length prefix of the next blob.
///
/// `None` means the file ended cleanly on a blob boundary; a short read is a
/// truncation.
fn read_len(r: &mut impl Read, index: usize) -> Result<Option<u32>, LogError> {
    let mut buf = [0u8; 4];
    let read = r
        .read(&mut buf)
        .map_err(|_| LogError::Truncated { segment: index })?;
    if read == 0 {
        return Ok(None);
    }
    r.read_exact(&mut buf[read..])
        .map_err(|_| LogError::Truncated { segment: index })?;
    Ok(Some(u32::from_le_bytes(buf)))
}

/// Read exactly `len` bytes.
///
/// The length comes from the file, so it is never trusted for a single big
/// allocation: the body is read in chunks and a short file is a truncation.
fn read_body(r: &mut impl Read, len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(len.min(READ_CHUNK));
    let mut buf = [0u8; READ_CHUNK];
    let mut left = len;
    while left > 0 {
        let want = left.min(READ_CHUNK);
        match r.read(&mut buf[..want]) {
            Ok(0) => return None,
            Ok(read) => {
                out.extend_from_slice(&buf[..read]);
                left -= read;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    Some(out)
}

/// The recorder: builds a round log while the round runs.
///
/// The recorder holds no clock and no randomness: it only groups the ticks it
/// is told about into segments and seals each segment with the hash of the
/// one before it.
#[derive(Clone, Debug)]
pub struct Recorder {
    header: Header,
    segment_ticks: u32,
    root: u64,
    segments: Vec<Segment>,
    current: Segment,
    current_ticks: u64,
}

impl Recorder {
    /// A recorder for a round with the given header.
    ///
    /// `segment_ticks` is the number of ticks per segment; 0 means the whole
    /// round is one segment. The chain of segments is rooted in the header's
    /// hash, so the segments cannot be swapped between rounds.
    pub fn new(h: Header, segment_ticks: u32) -> Self {
        let root = h.hash();
        Recorder {
            header: h,
            segment_ticks,
            root,
            segments: Vec::new(),
            current: Segment::empty(),
            current_ticks: 0,
        }
    }

    /// Record one tick: the decisions applied on it and, optionally, the
    /// state hash of the world after it.
    ///
    /// The ticks are grouped into segments of `segment_ticks` ticks; the
    /// segment is closed and sealed as soon as it is full.
    pub fn record(&mut self, tick: Tick, applied: &[Stamped], hash: Option<u64>) {
        if self.current_ticks == 0 {
            self.current.first_tick = tick;
        }
        self.current.last_tick = tick;
        self.current.decisions.extend_from_slice(applied);
        if let Some(hash) = hash {
            self.current.hashes.push((tick, hash));
        }
        self.current_ticks += 1;
        if self.segment_ticks > 0 && self.current_ticks >= u64::from(self.segment_ticks) {
            self.close_segment();
        }
    }

    /// Close the last segment and hand back the log.
    pub fn finish(mut self) -> RoundLog {
        self.close_segment();
        RoundLog {
            header: self.header,
            segments: self.segments,
        }
    }

    /// Seal the segment being filled and start the next one.
    fn close_segment(&mut self) {
        if self.current_ticks == 0 {
            return;
        }
        let prev = self.segments.last().map_or(self.root, |s| s.hash);
        let mut segment = std::mem::replace(&mut self.current, Segment::empty());
        segment.prev = prev;
        segment.hash = segment.compute_hash();
        self.segments.push(segment);
        self.current_ticks = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{Action, Decision, Kind, Provenance, Source};

    /// A stamped decision: agent `agent` holds still on `tick`.
    fn stamped(agent: u16, tick: Tick, seq: u32) -> Stamped {
        Stamped {
            d: Decision {
                apply_tick: tick,
                source: Source::Agent(agent),
                seq,
                kind: Kind::ActionOn(Action::Hold),
                provenance: Provenance::Test,
            },
            arrived: tick,
            tick,
            late_by: 0,
        }
    }

    /// The header of the recorded rounds.
    fn header() -> Header {
        Header::default_round(7, "fn reflex(v){}")
    }

    /// A log of `ticks` ticks, `per_segment` ticks per segment, with one
    /// decision per tick and a state hash every 30 ticks.
    fn recorded(ticks: u64, per_segment: u32) -> RoundLog {
        let mut rec = Recorder::new(header(), per_segment);
        for t in 0..ticks {
            let agent = u16::try_from(t % 4).unwrap();
            let seq = u32::try_from(t).unwrap();
            let applied = vec![stamped(agent, t, seq)];
            let hash = if t % 30 == 0 {
                Some(t.wrapping_mul(31))
            } else {
                None
            };
            rec.record(t, &applied, hash);
        }
        rec.finish()
    }

    /// The bytes of a log.
    fn to_bytes(log: &RoundLog) -> Vec<u8> {
        let mut out = Vec::new();
        log.write(&mut out).unwrap();
        out
    }

    /// The byte span of every blob in a log file: blob 0 is the header.
    fn blob_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut off = MAGIC.len() + 2;
        while off + 4 <= bytes.len() {
            let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            off += 4;
            spans.push((off, len));
            off += len;
        }
        spans
    }

    #[test]
    fn roundtrip_bytes_identical() {
        let log = recorded(900, 300);
        let bytes = to_bytes(&log);

        let mut cursor = io::Cursor::new(bytes.clone());
        let read = RoundLog::read(&mut cursor).unwrap();
        assert!(read.verify().is_ok());
        assert_eq!(read, log);

        let mut again = Vec::new();
        read.write(&mut again).unwrap();
        assert_eq!(again, bytes);
    }

    #[test]
    fn truncated_log_names_segment() {
        let log = recorded(900, 300);
        let bytes = to_bytes(&log);

        // Blob 0 is the header, blob 1 the first segment, blob 2 the second.
        let (start, len) = blob_spans(&bytes)[2];
        let cut = start + len / 2;

        let err = RoundLog::read(&mut io::Cursor::new(bytes[..cut].to_vec())).unwrap_err();
        assert_eq!(err, LogError::Truncated { segment: 2 });
    }

    #[test]
    fn flipped_byte_breaks_chain() {
        let log = recorded(900, 300);
        let mut bytes = to_bytes(&log);

        // Flip one byte inside the first segment's decisions: the byte that
        // starts its first decision.
        let (start, _) = blob_spans(&bytes)[1];
        let first = postcard::to_allocvec(&log.segments[0].decisions[0]).unwrap();
        let at = start
            + bytes[start..]
                .windows(first.len())
                .position(|w| w == first.as_slice())
                .expect("the first decision's bytes are in the segment");
        bytes[at] ^= 0x01;

        let err = match RoundLog::read(&mut io::Cursor::new(bytes)) {
            Ok(log) => log.verify().unwrap_err(),
            Err(e) => e,
        };
        assert!(
            matches!(
                err,
                LogError::ChainBroken { segment: 1 } | LogError::Decode { segment: 1 }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn wrong_version_rejected() {
        let log = recorded(900, 300);
        let mut bytes = to_bytes(&log);

        // The version sits right after the magic.
        let at = MAGIC.len();
        bytes[at..at + 2].copy_from_slice(&99u16.to_le_bytes());

        let err = RoundLog::read(&mut io::Cursor::new(bytes)).unwrap_err();
        assert_eq!(err, LogError::Version { found: 99 });
    }

    #[test]
    fn segments_cover_segment_ticks() {
        let log = recorded(900, 300);
        assert_eq!(log.segments.len(), 3);
        assert_eq!(log.segments[0].first_tick, 0);
        assert_eq!(log.segments[1].first_tick, 300);
        assert_eq!(log.segments[2].first_tick, 600);
        assert_eq!(log.segments[0].last_tick, 299);
        assert_eq!(log.segments[1].last_tick, 599);
        assert_eq!(log.segments[2].last_tick, 899);
    }

    #[test]
    fn the_chain_is_rooted_in_the_header() {
        let log = recorded(900, 300);
        assert_eq!(log.segments[0].prev, log.header.hash());
        assert_eq!(log.segments[1].prev, log.segments[0].hash);
        assert_eq!(log.segments[2].prev, log.segments[1].hash);
        for seg in &log.segments {
            assert_eq!(seg.hash, seg.compute_hash());
        }
    }

    #[test]
    fn the_hashed_body_is_the_front_of_the_segment_bytes() {
        let log = recorded(900, 300);
        let seg = &log.segments[0];
        let body = postcard::to_allocvec(&SegmentBody {
            first_tick: seg.first_tick,
            last_tick: seg.last_tick,
            decisions: &seg.decisions,
            hashes: &seg.hashes,
        })
        .unwrap();
        let full = postcard::to_allocvec(seg).unwrap();
        assert!(
            full.starts_with(&body),
            "the hash must cover the segment's own bytes"
        );
    }

    #[test]
    fn decisions_and_hashes_are_found_by_tick() {
        let log = recorded(900, 300);
        let at_42 = log.decisions_for(42);
        assert_eq!(at_42.len(), 1);
        assert_eq!(at_42[0].tick, 42);
        assert!(log.decisions_for(1_000).is_empty());

        let hashes = log.hashes();
        assert_eq!(hashes.len(), 30);
        assert_eq!(hashes[0], (0, 0));
        assert_eq!(hashes[1], (30, 930));
    }

    #[test]
    fn a_bad_magic_is_refused() {
        let log = recorded(900, 300);
        let mut bytes = to_bytes(&log);
        bytes[0] = b'X';
        let err = RoundLog::read(&mut io::Cursor::new(bytes)).unwrap_err();
        assert_eq!(err, LogError::BadMagic);
    }

    #[test]
    fn an_edited_segment_breaks_the_chain() {
        let mut log = recorded(900, 300);
        log.segments[0].decisions[0].d.seq = 999;
        let err = log.verify().unwrap_err();
        assert_eq!(err, LogError::ChainBroken { segment: 1 });
    }
}

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
//!
//! ## The version gate
//!
//! The version after the magic says which header shape the file carries. This
//! build reads every version from [`MIN_READABLE_VERSION`] up to
//! [`PROTO_VERSION`]; anything else is refused, by name.
//!
//! A version-1 log is read through the [`HeaderV1`] shim and **migrated**:
//! the talk params take their defaults and the version becomes the current
//! one. The migration changes the *header*, so the chain is **re-rooted**
//! over the migrated header: every segment's `prev` and `hash` is recomputed.
//! The state hashes inside the segments are not touched — the world the log
//! records did not change, so the round still replays to the hashes it
//! recorded.
//!
//! A version-2 log is read through the [`SegmentV2`] shim and migrated the
//! same way: the segments gain no refusals (a version-2 log recorded none)
//! and the version becomes the current one.
//!
//! ## The refusals are in the record
//!
//! A segment carries the refusals of its ticks: the model that did not answer
//! in time, the gate the backend was unsure about, the request that expired
//! in the queue. They are in the chain, so they cannot be edited after the
//! fact, and they are in the log, so a replay draws the same hollow marks a
//! live round drew (E7).

use std::fmt;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::decision::{Stamped, Tick};
use crate::hash::Fnv64;
use crate::header::{Header, HeaderV1, PROTO_VERSION};
use crate::refusal::{RefusalCounts, Refused};

/// The bytes that start every log file.
pub const MAGIC: [u8; 8] = *b"LMLOG1\0\0";

/// The oldest log version this build can read.
///
/// The reader accepts every version from here up to [`PROTO_VERSION`]; a
/// version outside that range is refused by name.
pub const MIN_READABLE_VERSION: u16 = 1;

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
                write!(
                    f,
                    "log version {found}, this build speaks {MIN_READABLE_VERSION}..={PROTO_VERSION}"
                )
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
    /// The refusals of the segment, in tick order.
    ///
    /// A refusal is an outcome (D26), and an outcome that is not in the
    /// record cannot be replayed: "8 refusals" in a summary hides four
    /// different fixes, and a replay that cannot say *why* a mind said
    /// nothing cannot teach anything about it (D64).
    pub refusals: Vec<Refused>,
    /// The hash of the previous segment; the header's hash for the first one.
    pub prev: u64,
    /// The hash of this segment.
    pub hash: u64,
}

/// The part of a segment the hash covers.
///
/// It has the same five fields, in the same order and with the same types, as
/// the front of the segment's own encoding, so the bytes the hash covers are
/// the bytes the segment starts with.
#[derive(Serialize)]
struct SegmentBody<'a> {
    first_tick: Tick,
    last_tick: Tick,
    decisions: &'a [Stamped],
    hashes: &'a [(Tick, u64)],
    refusals: &'a [Refused],
}

/// A version-2 segment: the same segment without the refusals.
///
/// The refusals were added to the segment in version 3, and a postcard blob
/// is read positionally, so an old segment cannot be read as a new one: the
/// old shape has its own type, and the migration gives it empty refusals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentV2 {
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

/// The part of a version-2 segment its hash covered.
///
/// The same four fields, in the same order and with the same types, as the
/// front of the version-2 segment's own encoding.
#[derive(Serialize)]
struct SegmentBodyV2<'a> {
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
            refusals: Vec::new(),
            prev: 0,
            hash: 0,
        }
    }

    /// Whether the segment holds nothing at all.
    ///
    /// A segment that holds a refusal is not empty: the refusal's tick is
    /// the segment's first tick, and the tick that follows does not
    /// overwrite it.
    fn is_empty(&self) -> bool {
        self.decisions.is_empty() && self.hashes.is_empty() && self.refusals.is_empty()
    }

    /// The hash of this segment: FNV-1a 64 over the previous hash
    /// (little-endian) followed by the canonical bytes of the segment body.
    pub fn compute_hash(&self) -> u64 {
        let body = postcard::to_allocvec(&SegmentBody {
            first_tick: self.first_tick,
            last_tick: self.last_tick,
            decisions: &self.decisions,
            hashes: &self.hashes,
            refusals: &self.refusals,
        })
        .expect("postcard encoding of a segment cannot fail");
        let mut h = Fnv64::new();
        h.u64(self.prev);
        h.write(&body);
        h.finish()
    }
}

impl SegmentV2 {
    /// The hash of a version-2 segment: the same rule, over the body that
    /// version covered.
    ///
    /// The version-2 body has no refusals in it, so a version-2 log still
    /// verifies against the hashes it was written with.
    pub fn compute_hash(&self) -> u64 {
        let body = postcard::to_allocvec(&SegmentBodyV2 {
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

    /// The segment as the current version writes it: the same decisions and
    /// the same state hashes, and no refusals — a version-2 log recorded none.
    pub fn migrate(self) -> Segment {
        Segment {
            first_tick: self.first_tick,
            last_tick: self.last_tick,
            decisions: self.decisions,
            hashes: self.hashes,
            refusals: Vec::new(),
            prev: self.prev,
            hash: self.hash,
        }
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

/// A version-1 round log: the phase-1 header and the segments, before any
/// migration.
///
/// The version gate reads an old log through this shape, so the old header
/// can be seen as it was written — and so the migration can be checked
/// against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundLogV1 {
    /// The header as it was written.
    pub header: HeaderV1,
    /// The segments as they were written.
    pub segments: Vec<SegmentV2>,
}

impl RoundLogV1 {
    /// Migrate the log to the current version.
    ///
    /// The header gains the talk params and the current version, so the
    /// round's identity moves; the chain is re-rooted over the migrated
    /// header to match. The state hashes inside the segments stay exactly
    /// as they were: the world the log records did not change, so the round
    /// still replays to the hashes it recorded.
    pub fn migrate(self) -> RoundLog {
        let header = self.header.migrate();
        let mut log = RoundLog {
            header,
            segments: self.segments.into_iter().map(SegmentV2::migrate).collect(),
        };
        log.reroot_chain();
        log
    }
}

impl RoundLog {
    /// Write the log: magic, version, then the header and every segment as
    /// length-prefixed postcard blobs.
    ///
    /// The version is the header's own, not the build's constant: a log that
    /// was migrated from version 1 writes back as a valid version-3 log.
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&MAGIC)?;
        w.write_all(&self.header.proto_version.to_le_bytes())?;
        write_blob(w, &self.header)?;
        for segment in &self.segments {
            write_blob(w, segment)?;
        }
        Ok(())
    }

    /// Read a log back, in any version this build speaks.
    ///
    /// The file is decoded strictly: a wrong magic, a wrong version, a
    /// truncated blob or a blob that does not decode is an error that names
    /// the blob it happened in. The hash chain is verified before the log is
    /// handed back, so a damaged log never replays as a prefix.
    ///
    /// A version-1 log is migrated on the way in: the header gains the talk
    /// params and the current version, and the chain is re-rooted over it.
    /// A version-2 log is migrated the same way: its segments gain no
    /// refusals, because it recorded none. The state hashes inside the
    /// segments are not touched.
    pub fn read(r: &mut impl Read) -> Result<RoundLog, LogError> {
        let blobs = read_blobs(r)?;
        let log = match blobs.version {
            MIN_READABLE_VERSION => decode_v1(&blobs)?.migrate(),
            2 => decode_v2(&blobs)?.migrate(),
            _ => decode_v3(&blobs)?,
        };
        log.verify()?;
        Ok(log)
    }

    /// Read a version-1 log, without migrating it.
    ///
    /// This is the pre-migration view: the header and the chain as phase 1
    /// wrote them. The version gate is checked against it.
    pub fn read_v1(r: &mut impl Read) -> Result<RoundLogV1, LogError> {
        let blobs = read_blobs(r)?;
        if blobs.version != MIN_READABLE_VERSION {
            return Err(LogError::Version {
                found: blobs.version,
            });
        }
        decode_v1(&blobs)
    }

    /// Re-root the chain over the header.
    ///
    /// Every segment's `prev` and `hash` is recomputed, so the chain
    /// follows the header it belongs to. The state hashes inside the
    /// segments are not touched: the world the log records did not change.
    pub fn reroot_chain(&mut self) {
        let mut prev = self.header.hash();
        for segment in &mut self.segments {
            segment.prev = prev;
            segment.hash = segment.compute_hash();
            prev = segment.hash;
        }
    }

    /// Check the log: the version and the hash chain.
    ///
    /// Every segment must carry the hash of the one before it, and the first
    /// one must carry the header's hash, and every segment's hash must be the
    /// hash of its own contents.
    pub fn verify(&self) -> Result<(), LogError> {
        if self.header.proto_version < MIN_READABLE_VERSION
            || self.header.proto_version > PROTO_VERSION
        {
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

    /// Every refusal in the log, in tick order.
    ///
    /// This is the record the score view replays its hollow marks from (E7)
    /// and the record the telemetry counts its refusals by kind from: a
    /// refusal that is not in the log is a refusal nobody can learn from.
    pub fn refusals(&self) -> Vec<Refused> {
        let mut out = Vec::new();
        for segment in &self.segments {
            out.extend_from_slice(&segment.refusals);
        }
        out.sort_by_key(|refused| refused.tick);
        out
    }

    /// The refusals of the log, counted by kind.
    ///
    /// This is what the telemetry reads to split its refusals column: the
    /// count comes from the record, so a replay and a live round agree.
    pub fn refusal_counts(&self) -> RefusalCounts {
        let mut counts = RefusalCounts::new();
        for refused in self.refusals() {
            counts.count(refused.kind);
        }
        counts
    }
}

/// The blobs of a log file, read but not yet interpreted.
struct Blobs {
    /// The version the file claims.
    version: u16,
    /// The header blob.
    header: Vec<u8>,
    /// The segment blobs, in order.
    segments: Vec<Vec<u8>>,
}

/// Read the magic, the version and the blobs of a log file.
///
/// The version is the gate: a file whose version this build does not speak
/// is refused here, by name, before anything is decoded.
fn read_blobs(r: &mut impl Read) -> Result<Blobs, LogError> {
    let mut magic = [0u8; MAGIC.len()];
    // A file too short to hold the magic is not a log file.
    r.read_exact(&mut magic).map_err(|_| LogError::BadMagic)?;
    if magic != MAGIC {
        return Err(LogError::BadMagic);
    }
    let mut version = [0u8; 2];
    r.read_exact(&mut version)
        .map_err(|_| LogError::Truncated { segment: 0 })?;
    let version = u16::from_le_bytes(version);
    if !(MIN_READABLE_VERSION..=PROTO_VERSION).contains(&version) {
        return Err(LogError::Version { found: version });
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
    let mut blobs = blobs.into_iter();
    let header = blobs.next().ok_or(LogError::Truncated { segment: 0 })?;
    Ok(Blobs {
        version,
        header,
        segments: blobs.collect(),
    })
}

/// Decode a version-1 log's blobs: the old header shape and the segments.
///
/// The segments have not changed shape, so they are read the same way in
/// every version.
fn decode_v1(blobs: &Blobs) -> Result<RoundLogV1, LogError> {
    let header: HeaderV1 =
        postcard::from_bytes(&blobs.header).map_err(|_| LogError::Decode { segment: 0 })?;
    let segments = decode_segments_v2(&blobs.segments)?;
    Ok(RoundLogV1 { header, segments })
}

/// A version-2 log: the current header shape and the segments without the
/// refusals.
struct RoundLogV2 {
    /// The header as it was written.
    header: Header,
    /// The segments as they were written.
    segments: Vec<SegmentV2>,
}

impl RoundLogV2 {
    /// Migrate the log to the current version.
    ///
    /// The segments gain no refusals — a version-2 log recorded none — and
    /// the version moves, so the chain is re-rooted over the new header.
    /// The state hashes inside the segments stay exactly as they were.
    fn migrate(self) -> RoundLog {
        let mut header = self.header;
        header.proto_version = PROTO_VERSION;
        let mut log = RoundLog {
            header,
            segments: self.segments.into_iter().map(SegmentV2::migrate).collect(),
        };
        log.reroot_chain();
        log
    }
}

/// Decode a version-2 log's blobs.
fn decode_v2(blobs: &Blobs) -> Result<RoundLogV2, LogError> {
    let header: Header =
        postcard::from_bytes(&blobs.header).map_err(|_| LogError::Decode { segment: 0 })?;
    let segments = decode_segments_v2(&blobs.segments)?;
    Ok(RoundLogV2 { header, segments })
}

/// Decode a current-version log's blobs.
fn decode_v3(blobs: &Blobs) -> Result<RoundLog, LogError> {
    let header: Header =
        postcard::from_bytes(&blobs.header).map_err(|_| LogError::Decode { segment: 0 })?;
    let segments = decode_segments(&blobs.segments)?;
    Ok(RoundLog { header, segments })
}

/// Decode the segment blobs, in order.
fn decode_segments(blobs: &[Vec<u8>]) -> Result<Vec<Segment>, LogError> {
    let mut segments = Vec::with_capacity(blobs.len());
    for (index, blob) in blobs.iter().enumerate() {
        // The blobs are numbered from 1: blob 0 is the header.
        let segment: Segment =
            postcard::from_bytes(blob).map_err(|_| LogError::Decode { segment: index + 1 })?;
        segments.push(segment);
    }
    Ok(segments)
}

/// Decode the version-2 segment blobs, in order.
fn decode_segments_v2(blobs: &[Vec<u8>]) -> Result<Vec<SegmentV2>, LogError> {
    let mut segments = Vec::with_capacity(blobs.len());
    for (index, blob) in blobs.iter().enumerate() {
        // The blobs are numbered from 1: blob 0 is the header.
        let segment: SegmentV2 =
            postcard::from_bytes(blob).map_err(|_| LogError::Decode { segment: index + 1 })?;
        segments.push(segment);
    }
    Ok(segments)
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
        if self.current_ticks == 0 && self.current.refusals.is_empty() {
            // The segment holds no tick yet, so this tick opens it. A segment
            // that a refusal opened keeps the refusal's tick as its first
            // one: the refusal happened on that tick too.
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

    /// Record one refusal: the model that did not answer, the gate the
    /// backend was unsure about, the request that expired in the queue.
    ///
    /// The refusal goes into the segment that is being filled, so it is
    /// sealed with the rest of that segment's contents: a refusal cannot be
    /// edited after the fact without breaking the chain.
    ///
    /// The refusal is recorded on the tick it happened on, so it belongs to
    /// the segment of that tick.
    pub fn record_refusal(&mut self, refused: Refused) {
        if self.current.is_empty() {
            // The refusal opened the segment: the tick it happened on is the
            // segment's first tick.
            self.current.first_tick = refused.tick;
            self.current.last_tick = refused.tick;
        } else {
            // The segment already holds ticks. The refusal happened on its own
            // tick, which is never before the segment's first tick, so only
            // the last one can move.
            self.current.last_tick = self.current.last_tick.max(refused.tick);
        }
        self.current.refusals.push(refused);
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
        // A segment that holds a refusal and no tick is still a segment: the
        // refusal has to be sealed into the chain like everything else.
        if self.current.is_empty() {
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
    use crate::hash::StateHash;
    use crate::refusal::RefusalKind;
    use crate::talk::TalkParams;

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

    /// The phase-1 header of a header: the same round, without the talk
    /// params and with the old version.
    fn header_v1(header: &Header) -> HeaderV1 {
        HeaderV1 {
            proto_version: MIN_READABLE_VERSION,
            seed: header.seed,
            params: header.params,
            ruleset: header.ruleset.clone(),
            roster: header.roster.clone(),
            scripts: header.scripts.clone(),
            start_tick: header.start_tick,
            lineage: header.lineage.clone(),
        }
    }

    /// The bytes of a version-1 log: the round written the old way, with
    /// its chain rooted in the old header's hash.
    fn to_v1_bytes(log: &RoundLog) -> Vec<u8> {
        let v1 = header_v1(&log.header);
        let mut prev = v1.state_hash();
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&MIN_READABLE_VERSION.to_le_bytes());
        write_blob(&mut out, &v1).unwrap();
        for segment in &log.segments {
            let segment = segment_v2(segment);
            let mut segment = segment;
            segment.prev = prev;
            segment.hash = segment.compute_hash();
            prev = segment.hash;
            write_blob(&mut out, &segment).unwrap();
        }
        out
    }

    /// A segment as the version-2 build wrote it: the same contents, without
    /// the refusals.
    fn segment_v2(segment: &Segment) -> SegmentV2 {
        SegmentV2 {
            first_tick: segment.first_tick,
            last_tick: segment.last_tick,
            decisions: segment.decisions.clone(),
            hashes: segment.hashes.clone(),
            prev: segment.prev,
            hash: segment.hash,
        }
    }

    /// The bytes of a version-2 log: the current header with the old version,
    /// and the segments without the refusals.
    fn to_v2_bytes(log: &RoundLog) -> Vec<u8> {
        let mut header = log.header.clone();
        header.proto_version = 2;
        let mut prev = header.hash();
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&2u16.to_le_bytes());
        write_blob(&mut out, &header).unwrap();
        for segment in &log.segments {
            let mut segment = segment_v2(segment);
            segment.prev = prev;
            segment.hash = segment.compute_hash();
            prev = segment.hash;
            write_blob(&mut out, &segment).unwrap();
        }
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
    fn v99_log_rejected_by_name() {
        let log = recorded(900, 300);
        let mut bytes = to_bytes(&log);

        // The version sits right after the magic.
        let at = MAGIC.len();
        bytes[at..at + 2].copy_from_slice(&99u16.to_le_bytes());

        let err = RoundLog::read(&mut io::Cursor::new(bytes)).unwrap_err();
        assert_eq!(err, LogError::Version { found: 99 });
    }

    #[test]
    fn a_version_outside_the_gate_is_refused_by_name() {
        let log = recorded(900, 300);
        for version in [0u16, PROTO_VERSION + 1] {
            let mut bytes = to_bytes(&log);
            let at = MAGIC.len();
            bytes[at..at + 2].copy_from_slice(&version.to_le_bytes());
            let err = RoundLog::read(&mut io::Cursor::new(bytes)).unwrap_err();
            assert_eq!(err, LogError::Version { found: version });
        }
    }

    #[test]
    fn a_v1_log_reads_and_migrates() {
        let log = recorded(900, 300);
        let bytes = to_v1_bytes(&log);

        let read = RoundLog::read(&mut io::Cursor::new(bytes)).expect("the v1 log reads");
        assert_eq!(read.header.proto_version, PROTO_VERSION);
        assert_eq!(read.header.talk, TalkParams::default());
        // The state hashes are the ones the log recorded.
        assert_eq!(read.hashes(), log.hashes());
        // The chain is re-rooted over the migrated header.
        assert_eq!(read.segments[0].prev, read.header.hash());
        assert!(read.verify().is_ok());

        // And it writes back as a log of the current version.
        let mut again = Vec::new();
        read.write(&mut again).unwrap();
        assert_eq!(
            &again[MAGIC.len()..MAGIC.len() + 2],
            &PROTO_VERSION.to_le_bytes()
        );
        let reread = RoundLog::read(&mut io::Cursor::new(again)).expect("the v2 log reads");
        assert_eq!(reread, read);
    }

    #[test]
    fn the_migration_moves_the_header_and_the_chain_and_not_the_world() {
        let log = recorded(900, 300);
        let bytes = to_v1_bytes(&log);

        let v1 = RoundLog::read_v1(&mut io::Cursor::new(bytes.clone())).expect("the v1 log reads");
        let migrated = RoundLog::read(&mut io::Cursor::new(bytes)).expect("the v1 log migrates");

        // The header is a new identity...
        assert_ne!(v1.header.state_hash(), migrated.header.hash());
        // ...and so is the chain...
        assert_ne!(v1.segments[0].prev, migrated.segments[0].prev);
        assert_ne!(v1.segments[0].hash, migrated.segments[0].hash);
        // ...but the world the log records did not change.
        let v1_hashes: Vec<(Tick, u64)> = v1
            .segments
            .iter()
            .flat_map(|segment| segment.hashes.clone())
            .collect();
        assert_eq!(v1_hashes, migrated.hashes());
        assert_eq!(v1_hashes, log.hashes());
    }

    #[test]
    fn a_v1_log_keeps_its_own_identity_before_the_migration() {
        // The shim decodes the old header exactly as phase 1 did: the chain
        // of a version-1 file is rooted in the shim's hash.
        let log = recorded(900, 300);
        let bytes = to_v1_bytes(&log);
        let v1 = RoundLog::read_v1(&mut io::Cursor::new(bytes)).expect("the v1 log reads");
        assert_eq!(v1.header.proto_version, MIN_READABLE_VERSION);
        assert_eq!(v1.segments[0].prev, v1.header.state_hash());
    }

    #[test]
    fn reading_a_current_log_as_a_v1_log_is_refused() {
        let log = recorded(900, 300);
        let bytes = to_bytes(&log);
        let err = RoundLog::read_v1(&mut io::Cursor::new(bytes)).unwrap_err();
        assert_eq!(
            err,
            LogError::Version {
                found: PROTO_VERSION
            }
        );
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
            refusals: &seg.refusals,
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

    /// A round with a refusal in it: one refusal every 100 ticks.
    fn recorded_with_refusals(ticks: u64, per_segment: u32) -> RoundLog {
        let mut rec = Recorder::new(header(), per_segment);
        for t in 0..ticks {
            let agent = u16::try_from(t % 4).unwrap();
            let seq = u32::try_from(t).unwrap();
            let applied = vec![stamped(agent, t, seq)];
            rec.record(t, &applied, None);
            if t % 100 == 0 {
                rec.record_refusal(Refused::new(t, agent, RefusalKind::Timeout));
            }
        }
        rec.finish()
    }

    #[test]
    fn a_refusal_is_in_the_record_and_in_the_chain() {
        let log = recorded_with_refusals(900, 300);
        let refusals = log.refusals();
        assert_eq!(refusals.len(), 9);
        assert_eq!(refusals[0], Refused::new(0, 0, RefusalKind::Timeout));
        assert_eq!(refusals[8].tick, 800);

        // The refusals travel with the log, byte for byte.
        let bytes = to_bytes(&log);
        let read = RoundLog::read(&mut io::Cursor::new(bytes)).expect("the log reads");
        assert_eq!(read.refusals(), refusals);
        assert!(read.verify().is_ok());

        // And they are sealed: an edited refusal breaks the chain.
        let mut edited = log.clone();
        edited.segments[0].refusals[0].detail = 7;
        assert_eq!(
            edited.verify().unwrap_err(),
            LogError::ChainBroken { segment: 1 }
        );
    }

    #[test]
    fn a_refusal_only_segment_is_still_a_segment() {
        // A refusal that happens between two ticks opens its own segment:
        // it must not be dropped on the way into the chain.
        let mut rec = Recorder::new(header(), 0);
        rec.record_refusal(Refused::new(5, 1, RefusalKind::Budget));
        let log = rec.finish();
        assert_eq!(log.segments.len(), 1);
        assert_eq!(log.segments[0].first_tick, 5);
        assert_eq!(log.segments[0].refusals.len(), 1);
        assert!(log.verify().is_ok());
    }

    #[test]
    fn a_v2_log_reads_and_migrates() {
        let log = recorded_with_refusals(900, 300);
        let bytes = to_v2_bytes(&log);

        let read = RoundLog::read(&mut io::Cursor::new(bytes)).expect("the v2 log reads");
        assert_eq!(read.header.proto_version, PROTO_VERSION);
        // A version-2 log recorded no refusals, so the migration has none.
        assert!(read.refusals().is_empty());
        // The state hashes are the ones the log recorded.
        assert_eq!(read.hashes(), log.hashes());
        // The chain is re-rooted over the migrated header.
        assert_eq!(read.segments[0].prev, read.header.hash());
        assert!(read.verify().is_ok());

        // And it writes back as a log of the current version.
        let mut again = Vec::new();
        read.write(&mut again).unwrap();
        assert_eq!(
            &again[MAGIC.len()..MAGIC.len() + 2],
            &PROTO_VERSION.to_le_bytes()
        );
        let reread = RoundLog::read(&mut io::Cursor::new(again)).expect("the v3 log reads");
        assert_eq!(reread, read);
    }

    /// The pinned bytes of one refusal, and the pinned hash of the segment
    /// that holds it (D69): the log is a contract between machines, so the
    /// bytes have to be the same on x86-64 and aarch64 or the chain breaks
    /// the moment a log crosses machines.
    ///
    /// The record is whole numbers in postcard varints and the hash is
    /// FNV-1a over them — machine-independent by construction, and pinned
    /// here so a refactor that breaks the promise cannot pass quietly. The
    /// constants were produced on the aarch64 box (Jetson Thor); an x86-64
    /// run of this same test reads the same ones (D69's pattern). If this
    /// fails after an intentional format change, re-pin by running
    /// `REFUSAL_PRINT=1 cargo test -p lm-core --lib refusal -- --nocapture`
    /// and pasting what it prints — and say so in the commit.
    #[test]
    fn a_refusals_bytes_and_chain_hash_are_pinned() {
        let refused = Refused::new(40, 3, RefusalKind::Timeout).with_detail(503);
        let bytes = postcard::to_allocvec(&refused).expect("a refusal encodes");
        let segment = Segment {
            first_tick: 40,
            last_tick: 40,
            decisions: Vec::new(),
            hashes: Vec::new(),
            refusals: vec![refused],
            prev: 0x1234_5678_9abc_def0,
            hash: 0,
        };
        let hash = segment.compute_hash();
        if std::env::var("REFUSAL_PRINT").is_ok() {
            println!("bytes: {bytes:?}");
            println!("hash: 0x{hash:016x}");
        }
        // tick 40, agent 3, kind 0 (Timeout), detail 503 — five varint bytes.
        assert_eq!(bytes, vec![40, 3, 0, 247, 3]);
        assert_eq!(hash, 0x7bbe_6854_6322_a489);
    }
}

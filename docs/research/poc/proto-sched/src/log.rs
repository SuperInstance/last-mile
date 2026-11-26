//! The decision log: a fixed little-endian binary encoding, written and read
//! by hand.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! file  := MAGIC record*
//! MAGIC := "SPLOG\x01"                       (6 bytes)
//! record:= len:u32 body:len bytes
//! body  := tag:u8 apply_tick:u64 source:u16 seq:u32 payload
//! ```
//!
//! `len` is the size of `body`, so a record can be skipped or split on a
//! record boundary without decoding it. Payloads:
//!
//! ```text
//! ActionOn: action:u8
//! Lane:     gate:u8 p0:u8 p1:u8 p2:u8 p3:u8
//! Intent:   ttl:u16 plan_len:u16 safe_len:u16 plan*[i16,i16] safe*[i16,i16]
//! Marker:   value:u32
//! ```

use crate::types::{Kind, Stamped};

pub const MAGIC: [u8; 6] = *b"SPLOG\x01";

/// Bytes of the fixed part of a record: len(4) + tag(1) + tick(8) + source(2)
/// + seq(4).
pub const RECORD_HEADER_BYTES: usize = 19;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogError {
    BadMagic,
    Truncated,
    UnknownTag(u8),
    BadLength,
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::BadMagic => write!(f, "bad log magic"),
            LogError::Truncated => write!(f, "truncated log record"),
            LogError::UnknownTag(t) => write!(f, "unknown record tag {t}"),
            LogError::BadLength => write!(f, "bad record length"),
        }
    }
}

impl std::error::Error for LogError {}

fn kind_tag(kind: &Kind) -> u8 {
    kind.tag()
}

/// Encode one stamped decision into `buf` (length prefix included).
pub fn encode_record(buf: &mut Vec<u8>, s: &Stamped) {
    let mut body = Vec::with_capacity(32);
    body.push(kind_tag(&s.kind));
    body.extend_from_slice(&s.apply_tick.to_le_bytes());
    body.extend_from_slice(&s.source.to_le_bytes());
    body.extend_from_slice(&s.seq.to_le_bytes());
    match &s.kind {
        Kind::ActionOn { action } => body.push(*action),
        Kind::Lane { gate, p } => {
            body.push(*gate);
            body.extend_from_slice(p);
        }
        Kind::Intent { ttl, plan, safe } => {
            body.extend_from_slice(&ttl.to_le_bytes());
            body.extend_from_slice(&(plan.len() as u16).to_le_bytes());
            body.extend_from_slice(&(safe.len() as u16).to_le_bytes());
            for &(x, y) in plan {
                body.extend_from_slice(&x.to_le_bytes());
                body.extend_from_slice(&y.to_le_bytes());
            }
            for &(x, y) in safe {
                body.extend_from_slice(&x.to_le_bytes());
                body.extend_from_slice(&y.to_le_bytes());
            }
        }
        Kind::Marker(v) => body.extend_from_slice(&v.to_le_bytes()),
    }
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
}

/// Encode a whole log (magic + records).
pub fn encode_log(records: &[Stamped]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(records.len() * 24 + MAGIC.len());
    buf.extend_from_slice(&MAGIC);
    for r in records {
        encode_record(&mut buf, r);
    }
    buf
}

fn decode_body(body: &[u8]) -> Result<Stamped, LogError> {
    if body.len() < 15 {
        return Err(LogError::Truncated);
    }
    let tag = body[0];
    let apply_tick = u64::from_le_bytes(body[1..9].try_into().unwrap());
    let source = u16::from_le_bytes(body[9..11].try_into().unwrap());
    let seq = u32::from_le_bytes(body[11..15].try_into().unwrap());
    let rest = &body[15..];
    let kind = match tag {
        0 => {
            if rest.len() < 1 {
                return Err(LogError::Truncated);
            }
            Kind::ActionOn { action: rest[0] }
        }
        1 => {
            if rest.len() < 5 {
                return Err(LogError::Truncated);
            }
            Kind::Lane { gate: rest[0], p: [rest[1], rest[2], rest[3], rest[4]] }
        }
        2 => {
            if rest.len() < 6 {
                return Err(LogError::Truncated);
            }
            let ttl = u16::from_le_bytes(rest[0..2].try_into().unwrap());
            let plan_len = u16::from_le_bytes(rest[2..4].try_into().unwrap()) as usize;
            let safe_len = u16::from_le_bytes(rest[4..6].try_into().unwrap()) as usize;
            let need = 6 + 4 * (plan_len + safe_len);
            if rest.len() < need {
                return Err(LogError::Truncated);
            }
            let mut plan = Vec::with_capacity(plan_len);
            let mut off = 6;
            for _ in 0..plan_len {
                let x = i16::from_le_bytes(rest[off..off + 2].try_into().unwrap());
                let y = i16::from_le_bytes(rest[off + 2..off + 4].try_into().unwrap());
                plan.push((x, y));
                off += 4;
            }
            let mut safe = Vec::with_capacity(safe_len);
            for _ in 0..safe_len {
                let x = i16::from_le_bytes(rest[off..off + 2].try_into().unwrap());
                let y = i16::from_le_bytes(rest[off + 2..off + 4].try_into().unwrap());
                safe.push((x, y));
                off += 4;
            }
            Kind::Intent { ttl, plan, safe }
        }
        3 => {
            if rest.len() < 4 {
                return Err(LogError::Truncated);
            }
            Kind::Marker(u32::from_le_bytes(rest[0..4].try_into().unwrap()))
        }
        other => return Err(LogError::UnknownTag(other)),
    };
    Ok(Stamped {
        apply_tick,
        source,
        seq,
        kind,
        arrival_tick: 0,
        late: false,
        lateness: 0,
    })
}

/// Decode a whole log.
pub fn decode_log(bytes: &[u8]) -> Result<Vec<Stamped>, LogError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != &MAGIC {
        return Err(LogError::BadMagic);
    }
    decode_records(&bytes[MAGIC.len()..])
}

/// Decode a bare sequence of records (no magic header).
pub fn decode_records(bytes: &[u8]) -> Result<Vec<Stamped>, LogError> {
    let mut out = Vec::new();
    let mut off = 0;
    while off < bytes.len() {
        if bytes.len() - off < 4 {
            return Err(LogError::Truncated);
        }
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if bytes.len() - off < len {
            return Err(LogError::Truncated);
        }
        out.push(decode_body(&bytes[off..off + len])?);
        off += len;
    }
    Ok(out)
}

/// Byte offsets of every record boundary: index 0 is the start of the first
/// record, the last entry is the end of the log.
pub fn record_offsets(bytes: &[u8]) -> Result<Vec<usize>, LogError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != &MAGIC {
        return Err(LogError::BadMagic);
    }
    let mut offsets = vec![MAGIC.len()];
    let mut off = MAGIC.len();
    while off < bytes.len() {
        if bytes.len() - off < 4 {
            return Err(LogError::Truncated);
        }
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if bytes.len() - off < len {
            return Err(LogError::Truncated);
        }
        off += len;
        offsets.push(off);
    }
    Ok(offsets)
}

/// Split a log after `n_records` records: returns `(prefix, rest)`.
/// The prefix keeps the magic, so it is a valid log on its own.
pub fn split_log(bytes: &[u8], n_records: usize) -> Result<(Vec<u8>, Vec<u8>), LogError> {
    let offsets = record_offsets(bytes)?;
    if n_records + 1 > offsets.len() {
        return Err(LogError::BadLength);
    }
    let cut = offsets[n_records];
    Ok((bytes[..cut].to_vec(), bytes[cut..].to_vec()))
}

/// Number of records in a log.
pub fn record_count(bytes: &[u8]) -> Result<usize, LogError> {
    Ok(record_offsets(bytes)?.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(kind: Kind, tick: u64, source: u16, seq: u32) -> Stamped {
        Stamped {
            apply_tick: tick,
            source,
            seq,
            kind,
            arrival_tick: 0,
            late: false,
            lateness: 0,
        }
    }

    #[test]
    fn roundtrip_all_kinds() {
        let recs = vec![
            st(Kind::ActionOn { action: 3 }, 10, 1, 7),
            st(Kind::Lane { gate: 2, p: [10, 20, 30, 40] }, 11, 2, 8),
            st(
                Kind::Intent {
                    ttl: 30,
                    plan: vec![(1, -2), (3, 4)],
                    safe: vec![(-5, 6)],
                },
                12,
                3,
                9,
            ),
            st(Kind::Marker(0xDEAD_BEEF), 13, 4, 10),
        ];
        let bytes = encode_log(&recs);
        let back = decode_log(&bytes).unwrap();
        assert_eq!(back.len(), recs.len());
        for (a, b) in recs.iter().zip(back.iter()) {
            assert_eq!(a.apply_tick, b.apply_tick);
            assert_eq!(a.source, b.source);
            assert_eq!(a.seq, b.seq);
            assert_eq!(a.kind, b.kind);
        }
    }

    #[test]
    fn byte_costs_per_kind() {
        let a = encode_log(&[st(Kind::ActionOn { action: 1 }, 1, 1, 1)]);
        let l = encode_log(&[st(Kind::Lane { gate: 0, p: [1, 2, 3, 4] }, 1, 1, 1)]);
        let m = encode_log(&[st(Kind::Marker(7), 1, 1, 1)]);
        let i0 = encode_log(&[st(
            Kind::Intent { ttl: 1, plan: vec![], safe: vec![] },
            1,
            1,
            1,
        )]);
        let i1 = encode_log(&[st(
            Kind::Intent { ttl: 1, plan: vec![(0, 0)], safe: vec![(0, 0)] },
            1,
            1,
            1,
        )]);
        assert_eq!(a.len() - MAGIC.len(), 20);
        assert_eq!(l.len() - MAGIC.len(), 24);
        assert_eq!(m.len() - MAGIC.len(), 23);
        assert_eq!(i0.len() - MAGIC.len(), 25);
        assert_eq!(i1.len() - MAGIC.len(), 33);
    }

    #[test]
    fn truncated_log_is_rejected() {
        let recs = vec![st(Kind::Marker(7), 1, 1, 1)];
        let bytes = encode_log(&recs);
        assert_eq!(decode_log(&bytes[..bytes.len() - 2]), Err(LogError::Truncated));
        assert_eq!(decode_log(b"nope"), Err(LogError::BadMagic));
    }

    #[test]
    fn split_on_record_boundary() {
        let recs: Vec<Stamped> = (0..5)
            .map(|i| st(Kind::Marker(i as u32), i, 0, i as u32))
            .collect();
        let bytes = encode_log(&recs);
        assert_eq!(record_count(&bytes).unwrap(), 5);
        let (prefix, rest) = split_log(&bytes, 2).unwrap();
        assert_eq!(record_count(&prefix).unwrap(), 2);
        assert_eq!(record_count(&rest.clone()).unwrap_err(), LogError::BadMagic);
        let mut rejoined = prefix;
        rejoined.extend_from_slice(&rest);
        assert_eq!(rejoined, bytes);
    }
}

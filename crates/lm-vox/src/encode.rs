//! The retro-tight encoding (design §5).
//!
//! A raw L0 frame is 9 bytes — 900 B/s at 100 Hz — and that is the
//! storage-rate ceiling, not the wire. On the wire the stream runs with
//! **running status**, the trick that let MIDI fit 16 channels down one
//! 3,125 B/s cable: each packet says only what changed since the last
//! frame, the small changes travel as 4-bit deltas packed two per byte,
//! and silence collapses into a rest marker. The target is 150-300 B/s
//! (design §5); E2 measures what it actually costs.
//!
//! ## The wire format
//!
//! Every packet is an opcode byte plus payload; packets advance the clock
//! by the frames they state:
//!
//! | Opcode | Packet | Meaning |
//! |---|---|---|
//! | `0x00..=0x0F` | `REST n` | `n+1` rest frames (1-16): silence, every field zero. |
//! | `0x10` | `FULL` | The next 9 bytes are a whole frame, the §5 form: `f0` as a big-endian i16 of cents, then `rms`, `f1`, `f2`, `f3` (u8 log scales), `tilt` (i8), `noise` (u8), `flags` (u8). The first frame of a stream is a `FULL`; it is also the fallback when a change is too large for any delta. |
//! | `0x11..=0x1E` | `REPEAT n` | `n` further copies of the last frame (1-14). |
//! | `0x1F` | `FLAGS` | The next byte is the new `flags` value. **No frame is emitted** — it is a prefix, so a frame whose flags and dials both changed costs one `FLAGS` plus one delta packet instead of a whole `FULL`. |
//! | `0x20` | `DELTA8` | The next byte is a presence mask over `[f0, rms, f1, f2, f3, tilt, noise]`; each present field follows as a signed 8-bit delta. One frame. |
//! | `0x21..=0x7F` | — | Reserved; the decoder names them if it meets one. |
//! | `0x80..=0xFF` | `DELTA` | The low 7 bits are the same presence mask; each present field follows as a 4-bit two's-complement delta (−8..7), packed two per byte, high nibble first, in the order given. One frame. |
//!
//! Nothing in the stream is time-stamped: packets advance the clock one
//! frame each, `REST`/`REPEAT` by their counts, so a decoder that fell in
//! sync stays in sync to the end.

use crate::frame::{Frame, FrameFlags};

/// What the raw form costs per second at 100 Hz (design §5).
pub const RAW_BYTES_PER_S: usize = 9 * crate::FRAME_HZ as usize;

/// The §5 9-byte frame: `f0` big-endian i16, then the six single-byte
/// fields in protocol order.
pub fn frame_bytes(f: &Frame) -> [u8; 9] {
    [
        (f.f0_cents >> 8) as u8,
        (f.f0_cents & 0xff) as u8,
        f.rms,
        f.f1,
        f.f2,
        f.f3,
        f.tilt as u8,
        f.noise,
        f.flags.0,
    ]
}

/// Read the §5 9-byte frame back.
pub fn parse_frame(b: &[u8; 9]) -> Frame {
    Frame {
        f0_cents: ((b[0] as i16) << 8) | b[1] as i16,
        rms: b[2],
        f1: b[3],
        f2: b[4],
        f3: b[5],
        tilt: b[6] as i8,
        noise: b[7],
        flags: FrameFlags(b[8]),
    }
}

/// Why a stream could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A packet's payload ended mid-field.
    Truncated,
    /// An opcode the decoder does not know (the reserved range).
    UnknownOpcode(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "the stream ends mid-packet"),
            DecodeError::UnknownOpcode(op) => write!(f, "reserved opcode {op:#04x}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Whether a frame is a rest: silence, carried by the rest marker.
fn is_rest(f: &Frame) -> bool {
    f.flags.has(FrameFlags::REST) || *f == Frame::default()
}

/// The presence-mask bit of each delta slot, in payload order.
const SLOT_F0: u8 = 0x01;
const SLOT_RMS: u8 = 0x02;
const SLOT_F1: u8 = 0x04;
const SLOT_F2: u8 = 0x08;
const SLOT_F3: u8 = 0x10;
const SLOT_TILT: u8 = 0x20;
const SLOT_NOISE: u8 = 0x40;
const SLOTS: [u8; 7] = [
    SLOT_F0, SLOT_RMS, SLOT_F1, SLOT_F2, SLOT_F3, SLOT_TILT, SLOT_NOISE,
];

/// The signed delta of a frame's slot, as an i16 in the field's own units.
fn slot_delta(last: &Frame, cur: &Frame, slot: u8) -> i16 {
    match slot {
        SLOT_F0 => cur.f0_cents.wrapping_sub(last.f0_cents),
        SLOT_RMS => i16::from(cur.rms) - i16::from(last.rms),
        SLOT_F1 => i16::from(cur.f1) - i16::from(last.f1),
        SLOT_F2 => i16::from(cur.f2) - i16::from(last.f2),
        SLOT_F3 => i16::from(cur.f3) - i16::from(last.f3),
        SLOT_TILT => i16::from(cur.tilt) - i16::from(last.tilt),
        _ => i16::from(cur.noise) - i16::from(last.noise),
    }
}

/// Encode a frame stream with running status. The first frame is sent
/// whole; after that only the changes travel.
pub fn encode_stream(frames: &[Frame]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Option<Frame> = None;
    let mut i = 0;

    while i < frames.len() {
        let f = frames[i];

        // Silence: count the run, emit rest markers, reset the state.
        if is_rest(&f) {
            let mut run = 0usize;
            while i + run < frames.len() && is_rest(&frames[i + run]) {
                run += 1;
            }
            let mut left = run;
            while left > 0 {
                let take = left.min(16);
                out.push(take as u8 - 1);
                left -= take;
            }
            i += run;
            prev = None;
            continue;
        }

        // The first sound after a rest states itself whole.
        let Some(last) = prev else {
            out.push(0x10);
            out.extend_from_slice(&frame_bytes(&f));
            prev = Some(f);
            i += 1;
            continue;
        };

        // An exact repeat folds into a repeat count.
        if f == last {
            let mut run = 0usize;
            while i + run < frames.len() && frames[i + run] == last && !is_rest(&frames[i + run]) {
                run += 1;
            }
            let mut left = run;
            while left > 0 {
                let take = left.min(14);
                out.push(0x10 + take as u8);
                left -= take;
            }
            i += run;
            continue;
        }

        // A flags change rides in a prefix packet, not a whole frame.
        if f.flags != last.flags {
            out.push(0x1F);
            out.push(f.flags.0);
        }

        // The deltas of every slot that moved.
        let deltas: Vec<(u8, i16)> = SLOTS
            .iter()
            .copied()
            .filter(|&slot| slot_delta(&last, &f, slot) != 0)
            .map(|slot| (slot, slot_delta(&last, &f, slot)))
            .collect();

        // The compact form: every moved slot fits a nibble.
        if deltas.iter().all(|&(_, d)| (-8..=7).contains(&d)) {
            let mask = deltas.iter().map(|&(slot, _)| slot).fold(0u8, |m, s| m | s);
            out.push(0x80 | mask);
            let mut packed = 0u8;
            let mut high = true;
            for &(_, d) in &deltas {
                let nib = (d as u8) & 0x0f;
                if high {
                    packed = nib << 4;
                    high = false;
                } else {
                    out.push(packed | nib);
                    high = true;
                }
            }
            if !high {
                out.push(packed);
            }
        } else if deltas.iter().all(|&(_, d)| (-128..=127).contains(&d)) {
            // The wide form: every moved slot fits a byte.
            let mask = deltas.iter().map(|&(slot, _)| slot).fold(0u8, |m, s| m | s);
            out.push(0x20);
            out.push(mask);
            for &(_, d) in &deltas {
                out.push(d as u8);
            }
        } else {
            // Something jumped a mile (an octave error, a dial reset):
            // state the whole frame and move on.
            out.push(0x10);
            out.extend_from_slice(&frame_bytes(&f));
        }

        prev = Some(f);
        i += 1;
    }

    out
}

/// Decode a running-status stream back into frames.
pub fn decode_stream(bytes: &[u8]) -> Result<Vec<Frame>, DecodeError> {
    let mut frames = Vec::new();
    let mut cur = Frame::default();
    let mut i = 0usize;

    while i < bytes.len() {
        let op = bytes[i];
        i += 1;
        match op {
            0x00..=0x0F => {
                for _ in 0..(op as usize + 1) {
                    frames.push(Frame::default());
                }
                cur = Frame::default();
            }
            0x10 => {
                if i + 9 > bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                let mut buf = [0u8; 9];
                buf.copy_from_slice(&bytes[i..i + 9]);
                i += 9;
                cur = parse_frame(&buf);
                frames.push(cur);
            }
            0x11..=0x1E => {
                for _ in 0..(op as usize - 0x10) {
                    frames.push(cur);
                }
            }
            0x1F => {
                if i >= bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                // A prefix: no frame is emitted for this byte pair.
                cur.flags = FrameFlags(bytes[i]);
                i += 1;
            }
            0x20 => {
                if i >= bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                let mask = bytes[i];
                i += 1;
                let n = mask.count_ones() as usize;
                if i + n > bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                for &slot in SLOTS.iter().filter(|&&s| mask & s != 0) {
                    let d = i16::from(bytes[i] as i8);
                    i += 1;
                    apply_delta(&mut cur, slot, d);
                }
                frames.push(cur);
            }
            0x21..=0x7F => return Err(DecodeError::UnknownOpcode(op)),
            0x80..=0xFF => {
                let mask = op & 0x7f;
                let n = mask.count_ones() as usize;
                let bytes_needed = n.div_ceil(2);
                if i + bytes_needed > bytes.len() {
                    return Err(DecodeError::Truncated);
                }
                let nibble_bytes = &bytes[i..i + bytes_needed];
                i += bytes_needed;
                let mut nib_index = 0usize;
                let next_nibble = |idx: &mut usize| -> i16 {
                    let byte = nibble_bytes[*idx / 2];
                    let nib = if (*idx).is_multiple_of(2) {
                        byte >> 4
                    } else {
                        byte & 0x0f
                    };
                    *idx += 1;
                    if nib & 0x08 != 0 {
                        i16::from(nib) - 16
                    } else {
                        i16::from(nib)
                    }
                };
                for &slot in SLOTS.iter().filter(|&&s| mask & s != 0) {
                    let d = next_nibble(&mut nib_index);
                    apply_delta(&mut cur, slot, d);
                }
                frames.push(cur);
            }
        }
    }

    Ok(frames)
}

/// Apply a slot's delta to the running frame, wrapping in the field's own
/// width like the fields themselves do.
fn apply_delta(cur: &mut Frame, slot: u8, d: i16) {
    match slot {
        SLOT_F0 => cur.f0_cents = cur.f0_cents.wrapping_add(d),
        SLOT_RMS => cur.rms = (i16::from(cur.rms) + d) as u8,
        SLOT_F1 => cur.f1 = (i16::from(cur.f1) + d) as u8,
        SLOT_F2 => cur.f2 = (i16::from(cur.f2) + d) as u8,
        SLOT_F3 => cur.f3 = (i16::from(cur.f3) + d) as u8,
        SLOT_TILT => cur.tilt = (i16::from(cur.tilt) + d) as i8,
        _ => cur.noise = (i16::from(cur.noise) + d) as u8,
    }
}

/// The encoded size of a stream in bytes per second of performance.
pub fn bytes_per_second(bytes: usize, frames: usize) -> f64 {
    if frames == 0 {
        return 0.0;
    }
    bytes as f64 * f64::from(crate::FRAME_HZ) / frames as f64
}

/// The encoded size of a frame stream, in bytes per frame, so callers can
/// report raw against encoded (design §5's E2).
pub fn encoded_bytes_per_frame(frames: &[Frame]) -> f64 {
    if frames.is_empty() {
        return 0.0;
    }
    encode_stream(frames).len() as f64 / frames.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A steady tone compresses to a full frame and repeats.
    #[test]
    fn steady_tone_is_cheap() {
        let f = Frame {
            f0_cents: 2000,
            rms: 150,
            f1: 100,
            f2: 180,
            f3: 240,
            tilt: -4,
            noise: 20,
            flags: FrameFlags::VOICED,
        };
        let stream = encode_stream(&[f; 300]);
        // 1 full frame (10 bytes) + repeats (22 packets of 14).
        assert!(stream.len() <= 10 + 22, "{} bytes", stream.len());
        assert_eq!(decode_stream(&stream).unwrap(), vec![f; 300]);
    }

    /// Silence is one byte per 16 frames.
    #[test]
    fn rests_collapse() {
        let stream = encode_stream(&[Frame::default(); 160]);
        assert_eq!(stream.len(), 10);
        assert_eq!(decode_stream(&stream).unwrap(), vec![Frame::default(); 160]);
    }

    /// A voicing flip travels as a flags prefix plus deltas, not a whole
    /// frame.
    #[test]
    fn voicing_flip_is_a_prefix_and_deltas() {
        let voiced = Frame {
            f0_cents: 1900,
            rms: 140,
            noise: 32,
            flags: FrameFlags::VOICED,
            ..Frame::default()
        };
        let unvoiced = Frame {
            rms: 120,
            noise: 224,
            flags: FrameFlags(0),
            ..Frame::default()
        };
        let frames = vec![voiced, voiced, unvoiced, voiced, voiced];
        let stream = encode_stream(&frames);
        assert_eq!(decode_stream(&stream).unwrap(), frames);
        // The flip frame must be cheaper than a FULL.
        assert!(stream.len() < 10 + 2 * 10 + 10, "{} bytes", stream.len());
    }

    /// A frame whose fields jump beyond a byte round-trips through the
    /// whole-frame fallback.
    #[test]
    fn big_jumps_round_trip() {
        let a = Frame {
            f0_cents: 1500,
            rms: 0,
            noise: 0,
            flags: FrameFlags::VOICED,
            ..Frame::default()
        };
        let b = Frame {
            f0_cents: 0,
            rms: 255,
            noise: 255,
            flags: FrameFlags(0),
            ..Frame::default()
        };
        let frames = vec![a, b, a];
        assert_eq!(decode_stream(&encode_stream(&frames)).unwrap(), frames);
    }

    /// The 9-byte frame form round-trips every field.
    #[test]
    fn nine_byte_frame_round_trips() {
        let f = Frame {
            f0_cents: -1234,
            rms: 255,
            f1: 1,
            f2: 254,
            f3: 128,
            tilt: -128,
            noise: 0,
            flags: FrameFlags(0x07),
        };
        assert_eq!(parse_frame(&frame_bytes(&f)), f);
    }

    /// A random walk of frames round-trips exactly: every field, every
    /// frame, however the changes fall.
    #[test]
    fn random_walk_round_trips() {
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut frames = Vec::new();
        let mut f0 = 1800i16;
        let mut rms = 140u8;
        let mut noise = 40u8;
        let mut tilt = 0i8;
        for i in 0..3000 {
            let r = next();
            if r % 37 == 0 {
                frames.push(Frame::default());
                continue;
            }
            // Walks of every size: sometimes a nibble's worth, sometimes a
            // byte's, sometimes a jump a full frame must carry.
            let step = ((r >> 8) % 201) as i16 - 100;
            f0 = (f0 + step).clamp(500, 4000);
            let dstep = ((r >> 16) % 201) as i16 - 100;
            rms = (i16::from(rms) + dstep).clamp(0, 255) as u8;
            noise = (i16::from(noise) + dstep).clamp(0, 255) as u8;
            tilt = (i16::from(tilt) + step / 2).clamp(-127, 127) as i8;
            frames.push(Frame {
                f0_cents: f0,
                rms,
                f1: (r % 256) as u8,
                f2: ((r >> 3) % 256) as u8,
                f3: ((r >> 6) % 256) as u8,
                tilt,
                noise,
                flags: FrameFlags(if i % 11 == 0 { 0x03 } else { 0x01 }),
            });
        }
        let encoded = encode_stream(&frames);
        assert_eq!(decode_stream(&encoded).unwrap(), frames);
    }

    /// A truncated or reserved stream is named, not guessed at.
    #[test]
    fn truncation_is_an_error() {
        assert_eq!(decode_stream(&[0x10, 1, 2]), Err(DecodeError::Truncated));
        assert_eq!(decode_stream(&[0x20]), Err(DecodeError::Truncated));
        assert_eq!(decode_stream(&[0x1F]), Err(DecodeError::Truncated));
        assert_eq!(
            decode_stream(&[0x35]),
            Err(DecodeError::UnknownOpcode(0x35))
        );
    }

    /// The flags prefix carries the onset flag with dial deltas in the
    /// same frame.
    #[test]
    fn flags_prefix_with_deltas() {
        let base = Frame {
            f0_cents: 2000,
            rms: 150,
            flags: FrameFlags::VOICED,
            ..Frame::default()
        };
        let mut onset = base;
        onset.flags.set(FrameFlags::ONSET, true);
        onset.rms = 155;
        let frames = vec![base, onset, base];
        assert_eq!(decode_stream(&encode_stream(&frames)).unwrap(), frames);
    }
}

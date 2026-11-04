//! Vox Score: speech as a performance, not a recording (docs/design/vox-score.md).
//!
//! The crate is the protocol's core (design §10): the four layers (L0 frame,
//! L1 gesture, L2 phrase, L3 intent), the speaker profile, the retro-tight
//! encoder, the analyzer that measures L0 from PCM, the gesture and phrase
//! derivations, and the LPC/formant renderer that plays an L0 stream back.
//! It depends on nothing — not on lm-core, not on any crate — because the
//! protocol must be portable by itself.
//!
//! # The layers
//!
//! L0 is a frame at 100 Hz: pitch, level, formants, tilt, noise, flags. L1
//! segments L0 into gestures (a syllable's worth). L2 compresses an utterance
//! into the patch summary a decision model reads as one text line. L3 is the
//! gate answers plus the words. Every derivation except L3 is mechanical and
//! deterministic.
//!
//! # Determinism
//!
//! The analyzer never calls a libm transcendental: `cos` and `log` can differ
//! in the last bit between libms, and those bits reach L2 and L3. Instead it
//! uses committed lookup tables ([`tables`]) with linear interpolation, and
//! IEEE-754 `+ - * / sqrt` only — every one of those is correctly rounded
//! and bit-exact on x86-64 and aarch64 alike, and no fused multiply-add is
//! used anywhere. L0 quantizes each frame to integer fields, so L1, L2 and
//! the encoder are pure integer arithmetic downstream. The
//! `tests/golden.rs` test pins the whole chain's output by hash.

pub mod analyzer;
pub mod encode;
pub mod fft;
pub mod frame;
pub mod math;
pub mod phrase;
pub mod profile;
pub mod render;
pub mod schedule;
pub mod segment;
pub mod smooth;
pub mod tables;
pub mod wav;

pub use analyzer::{analyze, AnalyzerConfig};
pub use encode::{decode_stream, encode_stream, encoded_bytes_per_frame};
pub use frame::{Frame, FrameFlags, Gesture, Intent, Phrase, F0_REF_HZ};
pub use phrase::{l2_line, phrase_summary};
pub use profile::SpeakerProfile;
pub use render::{render_l0, render_with_words};
pub use schedule::WordTiming;
pub use segment::segment_gestures;
pub use smooth::{smooth_l0, AbgGains, SmoothConfig};

/// The L0 frame rate in Hz: one frame every 10 ms.
pub const FRAME_HZ: u32 = 100;
/// The frame period in milliseconds.
pub const FRAME_MS: u32 = 10;

/// FNV-1a 64-bit, the workspace's non-cryptographic hash (same constants as
/// lm-core's, written here so this crate stays standalone). Used for the
/// golden determinism test; never for security.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

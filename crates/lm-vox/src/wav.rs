//! Minimal RIFF/WAVE I/O: mono PCM16 in, mono PCM16 out.
//!
//! skald answers with 16-bit mono at 22.05 kHz and the microphone capture is
//! 16-bit mono at 16 kHz (design §2), so PCM16 mono is the only container the
//! analyzer needs to read. Anything else is named as an error rather than
//! half-read. No dependency: a WAV header is 44 bytes of little-endian
//! integers.

use std::fmt;

/// Why a WAV could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WavError {
    /// The file is shorter than a RIFF header.
    TooShort,
    /// Not `RIFF....WAVE`.
    NotRiff,
    /// The format chunk says something other than uncompressed PCM.
    NotPcm { format: u16 },
    /// The data is not 16-bit samples.
    NotPcm16 { bits: u16 },
    /// The stream is not mono.
    NotMono { channels: u16 },
    /// The sample rate is not one the analyzer runs at (16 or 22.05 kHz).
    BadRate { rate: u32 },
    /// A chunk length walks past the end of the file.
    BadChunk,
    /// No data chunk was found.
    NoData,
    /// An odd number of bytes where samples were expected.
    OddData,
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WavError::TooShort => write!(f, "shorter than a WAV header"),
            WavError::NotRiff => write!(f, "not a RIFF/WAVE file"),
            WavError::NotPcm { format } => write!(f, "format {format} is not uncompressed PCM"),
            WavError::NotPcm16 { bits } => write!(f, "{bits}-bit samples, not 16-bit"),
            WavError::NotMono { channels } => write!(f, "{channels} channels, not mono"),
            WavError::BadRate { rate } => {
                write!(f, "{rate} Hz: the analyzer runs at 16 kHz or 22.05 kHz")
            }
            WavError::BadChunk => write!(f, "a chunk length walks past the end of the file"),
            WavError::NoData => write!(f, "no data chunk"),
            WavError::OddData => write!(f, "an odd number of data bytes"),
        }
    }
}

impl std::error::Error for WavError {}

/// One mono PCM16 clip: the samples and the rate they were taken at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    /// The samples, in i16.
    pub samples: Vec<i16>,
    /// The sample rate in Hz (16 000 or 22 050).
    pub rate: u32,
}

/// The sample rates the analyzer runs at, in the order they are documented.
pub const RATES: [u32; 2] = [16_000, 22_050];

impl Pcm {
    /// Read a mono PCM16 WAV held in memory.
    pub fn from_wav_bytes(bytes: &[u8]) -> Result<Pcm, WavError> {
        if bytes.len() < 12 {
            return Err(WavError::TooShort);
        }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(WavError::NotRiff);
        }
        let mut pos = 12;
        let mut format = 1u16;
        let mut channels = 1u16;
        let mut rate = 0u32;
        let mut bits = 16u16;
        let mut data: Option<&[u8]> = None;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let len = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            let body_start = pos + 8;
            if body_start + len > bytes.len() {
                return Err(WavError::BadChunk);
            }
            let body = &bytes[body_start..body_start + len];
            match id {
                b"fmt " => {
                    if len < 16 {
                        return Err(WavError::BadChunk);
                    }
                    format = u16::from_le_bytes([body[0], body[1]]);
                    channels = u16::from_le_bytes([body[2], body[3]]);
                    rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                    bits = u16::from_le_bytes([body[14], body[15]]);
                }
                b"data" => data = Some(body),
                _ => {}
            }
            // Chunks are word-aligned.
            pos = body_start + len + (len & 1);
        }
        let data = data.ok_or(WavError::NoData)?;
        if format != 1 {
            return Err(WavError::NotPcm { format });
        }
        if bits != 16 {
            return Err(WavError::NotPcm16 { bits });
        }
        if channels != 1 {
            return Err(WavError::NotMono { channels });
        }
        if !RATES.contains(&rate) {
            return Err(WavError::BadRate { rate });
        }
        if data.len() % 2 != 0 {
            return Err(WavError::OddData);
        }
        let samples = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        Ok(Pcm { samples, rate })
    }

    /// Read a mono PCM16 WAV file.
    pub fn from_wav_file(path: &std::path::Path) -> Result<Pcm, WavError> {
        let bytes = std::fs::read(path).map_err(|_| WavError::TooShort)?;
        Pcm::from_wav_bytes(&bytes)
    }

    /// Wrap the clip as a mono PCM16 WAV file in memory.
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let data_len = self.samples.len() * 2;
        let mut out = Vec::with_capacity(44 + data_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * 2).to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// Write the clip as a mono PCM16 WAV file.
    pub fn to_wav_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_wav_bytes())
    }

    /// The clip's duration in whole L0 frames (10 ms each).
    pub fn frames(&self) -> usize {
        self.samples.len() * 100 / self.rate as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round trip must return the same samples, byte for byte.
    #[test]
    fn wav_round_trip() {
        let pcm = Pcm {
            samples: vec![0, -1, i16::MAX, i16::MIN, 123],
            rate: 22_050,
        };
        let bytes = pcm.to_wav_bytes();
        let back = Pcm::from_wav_bytes(&bytes).unwrap();
        assert_eq!(back, pcm);
    }

    /// A negative sample must survive the little-endian wrap.
    #[test]
    fn wav_negative_samples() {
        let pcm = Pcm {
            samples: vec![-30_000, 30_000],
            rate: 16_000,
        };
        assert_eq!(Pcm::from_wav_bytes(&pcm.to_wav_bytes()).unwrap(), pcm);
    }

    /// Stereo and float WAVs are named, not half-read.
    #[test]
    fn wav_rejects_the_wrong_shapes() {
        let stereo = [
            b"RIFF".as_slice(),
            &[36, 0, 0, 0],
            b"WAVE".as_slice(),
            b"fmt ".as_slice(),
            &16u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &2u16.to_le_bytes(),
            &22_050u32.to_le_bytes(),
            &88_200u32.to_le_bytes(),
            &4u16.to_le_bytes(),
            &16u16.to_le_bytes(),
            b"data".as_slice(),
            &0u32.to_le_bytes(),
        ]
        .concat();
        assert_eq!(
            Pcm::from_wav_bytes(&stereo),
            Err(WavError::NotMono { channels: 2 })
        );

        let junk = b"not a wav at all, just text to be long enough";
        assert_eq!(Pcm::from_wav_bytes(junk), Err(WavError::NotRiff));
    }
}

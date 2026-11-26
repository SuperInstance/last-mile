//! The wave file: a capture that comes from a file instead of a device.
//!
//! The host's test hook feeds the capture path a wave, so the whole path
//! -- the health check, the ears, the command, the score -- can be run
//! with no microphone and no human. The wave is read here and brought to
//! the rate the microphone takes it at: the capture path is mono at 16 kHz,
//! and a wave that is not is resampled into it.
//!
//! The reader is deliberately small: it takes what the voice service
//! writes -- 16-bit PCM in a RIFF/WAVE file -- and refuses everything
//! else by name.

/// The rate the capture path takes its samples at.
///
/// It is the microphone's rate, and the ears' rate: a capture that was
/// taken at another rate is resampled into this one before it is asked
/// about.
pub const CAPTURE_RATE: u32 = 16_000;

/// A wave: the samples, and the rate they were taken at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wave {
    /// The samples, mono.
    pub samples: Vec<i16>,
    /// The rate they were taken at.
    pub sample_rate: u32,
}

impl Wave {
    /// The wave at the capture path's rate.
    ///
    /// A wave that is already at the rate is left alone.
    pub fn at_capture_rate(&self) -> Wave {
        Wave {
            samples: resample(&self.samples, self.sample_rate, CAPTURE_RATE),
            sample_rate: CAPTURE_RATE,
        }
    }
}

/// Read a wave: a RIFF/WAVE file with 16-bit PCM.
///
/// The reader takes the first channel of a stereo file and says so: the
/// capture path is mono, and a wave that is not is not a capture.
pub fn read(bytes: &[u8]) -> Result<Wave, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(String::from("the file is not a RIFF/WAVE file"));
    }
    let mut cursor = 12usize;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits = 0u16;
    let mut samples: Option<Vec<i16>> = None;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        let body_start = cursor + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && body_end > body_start {
            let fmt = &bytes[body_start..body_end];
            if fmt.len() < 16 {
                return Err(String::from("the wave's format is too short to read"));
            }
            let format = u16::from_le_bytes([fmt[0], fmt[1]]);
            if format != 1 {
                return Err(String::from(
                    "the wave is not plain PCM: only 16-bit PCM is read",
                ));
            }
            channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            bits = u16::from_le_bytes([fmt[14], fmt[15]]);
        } else if id == b"data" && samples.is_none() {
            let body = &bytes[body_start..body_end];
            if bits != 16 {
                return Err(String::from(
                    "the wave is not 16-bit: only 16-bit PCM is read",
                ));
            }
            let mut taken = Vec::with_capacity(body.len() / 2);
            for pair in body.as_chunks::<2>().0 {
                taken.push(i16::from_le_bytes([pair[0], pair[1]]));
            }
            samples = Some(taken);
        }
        // The chunks are padded to an even length.
        cursor = body_end + (size % 2);
    }
    let samples = samples.ok_or_else(|| String::from("the wave has no data chunk"))?;
    if channels == 0 {
        return Err(String::from("the wave says it has no channels"));
    }
    if sample_rate == 0 {
        return Err(String::from("the wave says it has no rate"));
    }
    if channels > 1 {
        // The first channel is the voice; the rest is room.
        let mono = samples
            .chunks(channels as usize)
            .map(|frame| frame[0])
            .collect();
        return Ok(Wave {
            samples: mono,
            sample_rate,
        });
    }
    Ok(Wave {
        samples,
        sample_rate,
    })
}

/// The samples at another rate.
///
/// The interpolation is linear and it is exact: the whole thing is whole
/// numbers, so the same wave always gives the same samples.
pub fn resample(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == 0 || to == 0 || from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let count = u64::try_from(samples.len()).unwrap_or(u64::MAX) * u64::from(to) / u64::from(from);
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    let den = i64::from(to);
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        // Where the output sample sits in the input: whole samples, and the
        // part of a sample it is between.
        let num = u64::try_from(index).unwrap_or(u64::MAX) * u64::from(from);
        let left = usize::try_from(num / u64::from(to)).unwrap_or(usize::MAX);
        let frac = i64::try_from(num % u64::from(to)).unwrap_or(0);
        let a = i64::from(samples.get(left).copied().unwrap_or(0));
        let b = i64::from(
            samples
                .get(left + 1)
                .copied()
                .unwrap_or_else(|| samples.get(left).copied().unwrap_or(0)),
        );
        let value = a + (b - a) * frac / den;
        let clamped = value.clamp(i64::from(i16::MIN), i64::from(i16::MAX));
        out.push(i16::try_from(clamped).unwrap_or(0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wave of `frames` samples at a rate, in a RIFF/WAVE file.
    fn wave_bytes(frames: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
        let mut data = Vec::new();
        for frame in frames {
            for _ in 0..channels {
                data.extend_from_slice(&frame.to_le_bytes());
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(u16::try_from(u32::from(channels) * 2).unwrap_or(2)).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn a_wave_is_read() {
        let bytes = wave_bytes(&[1, -1, 2, -2], 16_000, 1);

        let wave = read(&bytes).expect("the wave reads");

        assert_eq!(wave.samples, vec![1, -1, 2, -2]);
        assert_eq!(wave.sample_rate, 16_000);
    }

    #[test]
    fn a_stereo_wave_is_taken_down_to_its_first_channel() {
        // The helper writes each frame once per channel, so this is the
        // stereo wave 1 9 / -1 9.
        let bytes = wave_bytes(&[1, -1], 16_000, 2);

        let wave = read(&bytes).expect("the wave reads");

        assert_eq!(wave.samples, vec![1, -1]);
    }

    #[test]
    fn a_file_that_is_not_a_wave_is_refused_by_name() {
        let err = read(b"not a wave at all").expect_err("the file is not a wave");
        assert!(err.contains("RIFF"), "{err}");
    }

    #[test]
    fn a_wave_without_data_is_refused() {
        // The header and the format, and nothing else.
        let bytes = wave_bytes(&[1, 2], 16_000, 1);
        let bytes = &bytes[..36];

        let err = read(bytes).expect_err("the wave has no data");
        assert!(err.contains("data"), "{err}");
    }

    #[test]
    fn the_samples_are_brought_to_the_capture_rate() {
        // A second at 22.05 kHz: the capture path takes 16 kHz.
        let samples: Vec<i16> = (0..22_050).map(|i| (i % 100) as i16).collect();

        let out = resample(&samples, 22_050, 16_000);

        // The length is the length the ratio asks for.
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn a_wave_at_the_capture_rate_is_left_alone() {
        let samples = vec![1, -1, 2, -2];

        assert_eq!(resample(&samples, 16_000, 16_000), samples);
    }

    #[test]
    fn the_capture_rate_is_the_microphone_s_rate() {
        let bytes = wave_bytes(&[1, -1, 2, -2], 22_050, 1);

        let wave = read(&bytes).expect("the wave reads").at_capture_rate();

        assert_eq!(wave.sample_rate, CAPTURE_RATE);
        // Four samples at 22.05 kHz are two at 16 kHz.
        assert_eq!(wave.samples.len(), 2);
    }
}

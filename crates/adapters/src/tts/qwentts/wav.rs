//! Minimal WAV reader for reference recordings: PCM16 and float32, any channel count, downmixed to
//! mono `f32`.

use std::path::Path;

/// A WAV file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum WavError {
    /// The file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The cause.
        source: std::io::Error,
    },
    /// The bytes are not a WAV file this reader supports.
    #[error("wav: {0}")]
    Format(&'static str),
}

/// Decoded audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    /// Mono samples in [-1, 1] (channels are averaged).
    pub samples: Vec<f32>,
    /// Sample rate in hertz.
    pub sample_rate: u32,
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Decode a WAV file held in memory.
pub fn parse(bytes: &[u8]) -> Result<Wav, WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::Format("not a RIFF/WAVE file"));
    }
    let (mut fmt, mut channels, mut rate, mut bits) = (0u16, 0u16, 0u32, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let size = le32(bytes, pos + 4).ok_or(WavError::Format("truncated chunk header"))? as usize;
        let body = pos + 8;
        match bytes.get(pos..pos + 4) {
            Some(b"fmt ") => {
                let short = WavError::Format("truncated fmt chunk");
                fmt = le16(bytes, body).ok_or(short)?;
                channels = le16(bytes, body + 2).ok_or(WavError::Format("truncated fmt chunk"))?;
                rate = le32(bytes, body + 4).ok_or(WavError::Format("truncated fmt chunk"))?;
                bits = le16(bytes, body + 14).ok_or(WavError::Format("truncated fmt chunk"))?;
            }
            Some(b"data") => {
                if channels == 0 || rate == 0 {
                    return Err(WavError::Format("data chunk before a valid fmt chunk"));
                }
                let end = body.saturating_add(size).min(bytes.len());
                let data = bytes
                    .get(body..end)
                    .ok_or(WavError::Format("truncated chunk"))?;
                let width = usize::from(bits / 8);
                let frame = usize::from(channels) * width;
                let mono = |sample: fn(&[u8]) -> f32| -> Vec<f32> {
                    data.chunks_exact(frame)
                        .map(|f| {
                            f.chunks_exact(width).map(sample).sum::<f32>() / f32::from(channels)
                        })
                        .collect()
                };
                let samples = match (fmt, bits) {
                    (1, 16) => mono(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0),
                    (3, 32) => mono(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]])),
                    _ => return Err(WavError::Format("only PCM16 and float32 are supported")),
                };
                return Ok(Wav {
                    samples,
                    sample_rate: rate,
                });
            }
            _ => {}
        }
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(WavError::Format("no data chunk"))
}

/// Read and decode a WAV file.
pub fn read(path: &Path) -> Result<Wav, WavError> {
    let bytes = std::fs::read(path).map_err(|source| WavError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse(&bytes)
}

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) fn pcm16_file(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::pcm16_file;
    use super::*;

    #[test]
    fn decodes_mono_pcm16() {
        let w = parse(&pcm16_file(24000, 1, &[0, 16384, -32768])).expect("wav");
        assert_eq!(w.sample_rate, 24000);
        assert_eq!(w.samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn averages_channels() {
        let w = parse(&pcm16_file(16000, 2, &[16384, -16384, 32767, 0])).expect("wav");
        assert_eq!(w.samples.len(), 2);
        assert!(w.samples[0].abs() < 1e-6);
    }

    #[test]
    fn rejects_garbage_and_missing_files() {
        assert!(parse(b"not a wav file at all").is_err());
        assert!(parse(&[]).is_err());
        assert!(matches!(
            read(Path::new("/nonexistent/prxvc-no-such.wav")),
            Err(WavError::Io { .. })
        ));
    }
}

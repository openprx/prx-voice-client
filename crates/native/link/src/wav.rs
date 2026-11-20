//! Minimal WAV reader: mono or multi-channel PCM16 and float32 files into mono `f32` samples.

use crate::NativeError;
use std::path::Path;

/// Decoded audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    /// Mono samples in [-1, 1] (channels are averaged).
    pub samples: Vec<f32>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Decode a WAV file held in memory.
pub fn parse(bytes: &[u8]) -> Result<Wav, NativeError> {
    let bad = |what: &str| NativeError(format!("wav: {what}"));
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let (mut fmt, mut channels, mut rate, mut bits) = (0u16, 0u16, 0u32, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let size = le32(bytes, pos + 4).ok_or_else(|| bad("truncated chunk header"))? as usize;
        let body = pos + 8;
        match bytes.get(pos..pos + 4) {
            Some(b"fmt ") => {
                fmt = le16(bytes, body).ok_or_else(|| bad("truncated fmt chunk"))?;
                channels = le16(bytes, body + 2).ok_or_else(|| bad("truncated fmt chunk"))?;
                rate = le32(bytes, body + 4).ok_or_else(|| bad("truncated fmt chunk"))?;
                bits = le16(bytes, body + 14).ok_or_else(|| bad("truncated fmt chunk"))?;
            }
            Some(b"data") => {
                let end = (body + size).min(bytes.len());
                let data = &bytes[body..end];
                if channels == 0 {
                    return Err(bad("data chunk before fmt chunk"));
                }
                let frame = usize::from(channels) * usize::from(bits / 8);
                let samples: Vec<f32> = match (fmt, bits) {
                    (1, 16) => data
                        .chunks_exact(frame)
                        .map(|f| {
                            let sum: f32 = f
                                .chunks(2)
                                .map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0)
                                .sum();
                            sum / f32::from(channels)
                        })
                        .collect(),
                    (3, 32) => data
                        .chunks_exact(frame)
                        .map(|f| {
                            let sum: f32 = f
                                .chunks(4)
                                .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                                .sum();
                            sum / f32::from(channels)
                        })
                        .collect(),
                    _ => return Err(bad("only PCM16 and float32 are supported")),
                };
                return Ok(Wav {
                    samples,
                    sample_rate: rate,
                });
            }
            _ => {}
        }
        pos = body + size + (size & 1);
    }
    Err(bad("no data chunk"))
}

/// Read and decode a WAV file.
pub fn read(path: &Path) -> Result<Wav, NativeError> {
    let bytes = std::fs::read(path).map_err(|e| NativeError(format!("{}: {e}", path.display())))?;
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm16_file(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
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

    #[test]
    fn decodes_mono_pcm16() {
        let w = parse(&pcm16_file(24000, 1, &[0, 16384, -32768])).unwrap();
        assert_eq!(w.sample_rate, 24000);
        assert_eq!(w.samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn averages_channels() {
        let w = parse(&pcm16_file(16000, 2, &[16384, -16384, 32767, 0])).unwrap();
        assert_eq!(w.samples.len(), 2);
        assert!(w.samples[0].abs() < 1e-6);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"not a wav file at all").is_err());
        assert!(parse(&[]).is_err());
    }
}

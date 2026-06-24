//! Capture-format to ASR-format conversion.
//!
//! The Core Audio tap (and most system audio) delivers **interleaved float32 at
//! the device's native rate** — typically 48 kHz stereo. ASR engines
//! (SenseVoice / Whisper / Moonshine) want **16 kHz mono float32**. This module
//! does the two conversions, streaming:
//!
//!   1. downmix interleaved N-channel → mono (channel average)
//!   2. resample source-rate → 16 kHz (rubato sinc, when rates differ)
//!
//! Pure DSP, platform-independent: the same path serves the Windows backend.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{AudioError, AudioFormat, AudioFrame};

/// Target sample rate expected by the ASR stage.
pub const ASR_SAMPLE_RATE: u32 = 16_000;

/// Number of source-rate mono frames fed to the resampler per processing block.
const RESAMPLER_CHUNK: usize = 1024;

/// Average interleaved float32 channels down to a mono sample vector.
///
/// Returns [`AudioError::FormatMismatch`] if the frame is not float32, since the
/// tap path is always F32. `channels == 0` yields an empty vector.
pub fn downmix_to_mono(frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
    if frame.format != AudioFormat::F32 {
        return Err(AudioError::FormatMismatch {
            expected: "F32".to_owned(),
            got: format!("{:?}", frame.format),
        });
    }
    let channels = frame.channels.max(1) as usize;
    let sample_size = std::mem::size_of::<f32>();
    let total = frame.data.len() / sample_size;
    let frames = total / channels;

    let mut mono = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..channels {
            let idx = (f * channels + c) * sample_size;
            let bytes = [
                frame.data[idx],
                frame.data[idx + 1],
                frame.data[idx + 2],
                frame.data[idx + 3],
            ];
            acc += f32::from_le_bytes(bytes);
        }
        mono.push(acc / channels as f32);
    }
    Ok(mono)
}

/// Streaming converter: interleaved float32 [`AudioFrame`]s of arbitrary size →
/// 16 kHz mono float32 samples, ready for ASR.
///
/// Feed captured frames with [`push_frame`](Self::push_frame); it buffers across
/// calls and emits whatever 16 kHz samples are ready. When the source is already
/// 16 kHz it bypasses the resampler (downmix only).
pub struct MonoResampler16k {
    /// `None` when source rate already equals [`ASR_SAMPLE_RATE`] (bypass).
    resampler: Option<SincFixedIn<f32>>,
    /// Mono samples at the source rate, awaiting a full resampler chunk.
    pending: Vec<f32>,
    source_rate: u32,
}

impl MonoResampler16k {
    /// Build a converter for the given source sample rate.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] if the resampler cannot be built
    /// (e.g. an invalid source rate).
    pub fn new(source_rate: u32) -> Result<Self, AudioError> {
        if source_rate == 0 {
            return Err(AudioError::InitializationFailed(
                "source sample rate is zero".to_owned(),
            ));
        }
        let resampler = if source_rate == ASR_SAMPLE_RATE {
            None
        } else {
            let ratio = ASR_SAMPLE_RATE as f64 / source_rate as f64;
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            };
            let r = SincFixedIn::<f32>::new(ratio, 2.0, params, RESAMPLER_CHUNK, 1)
                .map_err(|e| AudioError::InitializationFailed(format!("resampler init: {e}")))?;
            Some(r)
        };
        Ok(Self {
            resampler,
            pending: Vec::new(),
            source_rate,
        })
    }

    /// Source sample rate this converter was built for.
    pub fn source_rate(&self) -> u32 {
        self.source_rate
    }

    /// Convert one captured frame, returning any 16 kHz mono samples now ready.
    ///
    /// May return an empty vector when not enough input has accumulated yet.
    ///
    /// # Errors
    /// Propagates [`downmix_to_mono`] format errors and resampler failures.
    pub fn push_frame(&mut self, frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
        let mono = downmix_to_mono(frame)?;
        match self.resampler.as_mut() {
            // Already 16 kHz: downmix only.
            None => Ok(mono),
            Some(resampler) => {
                self.pending.extend_from_slice(&mono);
                let mut out = Vec::new();
                loop {
                    let needed = resampler.input_frames_next();
                    if self.pending.len() < needed {
                        break;
                    }
                    let chunk: Vec<f32> = self.pending.drain(..needed).collect();
                    let resampled = resampler
                        .process(&[chunk], None)
                        .map_err(|e| AudioError::InitializationFailed(format!("resample: {e}")))?;
                    if let Some(channel0) = resampled.into_iter().next() {
                        out.extend_from_slice(&channel0);
                    }
                }
                Ok(out)
            }
        }
    }
}

/// Wrap 16 kHz mono float32 samples into an [`AudioFrame`] for downstream stages.
pub fn mono_16k_frame(sequence_no: u64, samples: &[f32]) -> AudioFrame {
    let mut data = Vec::with_capacity(samples.len() * std::mem::size_of::<f32>());
    for s in samples {
        data.extend_from_slice(&s.to_le_bytes());
    }
    AudioFrame {
        sequence_no,
        sample_rate: ASR_SAMPLE_RATE,
        channels: 1,
        format: AudioFormat::F32,
        timestamp_ms: 0,
        data,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_frame(samples_lr: &[(f32, f32)], rate: u32) -> AudioFrame {
        let mut data = Vec::new();
        for (l, r) in samples_lr {
            data.extend_from_slice(&l.to_le_bytes());
            data.extend_from_slice(&r.to_le_bytes());
        }
        AudioFrame {
            sequence_no: 0,
            sample_rate: rate,
            channels: 2,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        }
    }

    #[test]
    fn downmix_averages_channels() {
        let frame = stereo_frame(&[(1.0, -1.0), (0.5, 0.5), (0.0, 1.0)], 48_000);
        let mono = downmix_to_mono(&frame).unwrap();
        assert_eq!(mono, vec![0.0, 0.5, 0.5]);
    }

    #[test]
    fn downmix_rejects_non_f32() {
        let frame = AudioFrame {
            sequence_no: 0,
            sample_rate: 16_000,
            channels: 1,
            format: AudioFormat::I16,
            timestamp_ms: 0,
            data: vec![0u8; 4],
        };
        assert!(downmix_to_mono(&frame).is_err());
    }

    #[test]
    fn bypass_when_already_16k() {
        let mut conv = MonoResampler16k::new(16_000).unwrap();
        assert!(conv.resampler.is_none());
        let frame = stereo_frame(&[(0.2, 0.4), (0.6, 0.8)], 16_000);
        let out = conv.push_frame(&frame).unwrap();
        let expected = [0.3f32, 0.7f32];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn resamples_48k_to_16k_roughly_one_third() {
        let mut conv = MonoResampler16k::new(48_000).unwrap();
        // Feed ~4 resampler chunks worth of 48k mono-as-stereo silence-ish ramp.
        let mut produced = 0usize;
        for _ in 0..8 {
            let block: Vec<(f32, f32)> = (0..1024).map(|i| (i as f32 / 1024.0, i as f32 / 1024.0)).collect();
            let frame = stereo_frame(&block, 48_000);
            produced += conv.push_frame(&frame).unwrap().len();
        }
        // 8*1024 input @48k -> ~ (8*1024)/3 output @16k; allow generous slack for
        // sinc warm-up / internal buffering.
        let expected = (8 * 1024) / 3;
        assert!(produced > expected / 2, "produced {produced}, expected ~{expected}");
    }
}

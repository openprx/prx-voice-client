//! Audio value types: the captured frame that travels through the pipeline and the chunk a synthesiser produces.

use std::time::Duration;

use crate::rate::SampleRate;
use crate::time::MonoTime;

/// A frame could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// A frame holds at least one sample.
    #[error("audio frame has no samples")]
    Empty,
    /// Samples are finite numbers.
    #[error("audio frame sample {index} is not finite")]
    NonFinite {
        /// Index of the first offending sample.
        index: usize,
    },
}

/// A block of mono f32 samples with its sample rate and capture time.
///
/// Invariants, checked by [`AudioFrame::new`]: not empty, every sample finite.
/// Stereo input is downmixed by the endpoint before a frame exists.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioFrame {
    samples: Vec<f32>,
    rate: SampleRate,
    ts: MonoTime,
    seq: u64,
    discontinuity: bool,
}

impl AudioFrame {
    /// A frame of `samples` captured at `ts`, number `seq` of its source.
    pub fn new(
        samples: Vec<f32>,
        rate: SampleRate,
        ts: MonoTime,
        seq: u64,
    ) -> Result<Self, FrameError> {
        if samples.is_empty() {
            return Err(FrameError::Empty);
        }
        if let Some(index) = samples.iter().position(|s| !s.is_finite()) {
            return Err(FrameError::NonFinite { index });
        }
        Ok(Self {
            samples,
            rate,
            ts,
            seq,
            discontinuity: false,
        })
    }

    /// Mark whether audio was lost between the previous frame and this one.
    #[must_use]
    pub fn with_discontinuity(mut self, discontinuity: bool) -> Self {
        self.discontinuity = discontinuity;
        self
    }

    /// The samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Take the samples out of the frame.
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }

    /// The sample rate.
    pub fn rate(&self) -> SampleRate {
        self.rate
    }

    /// Capture time of the first sample.
    pub fn ts(&self) -> MonoTime {
        self.ts
    }

    /// Sequence number within the source, increasing by one per frame.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Whether audio was dropped just before this frame.
    pub fn discontinuity(&self) -> bool {
        self.discontinuity
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Always `false`: a frame is never empty. Present for the `len` convention.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Playing time of the frame.
    pub fn duration(&self) -> Duration {
        self.rate.duration_of(self.samples.len())
    }

    /// Capture time just after the last sample.
    pub fn end_ts(&self) -> MonoTime {
        self.ts.saturating_add(self.duration())
    }
}

/// Audio produced by a synthesiser: mono f32 at the synthesiser's rate.
///
/// Adapters validate vendor output before building a chunk; unlike
/// [`AudioFrame`] the type does not re-check it.
#[derive(Clone, Debug, PartialEq)]
pub struct PcmChunk {
    /// Mono samples.
    pub samples: Vec<f32>,
    /// Their sample rate.
    pub rate: SampleRate,
}

impl PcmChunk {
    /// Playing time of the chunk.
    pub fn duration(&self) -> Duration {
        self.rate.duration_of(self.samples.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(samples: Vec<f32>) -> Result<AudioFrame, FrameError> {
        AudioFrame::new(samples, SampleRate::HZ_16000, MonoTime::from_millis(100), 7)
    }

    #[test]
    fn empty_and_non_finite_frames_are_rejected() {
        assert_eq!(frame(vec![]), Err(FrameError::Empty));
        assert_eq!(
            frame(vec![0.0, f32::NAN]),
            Err(FrameError::NonFinite { index: 1 })
        );
        assert_eq!(
            frame(vec![f32::INFINITY]),
            Err(FrameError::NonFinite { index: 0 })
        );
    }

    #[test]
    fn a_20_ms_frame_reports_its_span() {
        let f = frame(vec![0.25; 320]).expect("valid frame");
        assert_eq!(f.len(), 320);
        assert!(!f.is_empty());
        assert_eq!(f.duration(), Duration::from_millis(20));
        assert_eq!(f.ts(), MonoTime::from_millis(100));
        assert_eq!(f.end_ts(), MonoTime::from_millis(120));
        assert_eq!(f.seq(), 7);
        assert_eq!(f.rate(), SampleRate::HZ_16000);
        assert!(!f.discontinuity());
    }

    #[test]
    fn discontinuity_is_a_flag_set_after_construction() {
        let f = frame(vec![0.0; 4])
            .expect("valid frame")
            .with_discontinuity(true);
        assert!(f.discontinuity());
        assert_eq!(f.into_samples(), vec![0.0; 4]);
    }

    #[test]
    fn a_pcm_chunk_reports_its_duration() {
        let c = PcmChunk {
            samples: vec![0.0; 2400],
            rate: SampleRate::HZ_24000,
        };
        assert_eq!(c.duration(), Duration::from_millis(100));
    }
}

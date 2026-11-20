//! Fake voice activity detector that plays back a script of speech probabilities.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use prxvc_media::SampleRate;

use crate::descriptor::{ProviderDescriptor, StreamingKind};
use crate::vad::{VadEngine, VadError, VadProvider};

/// Samples per frame of the fake engine (32 ms at 16 kHz).
pub const FAKE_VAD_FRAME_LEN: usize = 512;

/// What the fake engine answers, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct VadScript {
    /// The probability returned for frame 0, 1, 2, ...; later frames get 0.0.
    pub probabilities: Vec<f32>,
    /// Fail the frame with this index (counted from the last reset).
    pub fail_at: Option<(usize, VadError)>,
    /// Fail `open` instead of producing an engine.
    pub open_error: Option<VadError>,
}

impl VadScript {
    /// A script of probabilities without failures.
    pub fn probabilities(probabilities: impl IntoIterator<Item = f32>) -> Self {
        Self {
            probabilities: probabilities.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Fail the frame with index `index`.
    #[must_use]
    pub fn failing_at(mut self, index: usize, error: VadError) -> Self {
        self.fail_at = Some((index, error));
        self
    }

    /// Make `open` fail.
    #[must_use]
    pub fn failing_open(mut self, error: VadError) -> Self {
        self.open_error = Some(error);
        self
    }
}

/// A [`VadProvider`] whose engines all play the same script.
#[derive(Clone, Debug)]
pub struct FakeVadProvider {
    script: VadScript,
    opens: Arc<AtomicUsize>,
}

impl FakeVadProvider {
    /// A provider whose engines play `script`.
    pub fn new(script: VadScript) -> Self {
        Self {
            script,
            opens: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// How many engines were opened.
    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl VadProvider for FakeVadProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new("fake-vad", "fake", StreamingKind::Native)
            .with_sample_rates([SampleRate::HZ_16000])
    }

    fn open(&self) -> Result<Box<dyn VadEngine>, VadError> {
        if let Some(e) = &self.script.open_error {
            return Err(e.clone());
        }
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeVadEngine {
            script: self.script.clone(),
            next: 0,
        }))
    }
}

struct FakeVadEngine {
    script: VadScript,
    next: usize,
}

impl VadEngine for FakeVadEngine {
    fn frame_len(&self) -> usize {
        FAKE_VAD_FRAME_LEN
    }

    fn rate(&self) -> SampleRate {
        SampleRate::HZ_16000
    }

    fn probability(&mut self, frame: &[f32]) -> Result<f32, VadError> {
        if frame.len() != FAKE_VAD_FRAME_LEN {
            return Err(VadError::FrameLength {
                expected: FAKE_VAD_FRAME_LEN,
                got: frame.len(),
            });
        }
        let index = self.next;
        self.next += 1;
        if let Some((at, error)) = &self.script.fail_at
            && *at == index
        {
            return Err(error.clone());
        }
        Ok(self.script.probabilities.get(index).copied().unwrap_or(0.0))
    }

    fn reset(&mut self) {
        self.next = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Vec<f32> {
        vec![0.0; FAKE_VAD_FRAME_LEN]
    }

    #[test]
    fn engine_plays_the_script_then_reports_silence_and_reset_rewinds() {
        let provider = FakeVadProvider::new(VadScript::probabilities([0.1, 0.9]));
        let mut engine = provider.open().expect("engine");
        assert_eq!(engine.frame_len(), FAKE_VAD_FRAME_LEN);
        assert_eq!(engine.rate(), SampleRate::HZ_16000);
        assert_eq!(engine.probability(&frame()), Ok(0.1));
        assert_eq!(engine.probability(&frame()), Ok(0.9));
        assert_eq!(
            engine.probability(&frame()),
            Ok(0.0),
            "past the end of the script"
        );
        engine.reset();
        assert_eq!(engine.probability(&frame()), Ok(0.1));
        assert_eq!(provider.opens(), 1);
    }

    #[test]
    fn a_frame_of_the_wrong_length_is_a_typed_error() {
        let mut engine = FakeVadProvider::new(VadScript::default())
            .open()
            .expect("engine");
        assert_eq!(
            engine.probability(&[0.0; 10]),
            Err(VadError::FrameLength {
                expected: FAKE_VAD_FRAME_LEN,
                got: 10
            })
        );
    }

    #[test]
    fn an_injected_failure_hits_only_the_chosen_frame() {
        let script = VadScript::probabilities([0.2, 0.3, 0.4])
            .failing_at(1, VadError::Internal("boom".to_owned()));
        let mut engine = FakeVadProvider::new(script).open().expect("engine");
        assert_eq!(engine.probability(&frame()), Ok(0.2));
        assert_eq!(
            engine.probability(&frame()),
            Err(VadError::Internal("boom".to_owned()))
        );
        assert_eq!(engine.probability(&frame()), Ok(0.4));
    }

    #[test]
    fn open_can_fail_and_the_descriptor_declares_16_khz() {
        let provider =
            FakeVadProvider::new(VadScript::default().failing_open(VadError::Unavailable {
                reason: "no model".to_owned(),
            }));
        assert!(matches!(provider.open(), Err(VadError::Unavailable { .. })));
        assert_eq!(provider.opens(), 0);
        let d = provider.descriptor();
        assert!(d.require_rate(SampleRate::HZ_16000).is_ok());
        assert!(d.require_rate(SampleRate::HZ_48000).is_err());
    }
}

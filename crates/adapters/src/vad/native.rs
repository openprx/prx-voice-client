//! `SileroVad`: the Silero v6.2 voice activity model over the `transcribe.cpp` runtime.

use std::path::Path;
use std::sync::Arc;

use prxvc_media::SampleRate;
use prxvc_ports::{ProviderDescriptor, StreamingKind, VadEngine, VadError, VadProvider};

use crate::transcribe::{Backend, NativeError, NativeVad, TranscribeModel};

/// The configured provider id of the Silero VAD.
pub const PROVIDER_ID: &str = "silero-vad-6.2";

fn unavailable(e: &NativeError) -> VadError {
    VadError::Unavailable {
        reason: e.to_string(),
    }
}

/// One open Silero session: 32 ms frames of 16 kHz audio in, a speech probability out.
pub struct SileroVad {
    session: NativeVad,
}

impl SileroVad {
    /// Open a session on a loaded VAD model.
    pub fn open(model: &Arc<TranscribeModel>) -> Result<Self, VadError> {
        let session = NativeVad::new(model).map_err(|e| unavailable(&e))?;
        let info = session.info();
        if info.sample_rate != SampleRate::HZ_16000.hz() {
            return Err(VadError::Unavailable {
                reason: format!("the model needs {} Hz audio", info.sample_rate),
            });
        }
        Ok(Self { session })
    }
}

impl VadEngine for SileroVad {
    fn frame_len(&self) -> usize {
        self.session.info().frame_samples
    }

    fn rate(&self) -> SampleRate {
        SampleRate::HZ_16000
    }

    fn probability(&mut self, frame: &[f32]) -> Result<f32, VadError> {
        let expected = self.frame_len();
        if frame.len() != expected {
            return Err(VadError::FrameLength {
                expected,
                got: frame.len(),
            });
        }
        match self.session.feed_frame(frame) {
            Ok(Some(p)) => Ok(p.clamp(0.0, 1.0)),
            Ok(None) => Err(VadError::Internal(
                "the runtime returned no probability for a full frame".to_owned(),
            )),
            Err(e) => Err(VadError::Internal(e.to_string())),
        }
    }

    fn reset(&mut self) {
        self.session.reset();
    }
}

/// Opens Silero sessions, one per direction, on one loaded model. The model's compute lock keeps
/// the sessions from computing at the same time (the runtime allows one compute per model).
pub struct SileroVadProvider {
    model: Arc<TranscribeModel>,
}

impl SileroVadProvider {
    /// Load the Silero GGUF (1.2 MB) on the CPU, where the benchmark measured it and where it stays
    /// off the GPU the recogniser and the synthesiser share, and check that it serves the VAD role.
    pub fn load(path: &Path) -> Result<Self, VadError> {
        let model = TranscribeModel::load_on(path, Backend::Cpu).map_err(|e| unavailable(&e))?;
        if !model.is_vad() {
            return Err(VadError::Unavailable {
                reason: format!("{} is not a VAD model", path.display()),
            });
        }
        Ok(Self { model })
    }
}

impl VadProvider for SileroVadProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(PROVIDER_ID, "silero-vad-v6.2", StreamingKind::Native)
            .with_sample_rates([SampleRate::HZ_16000])
    }

    fn open(&self) -> Result<Box<dyn VadEngine>, VadError> {
        Ok(Box::new(SileroVad::open(&self.model)?))
    }
}

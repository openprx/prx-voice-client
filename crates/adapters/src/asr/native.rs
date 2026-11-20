//! The engine over `transcribe.cpp`: Qwen3-ASR decoding on a loaded model.

use std::path::Path;
use std::sync::Arc;

use prxvc_media::Lang;

use super::adapter::{LoadError, QwenAsr, QwenAsrConfig};
use super::seam::{EngineError, RecognizeEngine};
use crate::transcribe::{NativeError, NativeRecognizer, TranscribeModel};

/// A loaded Qwen3-ASR model with its session.
pub struct NativeEngine {
    session: NativeRecognizer,
}

impl NativeEngine {
    /// Load the model on Metal and open its session.
    pub fn load(path: &Path) -> Result<Self, EngineError> {
        let model = TranscribeModel::load(path).map_err(|e| EngineError::Failed(e.to_string()))?;
        Self::on_model(&model)
    }

    /// Open a session on an already loaded model.
    pub fn on_model(model: &Arc<TranscribeModel>) -> Result<Self, EngineError> {
        let session =
            NativeRecognizer::new(model).map_err(|e| EngineError::Failed(e.to_string()))?;
        Ok(Self { session })
    }
}

impl RecognizeEngine for NativeEngine {
    fn decode(
        &mut self,
        audio: &[f32],
        lang: Lang,
        stop: &dyn Fn() -> bool,
    ) -> Result<String, EngineError> {
        self.session.decode(audio, lang, stop).map_err(|e| match e {
            NativeError::Aborted => EngineError::Aborted,
            other => EngineError::Failed(other.to_string()),
        })
    }
}

impl QwenAsr {
    /// Load the model file, start the worker and run the warm-up. Blocks for the model load (about
    /// 0.1 s once the system has compiled the Metal pipelines, several seconds on the first launch
    /// after a rebuild) and the warm-up; call it from a loading thread.
    pub fn load_native(path: &Path, cfg: QwenAsrConfig) -> Result<Self, LoadError> {
        let engine = NativeEngine::load(path)?;
        Self::start_system(Box::new(engine), cfg)
    }
}

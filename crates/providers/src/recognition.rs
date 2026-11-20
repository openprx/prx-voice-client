//! Construction of the recognition roles: VAD, streaming ASR and the refine pass.
//!
//! The adapters (`SileroVadProvider`, `QwenAsr`) live in `prxvc-adapters` behind the cargo feature
//! `transcribe`, which this crate forwards. With the feature off, [`BuiltInRecognition`] reports
//! the `transcribe` runtime as not built in: resolution answers with the typed problem
//! `RuntimeNotBuiltIn` and the constructors return [`BuildError::NotBuiltIn`]. They stub nothing: no
//! provider is faked.
//!
//! Sharing, reference counting, failure retirement, `build_*`, the test functions and the egress
//! summary are wired around [`RecognitionLoader`] in `registry.rs`: one engine per model, shared by
//! the two directions, one stream handle per direction, freed with the last handle.
//!
//! Parameters of the recogniser come from the `[pipeline]` keys ([`asr_params`]); the VAD engine
//! only scores frames, and its thresholds are read by whoever segments the scores
//! ([`gate_params`] for the adapters' own gate, `PipelineParams` for the pipeline).

use std::sync::Arc;
use std::time::Duration;

use prxvc_adapters::asr::AsrParams;
use prxvc_adapters::vad::GateParams;
use prxvc_config::PipelineConfig;
use prxvc_ports::{RefineAsr, StreamingAsr, VadProvider};

use crate::problem::BuildError;
use crate::spec::LocalModelSpec;

/// Whether the `transcribe` runtime and its adapters are part of this binary.
pub const TRANSCRIBE_BUILT_IN: bool = cfg!(feature = "transcribe");

/// The purpose of the model file of a recognition model in the catalogue.
pub(crate) const ASR_FILE_PURPOSE: &str = "model";
/// The purpose of the model file of the VAD in the catalogue.
pub(crate) const VAD_FILE_PURPOSE: &str = "vad";

/// Loads the recognition engines. Each call loads one engine; the registry shares it between the
/// directions and calls these only when no live engine exists.
pub trait RecognitionLoader: Send + Sync {
    /// Whether the `transcribe` runtime can be loaded by this loader.
    fn available(&self) -> bool;
    /// Load the voice activity detector of `spec`.
    fn vad(&self, spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError>;
    /// Load the streaming recogniser of `spec` with `params` (blocks for the load and the warm-up).
    fn asr(
        &self,
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn StreamingAsr>, BuildError>;
    /// Load the refine-pass recogniser of `spec` with `params`.
    fn refine(
        &self,
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn RefineAsr>, BuildError>;
}

/// The recogniser parameters of the `[pipeline]` section. Keys the adapter has no use for are not
/// here, and adapter parameters without a key (`cut_search`, `final_timeout`) keep their defaults.
pub fn asr_params(cfg: &PipelineConfig) -> AsrParams {
    let defaults = AsrParams::default();
    AsrParams {
        redecode_interval: Duration::from_millis(u64::from(cfg.asr_redecode_interval_ms)),
        first_decode_after: Duration::from_millis(u64::from(cfg.asr_first_decode_after_ms)),
        // The adapter needs at least two agreeing hypotheses for a prefix to be stable.
        stable_agreement: usize::try_from(cfg.asr_stable_agreement)
            .unwrap_or(usize::MAX)
            .max(2),
        stable_tail_guard: usize::try_from(cfg.asr_stable_tail_guard).unwrap_or(usize::MAX),
        max_segment: Duration::from_secs(u64::from(cfg.max_segment_s)),
        ..defaults
    }
}

/// The thresholds of the adapters' speech gate from the `[pipeline]` section.
pub fn gate_params(cfg: &PipelineConfig) -> GateParams {
    let defaults = GateParams::default();
    GateParams {
        threshold: cfg.vad_threshold,
        neg_threshold: defaults.neg_threshold.min(cfg.vad_threshold),
        min_speech: Duration::from_millis(u64::from(cfg.min_speech_ms)),
        pad: Duration::from_millis(u64::from(cfg.pad_ms)),
        min_silence: Duration::from_millis(u64::from(cfg.segment_end_pause_ms)),
        long_after: Duration::from_secs(u64::from(cfg.long_segment_after_s)),
        long_silence: Duration::from_millis(u64::from(cfg.long_segment_pause_ms)),
        max_segment: Duration::from_secs(u64::from(cfg.max_segment_s)),
    }
}

/// The loader of this binary.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInRecognition;

#[cfg(not(feature = "transcribe"))]
fn not_built_in(spec: &LocalModelSpec) -> BuildError {
    BuildError::NotBuiltIn {
        provider: spec.id.clone(),
        runtime: spec.runtime.name().to_owned(),
    }
}

#[cfg(feature = "transcribe")]
mod native {
    //! The loaders with the runtime built in.

    use std::path::PathBuf;

    use prxvc_adapters::asr::{QwenAsr, QwenAsrConfig};
    use prxvc_adapters::vad::SileroVadProvider;

    use super::*;

    fn start_error(spec: &LocalModelSpec, detail: impl ToString) -> BuildError {
        BuildError::Start {
            provider: spec.id.clone(),
            detail: detail.to_string(),
        }
    }

    fn model_file(spec: &LocalModelSpec, purpose: &str) -> Result<PathBuf, BuildError> {
        spec.model
            .file(purpose)
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| start_error(spec, format!("the model has no `{purpose}` file")))
    }

    pub(super) fn vad(spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
        let path = model_file(spec, VAD_FILE_PURPOSE)?;
        let provider = SileroVadProvider::load(&path).map_err(|e| start_error(spec, e))?;
        Ok(Arc::new(provider))
    }

    /// One recogniser engine: the model load and the warm-up of both languages.
    fn load(spec: &LocalModelSpec, params: &AsrParams) -> Result<Arc<QwenAsr>, BuildError> {
        let path = model_file(spec, ASR_FILE_PURPOSE)?;
        let cfg = QwenAsrConfig {
            id: spec.id.as_str().to_owned(),
            model: spec.id.as_str().to_owned(),
            params: params.clone(),
        };
        let engine = QwenAsr::load_native(&path, cfg).map_err(|e| start_error(spec, e))?;
        tracing::info!(
            provider = spec.id.as_str(),
            warm_up_ms = engine.warm_up_duration().as_millis() as u64,
            "recognition engine loaded"
        );
        Ok(Arc::new(engine))
    }

    pub(super) fn asr(
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn StreamingAsr>, BuildError> {
        Ok(load(spec, params)? as Arc<dyn StreamingAsr>)
    }

    pub(super) fn refine(
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn RefineAsr>, BuildError> {
        Ok(load(spec, params)? as Arc<dyn RefineAsr>)
    }
}

impl RecognitionLoader for BuiltInRecognition {
    fn available(&self) -> bool {
        TRANSCRIBE_BUILT_IN
    }

    #[cfg(feature = "transcribe")]
    fn vad(&self, spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
        native::vad(spec)
    }

    #[cfg(not(feature = "transcribe"))]
    fn vad(&self, spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
        Err(not_built_in(spec))
    }

    #[cfg(feature = "transcribe")]
    fn asr(
        &self,
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn StreamingAsr>, BuildError> {
        native::asr(spec, params)
    }

    #[cfg(not(feature = "transcribe"))]
    fn asr(
        &self,
        spec: &LocalModelSpec,
        _params: &AsrParams,
    ) -> Result<Arc<dyn StreamingAsr>, BuildError> {
        Err(not_built_in(spec))
    }

    #[cfg(feature = "transcribe")]
    fn refine(
        &self,
        spec: &LocalModelSpec,
        params: &AsrParams,
    ) -> Result<Arc<dyn RefineAsr>, BuildError> {
        native::refine(spec, params)
    }

    #[cfg(not(feature = "transcribe"))]
    fn refine(
        &self,
        spec: &LocalModelSpec,
        _params: &AsrParams,
    ) -> Result<Arc<dyn RefineAsr>, BuildError> {
        Err(not_built_in(spec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_of_the_section_are_the_defaults_of_the_adapter() {
        let from_config = asr_params(&PipelineConfig::default());
        assert_eq!(from_config, AsrParams::default());
        assert_eq!(
            gate_params(&PipelineConfig::default()),
            GateParams::default()
        );
    }

    #[test]
    fn configured_values_reach_the_parameters() {
        let cfg = PipelineConfig {
            asr_redecode_interval_ms: 500,
            asr_first_decode_after_ms: 640,
            asr_stable_agreement: 3,
            asr_stable_tail_guard: 4,
            max_segment_s: 20,
            vad_threshold: 0.5,
            segment_end_pause_ms: 700,
            ..PipelineConfig::default()
        };
        let p = asr_params(&cfg);
        assert_eq!(p.redecode_interval, Duration::from_millis(500));
        assert_eq!(p.first_decode_after, Duration::from_millis(640));
        assert_eq!((p.stable_agreement, p.stable_tail_guard), (3, 4));
        assert_eq!(p.max_segment, Duration::from_secs(20));
        let g = gate_params(&cfg);
        assert_eq!(g.threshold, 0.5);
        assert_eq!(g.min_silence, Duration::from_millis(700));
        assert_eq!(g.max_segment, Duration::from_secs(20));
    }

    #[test]
    fn an_agreement_below_two_is_raised_to_the_adapters_minimum() {
        let cfg = PipelineConfig {
            asr_stable_agreement: 0,
            ..PipelineConfig::default()
        };
        assert_eq!(asr_params(&cfg).stable_agreement, 2);
    }

    #[test]
    fn a_low_threshold_keeps_the_release_threshold_below_it() {
        let cfg = PipelineConfig {
            vad_threshold: 0.1,
            ..PipelineConfig::default()
        };
        let g = gate_params(&cfg);
        assert!(g.neg_threshold <= g.threshold);
    }

    #[test]
    fn the_loader_is_available_exactly_when_the_feature_is_on() {
        assert_eq!(BuiltInRecognition.available(), cfg!(feature = "transcribe"));
        assert_eq!(TRANSCRIBE_BUILT_IN, BuiltInRecognition.available());
    }
}

//! The extension point of the recognition roles: VAD, streaming ASR and the refine pass.
//!
//! The recognition adapters (`SileroVad`, `QwenAsr`, behind the cargo feature `transcribe` of
//! `prxvc-adapters`) arrive with work package CT-11 and are not part of this binary yet. Until
//! they are, [`BuiltInRecognition`] reports the `transcribe` runtime as not built in, which is
//! the typed problem `RuntimeNotBuiltIn` at resolution, and its three constructors return the
//! typed error [`BuildError::NotBuiltIn`]. They stub nothing: no provider is faked.
//!
//! When the adapters land, a developer does exactly this, in this file only:
//!
//! 1. Add the feature `transcribe = ["prxvc-adapters/transcribe"]` to this crate's `Cargo.toml`.
//! 2. Set [`TRANSCRIBE_BUILT_IN`] to `cfg!(feature = "transcribe")`.
//! 3. Fill the bodies of `construct_vad`, `construct_asr` and `construct_refine` to load the
//!    model files of the spec (`spec.model.file("model")`, or whichever purposes the catalogue
//!    entries of CT-11 use) and return the adapter as the port trait object.
//!
//! Sharing, reference counting, failure retirement, `build_*`, the test functions and the
//! egress summary are already wired around [`RecognitionLoader`] (`registry.rs`).

use std::sync::Arc;

use prxvc_ports::{RefineAsr, StreamingAsr, VadProvider};

use crate::problem::BuildError;
use crate::spec::LocalModelSpec;

/// Whether the `transcribe` runtime and its adapters are part of this binary.
pub const TRANSCRIBE_BUILT_IN: bool = false;

/// Loads the recognition engines. Each call loads one engine; the registry shares it between the
/// directions and calls these only when no live engine exists.
pub trait RecognitionLoader: Send + Sync {
    /// Whether the `transcribe` runtime can be loaded by this loader.
    fn available(&self) -> bool;
    /// Load the voice activity detector of `spec`.
    fn vad(&self, spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError>;
    /// Load the streaming recogniser of `spec`.
    fn asr(&self, spec: &LocalModelSpec) -> Result<Arc<dyn StreamingAsr>, BuildError>;
    /// Load the refine-pass recogniser of `spec`.
    fn refine(&self, spec: &LocalModelSpec) -> Result<Arc<dyn RefineAsr>, BuildError>;
}

/// The loader of this binary.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInRecognition;

fn not_built_in(spec: &LocalModelSpec) -> BuildError {
    BuildError::NotBuiltIn {
        provider: spec.id.clone(),
        runtime: spec.runtime.name().to_owned(),
    }
}

fn construct_vad(spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
    Err(not_built_in(spec))
}

fn construct_asr(spec: &LocalModelSpec) -> Result<Arc<dyn StreamingAsr>, BuildError> {
    Err(not_built_in(spec))
}

fn construct_refine(spec: &LocalModelSpec) -> Result<Arc<dyn RefineAsr>, BuildError> {
    Err(not_built_in(spec))
}

impl RecognitionLoader for BuiltInRecognition {
    fn available(&self) -> bool {
        TRANSCRIBE_BUILT_IN
    }

    fn vad(&self, spec: &LocalModelSpec) -> Result<Arc<dyn VadProvider>, BuildError> {
        construct_vad(spec)
    }

    fn asr(&self, spec: &LocalModelSpec) -> Result<Arc<dyn StreamingAsr>, BuildError> {
        construct_asr(spec)
    }

    fn refine(&self, spec: &LocalModelSpec) -> Result<Arc<dyn RefineAsr>, BuildError> {
        construct_refine(spec)
    }
}

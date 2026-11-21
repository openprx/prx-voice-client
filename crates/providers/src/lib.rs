//! The provider registry of the call pipeline (layer L3, CS-2.2).
//!
//! From configuration, the model catalogue and the model store, the registry resolves each role
//! of a call (voice activity detection, recognition, the refine pass, translation, synthesis) and
//! the assistant's dialogue backend
//! into a typed specification, and builds the provider as a boxed port. It is the one place that
//! names concrete adapters.
//!
//! # Fail loud
//!
//! Provider ids are parsed once; an unknown id, a model that is not installed or failed
//! verification, a runtime that is not built in, a provider that cannot stream, an unsupported
//! language, a missing credential, an endpoint the offline policy blocks, invalid parameters and
//! a missing voice are each a typed [`ProblemKind`]. [`ProviderRegistry::resolve_call`] returns
//! the full set for both directions or *every* problem. No code path falls back to a mock or to a
//! different provider: the crate has no mock, and its test fakes are compiled only for its own
//! tests.
//!
//! # Fallbacks
//!
//! The fallback of each role (`providers.fallback`) is resolved and built like the primary and
//! handed out as a separate provider ([`Resolved`]). A fallback that cannot be resolved is a
//! warning, fatal only when the primary also fails. Switching is the pipeline's decision.
//!
//! # Engines
//!
//! In-process engines are shared between the two directions and counted by their handles; see
//! `engines`. Dropping the last handle frees the engine. A handle that observes an engine failure
//! retires it and the next build creates a fresh one.
//!
//! # Modules
//!
//! * `keys`, `problem`, `spec`, `plan`: the vocabulary.
//! * `resolve`: resolution; `credentials`, `schema`, `model_source`, `fingerprint`: its inputs.
//! * `registry`, `engines`, `handles`, `tts_loader`, `recognition`: construction and lifecycle.
//! * `test_provider`: "test provider" per role; `egress`: the disclosure summary.
//! * `runtime`: the runtimes of the catalogue and which of them this binary contains.

pub mod credentials;
pub mod egress;
pub mod engines;
pub mod fingerprint;
pub mod handles;
pub mod keys;
pub mod model_source;
pub mod plan;
pub mod problem;
pub mod recognition;
pub mod registry;
pub mod resolve;
pub mod runtime;
pub mod schema;
pub mod spec;
pub mod test_provider;
pub mod tts_loader;

#[cfg(test)]
mod tests;

pub use credentials::{
    CredentialReadError, CredentialReader, CredentialRef, EnvCredentials, Secret,
};
pub use egress::{EgressSummary, RoleEgress};
pub use engines::Lease;
pub use handles::{
    SharedAsr, SharedDialogue, SharedRefine, SharedSynth, SharedSystemTranslator, SharedTranslator,
    SharedVad,
};
pub use keys::{CallLanguages, CallRole, RoleKey};
pub use model_source::ModelSource;
pub use plan::{CallPlan, DirectionPlan, DirectionsPlan, RecognitionMode};
pub use problem::{BuildError, Problem, ProblemKind, Slot};
pub use recognition::{BuiltInRecognition, RecognitionLoader, TRANSCRIBE_BUILT_IN};
pub use registry::{LiveEngines, PrepareError, ProviderRegistry, VoicePrepared};
pub use resolve::{
    DEFAULT_DIALOGUE_PROVIDER, DEFAULT_MT_PROVIDER, SYSTEM_MT_PROVIDER, DialogueChoice, DialogueKind, OPENPRX_NAME,
    OPENPRX_PROVIDER, VoiceChoice, VoiceDirs,
};
pub use runtime::{BuiltRuntimes, Runtime};
pub use spec::{
    DataLeaving, DialogueSpec, LocalModelSpec, Locality, MtEngineKind, MtSpec, Resolved, TtsFiles, TtsSpec,
    VoicePlan,
};
pub use test_provider::{FailureClass, TestError, TestReport};
pub use tts_loader::{BuiltInTts, TtsLoader};

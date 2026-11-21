//! System translation: the translation port over Apple's on-device Translation framework.
//!
//! * `api`: the two traits that stand for the operating system (`SystemTranslationApi`,
//!   `SystemSession`), so every rule here is tested against a fake.
//! * `fault`: the typed failures (old system, missing pack, unsupported pair, timeout, framework).
//! * `language`: the framework's language tags.
//! * `translator`, `stream`: `AppleSystemMt`, the engine of one language pair, and its request stream.
//! * `ffi` (feature `apple-translation`): the real `SystemTranslationApi` over the Swift bridge.
//!
//! The Translation framework is an operating-system feature, like the microphone API: the owner
//! approved it as an exception to the open-source-only rule (see `THIRD_PARTY_NOTICES.md`).

pub mod api;
pub mod fault;
#[cfg(feature = "apple-translation")]
pub mod ffi;
pub mod language;
mod stream;
mod translator;

#[cfg(test)]
mod tests;

pub use api::{Availability, Strategy, SystemSession, SystemTranslationApi};
pub use fault::{SystemFault, SystemFaultKind};
pub use translator::{
    AppleSystemMt, MODEL_NAME, PROVIDER_ID, SystemTimeouts, SystemTranslationConfig,
};

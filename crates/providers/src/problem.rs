//! The typed reasons a role cannot be served, and the errors of building a provider.
//!
//! Resolution collects every [`Problem`] of a call instead of stopping at the first, so the UI
//! can show them together and offer the matching action (download a model, enter a key, ...).
//! Nothing here is ever answered with a mock or another provider (CS-2.2.3).

use std::path::PathBuf;

use prxvc_media::Lang;
use prxvc_models::ModelId;
use prxvc_ports::{CapabilityError, ProviderId};

use crate::keys::{CallRole, RoleKey};

/// Which of a role's two providers a problem concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// The provider named in `providers.active`.
    Primary,
    /// The provider named in `providers.fallback`. Non-fatal unless the primary also fails.
    Fallback,
}

/// One reason a role or the whole call cannot be served.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The role it concerns; `None` for a problem of the call as a whole (languages, settings).
    pub role: Option<RoleKey>,
    /// Primary or fallback.
    pub slot: Slot,
    /// What is wrong.
    pub kind: ProblemKind,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.role {
            Some(role) => write!(f, "{role} ({:?}): {}", self.slot, self.kind),
            None => write!(f, "call: {}", self.kind),
        }
    }
}

/// What is wrong, with the data the UI needs for its action.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProblemKind {
    /// A language setting is not a language of the pipeline.
    #[error("`{key}` is `{value}`, which is not a supported language")]
    InvalidLanguage {
        /// The configuration key.
        key: String,
        /// The text found.
        value: String,
    },
    /// No call language is configured and none could be derived.
    #[error("the language of `{key}` is not set")]
    LanguageNotSet {
        /// The configuration key.
        key: String,
    },
    /// A setting has a value the registry does not know.
    #[error("`{key}` is `{value}`, which is not a known value")]
    InvalidSetting {
        /// The configuration key.
        key: String,
        /// The text found.
        value: String,
    },
    /// Nothing is selected for the role and the catalogue has no default for it.
    #[error("no provider is selected for the role and the catalogue has no default")]
    NoProviderSelected,
    /// The configured id is neither a catalogue model nor a provider entry.
    #[error("the configured provider `{id}` does not exist")]
    UnknownProvider {
        /// The configured id.
        id: String,
    },
    /// An entry names a protocol this client does not know.
    #[error("provider entry `{entry}` uses the unknown protocol `{protocol}`")]
    UnknownProtocol {
        /// The entry id.
        entry: String,
        /// The protocol text.
        protocol: String,
    },
    /// An entry names a template that is not in the catalogue.
    #[error("provider entry `{entry}` uses the unknown template `{template}`")]
    UnknownTemplate {
        /// The entry id.
        entry: String,
        /// The template id.
        template: String,
    },
    /// The provider is of a kind that cannot serve the role.
    #[error("provider `{id}` is {found} and cannot serve the {role} role")]
    KindNotValidForRole {
        /// The provider.
        id: ProviderId,
        /// The role asked for.
        role: CallRole,
        /// What the provider is, for display.
        found: String,
    },
    /// The model is not installed.
    #[error("model `{model}` is not installed")]
    ModelNotInstalled {
        /// The catalogue id, for the download action.
        model: ModelId,
        /// Download size in bytes.
        size_bytes: u64,
    },
    /// The model is still being downloaded or verified.
    #[error("model `{model}` is still being installed")]
    ModelInstalling {
        /// The catalogue id.
        model: ModelId,
    },
    /// The model is installed but its files no longer match the catalogue.
    #[error("model `{model}` failed verification")]
    ModelFailedVerification {
        /// The catalogue id, for the repair action.
        model: ModelId,
        /// The file that failed, when known.
        file: Option<String>,
    },
    /// The model's installation failed for another reason.
    #[error("model `{model}` is unusable: {detail}")]
    ModelFailed {
        /// The catalogue id.
        model: ModelId,
        /// Why.
        detail: String,
    },
    /// The catalogue entry lacks a file the runtime needs.
    #[error("model `{model}` has no `{purpose}` file")]
    ModelFilesIncomplete {
        /// The catalogue id.
        model: ModelId,
        /// The missing file's purpose.
        purpose: String,
    },
    /// The runtime or protocol is not built into this binary.
    #[error("provider `{id}` needs `{runtime}`, which is not built into this binary")]
    RuntimeNotBuiltIn {
        /// The provider.
        id: ProviderId,
        /// The runtime or protocol name.
        runtime: String,
    },
    /// The provider cannot stream, so it is not eligible for the live path.
    #[error("provider `{id}` cannot be used live: {reason}")]
    NotStreaming {
        /// The provider.
        id: ProviderId,
        /// Why, as the catalogue or the adapter says it.
        reason: String,
    },
    /// The provider does not support a language.
    #[error("provider `{id}` does not support language `{lang}`")]
    LanguageUnsupported {
        /// The provider.
        id: ProviderId,
        /// The missing language.
        lang: Lang,
    },
    /// The provider does not support a translation pair.
    #[error("provider `{id}` does not support {from}>{to}")]
    PairUnsupported {
        /// The provider.
        id: ProviderId,
        /// Language spoken.
        from: Lang,
        /// Language produced.
        to: Lang,
    },
    /// The entry needs a credential and none is stored.
    #[error("provider `{entry}` needs a credential and none is stored")]
    CredentialMissing {
        /// The entry id.
        entry: String,
    },
    /// The credential could not be read.
    #[error("the credential of provider `{entry}` cannot be read: {detail}")]
    CredentialUnreadable {
        /// The entry id.
        entry: String,
        /// Why (never the secret).
        detail: String,
    },
    /// The offline policy forbids the endpoint.
    #[error("offline mode blocks provider `{entry}` at {host}")]
    EndpointBlocked {
        /// The entry id.
        entry: String,
        /// The blocked host.
        host: String,
    },
    /// The base URL cannot be used.
    #[error("provider `{entry}` has an unusable address: {detail}")]
    InvalidEndpoint {
        /// The entry id.
        entry: String,
        /// What is wrong.
        detail: String,
    },
    /// A parameter does not satisfy the template's schema.
    #[error("provider `{id}`: parameter `{param}` {reason}")]
    InvalidParams {
        /// The provider.
        id: ProviderId,
        /// The parameter name.
        param: String,
        /// What is wrong with it.
        reason: String,
    },
    /// No voice profile id is configured for the cloned voice.
    #[error("no voice profile is selected for the cloned voice")]
    VoiceProfileNotSet,
    /// The profile directory or its reference recording does not exist.
    #[error("voice profile `{profile_id}` is missing at {}", .dir.display())]
    VoiceProfileMissing {
        /// The configured profile id.
        profile_id: String,
        /// Where it should be.
        dir: PathBuf,
    },
    /// The profile's files cannot be read.
    #[error("voice profile `{profile_id}` is unreadable: {detail}")]
    VoiceProfileUnreadable {
        /// The profile id.
        profile_id: String,
        /// Why.
        detail: String,
    },
    /// The profile was never prepared for any model.
    #[error("voice profile `{profile_id}` has not been prepared")]
    VoiceNotPrepared {
        /// The profile id.
        profile_id: String,
    },
    /// The profile was prepared for other models than the one configured.
    #[error("voice profile `{profile_id}` was prepared for a different model")]
    VoicePreparedForOtherModel {
        /// The profile id.
        profile_id: String,
        /// Fingerprint prefixes of the models it was prepared for.
        prepared_for: Vec<String>,
        /// Fingerprint prefix of the configured model.
        current: String,
    },
    /// The preset voice is not in the catalogue for this model and language.
    #[error("the preset voice `{name}` does not exist for this model")]
    VoicePresetUnknown {
        /// The configured name, or empty when the catalogue has no default.
        name: String,
    },
    /// The preset's reference recording is not on disk.
    #[error("the reference recording of preset voice `{preset}` is missing at {}", .path.display())]
    VoicePresetFileMissing {
        /// The preset id.
        preset: String,
        /// Where it should be.
        path: PathBuf,
    },
    /// A hand-installed preset cannot be read.
    #[error("the preset voice `{preset}` cannot be read: {detail}")]
    VoicePresetUnreadable {
        /// The preset id.
        preset: String,
        /// Why.
        detail: String,
    },
    /// A hand-installed preset is for another language than the direction speaks.
    #[error("the preset voice `{preset}` is for `{found}`, but `{wanted}` is needed")]
    VoicePresetLanguage {
        /// The preset id.
        preset: String,
        /// The language the direction needs.
        wanted: Lang,
        /// The language the preset declares.
        found: String,
    },
    /// The fallback is the primary provider.
    #[error("the fallback `{id}` is the same as the primary provider")]
    FallbackIsPrimary {
        /// The provider.
        id: ProviderId,
    },
}

/// Building a provider failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// The runtime or protocol is not built into this binary.
    #[error("provider `{provider}` needs `{runtime}`, which is not built into this binary")]
    NotBuiltIn {
        /// The provider.
        provider: ProviderId,
        /// The runtime or protocol name.
        runtime: String,
    },
    /// The offline policy forbids the endpoint; no connection was attempted.
    #[error("offline mode blocks provider `{provider}` at {host}")]
    Blocked {
        /// The provider.
        provider: ProviderId,
        /// The blocked host.
        host: String,
    },
    /// The address cannot be checked or used.
    #[error("provider `{provider}` has an unusable address: {detail}")]
    InvalidEndpoint {
        /// The provider.
        provider: ProviderId,
        /// What is wrong.
        detail: String,
    },
    /// The engine or client could not be started.
    #[error("provider `{provider}` failed to start: {detail}")]
    Start {
        /// The provider.
        provider: ProviderId,
        /// The adapter's message.
        detail: String,
    },
    /// The voice could not be prepared or registered on the engine.
    #[error("provider `{provider}`: voice setup failed: {detail}")]
    Voice {
        /// The provider.
        provider: ProviderId,
        /// Why.
        detail: String,
    },
    /// The built provider does not meet a requirement of its role.
    #[error(transparent)]
    Capability(#[from] CapabilityError),
}

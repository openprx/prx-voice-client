//! The top-level typed configuration, its defaults and its normalisation.

use serde::{Deserialize, Serialize};

use crate::migrate::CURRENT_SCHEMA_VERSION;
use crate::providers::ProvidersConfig;
use crate::sections::*;

/// Top-level application configuration.
///
/// The whole struct is `#[serde(default)]`: a section that is missing from the
/// file takes its defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Version of the file schema (see `migrate`). Must stay the first
    /// field: TOML requires plain values before tables.
    pub schema_version: u32,
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub assistant: AssistantConfig,
    pub privacy: PrivacyConfig,
    pub debug: DebugConfig,
    pub app: AppSection,
    pub call: CallConfig,
    pub pipeline: PipelineConfig,
    pub voice: VoiceConfig,
    pub hotkeys: HotkeysConfig,
    pub models: ModelsConfig,
    pub providers: ProvidersConfig,
    pub llm_server: LlmServerConfig,
    pub network_policy: NetworkPolicyConfig,
}

impl Default for AppConfig {
    /// The configuration of a new install.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            audio: AudioConfig::default(),
            ui: UiConfig::default(),
            assistant: AssistantConfig::default(),
            privacy: PrivacyConfig::default(),
            debug: DebugConfig::default(),
            app: AppSection::default(),
            call: CallConfig::default(),
            pipeline: PipelineConfig::default(),
            voice: VoiceConfig::default(),
            hotkeys: HotkeysConfig::default(),
            models: ModelsConfig::default(),
            providers: ProvidersConfig::default(),
            llm_server: LlmServerConfig::default(),
            network_policy: NetworkPolicyConfig::default(),
        }
    }
}

impl AppConfig {
    /// Bring redundant and bounded fields into a consistent state.
    ///
    /// * `audio.echo_control` is kept for older builds and always equals
    ///   `echo_mode != off`; when the two disagree, `echo_mode` wins.
    /// * `models.max_concurrent` is clamped to `1..=3`.
    pub fn normalize(&mut self) {
        self.audio.echo_control = self.audio.echo_mode != EchoMode::Off;
        self.models.max_concurrent = self.models.max_concurrent.clamp(1, 3);
    }
}

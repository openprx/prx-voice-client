//! The `[providers]` section: role selections and user-configured API provider
//! entries.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::credentials::PendingSecret;

/// A role to key to provider id map, e.g. `asr.zh = "whisper-small"`.
pub type RoleMap = BTreeMap<String, String>;

/// `[providers.active]` and `[providers.fallback]`: role, then key, then
/// provider id. The key is a language for `asr` and `tts`, a `"src>dst"` pair
/// for `mt`, and the empty string for `vad`. `asr_refine` is keyed by language
/// like `asr`: the model of the refine pass of two-pass recognition.
///
/// Empty by default: the design fills it at run time from the catalogue's
/// recommendations and writes it when the user first chooses (design 2.5,
/// migration table).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleSelection {
    pub vad: RoleMap,
    pub asr: RoleMap,
    pub asr_refine: RoleMap,
    pub mt: RoleMap,
    pub tts: RoleMap,
    pub tts_fallback: RoleMap,
}

/// `[[providers.entries]]`: a user-configured API provider (design 4.8.2).
///
/// A key (`api_key`, `token`, `secret`) is never written by this crate. One
/// that is found in a file and cannot be handed to a credential store is held
/// in the `pending_*` fields so that a save does not destroy it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderEntry {
    pub id: String,
    pub template: String,
    pub display_name: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    /// `"keychain:<id>"`, `"env:<VAR>"` or empty (no key).
    pub credential: String,
    pub local_endpoint: bool,
    /// Protocol-specific extras, passed through.
    #[serde(skip_serializing_if = "toml::Table::is_empty")]
    pub params: toml::Table,
    #[serde(rename = "api_key", skip_serializing_if = "Option::is_none")]
    pub pending_api_key: Option<PendingSecret>,
    #[serde(rename = "token", skip_serializing_if = "Option::is_none")]
    pub pending_token: Option<PendingSecret>,
    #[serde(rename = "secret", skip_serializing_if = "Option::is_none")]
    pub pending_secret: Option<PendingSecret>,
}

/// `[providers]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProvidersConfig {
    /// Informational: `"local"`, `"hybrid"`, `"cloud"` or `"custom"`.
    pub preset: String,
    /// Hash of the acknowledged egress list (design 4.8.3).
    pub egress_acknowledged: String,
    pub active: RoleSelection,
    pub fallback: RoleSelection,
    pub entries: Vec<ProviderEntry>,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            preset: "local".to_owned(),
            egress_acknowledged: String::new(),
            active: RoleSelection::default(),
            fallback: RoleSelection::default(),
            entries: Vec::new(),
        }
    }
}

//! The plain sections of `config.toml` (design section 2.5), each with its defaults.
//!
//! Every struct is `#[serde(default)]`, so any section or field missing from a
//! file takes its default.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// [audio]
// ---------------------------------------------------------------------------

/// Default silence (ms) after speech that ends an utterance.
pub const DEFAULT_END_SILENCE_MS: u32 = 800;
/// Default window (ms) in which a new utterance continues the previous turn.
pub const DEFAULT_MERGE_WINDOW_MS: u32 = 4_000;
/// Default minimum speech duration (ms) for an utterance to become a turn.
pub const DEFAULT_MIN_SPEECH_MS: u32 = 280;
/// Default time (ms) the microphone stays closed after playback ends.
pub const DEFAULT_HALF_DUPLEX_TAIL_MS: u32 = 300;

/// How the microphone is protected from the assistant's own speech.
///
/// An unknown value in the file reads as [`EchoMode::Auto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EchoMode {
    /// Decide from the audio route (speakers or headphones).
    #[default]
    Auto,
    /// Always close the microphone while the assistant speaks.
    Always,
    /// Never close the microphone.
    Off,
}

impl<'de> Deserialize<'de> for EchoMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "always" => Self::Always,
            "off" => Self::Off,
            _ => Self::Auto,
        })
    }
}

/// Audio-related settings.
///
/// Every field has a default, so a `config.toml` written before a field was
/// added still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub auto_play_reply: bool,
    /// Kept for older builds; always equals `echo_mode != off` once the
    /// configuration is normalised.
    pub echo_control: bool,
    /// Echo protection mode.
    pub echo_mode: EchoMode,
    /// The microphone stays closed this long after playback ends.
    pub half_duplex_tail_ms: u32,
    /// Silence after speech that ends an utterance, in milliseconds.
    /// Natural pauses inside a sentence are shorter than this.
    pub end_silence_ms: u32,
    /// An utterance recognised within this many milliseconds after the
    /// previous one was sent, before the assistant started answering, is a
    /// continuation of it: one merged turn replaces the first.
    pub merge_window_ms: u32,
    /// Utterances with less speech than this (milliseconds) are not sent as
    /// turns unless they continue the previous one.
    pub min_speech_ms: u32,
    /// Input device name; empty is the system default.
    pub input_device: String,
    /// Output device name; empty is the system default.
    pub output_device: String,
    /// Virtual microphone device name; empty is to detect.
    pub virtual_mic_device: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            auto_play_reply: true,
            echo_control: true,
            echo_mode: EchoMode::Auto,
            half_duplex_tail_ms: DEFAULT_HALF_DUPLEX_TAIL_MS,
            end_silence_ms: DEFAULT_END_SILENCE_MS,
            merge_window_ms: DEFAULT_MERGE_WINDOW_MS,
            min_speech_ms: DEFAULT_MIN_SPEECH_MS,
            input_device: String::new(),
            output_device: String::new(),
            virtual_mic_device: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// [ui]
// ---------------------------------------------------------------------------

/// Saved geometry and page of the settings window (design 2.1a). Every field
/// is absent until the window has been opened once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsWindowConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

/// UI presentation settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// `"en"`, `"zh-CN"` or `"system"`.
    pub language: String,
    pub show_transcript: bool,
    pub compact_mode: bool,
    pub theme: String,
    pub always_on_top: bool,
    pub sounds: bool,
    pub settings_window: SettingsWindowConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: String::from("en"),
            show_transcript: true,
            compact_mode: false,
            theme: String::from("rf"),
            always_on_top: true,
            sounds: false,
            settings_window: SettingsWindowConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// [network]
// ---------------------------------------------------------------------------

/// Network / connectivity settings.
///
/// Every field has a default, so a `config.toml` written before a field was
/// added still loads.
///
/// To run the desktop app against a real server set, in `[network]`:
/// `use_mock = false`, `server_url = "http://host:3000"`, and the provider
/// names the server should use (`asr_provider`, `agent_provider`,
/// `tts_provider`, e.g. `"sherpa"`, `"ollama"`, `"sherpa"` for the local
/// models) and `language`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub connect_timeout_ms: u64,
    pub reconnect_attempts: u32,
    pub reconnect_delay_ms: u64,
    /// PRX Voice server base URL.
    pub server_url: String,
    /// Whether to use the mock gateway (offline) instead of a live server.
    pub use_mock: bool,
    /// Session language requested from the server (e.g. `"zh-CN"`).
    pub language: String,
    /// Server-side ASR provider name (e.g. `"sherpa"`, `"mock"`).
    pub asr_provider: String,
    /// Server-side agent provider name (e.g. `"ollama"`, `"mock"`).
    pub agent_provider: String,
    /// Server-side TTS provider name (e.g. `"sherpa"`, `"mock"`).
    pub tts_provider: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 5_000,
            reconnect_attempts: 3,
            reconnect_delay_ms: 1_000,
            server_url: "http://localhost:3000".to_owned(),
            use_mock: true,
            language: "zh-CN".to_owned(),
            asr_provider: "mock".to_owned(),
            agent_provider: "mock".to_owned(),
            tts_provider: "mock".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// [privacy], [debug]
// ---------------------------------------------------------------------------

/// Privacy / data-retention settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub save_history: bool,
    /// Defaults to `false` for new installs (docs/codex.md 10.1). A file
    /// written before this default changed keeps the value it effectively
    /// had (`true`); see [`crate::migrate`].
    pub save_transcript: bool,
    pub log_sensitive: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            save_history: true,
            save_transcript: false,
            log_sensitive: false,
        }
    }
}

/// Debug / developer settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    pub enabled: bool,
    pub log_level: String,
    pub show_debug_panel: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_level: String::from("info"),
            show_debug_panel: false,
        }
    }
}

// ---------------------------------------------------------------------------
// New sections
// ---------------------------------------------------------------------------

/// `[app]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSection {
    /// `"assistant"` or `"call"`.
    pub mode: String,
    pub launch_at_login: bool,
    pub first_run_completed: bool,
    pub first_run_step: String,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            mode: "assistant".to_owned(),
            launch_at_login: false,
            first_run_completed: false,
            first_run_step: String::new(),
        }
    }
}

/// `[call]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CallConfig {
    /// Empty is derived from the interface language.
    pub self_language: String,
    pub peer_language: String,
    /// Bundle id; empty is to choose when starting.
    pub target_app: String,
    /// `"translation"`, `"both"` or `"original"`.
    pub local_output_mode: String,
    /// `"hands_free"` or `"push_to_talk"`.
    pub outgoing_capture: String,
    pub captions_show_source: bool,
    pub barge_in: bool,
}

impl Default for CallConfig {
    fn default() -> Self {
        Self {
            self_language: String::new(),
            peer_language: String::new(),
            target_app: String::new(),
            local_output_mode: "translation".to_owned(),
            outgoing_capture: "hands_free".to_owned(),
            captions_show_source: true,
            barge_in: true,
        }
    }
}

/// `[voice]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    pub self_profile_id: String,
    pub use_clone_outgoing: bool,
    /// Empty is the catalogue default.
    pub incoming_preset: String,
    pub fallback_preset: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            self_profile_id: String::new(),
            use_clone_outgoing: true,
            incoming_preset: String::new(),
            fallback_preset: String::new(),
        }
    }
}

/// `[hotkeys]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeysConfig {
    pub enabled: bool,
    pub session_toggle: String,
    pub push_to_talk: String,
    pub interrupt: String,
    /// `"toggle"` or `"hold"`.
    pub assistant_mode: String,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            session_toggle: "Ctrl+Shift+Space".to_owned(),
            push_to_talk: String::new(),
            interrupt: String::new(),
            assistant_mode: "toggle".to_owned(),
        }
    }
}

/// Default URL of the remote model catalogue.
pub const DEFAULT_CATALOGUE_URL: &str =
    "https://github.com/openprx/prx-voice-client/releases/latest/download/catalogue.json";

/// `[models]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    /// Empty is the default models directory.
    pub dir: String,
    pub preload: bool,
    pub warmup: bool,
    /// `"huggingface"`, `"modelscope"` or an `https://` URL.
    pub endpoint: String,
    /// Concurrent downloads, `1..=3`.
    pub max_concurrent: u32,
    pub catalogue_url: String,
    pub catalogue_auto_update: bool,
    /// `"<model-id>@<revision-key>"` entries.
    pub skipped_updates: Vec<String>,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            dir: String::new(),
            preload: true,
            warmup: true,
            endpoint: "huggingface".to_owned(),
            max_concurrent: 2,
            catalogue_url: DEFAULT_CATALOGUE_URL.to_owned(),
            catalogue_auto_update: false,
            skipped_updates: Vec::new(),
        }
    }
}

/// `[network_policy]`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkPolicyConfig {
    /// Block all outbound traffic except `allow_hosts` and loopback.
    pub offline: bool,
    pub allow_hosts: Vec<String>,
}

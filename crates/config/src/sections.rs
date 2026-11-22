//! The plain sections of `config.toml` (design section 2.5), each with its defaults.
//!
//! Every struct is `#[serde(default)]`, so any section or field missing from a
//! file takes its default.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// [audio]
// ---------------------------------------------------------------------------

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
    /// Input device name; empty is the system default.
    pub input_device: String,
    /// Output device name; empty is the system default.
    pub output_device: String,
    /// Virtual microphone device UID; empty is this app's own driver (no
    /// other virtual device is detected or suggested).
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
// [assistant]
// ---------------------------------------------------------------------------

/// Default silence (ms) after speech that ends the user's turn.
pub const DEFAULT_ASSISTANT_END_SILENCE_MS: u32 = 700;
/// Default most user turns the dialogue backend keeps in a conversation.
pub const DEFAULT_ASSISTANT_HISTORY_TURNS: u32 = 12;
/// Default most estimated tokens of conversation sent with a request.
pub const DEFAULT_ASSISTANT_HISTORY_TOKENS: u32 = 3_000;

/// The voice assistant: the language it listens and answers in, its voice, how it ends a turn and
/// how much conversation it remembers. The dialogue backend itself is chosen like any provider
/// (`providers.active.dialogue`, an entry of `providers.entries` for its address, model and
/// credential); the recognition and synthesis models are those of call translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssistantConfig {
    /// The language the assistant listens and answers in (`"zh-CN"` or `"en"`); empty follows the
    /// interface language.
    pub language: String,
    /// `"preset"` (a preset voice) or `"clone"` (the owner's enrolled voice).
    pub voice: String,
    /// The preset voice when `voice` is `"preset"`; empty is the catalogue default.
    pub voice_preset: String,
    /// Silence after speech that ends the user's turn, in milliseconds. Natural pauses inside a
    /// sentence are shorter than this.
    pub end_silence_ms: u32,
    /// Most user turns kept in a conversation (the current one included).
    pub history_max_turns: u32,
    /// Most estimated tokens of conversation sent with a request (the current turn is always sent).
    pub history_max_tokens: u32,
    /// How long the assistant's engines stay loaded after a session ends, in seconds, so that a
    /// new session within that time starts at once (released when call mode starts); 0 frees them
    /// with the session.
    pub keep_warm_s: u32,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        Self {
            language: String::new(),
            voice: "preset".to_owned(),
            voice_preset: String::new(),
            end_silence_ms: DEFAULT_ASSISTANT_END_SILENCE_MS,
            history_max_turns: DEFAULT_ASSISTANT_HISTORY_TURNS,
            history_max_tokens: DEFAULT_ASSISTANT_HISTORY_TOKENS,
            keep_warm_s: 60,
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
    /// Write the stage timestamps of every call clause to a file under the data directory
    /// (`traces/`). Off by default; `PRXVC_CALL_TRACE=1` also switches it on.
    pub call_trace: bool,
    /// Add the recognised, translated and spoken texts to the trace. Only with `call_trace`;
    /// off by default; `PRXVC_CALL_TRACE_TEXT=1` also switches it on.
    pub call_trace_text: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_level: String::from("info"),
            show_debug_panel: false,
            call_trace: false,
            call_trace_text: false,
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
    /// Tell the other party once, in their language, that the call is translated by a machine and
    /// that they hear a synthetic voice: a spoken notice played before the first translated
    /// sentence of each call mode session (off by default).
    pub disclosure: bool,
    /// The notice in English; empty uses the built-in text.
    pub disclosure_text_en: String,
    /// The notice in Simplified Chinese; empty uses the built-in text.
    pub disclosure_text_zh: String,
    /// Text size of the Call translation window's captions: `"small"`, `"medium"` or `"large"`.
    pub captions_size: String,
    pub barge_in: bool,
    /// `"manual"` or `"on_call"`: when call mode starts.
    pub start_mode: String,
    /// What the remote party hears while the outgoing direction is paused:
    /// `"original"` or `"silence"`.
    pub outgoing_when_paused: String,
    /// What the local user hears while the incoming direction is paused:
    /// `"original"` or `"silence"`.
    pub incoming_when_paused: String,
    /// What the remote party hears when the outgoing direction fails:
    /// `"original"` or `"silence"`.
    pub outgoing_on_failure: String,
    /// What the local user hears when the incoming direction fails:
    /// `"original"` or `"silence"`.
    pub incoming_on_failure: String,
    /// Gain of the original audio in `local_output_mode = "both"`, in dB.
    pub original_gain_db: f32,
    /// A queued clause whose source ended longer ago than this is dropped
    /// when the same speaker starts a new segment and `barge_in` is on.
    pub barge_in_max_lag_ms: u32,
    /// Whether a start translates (`true`) or only passes the audio through
    /// (`false`, the default). A start command may choose the other.
    pub translate: bool,
    /// Gain of the translated speech the local user hears, in dB. Compensates
    /// the lowering of other applications' sound while the call application
    /// runs its voice processing.
    pub output_gain_db: f32,
    /// Which engine translates a call: `"system"` (the operating system's translation) or `"llm"`
    /// (a language model; `providers.active.mt` says which). See [`crate::TranslationEngine`].
    pub translation_engine: String,
    /// How long a first Stop lets the last sentences play before call mode ends, in
    /// milliseconds: no new speech is accepted, what is committed is translated, synthesised and
    /// played to the end (to the listener and to the virtual microphone). A second Stop ends at
    /// once. 0 stops at once.
    pub stop_grace_ms: u32,
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
            disclosure: false,
            disclosure_text_en: String::new(),
            disclosure_text_zh: String::new(),
            captions_size: "medium".to_owned(),
            barge_in: true,
            start_mode: "manual".to_owned(),
            outgoing_when_paused: "original".to_owned(),
            incoming_when_paused: "original".to_owned(),
            outgoing_on_failure: "original".to_owned(),
            incoming_on_failure: "original".to_owned(),
            original_gain_db: -12.0,
            barge_in_max_lag_ms: 3_000,
            translate: false,
            output_gain_db: 0.0,
            stop_grace_ms: 6_000,
            translation_engine: crate::TranslationEngine::build_default().as_str().to_owned(),
        }
    }
}

/// `[llm_server]`: the local `llama-server` the application starts and stops by itself for the
/// language-model engine and the assistant's dialogue (the built-in `local-llama-server` provider).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmServerConfig {
    /// Path of the `llama-server` binary; empty searches `/opt/homebrew/bin`, `/usr/local/bin`
    /// and `PATH`.
    pub binary: String,
    /// Seconds the server keeps running after the last consumer released it while the engine is
    /// not the language model (the assistant's dialogue, for example). The language-model
    /// engine keeps it running without a limit.
    pub idle_stop_s: u32,
}

impl Default for LlmServerConfig {
    fn default() -> Self {
        Self {
            binary: String::new(),
            idle_stop_s: 60,
        }
    }
}

/// `[pipeline]`: tuning of the call-translation pipeline (segmentation,
/// commit rules, buffers). Shown under Advanced settings only. These keys are
/// not written into an existing file by the migration, so a better default
/// reaches existing installs in a later release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    /// `"streaming"` (the default: the streaming recogniser's text goes to translation, one model
    /// per direction) or `"two_pass"` (streaming draft, then a second decode of every clause with
    /// the larger refine model: more accurate text, a second model in memory and later clauses).
    pub recognition_mode: String,
    /// Silence inside a segment that commits a clause, in milliseconds.
    pub clause_pause_ms: u32,
    /// Silence that ends a segment, in milliseconds.
    pub segment_end_pause_ms: u32,
    /// After a segment has run this many seconds the shorter
    /// `long_segment_pause_ms` ends it.
    pub long_segment_after_s: u32,
    /// Segment-ending silence for a long segment, in milliseconds.
    pub long_segment_pause_ms: u32,
    /// Uncommitted audio is cut at this length, in seconds.
    pub max_clause_s: u32,
    /// A segment is cut at this length, in seconds.
    pub max_segment_s: u32,
    /// Speech probability above which a frame counts as speech.
    pub vad_threshold: f32,
    /// Speech shorter than this does not start a segment, in milliseconds.
    pub min_speech_ms: u32,
    /// Frames the VAD calls speech but whose mean level is below this (dBFS) do not start a
    /// segment and are never decoded. The level cannot tell a hum from speech (measured: hums
    /// -19 to -28 dBFS, benchmark speech down to -65 dBFS), so the floor only removes sounds at
    /// the level of dither, 10 dB below the quietest speech measured; -100 or less turns it off.
    pub min_speech_dbfs: f32,
    /// Audio kept before and after speech, in milliseconds.
    pub pad_ms: u32,
    /// When translatable units are committed from the recogniser's stable prefix, before the
    /// speaker pauses: `"segment_end"` (only at a pause or the end of the segment),
    /// `"punctuation"` (also at sentence-final and clause-level marks) or `"chunk"` (also, with no
    /// mark, once the stable text has `early_chunk_chars` or `early_chunk_words` units).
    pub early_commit: String,
    /// The most eager early commit allowed when the translation engine has no conversation context
    /// (the system translation translates each unit on its own): `"segment_end"`, `"punctuation"`
    /// or `"chunk"`. The mode in force is the less eager of this and `early_commit`, so the default
    /// `"punctuation"` keeps such an engine from receiving fragments cut mid-clause; `"chunk"`
    /// restores the same behaviour for every engine. An engine with context is not affected.
    pub early_commit_without_context: String,
    /// Fewest stable Chinese characters that make a chunk when no mark is available.
    pub early_chunk_chars: u32,
    /// Fewest stable English words that make a chunk when no mark is available.
    pub early_chunk_words: u32,
    /// Audio without a commit after which the chunk size is halved, in milliseconds.
    pub early_max_wait_ms: u32,
    /// Send the first target sentence to synthesis before the clause is complete.
    pub split_target_clauses: bool,
    /// Do not translate or speak a recognised unit that is only filler (嗯, 呃, 啊, um, uh, mm,
    /// hmm, mhm, and punctuation only). A real one-word "嗯" or "Mm." acknowledgement is dropped
    /// with them; content words such as 好, 对, yes, no and ok are never affected.
    pub drop_filler: bool,
    /// Do not translate or speak a recognised clause that is already in the language the listener
    /// hears (the other party spoke the listener's language): its original voice is passed through
    /// and the caption says "not translated: already in <language>".
    pub skip_source_in_target: bool,
    /// When a translation starts in the wrong language, ask for it once more with a stricter
    /// instruction before anything of it is spoken; a second miss fails the clause visibly and the
    /// direction's `*_on_failure` setting applies.
    pub retry_wrong_language: bool,
    /// Translate a stable partial ahead of its commit.
    pub speculative_mt: bool,
    /// Previous clauses given to translation as context.
    pub mt_context_clauses: u32,
    /// Playout jitter buffer, in milliseconds.
    pub jitter_buffer_ms: u32,
    /// Raise the start-of-clause pre-roll above `jitter_buffer_ms` when synthesis delivers audio
    /// slower than it is played (measured per call), so that a clause does not start with less
    /// queued than it needs to finish without a gap.
    pub adaptive_preroll: bool,
    /// The most pre-roll the adaptation asks for, in milliseconds.
    pub preroll_max_ms: u32,
    /// Catch up with a queue of translated speech without dropping any: while translated clauses
    /// wait behind speech still being played, the clauses played next are sped up (pitch kept)
    /// and their pauses cut short; normal pace returns when the queue is empty.
    pub catch_up: bool,
    /// Queued translated audio at which the speed-up starts, in milliseconds.
    pub catch_up_after_ms: u32,
    /// The fastest playback while catching up (1.25 is 25 % faster).
    pub catch_up_max_speed: f32,
    /// Cut the silence at the start and end of each translated clause (the synthesiser's lead-in
    /// and fade-out), keeping `silence_keep_ms` at the start and twice that at the end; a third
    /// of that while a queue exists.
    pub trim_silence: bool,
    /// Silence kept at the start of a translated clause, in milliseconds.
    pub silence_keep_ms: u32,
    /// Skip stale queued clauses to keep up with the speaker. Off plays everything, however late.
    pub backlog_skipping: bool,
    /// Queued audio per direction above which stale clauses are skipped, in milliseconds.
    pub max_backlog_ms: u32,
    /// A queued clause is stale once its source speech ended this long ago, in milliseconds.
    pub max_lag_ms: u32,
    /// Time between two partial recognition passes of one segment, in milliseconds (the
    /// recogniser re-decodes the growing audio of the open segment at this cadence).
    pub asr_redecode_interval_ms: u32,
    /// Audio a segment needs before its first partial recognition pass, in milliseconds.
    pub asr_first_decode_after_ms: u32,
    /// Consecutive partial results that must agree before text is declared stable (at least 2).
    pub asr_stable_agreement: u32,
    /// Words or characters at the end of a growing partial result that are never declared stable.
    pub asr_stable_tail_guard: u32,
    /// Commit a source prefix while the speaker goes on, once its translation is confirmed as the
    /// start of the translations of longer, later readings: `"auto"` (when the translation engine
    /// takes no conversation context), `"on"` (every engine) or `"off"` (never). Always off when
    /// the early commit in force is `"segment_end"`.
    pub retranslation: String,
    /// Confirming translations of longer readings that a prefix needs before it is spoken (at least 1).
    pub retranslate_agreement: u32,
    /// Units a confirming reading must extend past the prefix to count (at least 1).
    pub retranslate_lookahead: u32,
    /// Consecutive partial results whose units must agree on a prefix before it is a candidate (at least 2).
    pub retranslate_source_agreement: u32,
    /// Units at the end of the agreed prefix that are never part of a candidate.
    pub retranslate_source_guard: u32,
    /// Fewest Chinese characters in the new source part of a candidate.
    pub retranslate_min_chars: u32,
    /// Fewest words (other scripts) in the new source part of a candidate.
    pub retranslate_min_words: u32,
    /// Least time between two draft translations of one direction, in milliseconds.
    pub retranslate_interval_ms: u32,
    /// Largest share of the spoken units that may differ from the start of a new full translation
    /// for it to still count as continuing it (0.0 to 1.0; 0.0 asks for an exact start).
    pub retranslate_align_max_ratio: f32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            recognition_mode: "streaming".to_owned(),
            clause_pause_ms: 300,
            segment_end_pause_ms: 600,
            long_segment_after_s: 8,
            long_segment_pause_ms: 300,
            max_clause_s: 8,
            max_segment_s: 25,
            vad_threshold: 0.4,
            min_speech_ms: 160,
            min_speech_dbfs: -75.0,
            pad_ms: 200,
            early_commit: "chunk".to_owned(),
            early_commit_without_context: "punctuation".to_owned(),
            early_chunk_chars: 8,
            early_chunk_words: 6,
            early_max_wait_ms: 2_500,
            split_target_clauses: false,
            drop_filler: true,
            skip_source_in_target: true,
            retry_wrong_language: true,
            speculative_mt: false,
            mt_context_clauses: 4,
            jitter_buffer_ms: 150,
            adaptive_preroll: true,
            preroll_max_ms: 1_500,
            catch_up: true,
            catch_up_after_ms: 2_000,
            catch_up_max_speed: 1.25,
            trim_silence: true,
            silence_keep_ms: 60,
            backlog_skipping: true,
            max_backlog_ms: 15_000,
            max_lag_ms: 20_000,
            asr_redecode_interval_ms: 400,
            asr_first_decode_after_ms: 480,
            asr_stable_agreement: 4,
            asr_stable_tail_guard: 6,
            retranslation: "auto".to_owned(),
            retranslate_agreement: 2,
            retranslate_lookahead: 2,
            retranslate_source_agreement: 2,
            retranslate_source_guard: 1,
            retranslate_min_chars: 4,
            retranslate_min_words: 3,
            retranslate_interval_ms: 300,
            retranslate_align_max_ratio: 0.5,
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

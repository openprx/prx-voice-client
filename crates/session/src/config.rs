//! Per-session configuration supplied at creation time.

use serde::{Deserialize, Serialize};

/// Controls how the user can interrupt assistant playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterruptMode {
    /// User must explicitly press a button to interrupt.
    Manual,
    /// Voice activity detection triggers an interrupt automatically.
    VoiceActivity,
}

/// Controls transcript generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptMode {
    /// No transcript output.
    Off,
    /// Emit interim (partial) transcripts only.
    InterimOnly,
    /// Emit both interim and final transcripts.
    Full,
}

/// Per-session configuration supplied at creation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Language for speech recognition input.
    pub input_language: String,
    /// Language for assistant speech output.
    pub output_language: String,
    /// Whether to automatically play the assistant reply audio.
    pub auto_play_reply: bool,
    /// Whether to persist conversation history.
    pub save_history: bool,
    /// How the user can interrupt playback.
    pub interrupt_mode: InterruptMode,
    /// What level of transcription to produce.
    pub transcript_mode: TranscriptMode,
}

/// Shared fixture for unit tests in this crate.
#[cfg(test)]
pub(crate) fn test_config() -> SessionConfig {
    SessionConfig {
        input_language: "en".to_string(),
        output_language: "en".to_string(),
        auto_play_reply: true,
        save_history: false,
        interrupt_mode: InterruptMode::VoiceActivity,
        transcript_mode: TranscriptMode::Full,
    }
}

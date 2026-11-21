//! The protocols of the catalogue's provider templates (UX 4.8.1), parsed once.

use crate::keys::CallRole;

/// A client protocol of a provider entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Protocol {
    OpenAiChat,
    OllamaChat,
    DeepgramListen,
    OpenAiAudioSpeech,
    AzureSpeechTts,
    PrxHttpAsr,
    PrxHttpTts,
}

impl Protocol {
    pub(super) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "openai-chat" => Self::OpenAiChat,
            "ollama-chat" => Self::OllamaChat,
            "deepgram-listen" => Self::DeepgramListen,
            "openai-audio-speech" => Self::OpenAiAudioSpeech,
            "azure-speech-tts" => Self::AzureSpeechTts,
            "prx-http-asr" => Self::PrxHttpAsr,
            "prx-http-tts" => Self::PrxHttpTts,
            _ => return None,
        })
    }

    /// The role the protocol serves (a chat protocol serves translation and the assistant's
    /// dialogue; see [`Protocol::serves`]).
    pub(super) fn role(self) -> CallRole {
        match self {
            Self::OpenAiChat | Self::OllamaChat => CallRole::Mt,
            Self::DeepgramListen | Self::PrxHttpAsr => CallRole::Asr,
            Self::OpenAiAudioSpeech | Self::AzureSpeechTts | Self::PrxHttpTts => CallRole::Tts,
        }
    }

    /// Whether the protocol can serve `role`.
    pub(super) fn serves(self, role: CallRole) -> bool {
        self.role() == role || (self.role() == CallRole::Mt && role == CallRole::Dialogue)
    }
}

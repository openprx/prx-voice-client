//! The translation engine choice: `call.translation_engine`.
//!
//! One user-facing key decides which engine translates a call: `system` (the operating system's own
//! translation, macOS only) or `llm` (a language model behind an OpenAI-compatible endpoint, by
//! default the local `llama-server`). When it is `llm`, `providers.active.mt` still says which
//! language-model provider that is; when it is `system`, that selection is not consulted (and not
//! deleted), so switching back finds the language-model entry as it was.

use serde::{Deserialize, Serialize};

/// Which engine translates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationEngine {
    /// The operating system's translation service (Apple's Translation framework).
    System,
    /// A language model.
    Llm,
}

impl TranslationEngine {
    /// The value in the configuration file.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Llm => "llm",
        }
    }

    /// The engine of a configuration value (any case, surrounding space ignored).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "llm" => Some(Self::Llm),
            _ => None,
        }
    }

    /// The default of a new configuration in this build: system translation on macOS when the
    /// `apple-translation` feature is built in, the language model otherwise.
    pub const fn build_default() -> Self {
        if cfg!(all(target_os = "macos", feature = "apple-translation")) {
            Self::System
        } else {
            Self::Llm
        }
    }

    /// The engine the migration gives a configuration saved before the key existed: system
    /// translation on macOS (the owner's decision), the language model elsewhere. Independent of
    /// the build's features, so a development build without the bridge never pins `llm` into a
    /// user's file; such a build reports the missing bridge as a typed problem instead.
    pub const fn migrated_default() -> Self {
        if cfg!(target_os = "macos") {
            Self::System
        } else {
            Self::Llm
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_unknown_text_is_refused() {
        for e in [TranslationEngine::System, TranslationEngine::Llm] {
            assert_eq!(TranslationEngine::parse(e.as_str()), Some(e));
        }
        assert_eq!(
            TranslationEngine::parse(" LLM "),
            Some(TranslationEngine::Llm)
        );
        assert_eq!(TranslationEngine::parse("cloud"), None);
        assert_eq!(TranslationEngine::parse(""), None);
    }

    #[test]
    fn the_default_follows_the_platform_and_the_feature() {
        let expected = if cfg!(all(target_os = "macos", feature = "apple-translation")) {
            TranslationEngine::System
        } else {
            TranslationEngine::Llm
        };
        assert_eq!(TranslationEngine::build_default(), expected);
        assert_eq!(
            TranslationEngine::migrated_default() == TranslationEngine::System,
            cfg!(target_os = "macos")
        );
    }
}

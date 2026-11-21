//! Configuration of the OpenAI-compatible dialogue backend.

use serde_json::{Map, Value, json};

use crate::mt::config::{ApiKey, ConfigError, Timeouts};
use prxvc_media::Lang;

/// How much conversation the backend keeps and sends with every request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryLimits {
    /// Most user turns (the current one included) a request carries.
    pub max_turns: usize,
    /// Most estimated tokens of history (the current turn is always sent, whatever its size).
    pub max_tokens: u32,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self {
            max_turns: 12,
            max_tokens: 3_000,
        }
    }
}

/// Everything one `OpenAiChatDialogue` needs.
#[derive(Clone, Debug)]
pub struct DialogueConfig {
    /// The configured provider id, for the descriptor and traces.
    pub provider_id: String,
    /// Base URL; `http://host:8080` and `http://host:8080/v1` both work.
    pub base_url: String,
    /// Model name sent in the request.
    pub model: String,
    /// Bearer token, sent only when present.
    pub api_key: Option<ApiKey>,
    /// Extra request body fields (sampling, `chat_template_kwargs`), taken from the provider entry
    /// and its template. They override the adapter's own `max_tokens`.
    pub params: Map<String, Value>,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// History bounds.
    pub history: HistoryLimits,
    /// Languages the model answers in.
    pub languages: Vec<Lang>,
    /// Ask for token usage in the last chunk (`stream_options.include_usage`).
    pub include_usage: bool,
}

/// The longest reply asked for, in tokens: a spoken answer is short.
pub const DEFAULT_MAX_REPLY_TOKENS: u32 = 400;

impl DialogueConfig {
    /// A configuration for `base_url` and `model` with the defaults: a moderate temperature, the
    /// default history bounds, Chinese and English.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let mut params = Map::new();
        params.insert("temperature".into(), json!(0.7));
        Self {
            provider_id: "openai-chat-dialogue".into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            params,
            timeouts: Timeouts {
                // A reply may take longer to start than a clause translation.
                first_token: std::time::Duration::from_secs(15),
                idle: std::time::Duration::from_secs(8),
                ..Timeouts::default()
            },
            history: HistoryLimits::default(),
            languages: vec![Lang::Zh, Lang::En],
            include_usage: true,
        }
    }

    /// Set the provider id.
    #[must_use]
    pub fn with_provider_id(mut self, id: impl Into<String>) -> Self {
        self.provider_id = id.into();
        self
    }

    /// Set the bearer token; an empty key means none.
    #[must_use]
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        self.api_key = (!key.trim().is_empty()).then(|| ApiKey::new(key));
        self
    }

    /// Replace the extra request parameters.
    #[must_use]
    pub fn with_params(mut self, params: Map<String, Value>) -> Self {
        self.params = params;
        self
    }

    /// Set the timeouts.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Set the history bounds.
    #[must_use]
    pub fn with_history(mut self, history: HistoryLimits) -> Self {
        self.history = history;
        self
    }

    /// Check the configuration; called by `OpenAiChatDialogue::new`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        crate::chat_http::endpoint_url(&self.base_url)?;
        for key in crate::chat_http::RESERVED_PARAMS {
            if self.params.contains_key(key) {
                return Err(ConfigError::ReservedParam(key.to_owned()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reserved_parameter_and_a_bad_address_are_refused() {
        let mut cfg = DialogueConfig::new("http://127.0.0.1:8080/v1", "m");
        assert!(cfg.validate().is_ok());
        cfg.params.insert("messages".into(), json!([]));
        assert!(matches!(cfg.validate(), Err(ConfigError::ReservedParam(k)) if k == "messages"));
        assert!(matches!(
            DialogueConfig::new("ftp://h", "m").validate(),
            Err(ConfigError::InvalidBaseUrl(_))
        ));
    }

    #[test]
    fn an_empty_key_means_none() {
        assert!(
            DialogueConfig::new("http://h", "m")
                .with_api_key(" ")
                .api_key
                .is_none()
        );
        assert!(
            DialogueConfig::new("http://h", "m")
                .with_api_key("k")
                .api_key
                .is_some()
        );
    }
}

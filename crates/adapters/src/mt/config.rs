//! Configuration of the OpenAI-compatible chat translator, and the request body it produces.

use std::fmt;
use std::time::Duration;

use prxvc_media::Lang;
use prxvc_ports::MtRequest;
use serde_json::{Map, Value, json};

use super::assess::max_output_tokens;
use super::prompt::{PromptLimits, PromptTemplate, render_messages};

/// A bearer token. `Debug` never shows it and there is no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a key.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key text, for the `Authorization` header only.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// The three timeouts of a request, each with its own error.
///
/// Defaults are provisional \[C\]: the local server answers its first token in
/// 0.1 to 0.6 s on the development machine (`bench/e2e` logs), so these are
/// generous for a local server and tight for a cloud one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    /// Establishing the TCP connection.
    pub connect: Duration,
    /// From sending the request to the first content token.
    pub first_token: Duration,
    /// Silence on the stream after the first token.
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(2),
            first_token: Duration::from_secs(5),
            idle: Duration::from_secs(3),
        }
    }
}

/// A configuration that cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The base URL is not an `http` or `https` URL.
    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(String),
    /// An extra parameter names a field the adapter owns.
    #[error("extra parameter `{0}` is set by the adapter and cannot be overridden")]
    ReservedParam(String),
    /// The HTTP client or its runtime could not be built.
    #[error("cannot start the HTTP client: {0}")]
    Client(String),
}

/// Everything one `OpenAiChatMt` needs.
#[derive(Clone, Debug)]
pub struct OpenAiChatConfig {
    /// The configured provider id, for the descriptor and traces.
    pub provider_id: String,
    /// Base URL; `http://host:8080` and `http://host:8080/v1` both work.
    pub base_url: String,
    /// Model name sent in the request.
    pub model: String,
    /// Bearer token, sent only when present.
    pub api_key: Option<ApiKey>,
    /// Extra request body fields (sampling settings and the like), taken from
    /// the provider entry. They override the adapter's own `max_tokens`.
    pub params: Map<String, Value>,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// Prompt template.
    pub template: PromptTemplate,
    /// Context and glossary bounds.
    pub limits: PromptLimits,
    /// Languages the model is configured for.
    pub languages: Vec<Lang>,
    /// Ask for token usage in the last chunk (`stream_options.include_usage`).
    pub include_usage: bool,
}

impl OpenAiChatConfig {
    /// A configuration for `base_url` and `model` with the provisional
    /// defaults: `temperature = 0` [C, PD-2.6.3], the generic template, Chinese
    /// and English.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let mut params = Map::new();
        params.insert("temperature".into(), json!(0));
        Self {
            provider_id: "openai-chat".into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            params,
            timeouts: Timeouts::default(),
            template: PromptTemplate::Generic,
            limits: PromptLimits::default(),
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

    /// Set the prompt template.
    #[must_use]
    pub fn with_template(mut self, template: PromptTemplate) -> Self {
        self.template = template;
        self
    }

    /// Set the context and glossary bounds.
    #[must_use]
    pub fn with_limits(mut self, limits: PromptLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Check the configuration; called by `OpenAiChatMt::new`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.endpoint_url().map(|_| ())?;
        for key in crate::chat_http::RESERVED_PARAMS {
            if self.params.contains_key(key) {
                return Err(ConfigError::ReservedParam(key.to_owned()));
            }
        }
        Ok(())
    }

    /// The chat completions URL: `{base}/v1/chat/completions`, tolerating a
    /// base that already ends in `/v1` and trailing slashes.
    pub fn endpoint_url(&self) -> Result<reqwest::Url, ConfigError> {
        crate::chat_http::endpoint_url(&self.base_url)
    }

    /// The JSON body of the request for `req`.
    ///
    /// `max_tokens` is `3 x source tokens + 16` (PD-2.6.3) unless the extra
    /// parameters set their own.
    pub fn request_body(&self, req: &MtRequest) -> Value {
        let mut body = Map::new();
        body.insert("model".into(), json!(self.model));
        body.insert(
            "messages".into(),
            json!(render_messages(self.template, req, &self.limits)),
        );
        body.insert("stream".into(), json!(true));
        if self.include_usage {
            body.insert("stream_options".into(), json!({"include_usage": true}));
        }
        body.insert("max_tokens".into(), json!(max_output_tokens(&req.text)));
        for (key, value) in &self.params {
            body.insert(key.clone(), value.clone());
        }
        Value::Object(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_media::{ClauseId, Direction, SegmentId};

    fn req(source: Lang, target: Lang, text: &str) -> MtRequest {
        MtRequest {
            clause: ClauseId::new(SegmentId::first(Direction::Incoming), 0),
            source,
            target,
            text: text.into(),
            context: if source == Lang::Zh {
                vec![("你好。".into(), "Hi.".into())]
            } else {
                vec![("Hi.".into(), "你好。".into())]
            },
            glossary: if source == Lang::Zh {
                vec![("普罗米修斯".into(), "Prometheus".into())]
            } else {
                vec![("Prometheus".into(), "普罗米修斯".into())]
            },
            strict_language: false,
        }
    }

    #[test]
    fn the_endpoint_url_tolerates_v1_and_slashes() {
        for base in [
            "http://h:8080",
            "http://h:8080/",
            "http://h:8080/v1",
            "http://h:8080/v1/",
            " http://h:8080/v1 ",
        ] {
            let url = OpenAiChatConfig::new(base, "m").endpoint_url().unwrap();
            assert_eq!(url.as_str(), "http://h:8080/v1/chat/completions", "{base}");
        }
        assert!(matches!(
            OpenAiChatConfig::new("ftp://h", "m").validate(),
            Err(ConfigError::InvalidBaseUrl(_))
        ));
        assert!(matches!(
            OpenAiChatConfig::new("not a url", "m").validate(),
            Err(ConfigError::InvalidBaseUrl(_))
        ));
    }

    #[test]
    fn reserved_parameters_are_refused() {
        let mut params = Map::new();
        params.insert("messages".into(), json!([]));
        let cfg = OpenAiChatConfig::new("http://h", "m").with_params(params);
        assert_eq!(
            cfg.validate(),
            Err(ConfigError::ReservedParam("messages".into()))
        );
    }

    #[test]
    fn the_request_body_for_the_outgoing_direction_is_exactly_this() {
        let cfg = OpenAiChatConfig::new("http://127.0.0.1:8080/v1", "gemma-4-e4b")
            .with_template(PromptTemplate::Gemma4);
        let request = req(Lang::Zh, Lang::En, "三点见。");
        let body = cfg.request_body(&request);
        let system = render_messages(PromptTemplate::Gemma4, &request, &cfg.limits)
            .remove(0)
            .content;
        assert_eq!(
            body,
            json!({
                "model": "gemma-4-e4b",
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": "<source>三点见。</source>"}
                ],
                "stream": true,
                "stream_options": {"include_usage": true},
                "max_tokens": 28,
                "temperature": 0
            })
        );
        assert!(system.contains("from Simplified Chinese into English"));
        assert!(system.contains("普罗米修斯 => Prometheus"));
        assert!(system.contains("Simplified Chinese: 你好。\nEnglish: Hi."));
    }

    #[test]
    fn the_incoming_direction_swaps_the_languages_and_extra_parameters_win() {
        let mut params = Map::new();
        params.insert("temperature".into(), json!(0.2));
        params.insert("max_tokens".into(), json!(64));
        params.insert("cache_prompt".into(), json!(true));
        let cfg = OpenAiChatConfig::new("http://h", "m").with_params(params);
        let body = cfg.request_body(&req(Lang::En, Lang::Zh, "See you at three."));
        assert_eq!(body["temperature"], json!(0.2));
        assert_eq!(body["max_tokens"], json!(64));
        assert_eq!(body["cache_prompt"], json!(true));
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("from English into Simplified Chinese"));
        assert!(system.contains("Prometheus => 普罗米修斯"));
        assert!(system.contains("English: Hi.\nSimplified Chinese: 你好。"));
    }

    #[test]
    fn usage_can_be_switched_off_for_servers_that_reject_stream_options() {
        let mut cfg = OpenAiChatConfig::new("http://h", "m");
        cfg.include_usage = false;
        let body = cfg.request_body(&req(Lang::En, Lang::Zh, "x"));
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn the_api_key_never_appears_in_debug_output() {
        let cfg = OpenAiChatConfig::new("http://h", "m").with_api_key("sk-secret-123");
        assert!(!format!("{cfg:?}").contains("sk-secret-123"));
        assert!(format!("{cfg:?}").contains("<redacted>"));
        let body = cfg.request_body(&req(Lang::En, Lang::Zh, "x"));
        assert!(!body.to_string().contains("sk-secret-123"));
        assert!(
            OpenAiChatConfig::new("http://h", "m")
                .with_api_key("  ")
                .api_key
                .is_none()
        );
    }
}

//! OpenAI-compatible chat-completions machine translation.
//!
//! Works against any endpoint speaking the OpenAI `/chat/completions` schema —
//! the OpenAI cloud, or a local server such as Ollama / LM Studio / llama.cpp.
//! Configure via [`OpenAiTranslator::from_env`]:
//!
//! ```text
//! PRXVC_MT_BASE_URL   e.g. http://localhost:11434/v1   (Ollama) or
//!                          https://api.openai.com/v1
//! PRXVC_MT_MODEL      e.g. qwen2.5:7b  /  gpt-4o-mini
//! PRXVC_MT_API_KEY    optional; required by cloud endpoints
//! ```

use prxvc_session::adapter::{AdapterError, Translator};
use serde_json::Value;

/// Machine translation over an OpenAI-compatible chat endpoint.
pub struct OpenAiTranslator {
    client: reqwest::blocking::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiTranslator {
    /// Build a translator for an explicit endpoint.
    ///
    /// `base_url` should include the API root (e.g. `.../v1`); the trailing
    /// `/chat/completions` is appended.
    ///
    /// # Errors
    /// [`AdapterError::Translation`] if the HTTP client cannot be constructed.
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Result<Self, AdapterError> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| AdapterError::Translation(format!("http client: {e}")))?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            model: model.into(),
            api_key,
        })
    }

    /// Build from `PRXVC_MT_BASE_URL` / `PRXVC_MT_MODEL` / `PRXVC_MT_API_KEY`.
    ///
    /// # Errors
    /// [`AdapterError::Translation`] if the required env vars are unset.
    pub fn from_env() -> Result<Self, AdapterError> {
        let base_url = std::env::var("PRXVC_MT_BASE_URL")
            .map_err(|_| AdapterError::Translation("PRXVC_MT_BASE_URL not set".into()))?;
        let model = std::env::var("PRXVC_MT_MODEL")
            .map_err(|_| AdapterError::Translation("PRXVC_MT_MODEL not set".into()))?;
        let api_key = std::env::var("PRXVC_MT_API_KEY").ok();
        Self::new(base_url, model, api_key)
    }

    /// Whether the env configuration for [`from_env`](Self::from_env) is present.
    pub fn is_configured() -> bool {
        std::env::var("PRXVC_MT_BASE_URL").is_ok() && std::env::var("PRXVC_MT_MODEL").is_ok()
    }
}

/// Build the chat-completions request body for a translation (pure, testable).
fn build_request_body(model: &str, text: &str, source_lang: &str, target_lang: &str) -> Value {
    let system = format!(
        "You are a professional real-time interpreter. Translate the user's text \
         from {source_lang} to {target_lang}. Output ONLY the translation, with no \
         quotes, notes, or explanations."
    );
    serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text },
        ],
    })
}

/// Extract `choices[0].message.content` from a chat-completions response.
fn extract_translation(body: &Value) -> Result<String, AdapterError> {
    body.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_owned())
        .ok_or_else(|| AdapterError::Translation(format!("unexpected response shape: {body}")))
}

impl Translator for OpenAiTranslator {
    fn translate(
        &mut self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AdapterError> {
        if text.trim().is_empty() {
            return Ok(String::new());
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_request_body(&self.model, text, source_lang, target_lang);

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req
            .send()
            .map_err(|e| AdapterError::Translation(format!("request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(AdapterError::Translation(format!(
                "endpoint returned {}",
                resp.status()
            )));
        }
        let json: Value = resp
            .json()
            .map_err(|e| AdapterError::Translation(format!("decode failed: {e}")))?;
        extract_translation(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_has_system_and_user_messages() {
        let body = build_request_body("m", "hello", "zh", "en");
        assert_eq!(body["model"], "m");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .unwrap()
                .contains("from zh to en")
        );
        assert_eq!(messages[1]["content"], "hello");
    }

    #[test]
    fn extract_reads_choice_content() {
        let body = serde_json::json!({
            "choices": [ { "message": { "content": "  Hello  " } } ]
        });
        assert_eq!(extract_translation(&body).unwrap(), "Hello");
    }

    #[test]
    fn extract_rejects_bad_shape() {
        let body = serde_json::json!({ "error": "nope" });
        assert!(extract_translation(&body).is_err());
    }
}

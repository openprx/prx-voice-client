//! The system prompt of a spoken assistant and the request body of one turn.
//!
//! The prompt is a data file under `crates/adapters/prompts/`, embedded at build time. Its only
//! placeholder is `{language}`: the language the user spoke, so that the reply is in it.

use prxvc_media::Lang;
use prxvc_ports::{DialogueMessage, DialogueRole};
use serde_json::{Map, Value, json};

use super::config::{DEFAULT_MAX_REPLY_TOKENS, DialogueConfig};

const SYSTEM: &str = include_str!("../../prompts/assistant_system.txt");

/// The name of `lang` as the prompt spells it.
fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "Chinese (Simplified)",
        Lang::En => "English",
    }
}

/// The system prompt for a user who speaks `lang`.
pub fn system_prompt(lang: Lang) -> String {
    SYSTEM.trim().replace("{language}", language_name(lang))
}

fn role_name(role: DialogueRole) -> &'static str {
    match role {
        DialogueRole::User => "user",
        DialogueRole::Assistant => "assistant",
    }
}

/// The JSON body of a request: the system prompt, the history, then the new messages.
pub fn request_body<'a>(
    cfg: &DialogueConfig,
    lang: Lang,
    history: impl Iterator<Item = &'a DialogueMessage>,
    new: &'a [DialogueMessage],
) -> Value {
    let mut messages = vec![json!({"role": "system", "content": system_prompt(lang)})];
    messages.extend(
        history
            .chain(new.iter())
            .map(|m| json!({"role": role_name(m.role), "content": m.text})),
    );
    let mut body = Map::new();
    body.insert("model".into(), json!(cfg.model));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), json!(true));
    if cfg.include_usage {
        body.insert("stream_options".into(), json!({"include_usage": true}));
    }
    body.insert("max_tokens".into(), json!(DEFAULT_MAX_REPLY_TOKENS));
    for (key, value) in &cfg.params {
        body.insert(key.clone(), value.clone());
    }
    Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_names_the_users_language_and_asks_for_short_plain_speech() {
        let zh = system_prompt(Lang::Zh);
        assert!(zh.contains("Chinese (Simplified)"));
        assert!(!zh.contains("{language}"));
        assert!(system_prompt(Lang::En).contains("English"));
        assert!(zh.contains("No markdown"));
        assert!(zh.contains("briefly"));
    }

    #[test]
    fn the_body_has_the_system_prompt_first_then_history_then_the_new_messages() {
        let cfg = DialogueConfig::new("http://h", "gemma");
        let history = [
            DialogueMessage::user("hi"),
            DialogueMessage {
                role: DialogueRole::Assistant,
                text: "hello".into(),
            },
        ];
        let body = request_body(
            &cfg,
            Lang::En,
            history.iter(),
            &[DialogueMessage::user("and now?")],
        );
        let roles: Vec<_> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["role"].as_str().unwrap(),
                    m["content"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(roles[0].0, "system");
        assert_eq!(roles[1], ("user", "hi".to_owned()));
        assert_eq!(roles[2], ("assistant", "hello".to_owned()));
        assert_eq!(roles[3], ("user", "and now?".to_owned()));
        assert_eq!(body["model"], "gemma");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], DEFAULT_MAX_REPLY_TOKENS);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn extra_parameters_override_the_default_cap_and_pass_through() {
        let mut cfg = DialogueConfig::new("http://h", "m");
        cfg.params.insert("max_tokens".into(), json!(64));
        cfg.params.insert(
            "chat_template_kwargs".into(),
            json!({"enable_thinking": false}),
        );
        let body = request_body(
            &cfg,
            Lang::En,
            std::iter::empty(),
            &[DialogueMessage::user("x")],
        );
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    }
}

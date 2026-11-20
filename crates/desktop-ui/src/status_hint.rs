//! Status wording: the human label and the hint line shown for a session state.

/// The hint line shown under the state label for a session state.
pub(crate) fn state_hint(session_state: &str) -> String {
    match session_state {
        "" | "Idle" | "Completed" => prxvc_i18n::t("session.hint.idle"),
        "Listening" => prxvc_i18n::t("session.hint.listening"),
        "AssistantSpeaking" => prxvc_i18n::t("session.hint.speaking"),
        _ => String::new(),
    }
}

/// Translate a raw session state to a human-friendly i18n label for the orb UI.
pub(crate) fn translate_state_human(state: &str) -> String {
    match state {
        "" | "Idle" => prxvc_i18n::t("orb.ready"),
        "Listening" => prxvc_i18n::t("orb.listening"),
        "UserSpeaking" => prxvc_i18n::t("orb.hearing"),
        "Recognizing" => prxvc_i18n::t("orb.processing"),
        "Thinking" => prxvc_i18n::t("orb.thinking"),
        "AssistantSpeaking" => prxvc_i18n::t("orb.speaking"),
        "Interrupted" => prxvc_i18n::t("orb.interrupted"),
        "Completed" => prxvc_i18n::t("orb.ended"),
        other => other.to_string(),
    }
}

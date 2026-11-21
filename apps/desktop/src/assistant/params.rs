//! `assistant.*`, `voice.*` and `[pipeline]` as the parameters of one assistant session.

use std::time::Duration;

use prxvc_config::AppConfig;
use prxvc_media::Lang;
use prxvc_ports::{ConversationId, VoiceSelection};
use prxvc_providers::VoiceChoice;
use prxvc_session::assistant::AssistantParams;

fn ms(n: u32) -> Duration {
    Duration::from_millis(u64::from(n))
}

/// The language the assistant listens and answers in: `assistant.language`, else the interface
/// language (`"system"` follows `system`, the operating system's language tag). Chinese for a
/// language tag that starts with `zh`, English otherwise.
pub(crate) fn language_of(config: &AppConfig, system: Option<&str>) -> Lang {
    let setting = match config.assistant.language.trim() {
        "" => config.ui.language.trim(),
        text => text,
    };
    let tag = if setting.is_empty() || setting == "system" {
        system.unwrap_or("")
    } else {
        setting
    };
    if tag.to_ascii_lowercase().starts_with("zh") {
        Lang::Zh
    } else {
        Lang::En
    }
}

/// The voice the assistant asks for: the owner's enrolled voice (`assistant.voice = "clone"`), else
/// a preset. With no preset named for the assistant, the preset of the other party of a call
/// (`voice.incoming_preset`) speaks: the assistant is a voice other than the user's own, like the
/// remote party. When that is empty too, the name stays empty (the catalogue's default for the model
/// and language, which is a typed problem while the catalogue ships no preset).
pub(crate) fn voice_choice(config: &AppConfig) -> VoiceChoice {
    if config.assistant.voice.trim() == "clone" {
        return VoiceChoice::OwnClone;
    }
    let own = config.assistant.voice_preset.trim();
    let name = if own.is_empty() {
        config.voice.incoming_preset.trim()
    } else {
        own
    };
    VoiceChoice::Preset(name.to_owned())
}

/// The parameters of a session in `lang` speaking with `voice`: the user's turn ends after
/// `assistant.end_silence_ms` of silence, and the speech detector and the recogniser follow the
/// `[pipeline]` tuning of call translation.
pub(crate) fn session_params(
    config: &AppConfig,
    lang: Lang,
    voice: VoiceSelection,
    conversation: ConversationId,
) -> AssistantParams {
    let mut params = AssistantParams::new(
        conversation,
        lang,
        voice,
        ms(config.assistant.end_silence_ms.max(100)),
    );
    params.speak = config.audio.auto_play_reply;
    let p = &config.pipeline;
    let r = &mut params.recognition;
    r.vad_threshold = p.vad_threshold;
    r.min_speech = ms(p.min_speech_ms);
    r.min_speech_dbfs = p.min_speech_dbfs;
    r.pad = ms(p.pad_ms);
    r.drop_filler = p.drop_filler;
    r.commit.max_clause = Duration::from_secs(u64::from(p.max_clause_s));
    r.commit.max_segment = Duration::from_secs(u64::from(p.max_segment_s));
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_follows_the_setting_then_the_interface() {
        let mut c = AppConfig::default();
        c.ui.language = "en".into();
        assert_eq!(language_of(&c, None), Lang::En);
        c.ui.language = "zh-CN".into();
        assert_eq!(language_of(&c, None), Lang::Zh);
        c.assistant.language = "en".into();
        assert_eq!(
            language_of(&c, None),
            Lang::En,
            "the assistant's own setting wins"
        );
        c.assistant.language = "zh-CN".into();
        c.ui.language = "en".into();
        assert_eq!(language_of(&c, None), Lang::Zh);
        c.assistant.language.clear();
        c.ui.language = "system".into();
        assert_eq!(language_of(&c, Some("zh-Hans-CN")), Lang::Zh);
        assert_eq!(language_of(&c, Some("fr-FR")), Lang::En);
        assert_eq!(language_of(&c, None), Lang::En);
    }

    #[test]
    fn the_voice_is_a_preset_unless_the_owners_voice_is_chosen() {
        let mut c = AppConfig::default();
        assert_eq!(voice_choice(&c), VoiceChoice::Preset(String::new()));
        c.assistant.voice_preset = " warm-en ".into();
        assert_eq!(voice_choice(&c), VoiceChoice::Preset("warm-en".into()));
        c.assistant.voice_preset.clear();
        c.voice.incoming_preset = "calm-zh".into();
        assert_eq!(
            voice_choice(&c),
            VoiceChoice::Preset("calm-zh".into()),
            "without a preset of its own the assistant speaks with the other party's preset"
        );
        c.assistant.voice_preset = "warm-en".into();
        assert_eq!(
            voice_choice(&c),
            VoiceChoice::Preset("warm-en".into()),
            "its own preset wins"
        );
        c.assistant.voice = "clone".into();
        assert_eq!(voice_choice(&c), VoiceChoice::OwnClone);
    }

    #[test]
    fn the_session_takes_the_turn_end_and_the_pipeline_tuning() {
        let mut c = AppConfig::default();
        c.assistant.end_silence_ms = 900;
        c.pipeline.vad_threshold = 0.55;
        c.pipeline.drop_filler = false;
        let p = session_params(
            &c,
            Lang::En,
            VoiceSelection::Preset { name: "p".into() },
            ConversationId::new("s"),
        );
        assert_eq!(p.recognition.commit.segment_end_pause, ms(900));
        assert_eq!(
            p.recognition.commit.clause_pause,
            ms(900),
            "one decode per turn"
        );
        assert_eq!(p.recognition.vad_threshold, 0.55);
        assert!(!p.recognition.drop_filler);
        assert_eq!(p.lang, Lang::En);
        assert!(p.speak, "replies are spoken by default");
        c.audio.auto_play_reply = false;
        let p = session_params(
            &c,
            Lang::En,
            VoiceSelection::Preset { name: "p".into() },
            ConversationId::new("s"),
        );
        assert!(
            !p.speak,
            "audio.auto_play_reply off makes a reply text only"
        );
    }
}

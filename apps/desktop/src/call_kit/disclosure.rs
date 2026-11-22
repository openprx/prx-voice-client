//! The spoken notice to the other party as the call's configuration says it: on or off, and the sentence in the language the other party hears.

use prxvc_config::AppConfig;
use prxvc_i18n::Locale;
use prxvc_media::Lang;
use prxvc_session::call::DisclosureSpec;

/// The interface locale whose built-in text is the notice in `lang`.
fn locale_of(lang: Lang) -> Locale {
    match lang {
        Lang::Zh => Locale::ZhCn,
        Lang::En => Locale::En,
    }
}

/// The notice's sentence in `lang`: the configured text when it has one, otherwise the built-in
/// text of that language (never the interface language's: the other party does not read our UI).
pub(crate) fn disclosure_text(config: &AppConfig, lang: Lang) -> String {
    let configured = match lang {
        Lang::Zh => &config.call.disclosure_text_zh,
        Lang::En => &config.call.disclosure_text_en,
    };
    let configured = configured.trim();
    if configured.is_empty() {
        prxvc_i18n::t_in(locale_of(lang), "call.disclosure.text")
    } else {
        configured.to_owned()
    }
}

/// What the outgoing direction carries: the setting and the sentence for the language its
/// translation is spoken in.
pub(crate) fn disclosure_spec(config: &AppConfig, target: Lang) -> DisclosureSpec {
    DisclosureSpec {
        enabled: config.call.disclosure,
        text: disclosure_text(config, target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_is_off_by_default_and_in_the_language_of_the_other_party() {
        let mut config = AppConfig::default();
        assert!(!disclosure_spec(&config, Lang::En).enabled);
        config.call.disclosure = true;
        let en = disclosure_spec(&config, Lang::En);
        assert!(en.enabled);
        assert_eq!(
            en.text,
            "This call is translated live. You're hearing a synthesized voice."
        );
        assert_eq!(
            disclosure_spec(&config, Lang::Zh).text,
            "本通话为实时翻译，你听到的是合成语音。"
        );
    }

    #[test]
    fn a_configured_text_replaces_the_built_in_one_for_its_language_only() {
        let mut config = AppConfig::default();
        config.call.disclosure_text_en = "  Heads up: machine translation.  ".into();
        assert_eq!(
            disclosure_text(&config, Lang::En),
            "Heads up: machine translation."
        );
        assert!(disclosure_text(&config, Lang::Zh).starts_with("本通话"));
        config.call.disclosure_text_en = "   ".into();
        assert!(disclosure_text(&config, Lang::En).starts_with("This call"));
    }

    #[test]
    fn the_setting_turns_it_off() {
        let mut config = AppConfig::default();
        config.call.disclosure = false;
        assert!(!disclosure_spec(&config, Lang::En).enabled);
    }
}

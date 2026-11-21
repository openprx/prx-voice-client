//! Language tags of the framework (BCP 47) for the pipeline's languages.

use prxvc_media::Lang;

/// The framework's tag for `lang`: Chinese is Simplified (`zh-Hans`, the script the pipeline
/// speaks and writes), English is `en`. A language added to [`Lang`] must be added here: the match
/// has no wildcard.
pub const fn tag(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "zh-Hans",
        Lang::En => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_is_simplified_and_english_is_plain() {
        assert_eq!(tag(Lang::Zh), "zh-Hans");
        assert_eq!(tag(Lang::En), "en");
    }
}

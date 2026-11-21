//! Which language a text is written in, judged from its script alone.
//!
//! The pipeline knows two languages, Chinese and English, and they use different scripts, so a
//! count of Chinese characters against Latin letters tells them apart well enough for the rules that
//! need it: a recognised clause that is already in the language the listener hears (nothing to
//! translate), and a translation that came back in the wrong language. Both rules are pure and
//! live here so the pipeline and the translation adapter share one reading of "in language X"
//! (placement as for `speakable`).
//!
//! Text with fewer than [`MIN_LETTERS`] letters is never judged: a short answer such as "OK" or "好"
//! is as likely in one language as the other.

use crate::Lang;

/// The fewest letters (Chinese characters plus Latin letters) a text needs before it is judged.
pub const MIN_LETTERS: usize = 4;

/// The share of Chinese characters among Chinese and Latin letters at which a source text counts as
/// Chinese.
const SOURCE_IS_CHINESE_FROM: f32 = 0.6;

/// The share of Chinese characters at or below which a source text counts as English.
const SOURCE_IS_ENGLISH_UP_TO: f32 = 0.05;

/// A translation into Chinese is in the wrong language below this share of Chinese characters.
const OUTPUT_ZH_WRONG_BELOW: f32 = 0.3;

/// A translation into English is in the wrong language above this share of Chinese characters.
const OUTPUT_EN_WRONG_ABOVE: f32 = 0.2;

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// The share of Chinese characters among the Chinese and Latin letters of `text`, or `None` when it
/// has fewer than [`MIN_LETTERS`] of them.
pub fn cjk_share(text: &str) -> Option<f32> {
    let (mut cjk, mut latin) = (0usize, 0usize);
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else if c.is_alphabetic() {
            latin += 1;
        }
    }
    let letters = cjk + latin;
    if letters < MIN_LETTERS {
        return None;
    }
    #[allow(clippy::cast_precision_loss, reason = "character counts of one clause")]
    Some(cjk as f32 / letters as f32)
}

/// Whether `text` is clearly written in `lang` (and so needs no translation into it). `false` when
/// it is in the other language, mixed, or too short to tell.
pub fn is_written_in(text: &str, lang: Lang) -> bool {
    match (cjk_share(text), lang) {
        (Some(share), Lang::Zh) => share >= SOURCE_IS_CHINESE_FROM,
        (Some(share), Lang::En) => share <= SOURCE_IS_ENGLISH_UP_TO,
        (None, _) => false,
    }
}

/// Whether a translation `output` into `target` is in the wrong language: mostly the other script.
/// `false` when it is too short to tell.
pub fn is_wrong_language_for(output: &str, target: Lang) -> bool {
    match (cjk_share(output), target) {
        (Some(share), Lang::Zh) => share < OUTPUT_ZH_WRONG_BELOW,
        (Some(share), Lang::En) => share > OUTPUT_EN_WRONG_ABOVE,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_and_english_text_are_told_apart() {
        assert!(is_written_in("我们三点半在机场见面", Lang::Zh));
        assert!(!is_written_in("我们三点半在机场见面", Lang::En));
        assert!(is_written_in("See you at the airport", Lang::En));
        assert!(!is_written_in("See you at the airport", Lang::Zh));
    }

    #[test]
    fn mixed_text_is_in_neither_language() {
        // A Chinese sentence with an English name stays Chinese; half and half is neither.
        assert!(is_written_in("我们今天用Zoom开会议", Lang::Zh));
        assert!(!is_written_in("hello 你好 world 世界", Lang::Zh));
        assert!(!is_written_in("hello 你好 world 世界", Lang::En));
    }

    #[test]
    fn short_text_is_never_judged() {
        for text in ["OK", "好", "", "嗯嗯", "1 2 3"] {
            assert_eq!(cjk_share(text), None, "{text:?}");
            assert!(!is_written_in(text, Lang::Zh) && !is_written_in(text, Lang::En));
            assert!(!is_wrong_language_for(text, Lang::Zh));
            assert!(!is_wrong_language_for(text, Lang::En));
        }
    }

    #[test]
    fn a_translation_in_the_wrong_script_is_wrong() {
        assert!(is_wrong_language_for("我们三点见面。", Lang::En));
        assert!(is_wrong_language_for("See you at three.", Lang::Zh));
        assert!(!is_wrong_language_for("See you at three.", Lang::En));
        assert!(!is_wrong_language_for("我们三点见面。", Lang::Zh));
        assert!(is_wrong_language_for("好吧，我们走吧。Let us go", Lang::En));
        assert!(!is_wrong_language_for("好 That is fine with me", Lang::En));
    }
}

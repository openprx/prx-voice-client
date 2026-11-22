//! Recognised text that is only filler: backchannel sounds with no content (pipeline-design 3.2, call findings of 2026-11-21).
//!
//! The recogniser writes "嗯。" or "Mm." for audio without speech (a breath, a hum, the tail of a
//! played translation) and a lone function word for the faintest sounds. Such a unit is not
//! translated and not spoken: it would put a short filler into the listener's ear for nothing, and
//! very short targets are the text on which the synthesiser runs away.
//!
//! The rule is pure: a text is filler when every token of it, after punctuation and white space
//! are taken out, is in the table of the language it was recognised in or in the table of sounds
//! that every language shares (the recogniser writes "嗯" for English audio and "Mm." for Chinese).
//! A text with any other token is content: "嗯嗯好的", "嗯，我想说", "Uh, okay" and "Oh no" are kept,
//! and so are the short answers 好, 对, 是, yes, no and ok.
//!
//! A second rule, [`is_stray_tail`], covers the English fragments ("I.", "I'm.", "Still.") the recogniser
//! writes for the trailing silence of an utterance: they are dropped only as the last unit of a
//! segment that has an earlier unit, and kept as the only unit of a segment.
//!
//! A pure "嗯" or "Mm." that was a real acknowledgement ("uh-huh") is dropped too: the listener
//! cannot tell it from a hum, and `pipeline.drop_filler = false` turns the rule off for those who
//! want every unit.

use prxvc_media::Lang;

/// Sounds the recogniser writes for any language: hums and hesitations.
const COMMON: &[&str] = &[
    "嗯", "呃", "额", "唔", "mm", "hm", "hmm", "um", "uh", "mhm", "mmhm", "mmhmm", "uhhuh", "er",
    "erm",
];
/// Interjections of Chinese speech.
const ZH: &[&str] = &["啊", "哦", "噢", "喔", "呣"];
/// Interjections of English speech, and the single function words the recogniser leaves behind for
/// the faintest sounds (no utterance is only "the").
const EN: &[&str] = &["ah", "eh", "oh", "the", "a", "an", "and", "of", "to"];

/// Single English tokens (apostrophes dropped, case folded) that the recogniser leaves behind on the
/// trailing silence of an utterance: "I.", "I'm.", "Still.". They are real words, so they are not
/// filler anywhere else; see [`is_stray_tail`]. "the", "a" and "and" are in [`EN`] already, and
/// "okay" and "ok" are answers, not fragments.
const EN_TAIL_FRAGMENTS: &[&str] = &["i", "im", "ill", "ive", "but", "so", "still", "well"];

/// Whether `text`, recognised as `lang`, is a stray one-word fragment of the closed list for that
/// language. The caller applies it only to the last unit of a segment that has an earlier unit:
/// the same word alone is a real (if short) utterance. Chinese has no such list.
pub fn is_stray_tail(lang: Lang, text: &str) -> bool {
    if lang != Lang::En {
        return false;
    }
    let mut it = tokens(text);
    matches!((it.next(), it.next()), (Some(t), None) if EN_TAIL_FRAGMENTS.contains(&t.as_str()))
}

/// Whether `text`, recognised as `lang`, holds nothing but filler. Empty and punctuation-only text
/// are filler as well: there is nothing in them to translate.
pub fn is_pure_filler(lang: Lang, text: &str) -> bool {
    let own = match lang {
        Lang::Zh => ZH,
        Lang::En => EN,
    };
    let known = |t: &str| COMMON.contains(&t) || own.contains(&t);
    tokens(text).all(|t| known(&t) || known(&collapse(&t)))
}

/// `token` with every run of one letter cut to a single letter ("umm" is "um", "uhh" is "uh").
fn collapse(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        if !out.ends_with(c) {
            out.push(c);
        }
    }
    out
}

/// The tokens of `text`: one per CJK character, one per run of other letters or digits. Hyphens and
/// apostrophes join ("uh-huh" is one token), case is folded, and a letter repeated more than twice
/// counts twice ("hmmmm" is "hmm"; the lookup also tries a run cut to one letter).
fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    let mut chars = text.chars().peekable();
    std::iter::from_fn(move || {
        let mut token = String::new();
        let mut last: Option<char> = None;
        let mut run = 0;
        while let Some(&c) = chars.peek() {
            if is_cjk(c) {
                if token.is_empty() {
                    chars.next();
                    return Some(c.to_string());
                }
                break;
            }
            if c.is_alphanumeric() {
                chars.next();
                for lower in c.to_lowercase() {
                    run = if last == Some(lower) { run + 1 } else { 1 };
                    last = Some(lower);
                    if run <= 2 {
                        token.push(lower);
                    }
                }
            } else if token.is_empty() || matches!(c, '-' | '\'' | '\u{2019}') {
                // A separator before the token, or a hyphen or apostrophe inside it, is skipped.
                chars.next();
            } else {
                break;
            }
        }
        (!token.is_empty()).then_some(token)
    })
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2A6DF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_fillers_are_dropped_in_both_languages() {
        for text in [
            "嗯。",
            "嗯",
            "嗯嗯。",
            "嗯嗯嗯嗯",
            "呃……",
            "啊。",
            "哦",
            "唔。",
            "嗯，啊。",
            "嗯。 嗯。",
            "嗯哦",
        ] {
            assert!(is_pure_filler(Lang::Zh, text), "zh {text:?}");
        }
        for text in [
            "Mm.", "Um.", "Uh,", "Hmm...", "hmmmm", "Mhm.", "Mm-hmm.", "Uh-huh.", "um, uh", "Oh.",
            "Ah!", "Mm. Mm.", "The.", "  ", "", "...", "。", "?!",
        ] {
            assert!(is_pure_filler(Lang::En, text), "en {text:?}");
        }
    }

    #[test]
    fn the_recogniser_may_write_the_other_languages_hum() {
        assert!(is_pure_filler(Lang::En, "嗯。"));
        assert!(is_pure_filler(Lang::Zh, "Mm."));
        assert!(is_pure_filler(Lang::Zh, "哦"));
        assert!(
            !is_pure_filler(Lang::En, "哦"),
            "a Chinese interjection is content to the English rule"
        );
        assert!(
            !is_pure_filler(Lang::Zh, "Oh."),
            "an English interjection is content to the Chinese rule"
        );
    }

    #[test]
    fn real_short_answers_and_mixed_text_are_content() {
        for text in [
            "好",
            "好。",
            "对",
            "是",
            "是的。",
            "不是",
            "嗯嗯好的",
            "嗯，我想说",
            "啊，真的吗",
            "哦对了",
            "你好",
        ] {
            assert!(!is_pure_filler(Lang::Zh, text), "zh {text:?}");
        }
        for text in [
            "Yes.",
            "no",
            "OK",
            "okay",
            "Uh, okay.",
            "Oh no.",
            "The end.",
            "Hm, fine",
            "Right",
            "Hello",
            "I",
            "a b",
            "3",
            "Mm, yes",
        ] {
            assert!(!is_pure_filler(Lang::En, text), "en {text:?}");
        }
    }

    #[test]
    fn stray_tail_fragments_are_single_listed_english_tokens() {
        for text in [
            "I.",
            "I'm.",
            "I'll.",
            "I've.",
            "But",
            "So.",
            "Still.",
            "Well,",
            "i",
            "I\u{2019}m.",
        ] {
            assert!(is_stray_tail(Lang::En, text), "en {text:?}");
        }
        for text in [
            "OK.",
            "Okay.",
            "Yes.",
            "I am.",
            "Still here.",
            "So what?",
            "",
            "...",
            "Hello",
            "I.I.",
        ] {
            assert!(!is_stray_tail(Lang::En, text), "en {text:?}");
        }
        // The articles and "and" are filler already, and need no second list.
        for text in ["The.", "A.", "And."] {
            assert!(is_pure_filler(Lang::En, text), "en {text:?}");
        }
        for text in ["I.", "我。", "仍然。", "嗯。"] {
            assert!(!is_stray_tail(Lang::Zh, text), "zh {text:?}");
        }
    }

    #[test]
    fn a_letter_repeated_beyond_two_is_folded_but_other_words_are_not() {
        assert!(is_pure_filler(Lang::En, "ummmmm"));
        assert!(is_pure_filler(Lang::En, "UHHHH"));
        assert!(!is_pure_filler(Lang::En, "ummmmx"));
    }
}

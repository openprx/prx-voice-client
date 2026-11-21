//! Whether a text has anything to say aloud.
//!
//! A translation can come back as punctuation only (`...`, `。`, a lone quotation mark) or hold only
//! symbols and emoji. A synthesiser given such a text has nothing to speak and either fails or runs
//! on, so the pipeline never sends it. The rule is pure and shared by the pipeline stages and the
//! synthesis adapters (placement as for `clause_split`).

/// Whether `text` holds at least one letter, digit or CJK character, that is, something a voice can
/// say. Punctuation, symbols, emoji, control and white-space characters do not count.
pub fn has_speakable_content(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_cjk_are_speakable() {
        for text in ["Hello.", "3", "好", "ok!", "  a  ", "$5", "第3章", "été"] {
            assert!(has_speakable_content(text), "{text:?}");
        }
    }

    #[test]
    fn punctuation_symbols_and_emptiness_are_not() {
        for text in [
            "",
            "   ",
            "...",
            "。",
            "…",
            "\"\"",
            "—",
            "?!",
            "(...)",
            "😀",
            "♪ ♪",
            "\u{200b}",
            "\n\t",
            "，。！",
        ] {
            assert!(!has_speakable_content(text), "{text:?}");
        }
    }
}

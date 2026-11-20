//! Comparison units of a transcript: one per CJK character and one per word of other text.
//!
//! Punctuation, spacing and letter case do not matter when two hypotheses are compared, so the
//! stable-prefix rule works on these units, not on bytes (pipeline-design section 3).

/// One comparison unit of a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Unit {
    /// The case-folded unit.
    pub(crate) key: String,
    /// Byte offset in the text just after the unit.
    pub(crate) end: usize,
}

/// Whether `c` is written one character per unit (Han, kana, Hangul, and the CJK extensions).
pub(crate) fn is_cjk(c: char) -> bool {
    matches!(u32::from(c),
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// Split `text` into units: each CJK character alone, every other maximal run of letters and digits
/// as one word. An apostrophe inside a word, and a `.` or `,` between two digits, belong to the word.
pub(crate) fn units(text: &str) -> Vec<Unit> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, end: usize, out: &mut Vec<Unit>| {
        if !word.is_empty() {
            out.push(Unit {
                key: std::mem::take(word),
                end,
            });
        }
    };
    for (i, &(at, c)) in chars.iter().enumerate() {
        let end = at + c.len_utf8();
        let next = chars.get(i + 1).map(|&(_, n)| n);
        if is_cjk(c) {
            flush(&mut word, at, &mut out);
            out.push(Unit {
                key: c.to_string(),
                end,
            });
        } else if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else if !word.is_empty()
            && ((matches!(c, '\'' | '\u{2019}')
                && next.is_some_and(|n| n.is_alphanumeric() && !is_cjk(n)))
                || (matches!(c, '.' | ',')
                    && word.ends_with(|w: char| w.is_ascii_digit())
                    && next.is_some_and(|n| n.is_ascii_digit())))
        {
            word.push(if c == '\u{2019}' { '\'' } else { c });
        } else {
            flush(&mut word, at, &mut out);
        }
        if !word.is_empty() && next.is_none() {
            flush(&mut word, end, &mut out);
        }
    }
    out
}

/// The punctuation that directly follows byte offset `end` (up to whitespace or the next unit).
pub(crate) fn punctuation_after(text: &str, end: usize) -> &str {
    let rest = &text[end..];
    let len = rest
        .char_indices()
        .find(|&(_, c)| c.is_whitespace() || c.is_alphanumeric())
        .map_or(rest.len(), |(i, _)| i);
    &rest[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<String> {
        units(text).into_iter().map(|u| u.key).collect()
    }

    #[test]
    fn chinese_is_split_per_character_and_punctuation_is_dropped() {
        assert_eq!(keys("你好，世界。"), ["你", "好", "世", "界"]);
        assert_eq!(keys("  你 好 "), ["你", "好"]);
    }

    #[test]
    fn english_is_split_per_word_ignoring_case_and_punctuation() {
        assert_eq!(keys("Hello, World!"), ["hello", "world"]);
        assert_eq!(
            keys("It's 3.5 or 1,000, isn't it?"),
            ["it's", "3.5", "or", "1,000", "isn't", "it"]
        );
    }

    #[test]
    fn mixed_text_keeps_latin_words_whole() {
        assert_eq!(keys("用iPhone打电话"), ["用", "iphone", "打", "电", "话"]);
    }

    #[test]
    fn unit_ends_are_byte_offsets_after_the_unit() {
        let text = "好, ok.";
        let u = units(text);
        assert_eq!(&text[..u[0].end], "好");
        assert_eq!(&text[..u[1].end], "好, ok");
        assert_eq!(punctuation_after(text, u[0].end), ",");
        assert_eq!(punctuation_after(text, u[1].end), ".");
        assert_eq!(punctuation_after("好 嗎", 3), "");
    }

    #[test]
    fn a_trailing_word_is_flushed_and_an_empty_text_has_no_units() {
        assert_eq!(units("abc").len(), 1);
        assert_eq!(units("abc")[0].end, 3);
        assert!(units("").is_empty());
        assert!(units(" ,.! ").is_empty());
    }
}

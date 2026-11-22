//! Where the uncommitted remainder of a final text starts, once part of the segment's text is committed.
//!
//! The recogniser's partials never retract a prefix declared stable, and the pipeline commits
//! clauses from that prefix before the segment ends. Its final text is returned as decoded, so it
//! can differ from text already committed (and perhaps translated and spoken). Committed text is
//! never retracted (pipeline-design 3.1): only the part of the final that follows the committed
//! part is taken from it, and this module finds that part.
//!
//! The comparison is by content, not by length. Punctuation, spacing and letter case do not count,
//! which is also what the recogniser's stability rule ignores. If the letters and digits of the
//! committed text are the start of those of the final (a word may be split differently), the
//! remainder is what follows them. If the final rewrote a word inside the committed part, or is
//! shorter, the committed part is aligned to the closest start of the final by edit distance over
//! the units of [`prxvc_media::text_units`] (one per Chinese character, one per other word) and
//! the remainder is what follows that. If the final cannot be aligned at all, the remainder is the
//! suffix after the committed length, and the caller counts it.

use prxvc_media::text_units::{Unit, is_cjk, units};

/// How the committed text relates to the final.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alignment {
    /// The final starts with the committed units (spacing, punctuation and case aside), or nothing
    /// is committed.
    Agrees,
    /// The final differs inside the committed part but resembles it enough to tell where it ends.
    Rewritten,
    /// No alignment was found: the remainder is the suffix after the committed length.
    Unaligned,
}

/// Where the uncommitted remainder of a final begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Aligned {
    /// Byte offset in the final text, on a char boundary.
    pub(crate) start: usize,
    /// How the committed text matched.
    pub(crate) alignment: Alignment,
}

/// At most this share of the committed units may differ for the committed part to be found in the
/// final by content; one unit is always allowed.
const MAX_DIFFERING_SHARE: (usize, usize) = (1, 3);

/// Marks that close the committed text and are not the start of what follows it.
fn is_closing_mark(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | '.'
                | ';'
                | ':'
                | '!'
                | '?'
                | ')'
                | ']'
                | '}'
                | '\u{2019}'
                | '\u{201D}'
                | '\u{FF0C}'
                | '\u{3002}'
                | '\u{FF1B}'
                | '\u{FF1A}'
                | '\u{FF01}'
                | '\u{FF1F}'
                | '\u{3001}'
                | '\u{2026}'
                | '\u{FF09}'
                | '\u{300D}'
                | '\u{300F}'
        )
}

/// `at` moved past the closing marks that directly follow it.
fn skip_closing_marks(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    let len = rest
        .char_indices()
        .find(|&(_, c)| !is_closing_mark(c))
        .map_or(rest.len(), |(i, _)| i);
    at + len
}

/// The largest index `<= at` that is a char boundary of `text`.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `at`, moved back to the start of the word it falls inside when that word is of a script that
/// spaces its words; CJK text and positions between words are unchanged.
fn word_start_floor(text: &str, at: usize) -> usize {
    let at = floor_boundary(text, at);
    let spaced = |c: char| c.is_alphanumeric() && !is_cjk(c);
    let inside = text[..at].chars().next_back().is_some_and(spaced)
        && text[at..].chars().next().is_some_and(spaced);
    if !inside {
        return at;
    }
    text[..at]
        .char_indices()
        .rev()
        .find(|&(_, c)| !spaced(c))
        .map_or(0, |(i, c)| i + c.len_utf8())
}

/// The letters and digits of `text`, case folded, each with the byte offset just after it.
fn letters(text: &str) -> Vec<(char, usize)> {
    text.char_indices()
        .filter(|(_, c)| c.is_alphanumeric())
        .flat_map(|(at, c)| {
            let end = at + c.len_utf8();
            c.to_lowercase().map(move |l| (l, end))
        })
        .collect()
}

/// Where the final text ends the match of the committed letters, when they are the start of the
/// final's letters and the match ends at a word boundary.
fn letter_prefix_end(committed: &str, fin: &str) -> Option<usize> {
    let c = letters(committed);
    let f = letters(fin);
    if c.is_empty() || f.len() < c.len() || !c.iter().zip(&f).all(|(a, b)| a.0 == b.0) {
        return None;
    }
    let end = f[c.len() - 1].1;
    let prev = fin[..end].chars().next_back();
    let next = fin[end..].chars().next();
    let inside_word = prev.is_some_and(|p| p.is_alphanumeric() && !is_cjk(p))
        && next.is_some_and(|n| n.is_alphanumeric() && !is_cjk(n));
    (!inside_word).then_some(end)
}

/// The number of final units (`0..=n`) whose edit distance to the committed units is smallest, and
/// that distance. Ties go to the length closest to the committed length, then to the longer one.
fn best_prefix(committed: &[Unit], fin: &[Unit]) -> (usize, usize) {
    let (k, n) = (committed.len(), fin.len());
    // `row[j]`: distance between the first `i` committed units and the first `j` final units.
    let mut row: Vec<usize> = (0..=n).collect();
    for i in 1..=k {
        let mut diagonal = row[0];
        row[0] = i;
        for j in 1..=n {
            let above = row[j];
            let cost = usize::from(committed[i - 1].key != fin[j - 1].key);
            row[j] = (diagonal + cost).min(above + 1).min(row[j - 1] + 1);
            diagonal = above;
        }
    }
    let (mut best_j, mut best_cost) = (0usize, row[0]);
    for (j, &cost) in row.iter().enumerate().skip(1) {
        let closer = j.abs_diff(k) < best_j.abs_diff(k);
        let as_close_but_longer = j.abs_diff(k) == best_j.abs_diff(k) && j > best_j;
        if cost < best_cost || (cost == best_cost && (closer || as_close_but_longer)) {
            (best_j, best_cost) = (j, cost);
        }
    }
    (best_j, best_cost)
}

/// Find where in `fin` the text that follows `committed` begins.
///
/// `committed` is the committed text of the recogniser's round as it was committed; `fin` is the
/// final text of the same round.
pub(crate) fn align_after_committed(committed: &str, fin: &str) -> Aligned {
    let c = units(committed);
    if c.is_empty() {
        return Aligned {
            start: 0,
            alignment: Alignment::Agrees,
        };
    }
    if let Some(end) = letter_prefix_end(committed, fin) {
        return Aligned {
            start: skip_closing_marks(fin, end),
            alignment: Alignment::Agrees,
        };
    }
    let f = units(fin);
    let (j, cost) = best_prefix(&c, &f);
    let allowed = (c.len() * MAX_DIFFERING_SHARE.0 / MAX_DIFFERING_SHARE.1).max(1);
    if j > 0 && cost <= allowed {
        return Aligned {
            start: skip_closing_marks(fin, f[j - 1].end),
            alignment: Alignment::Rewritten,
        };
    }
    Aligned {
        start: word_start_floor(fin, committed.len()),
        alignment: Alignment::Unaligned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The remainder of `fin` after `committed`, and how they matched.
    fn split<'a>(committed: &str, fin: &'a str) -> (&'a str, Alignment) {
        let a = align_after_committed(committed, fin);
        (&fin[a.start..], a.alignment)
    }

    #[test]
    fn a_final_that_is_the_committed_text_plus_the_remainder_agrees() {
        assert_eq!(
            split("今天天气很好", "今天天气很好，我们去公园玩。"),
            ("我们去公园玩。", Alignment::Agrees)
        );
        assert_eq!(
            split(
                "the meeting starts at nine",
                "The meeting starts at nine, and then we break for lunch."
            ),
            ("and then we break for lunch.", Alignment::Agrees)
        );
    }

    #[test]
    fn an_unaligned_remainder_never_starts_inside_a_word() {
        let committed =
            "First among is seventy recommendations is that a new diplomatic initiative, s";
        let fin = "Number one of the seventy points is a new initiative, should be taken before the end of this year";
        let a = align_after_committed(committed, fin);
        assert_eq!(a.alignment, Alignment::Unaligned);
        assert!(
            fin[..a.start].is_empty() || fin[..a.start].ends_with(' '),
            "boundary at {:?}",
            &fin[a.start..]
        );
        let a = align_after_committed(&committed[..committed.len() - 2], fin);
        assert_eq!(a.alignment, Alignment::Unaligned);
        assert!(fin[..a.start].ends_with(' ') || a.start == 0);
        // Chinese text has no words to snap to.
        let a = align_after_committed("今天天气很好我们去公园玩", "完全不同的另一句话在这里说完了");
        assert_eq!(a.alignment, Alignment::Unaligned);
    }

    #[test]
    fn word_start_floor_snaps_latin_words_only() {
        assert_eq!(word_start_floor("a should b", 4), 2);
        assert_eq!(word_start_floor("a should b", 2), 2);
        assert_eq!(word_start_floor("a should b", 8), 8);
        assert_eq!(word_start_floor("should", 3), 0);
        assert_eq!(word_start_floor("今天天气", 3), 3);
    }

    #[test]
    fn nothing_committed_takes_the_whole_final() {
        assert_eq!(
            split("", "你好，世界。"),
            ("你好，世界。", Alignment::Agrees)
        );
        assert_eq!(
            split("", "Hello there."),
            ("Hello there.", Alignment::Agrees)
        );
        assert_eq!(split("，", "你好"), ("你好", Alignment::Agrees));
    }

    #[test]
    fn punctuation_spacing_and_case_differences_alone_are_not_a_disagreement() {
        // The committed part has no marks, the final has them.
        assert_eq!(
            split("我想去商店买东西", "我想去，商店买东西。然后回家。"),
            ("然后回家。", Alignment::Agrees)
        );
        assert_eq!(
            split(
                "we should leave now",
                "We should leave, now. Before it rains."
            ),
            ("Before it rains.", Alignment::Agrees)
        );
        // The committed part has marks the final lacks, and different spacing.
        assert_eq!(
            split("你好， 世界。", "你好世界我们开始吧"),
            ("我们开始吧", Alignment::Agrees)
        );
        assert_eq!(
            split("It's 3.5 dollars, ok?", "its 3.5 dollars ok and more"),
            ("and more", Alignment::Agrees)
        );
        assert_eq!(
            split("it costs 1,000 yuan", "It costs 1000 yuan today."),
            ("today.", Alignment::Agrees)
        );
    }

    #[test]
    fn nothing_left_uncommitted_gives_an_empty_remainder() {
        assert_eq!(
            split("今天天气很好", "今天天气很好。"),
            ("", Alignment::Agrees)
        );
        assert_eq!(
            split("see you tomorrow", "See you tomorrow!"),
            ("", Alignment::Agrees)
        );
        assert_eq!(
            split("今天天气很好", "今天天气很好"),
            ("", Alignment::Agrees)
        );
    }

    #[test]
    fn a_final_that_rewrites_a_word_inside_the_committed_part_is_aligned_by_content() {
        // One character of the committed part differs; the remainder still starts after it.
        assert_eq!(
            split("我想去商店买东西", "我想去超店买东西，然后回家。"),
            ("然后回家。", Alignment::Rewritten)
        );
        assert_eq!(
            split(
                "the meeting starts at nine tomorrow",
                "The meeting starts at five tomorrow and runs late."
            ),
            ("and runs late.", Alignment::Rewritten)
        );
        // The final splits a word the committed part had whole: the same letters.
        assert_eq!(
            split(
                "alpha bravo charlie delta echo foxtrot",
                "alpha bravo charlie delta echo fox trot golf hotel"
            ),
            ("golf hotel", Alignment::Agrees)
        );
        // The committed part ends inside a word of the final: not a boundary, aligned by units.
        assert_eq!(
            split(
                "alpha bravo charlie delta echo fox",
                "alpha bravo charlie delta echo foxtrot golf"
            )
            .1,
            Alignment::Rewritten
        );
    }

    #[test]
    fn a_final_shorter_than_the_committed_part_leaves_nothing_to_take() {
        // The final lost the last committed word.
        assert_eq!(
            split(
                "the meeting starts at nine tomorrow",
                "The meeting starts at nine"
            ),
            ("", Alignment::Rewritten)
        );
        // The final lost the last two characters of a long committed part.
        assert_eq!(
            split(
                "我们明天下午三点在公司开会讨论",
                "我们明天下午三点在公司开会"
            ),
            ("", Alignment::Rewritten)
        );
        // The final is much shorter: no alignment, and nothing after the committed length either.
        assert_eq!(
            split("我们明天下午三点在公司开会讨论", "我们"),
            ("", Alignment::Unaligned)
        );
    }

    #[test]
    fn a_final_that_has_nothing_to_do_with_the_committed_text_falls_back_to_the_length_suffix() {
        // "今天天气很好" is 18 bytes: the suffix after that length is taken, at a char boundary.
        let (rest, alignment) = split("今天天气很好", "我们去公园散步吧");
        assert_eq!(alignment, Alignment::Unaligned);
        assert_eq!(rest, "步吧");
        let (rest, alignment) = split("good morning everyone", "see you later alligator, bye");
        assert_eq!(alignment, Alignment::Unaligned);
        // 21 bytes fall inside "alligator": the boundary moves back to the start of that word.
        assert_eq!(rest, "alligator, bye");
        // The fallback never splits a character: 8 bytes fall inside the third one.
        let (rest, alignment) = split("abcd efg", "你好啊你好");
        assert_eq!(alignment, Alignment::Unaligned);
        assert_eq!(rest, "啊你好");
    }

    #[test]
    fn a_remainder_does_not_start_with_the_marks_that_closed_the_committed_text() {
        assert_eq!(
            split("今天天气很好", "今天天气很好！！ 我们走吧"),
            ("我们走吧", Alignment::Agrees)
        );
        // An opening quote belongs to what follows.
        assert_eq!(
            split("he said", "He said, \"let's go\""),
            ("\"let's go\"", Alignment::Agrees)
        );
    }

    #[test]
    fn mixed_language_text_aligns_on_cjk_characters_and_latin_words() {
        assert_eq!(
            split("用iPhone打电话", "用 iPhone 打电话给我妈妈"),
            ("给我妈妈", Alignment::Agrees)
        );
    }
}

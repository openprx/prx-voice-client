//! Approximate alignment of the spoken part of a sentence against a new translation of the whole
//! sentence (incremental-retranslation 4.6). A translation engine that is asked again for a longer
//! source often words the start differently ("of" for "for"); an exact prefix test then declares
//! the sentence diverged although the new translation clearly goes on from what was spoken. This
//! finds where the spoken part ends in the new translation when the two differ by a few units only.

use prxvc_media::text_units::{punctuation_after, units};

/// Most units of the spoken part the alignment looks at; above it the answer is "no match".
pub(crate) const ALIGN_MAX_SPOKEN_UNITS: usize = 120;
/// Most units of the new translation the alignment looks at.
pub(crate) const ALIGN_MAX_FULL_UNITS: usize = 240;

/// Where the spoken text `spoken` ends in `full`, when `full` begins with something close to it.
///
/// Both texts are split into the comparison units of the policy (case, punctuation and spacing do
/// not matter). For every end position `p` in `full` the edit distance between all spoken units and
/// the first `p` units of `full` is computed; the position with the least distance wins (a tie goes
/// to the position whose last unit equals the last spoken unit, then to the larger position). It is
/// accepted when `distance / spoken units <= max_ratio`. Returns the byte offset in `full` just
/// after that unit and the marks that follow it, like `prefix_end`. `None` when nothing is spoken,
/// the ratio is not positive, a text is over the size caps, or no position is close enough.
pub(crate) fn approx_prefix_end(spoken: &str, full: &str, max_ratio: f32) -> Option<usize> {
    if max_ratio.is_nan() || max_ratio <= 0.0 {
        return None;
    }
    let s = units(spoken);
    let f = units(full);
    if s.is_empty() || s.len() > ALIGN_MAX_SPOKEN_UNITS || f.len() > ALIGN_MAX_FULL_UNITS {
        return None;
    }
    // `row[j]`: distance between all spoken units so far and the first `j` units of `full`.
    let mut row: Vec<usize> = (0..=f.len()).collect();
    for (i, su) in s.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, fu) in f.iter().enumerate() {
            let up = row[j + 1];
            let cost = usize::from(su.key != fu.key);
            row[j + 1] = (diag + cost).min(up + 1).min(row[j] + 1);
            diag = up;
        }
    }
    let last = &s[s.len() - 1].key;
    let best = (1..=f.len()).min_by_key(|&p| {
        let ends_alike = f[p - 1].key == *last;
        (row[p], !ends_alike, std::cmp::Reverse(p))
    })?;
    if row[best] as f32 > max_ratio.min(1.0) * s.len() as f32 {
        return None;
    }
    let end = f[best - 1].end;
    Some(end + punctuation_after(full, end).len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest<'a>(spoken: &str, full: &'a str, ratio: f32) -> Option<&'a str> {
        approx_prefix_end(spoken, full, ratio).map(|at| full[at..].trim())
    }

    #[test]
    fn near_prefixes_are_aligned_and_the_remainder_is_what_follows() {
        let cases = [
            (
                "The budget of this project",
                "The budget for this project is $2.5 million.",
                "is $2.5 million.",
            ),
            (
                "During the War of Independence, thirteen states",
                "During the Revolutionary War, thirteen states formed a weak central government according to federal regulations.",
                "formed a weak central government according to federal regulations.",
            ),
            (
                "虽然文件的背面写着一些东西，",
                "虽然文件背面写着一些东西，但它不是一张藏宝图。",
                "但它不是一张藏宝图。",
            ),
            ("we meet", "we meet at nine", "at nine"),
        ];
        for (spoken, full, expected) in cases {
            assert_eq!(rest(spoken, full, 0.5), Some(expected), "{spoken}");
        }
    }

    #[test]
    fn a_translation_that_is_really_different_is_not_aligned() {
        assert_eq!(rest("我们应该见面，", "他们必须在办公室碰头。", 0.5), None);
        assert_eq!(
            rest("we should meet", "completely other words here", 0.5),
            None
        );
    }

    #[test]
    fn a_zero_ratio_disables_the_step_and_a_big_one_is_capped_at_one() {
        let full = "The budget for this project is $2.5 million.";
        assert_eq!(rest("The budget of this project", full, 0.0), None);
        assert_eq!(rest("The budget of this project", full, -1.0), None);
        assert_eq!(rest("The budget of this project", full, f32::NAN), None);
        assert!(rest("The budget of this project", full, 0.2).is_some());
        assert_eq!(rest("The budget of this project", full, 0.1), None);
        assert!(
            rest("one two", "alpha beta", 7.0).is_some(),
            "ratio 1.0 takes any text"
        );
    }

    #[test]
    fn ties_prefer_a_matching_last_unit_and_then_the_larger_position() {
        // "a b d" is one edit from "a b" (positions 2) and from "a b c" (position 3): the larger wins.
        assert_eq!(rest("a b d", "a b c e", 0.5), Some("e"));
        // Positions 2, 3 and 4 are all one edit from "a b c"; position 4 ends on the unit "c".
        assert_eq!(rest("a b c", "a b x c d e", 0.5), Some("d e"));
        // Position 3 ("a c d") ends on "d" like the spoken text and is as close as position 2.
        assert_eq!(rest("a b d", "a c d e", 0.5), Some("e"));
    }

    #[test]
    fn nothing_after_the_aligned_part_leaves_an_empty_remainder() {
        assert_eq!(
            rest("the budget of it", "The budget for it.", 0.5),
            Some("")
        );
    }

    #[test]
    fn texts_over_the_size_caps_are_not_aligned() {
        let long_spoken = vec!["w"; ALIGN_MAX_SPOKEN_UNITS + 1].join(" ");
        let ok_spoken = vec!["w"; ALIGN_MAX_SPOKEN_UNITS].join(" ");
        let full = format!("{ok_spoken} tail");
        assert!(approx_prefix_end(&ok_spoken, &full, 0.5).is_some());
        assert_eq!(approx_prefix_end(&long_spoken, &full, 0.5), None);
        let long_full = vec!["w"; ALIGN_MAX_FULL_UNITS + 1].join(" ");
        assert_eq!(approx_prefix_end("w w", &long_full, 0.5), None);
        assert_eq!(approx_prefix_end("", "a b", 0.5), None);
    }
}

//! Transcript normalisation and word error rate, for comparing recognised text with a known one.

/// Lower-case, drop punctuation and split into words.
pub fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Word error rate of `hyp` against `reference`: edit distance over reference length.
pub fn wer(reference: &str, hyp: &str) -> f64 {
    let (r, h) = (words(reference), words(hyp));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_case_and_punctuation() {
        assert_eq!(
            words("Thanks, for JOINING the call."),
            ["thanks", "for", "joining", "the", "call"]
        );
    }

    #[test]
    fn identical_text_has_zero_error() {
        assert_eq!(wer("Hello, world.", "hello world"), 0.0);
    }

    #[test]
    fn counts_substitution_deletion_and_insertion() {
        assert!((wer("a b c d", "a x c d") - 0.25).abs() < 1e-9);
        assert!((wer("a b c d", "a b c") - 0.25).abs() < 1e-9);
        assert!((wer("a b c d", "a b c d e") - 0.25).abs() < 1e-9);
    }

    #[test]
    fn empty_reference_is_handled() {
        assert_eq!(wer("", ""), 0.0);
        assert_eq!(wer("", "x"), 1.0);
    }
}
